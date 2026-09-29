use crate::{diagnostics, mcd_mcp, payment_mvp, AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

pub const TOOL_PREFIX: &str = "mcd_";
const ORDER_TYPE: u64 = 2;
const BE_TYPE: u64 = 2;
const QUOTE_VALID_FOR: Duration = Duration::from_secs(120);

#[derive(Default)]
pub struct ManagerState {
    sessions: Mutex<HashMap<String, Arc<tokio::sync::Mutex<OrderSession>>>>,
}

#[derive(Clone)]
struct CartItem {
    code: String,
    name: String,
    quantity: u64,
}

#[derive(Default)]
struct OrderSession {
    diagnostic_session_id: String,
    client: Option<mcd_mcp::Client>,
    addresses: Vec<Value>,
    address: Option<Value>,
    stores: Vec<Value>,
    store: Option<Value>,
    meals: Vec<Value>,
    candidates: Vec<Value>,
    cart: Vec<CartItem>,
    quote: Option<Value>,
    quoted_at: Option<Instant>,
    order_submission_attempted: bool,
    order_id: Option<String>,
}

impl ManagerState {
    fn session(&self, session_id: &str) -> Result<Arc<tokio::sync::Mutex<OrderSession>>, String> {
        let mut sessions = self.sessions.lock().map_err(|_| "麦当劳点餐会话锁已损坏")?;
        Ok(sessions
            .entry(session_id.to_owned())
            .or_insert_with(|| {
                Arc::new(tokio::sync::Mutex::new(OrderSession {
                    diagnostic_session_id: session_id.to_owned(),
                    ..OrderSession::default()
                }))
            })
            .clone())
    }

    pub fn reset(&self, session_id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(session_id);
        }
    }
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool("mcd_start_delivery", "开始麦乐送外送点餐，读取账号中已保存的配送地址。用户表达想点麦当劳外送时首先调用。", json!({"type":"object","properties":{}})),
        tool("mcd_select_address", "按刚才播报的序号选择配送地址，并查询可配送门店。地址语音标签仅包含区/县级信息，不得要求或复述小区、街道、楼栋、房间号。不能传地址 ID。", json!({"type":"object","properties":{"choice":{"type":"integer","minimum":1,"description":"用户选择的地址序号，从 1 开始"}},"required":["choice"]})),
        tool("mcd_select_store", "按刚才播报的序号选择门店并加载该店实时菜单。不能传门店编码。", json!({"type":"object","properties":{"choice":{"type":"integer","minimum":1,"description":"用户选择的门店序号，从 1 开始"}},"required":["choice"]})),
        tool("mcd_search_products", "在已经加载的门店实时菜单中搜索用户说出的商品名称，返回候选序号。", json!({"type":"object","properties":{"query":{"type":"string","description":"用户说出的商品名或关键词"}},"required":["query"]})),
        tool("mcd_add_simple_item", "按搜索结果序号查询餐品详情并加入购物车。餐品存在套餐、规格或特制选项且服务端提供完整默认值时，按默认配置加入；只有缺少必选默认值时才要求继续澄清。不能传商品编码。", json!({"type":"object","properties":{"choice":{"type":"integer","minimum":1},"quantity":{"type":"integer","minimum":1,"maximum":20}},"required":["choice","quantity"]})),
        tool("mcd_quote_order", "购物车完成后查询优惠券并计算外送订单最终价格。播报商品、配送费、打包费、优惠、总价、门店和区/县级配送地址标签，然后明确询问用户是否“确认下单”。严禁播报小区、街道、楼栋、房间号、联系人或手机号。", json!({"type":"object","properties":{}})),
        tool("mcd_submit_order", "仅在本轮用户逐字明确说出“确认下单”后创建订单。创建成功会由 EasyInput 自动在浏览器打开支付二维码；绝不能替用户支付。", json!({"type":"object","properties":{"confirmation":{"type":"string","enum":["确认下单"]}},"required":["confirmation"]})),
        tool("mcd_check_order", "用户支付后查询刚创建的订单状态。", json!({"type":"object","properties":{}})),
        tool("mcd_reset_order", "用户明确要求取消当前点餐流程或重新点餐时，清空 EasyInput 中尚未提交的点餐会话。不会取消已经创建的订单。", json!({"type":"object","properties":{}})),
    ]
}

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({"type":"function","name":name,"description":description,"parameters":parameters})
}

pub async fn execute(
    app: AppHandle,
    session_id: &str,
    name: &str,
    arguments: &str,
    latest_user_text: &str,
) -> Result<Value, String> {
    if name == "mcd_reset_order" {
        diagnostics::realtime(
            Some(session_id),
            "mcd.session.reset",
            json!({"reason":"voice_tool_requested"}),
        );
        app.state::<AppState>().mcd_orders.reset(session_id);
        return Ok(json!({"ok":true,"message":"当前点餐会话已清空，可以重新开始"}));
    }
    let args: Value = serde_json::from_str(arguments)
        .map_err(|error| format!("点餐工具参数不是有效 JSON：{error}"))?;
    let state = app.state::<AppState>();
    let config = state.storage.read_config()?.mcd_mcp;
    if !config.enabled || !config.token_saved {
        return Err("请先在“点餐”页面启用麦当劳 MCP 并保存 Token".into());
    }
    let token = crate::read_mcd_token(state.inner()).await?;
    let holder = state.mcd_orders.session(session_id)?;
    let mut order = holder.lock().await;
    diagnostics::realtime(
        Some(session_id),
        "mcd.workflow.step_started",
        json!({"tool":name,"hasAddress":order.address.is_some(),"hasStore":order.store.is_some(),"menuCount":order.meals.len(),"candidateCount":order.candidates.len(),"cartItemCount":order.cart.len(),"hasQuote":order.quote.is_some(),"orderSubmissionAttempted":order.order_submission_attempted,"hasOrder":order.order_id.is_some()}),
    );
    if order.client.is_none() {
        let mut client = mcd_mcp::Client::new(&config, &token)?;
        diagnostics::realtime(
            Some(session_id),
            "mcd.session.initializing",
            json!({"endpoint":config.endpoint,"protocolVersion":config.protocol_version}),
        );
        let started = Instant::now();
        let initialized = match client.initialize().await {
            Ok(value) => value,
            Err(error) => {
                diagnostics::realtime(
                    Some(session_id),
                    "mcd.session.initialize_failed",
                    json!({"durationMs":started.elapsed().as_millis(),"message":error}),
                );
                return Err(error);
            }
        };
        let session_id_assigned = client.has_session_id();
        diagnostics::realtime(
            Some(session_id),
            "mcd.session.initialized",
            json!({"durationMs":started.elapsed().as_millis(),"protocolVersion":initialized.get("protocolVersion"),"serverName":initialized.pointer("/serverInfo/name"),"sessionIdAssigned":session_id_assigned}),
        );
        order.client = Some(client);
    }
    match name {
        "mcd_start_delivery" => start_delivery(&mut order).await,
        "mcd_select_address" => select_address(&mut order, required_choice(&args)?).await,
        "mcd_select_store" => select_store(&mut order, required_choice(&args)?).await,
        "mcd_search_products" => search_products(&mut order, required_text(&args, "query")?).await,
        "mcd_add_simple_item" => {
            add_simple_item(
                &mut order,
                required_choice(&args)?,
                required_quantity(&args)?,
            )
            .await
        }
        "mcd_quote_order" => quote_order(&mut order).await,
        "mcd_submit_order" => submit_order(&mut order, &args, latest_user_text).await,
        "mcd_check_order" => check_order(&mut order).await,
        _ => Err("未知的麦当劳点餐动作".into()),
    }
}

async fn start_delivery(order: &mut OrderSession) -> Result<Value, String> {
    let payload = call(order, "delivery-query-addresses", json!({})).await?;
    order.addresses = find_array(&payload, &["addresses", "addressList"]).unwrap_or_default();
    order.address = None;
    order.store = None;
    order.meals.clear();
    order.candidates.clear();
    order.cart.clear();
    order.quote = None;
    order.quoted_at = None;
    order.order_submission_attempted = false;
    order.order_id = None;
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.addresses.loaded",
        json!({
            "count":order.addresses.len(),
            "options":order.addresses.iter().take(10).enumerate().map(|(index,address)|json!({
                "choice":index+1,
                "addressIdFingerprint":field_string(address,&["addressId","id"]).map(|value|fingerprint(&value)),
                "addressPreview":address_preview(address),
                "fields":object_keys(address)
            })).collect::<Vec<_>>()
        }),
    );
    if order.addresses.is_empty() {
        return Ok(
            json!({"ok":true,"needsUserInput":true,"message":"账号中没有已保存的配送地址。当前 MVP 不通过语音采集手机号和门牌号，请先在麦当劳官方应用添加地址后再说重新开始点餐。"}),
        );
    }
    Ok(
        json!({"ok":true,"message":"请选择配送地址","options":order.addresses.iter().take(5).enumerate().map(|(i,v)|json!({"choice":i+1,"label":address_label(v)})).collect::<Vec<_>>() }),
    )
}

async fn select_address(order: &mut OrderSession, choice: usize) -> Result<Value, String> {
    let address = choose(&order.addresses, choice, "地址")?.clone();
    let address_id = field_string(&address, &["addressId", "id"])
        .ok_or_else(|| "所选地址缺少 addressId".to_string())?;
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.address.selected",
        json!({"choice":choice,"addressIdFingerprint":fingerprint(&address_id),"addressPreview":address_preview(&address),"fields":object_keys(&address)}),
    );
    let payload = call(
        order,
        "delivery-query-stores",
        json!({"beType":BE_TYPE,"addressId":address_id.clone()}),
    )
    .await?;
    order.address = Some(address);
    order.stores = find_array(&payload, &["stores", "storeList"]).unwrap_or_default();
    order.store = None;
    order.meals.clear();
    order.candidates.clear();
    order.cart.clear();
    order.quote = None;
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.stores.loaded",
        json!({
            "addressIdFingerprint":fingerprint(&address_id),
            "count":order.stores.len(),
            "stores":order.stores.iter().take(10).enumerate().map(|(index,store)|json!({
                "choice":index+1,"storeCode":field_string(store,&["storeCode"]),"beCode":field_string(store,&["beCode"]),
                "storeName":field_string(store,&["storeName","name"]),"businessStatus":field_string(store,&["businessStatus"])
            })).collect::<Vec<_>>()
        }),
    );
    if order.stores.is_empty() {
        return Ok(
            json!({"ok":true,"needsUserInput":true,"message":"这个地址当前没有可配送门店，请换一个地址"}),
        );
    }
    Ok(
        json!({"ok":true,"message":"请选择配送门店","options":order.stores.iter().take(5).enumerate().map(|(i,v)|json!({"choice":i+1,"label":store_label(v)})).collect::<Vec<_>>() }),
    )
}

async fn select_store(order: &mut OrderSession, choice: usize) -> Result<Value, String> {
    let store = choose(&order.stores, choice, "门店")?.clone();
    let (store_code, be_code) = store_codes(&store)?;
    let payload = call(order, "query-meals", base_store_args(&store_code, &be_code)).await?;
    order.meals = find_meals(&payload);
    order.store = Some(store.clone());
    order.candidates.clear();
    order.cart.clear();
    order.quote = None;
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.menu.loaded",
        json!({"storeCode":store_code,"beCode":be_code,"mealCount":order.meals.len(),"payload":value_shape(&payload)}),
    );
    if order.meals.is_empty() {
        return Err("门店菜单为空或 MCP 返回结构已变化，请在点餐页重新测试连接".into());
    }
    Ok(
        json!({"ok":true,"message":format!("已选择{}并加载 {} 个餐品，请问想吃什么？",store_label(&store),order.meals.len())}),
    )
}

async fn search_products(order: &mut OrderSession, query: String) -> Result<Value, String> {
    if order.store.is_none() || order.meals.is_empty() {
        return Err("请先选择配送地址和门店".into());
    }
    let query = canonical(&query);
    if query.is_empty() {
        return Err("商品关键词不能为空".into());
    }
    let exact_candidates = order
        .meals
        .iter()
        .filter(|meal| {
            canonical(&field_string(meal, &["name", "productName", "mealName"]).unwrap_or_default())
                .contains(&query)
        })
        .take(8)
        .cloned()
        .collect::<Vec<_>>();
    let (candidates, match_type) = if exact_candidates.is_empty() {
        let mut scored = order
            .meals
            .iter()
            .filter_map(|meal| {
                let name = canonical(
                    &field_string(meal, &["name", "productName", "mealName"]).unwrap_or_default(),
                );
                let score = fuzzy_product_score(&query, &name);
                (score > 0).then_some((score, meal.clone()))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.0.cmp(&left.0));
        (
            scored.into_iter().take(5).map(|(_, meal)| meal).collect(),
            "fuzzy",
        )
    } else {
        (exact_candidates, "exact")
    };
    order.candidates = candidates;
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.products.searched",
        json!({
            "query":diagnostics::text_preview(&query),"matchType":match_type,"menuCount":order.meals.len(),"candidateCount":order.candidates.len(),
            "candidates":order.candidates.iter().take(8).map(|meal|json!({
                "code":field_string(meal,&["code","productCode"]),
                "name":field_string(meal,&["name","productName","mealName"]),
                "currentPriceRaw":field_string(meal,&["currentPrice","price"]),
                "priceSourceUnit":"yuan",
                "label":meal_label(meal)
            })).collect::<Vec<_>>()
        }),
    );
    if order.candidates.is_empty() {
        return Ok(
            json!({"ok":true,"needsUserInput":true,"message":"当前门店菜单中没有找到匹配餐品，请换个名称"}),
        );
    }
    let message = if match_type == "exact" {
        "请选择餐品"
    } else {
        "没有找到名称完全匹配的餐品，以下是相近餐品；请选择序号，或换一个更准确的商品名称"
    };
    Ok(
        json!({"ok":true,"matchType":match_type,"message":message,"options":order.candidates.iter().take(5).enumerate().map(|(i,v)|json!({"choice":i+1,"label":meal_label(v)})).collect::<Vec<_>>() }),
    )
}

async fn add_simple_item(
    order: &mut OrderSession,
    choice: usize,
    quantity: u64,
) -> Result<Value, String> {
    let meal = choose(&order.candidates, choice, "餐品")?.clone();
    let code = field_string(&meal, &["code", "productCode"])
        .ok_or_else(|| "所选餐品缺少商品编码".to_string())?;
    let name =
        field_string(&meal, &["name", "productName", "mealName"]).unwrap_or_else(|| "餐品".into());
    let store = order
        .store
        .clone()
        .ok_or_else(|| "请先选择门店".to_string())?;
    let (store_code, be_code) = store_codes(&store)?;
    let mut args = base_store_args(&store_code, &be_code);
    args["code"] = Value::String(code.clone());
    let detail = call(order, "query-meal-detail", args).await?;
    let support_modify =
        find_value(&detail, &["supportModify"]).and_then(Value::as_bool) == Some(true);
    let round_count = find_array(&detail, &["rounds"])
        .map(|rounds| rounds.len())
        .unwrap_or(0);
    let modification_group_count = count_selection_groups(&detail, "minValues", "values");
    let unresolved_required_selections = count_unresolved_required_selections(&detail);
    let uses_default_configuration =
        support_modify || round_count > 0 || modification_group_count > 0;
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.product.detail_loaded",
        json!({
            "productCode":code,
            "name":name,
            "supportModify":support_modify,
            "roundCount":round_count,
            "modificationGroupCount":modification_group_count,
            "unresolvedRequiredSelectionCount":unresolved_required_selections,
            "usesDefaultConfiguration":uses_default_configuration
        }),
    );
    if unresolved_required_selections > 0 {
        return Ok(
            json!({"ok":true,"needsUserInput":true,"message":format!("{name}存在没有默认值的必选规格，请换一个餐品；当前 MVP 暂不支持逐项配置。"),"unsupported":"requiredCustomization","unresolvedRequiredSelectionCount":unresolved_required_selections}),
        );
    }
    if let Some(existing) = order.cart.iter_mut().find(|item| item.code == code) {
        existing.quantity = (existing.quantity + quantity).min(20);
    } else {
        order.cart.push(CartItem {
            code,
            name: name.clone(),
            quantity,
        });
    }
    order.quote = None;
    order.quoted_at = None;
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.cart.item_added",
        json!({"productCode":order.cart.iter().find(|item|item.name==name).map(|item|item.code.clone()),"name":name,"quantity":quantity,"cartItemCount":order.cart.len(),"usesDefaultConfiguration":uses_default_configuration}),
    );
    let message = if uses_default_configuration {
        format!("已按默认配置加入{name} × {quantity}")
    } else {
        format!("已加入{name} × {quantity}")
    };
    Ok(
        json!({"ok":true,"message":message,"usesDefaultConfiguration":uses_default_configuration,"cart":cart_summary(&order.cart)}),
    )
}

async fn quote_order(order: &mut OrderSession) -> Result<Value, String> {
    if order.cart.is_empty() {
        return Err("购物车是空的，请先选择餐品".into());
    }
    let store = order
        .store
        .clone()
        .ok_or_else(|| "请先选择门店".to_string())?;
    let (store_code, be_code) = store_codes(&store)?;
    let base = base_store_args(&store_code, &be_code);
    let coupon_payload = call(order, "query-store-coupons", base.clone())
        .await
        .unwrap_or_else(|_| json!({}));
    let mut args = base;
    args["items"] = Value::Array(cart_items(&order.cart));
    let quote = call(order, "calculate-price", args).await?;
    order.quote = Some(quote.clone());
    order.quoted_at = Some(Instant::now());
    let summary = price_summary(&quote);
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.quote.ready",
        json!({"storeCode":store_code,"cart":cart_summary(&order.cart),"price":summary.clone(),"validForSeconds":QUOTE_VALID_FOR.as_secs()}),
    );
    Ok(
        json!({"ok":true,"requiresExplicitConfirmation":true,"message":"价格已锁定 2 分钟。请完整播报订单、费用与地址，并询问：是否确认下单？只有用户明确说‘确认下单’才能继续。","store":store_label(&store),"address":order.address.as_ref().map(address_label),"cart":cart_summary(&order.cart),"price":summary,"availableCouponCount":find_array(&coupon_payload,&["coupons","couponList"]).map(|v|v.len()).unwrap_or(0)}),
    )
}

async fn submit_order(
    order: &mut OrderSession,
    args: &Value,
    latest_user_text: &str,
) -> Result<Value, String> {
    if order.order_id.is_some() {
        return Err("订单已创建，不会重复提交；可查询订单状态".into());
    }
    if order.order_submission_attempted {
        return Err("本会话已经发起过创建订单请求。为避免网络异常导致重复下单，请先到麦当劳官方应用核对，EasyInput 不会自动重试".into());
    }
    if args.get("confirmation").and_then(Value::as_str) != Some("确认下单")
        || !explicit_confirmation(latest_user_text)
    {
        return Err("安全校验未通过：必须由用户在当前一轮亲口明确说“确认下单”".into());
    }
    let quoted_at = order
        .quoted_at
        .ok_or_else(|| "请先重新计算价格并向用户确认".to_string())?;
    if quoted_at.elapsed() > QUOTE_VALID_FOR {
        order.quote = None;
        return Err("报价已超过 2 分钟，请重新算价并再次确认".into());
    }
    let address = order
        .address
        .as_ref()
        .ok_or_else(|| "请先选择地址".to_string())?;
    let address_id = field_string(address, &["addressId", "id"])
        .ok_or_else(|| "地址缺少 addressId".to_string())?;
    let store = order
        .store
        .as_ref()
        .ok_or_else(|| "请先选择门店".to_string())?;
    let (store_code, be_code) = store_codes(store)?;
    let mut create_args = base_store_args(&store_code, &be_code);
    create_args["addressId"] = Value::String(address_id);
    create_args["items"] = Value::Array(cart_items(&order.cart));
    order.order_submission_attempted = true;
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.order.submission_started",
        json!({"addressIdFingerprint":create_args.get("addressId").and_then(Value::as_str).map(fingerprint),"storeCode":store_code,"beCode":be_code,"items":cart_items(&order.cart),"confirmationMatchedCurrentAsr":true}),
    );
    let created = call(order, "create-order", create_args).await?;
    let order_id = find_string(&created, &["orderId"]).ok_or_else(|| "订单已调用创建，但响应缺少 orderId；为防止重复下单，本会话不会自动重试，请到麦当劳官方应用核对".to_string())?;
    order.order_id = Some(order_id.clone());
    let pay_url = find_string(&created, &["payH5Url"])
        .ok_or_else(|| "订单已创建但响应缺少支付链接，请到麦当劳官方应用支付".to_string())?;
    let launch = payment_mvp::launch(&pay_url)?;
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.order.created",
        json!({"orderId":mask_id(&order_id),"paymentHost":launch.payment_host.clone(),"looksLikeMcdDomain":launch.looks_like_mcd_domain,"qrOpened":true,"status":find_string(&created,&["orderStatus","status"]),"expirePayTime":find_string(&created,&["expirePayTime"])}),
    );
    Ok(
        json!({"ok":true,"message":"订单已创建，浏览器已打开官方支付二维码。请用户自行扫码支付；EasyInput 不会代扣款。","orderId":mask_id(&order_id),"status":find_string(&created,&["orderStatus","status"]),"amount":find_number(&created,&["realTotalAmount","price"]).map(cents_to_yuan),"expirePayTime":find_string(&created,&["expirePayTime"]),"paymentHost":launch.payment_host,"qrOpened":true}),
    )
}

async fn check_order(order: &mut OrderSession) -> Result<Value, String> {
    let order_id = order
        .order_id
        .clone()
        .ok_or_else(|| "当前会话还没有创建订单".to_string())?;
    let payload = call(order, "query-order", json!({"orderId":order_id})).await?;
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.order.queried",
        json!({"orderId":mask_id(&order_id),"status":find_string(&payload,&["orderStatus","status"]),"payload":value_shape(&payload)}),
    );
    Ok(
        json!({"ok":true,"message":"已查询最新订单状态","orderId":mask_id(&order_id),"status":find_string(&payload,&["orderStatus","status"]),"amount":find_number(&payload,&["realTotalAmount","price"]).map(cents_to_yuan)}),
    )
}

async fn call(order: &mut OrderSession, name: &str, args: Value) -> Result<Value, String> {
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.mcp.request",
        json!({"tool":name,"arguments":safe_tool_arguments(name,&args)}),
    );
    let started = Instant::now();
    let result = match order
        .client
        .as_mut()
        .ok_or_else(|| "MCP 会话尚未初始化".to_string())?
        .call_tool(name, args)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            diagnostics::realtime(
                Some(&order.diagnostic_session_id),
                "mcd.mcp.failed",
                json!({"tool":name,"durationMs":started.elapsed().as_millis(),"message":error}),
            );
            return Err(error);
        }
    };
    diagnostics::realtime(
        Some(&order.diagnostic_session_id),
        "mcd.mcp.response",
        json!({"tool":name,"durationMs":started.elapsed().as_millis(),"result":mcp_result_summary(&result)}),
    );
    mcd_mcp::tool_payload(&result).map_err(|error| {
        diagnostics::realtime(
            Some(&order.diagnostic_session_id),
            "mcd.mcp.business_error",
            json!({"tool":name,"message":error}),
        );
        error
    })
}

fn safe_tool_arguments(name: &str, args: &Value) -> Value {
    let mut safe = serde_json::Map::new();
    for key in ["beType", "orderType", "storeCode", "beCode", "code"] {
        if let Some(value) = args.get(key) {
            safe.insert(key.to_owned(), value.clone());
        }
    }
    if let Some(address_id) = args.get("addressId").and_then(Value::as_str) {
        safe.insert(
            "addressIdFingerprint".into(),
            Value::String(fingerprint(address_id)),
        );
    }
    if let Some(order_id) = args.get("orderId").and_then(Value::as_str) {
        safe.insert("orderId".into(), Value::String(mask_id(order_id)));
    }
    if let Some(items) = args.get("items").and_then(Value::as_array) {
        safe.insert(
            "items".into(),
            Value::Array(
                items
                    .iter()
                    .map(|item| {
                        json!({
                            "productCode":item.get("productCode"),
                            "quantity":item.get("quantity")
                        })
                    })
                    .collect(),
            ),
        );
    }
    safe.insert("tool".into(), Value::String(name.to_owned()));
    Value::Object(safe)
}

fn mcp_result_summary(result: &Value) -> Value {
    let structured = result.get("structuredContent");
    let data = structured
        .and_then(|value| value.get("data"))
        .or_else(|| result.get("data"));
    json!({
        "isError":result.get("isError"),
        "success":structured.and_then(|value|value.get("success")),
        "code":structured.and_then(|value|value.get("code")),
        "message":structured.and_then(|value|value.get("message")).and_then(Value::as_str).map(diagnostics::text_preview),
        "data":data.map(value_shape),
        "resultFields":object_keys(result)
    })
}

fn value_shape(value: &Value) -> Value {
    match value {
        Value::Array(items) => json!({
            "type":"array","length":items.len(),"firstItemFields":items.first().map(object_keys)
        }),
        Value::Object(map) => json!({"type":"object","fields":map.keys().collect::<Vec<_>>() }),
        Value::Null => json!({"type":"null"}),
        Value::Bool(_) => json!({"type":"boolean"}),
        Value::Number(_) => json!({"type":"number"}),
        Value::String(value) => json!({"type":"string","characters":value.chars().count()}),
    }
}

fn object_keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

fn fingerprint(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
        .chars()
        .take(12)
        .collect()
}

fn address_preview(value: &Value) -> String {
    let address = field_string(value, &["fullAddress", "address", "addressName", "name"])
        .unwrap_or_else(|| "未识别地址".into());
    let chars = address.chars().collect::<Vec<_>>();
    if chars.len() <= 12 {
        return address;
    }
    format!(
        "{}…{}",
        chars[..8].iter().collect::<String>(),
        chars[chars.len() - 4..].iter().collect::<String>()
    )
}

fn required_choice(args: &Value) -> Result<usize, String> {
    args.get("choice")
        .and_then(Value::as_u64)
        .filter(|v| *v > 0)
        .map(|v| v as usize)
        .ok_or_else(|| "choice 必须是从 1 开始的序号".into())
}
fn required_quantity(args: &Value) -> Result<u64, String> {
    args.get("quantity")
        .and_then(Value::as_u64)
        .filter(|v| (1..=20).contains(v))
        .ok_or_else(|| "quantity 必须在 1 到 20 之间".into())
}
fn required_text(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("缺少参数 {key}"))
}
fn choose<'a>(values: &'a [Value], choice: usize, kind: &str) -> Result<&'a Value, String> {
    values
        .get(choice.saturating_sub(1))
        .ok_or_else(|| format!("{kind}序号无效，请使用刚才返回的序号"))
}

fn base_store_args(store_code: &str, be_code: &str) -> Value {
    json!({"storeCode":store_code,"beCode":be_code,"orderType":ORDER_TYPE,"beType":BE_TYPE})
}
fn store_codes(store: &Value) -> Result<(String, String), String> {
    Ok((
        field_string(store, &["storeCode"]).ok_or_else(|| "门店缺少 storeCode".to_string())?,
        field_string(store, &["beCode"]).ok_or_else(|| "门店缺少 beCode".to_string())?,
    ))
}
fn cart_items(cart: &[CartItem]) -> Vec<Value> {
    cart.iter()
        .map(|item| json!({"productCode":item.code,"quantity":item.quantity}))
        .collect()
}
fn cart_summary(cart: &[CartItem]) -> Vec<Value> {
    cart.iter()
        .map(|item| json!({"name":item.name,"quantity":item.quantity}))
        .collect()
}

fn find_value<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key) {
                    return Some(value);
                }
            }
            map.values().find_map(|value| find_value(value, keys))
        }
        Value::Array(items) => items.iter().find_map(|value| find_value(value, keys)),
        _ => None,
    }
}
fn find_array(value: &Value, keys: &[&str]) -> Option<Vec<Value>> {
    if let Value::Array(items) = value {
        return Some(items.clone());
    }
    find_value(value, keys).and_then(Value::as_array).cloned()
}

fn find_meals(value: &Value) -> Vec<Value> {
    let Some(meals) = find_value(value, &["meals", "mealList", "products"]) else {
        return if let Value::Array(items) = value {
            items.clone()
        } else {
            Vec::new()
        };
    };
    match meals {
        Value::Array(items) => items.clone(),
        Value::Object(items) => items
            .iter()
            .filter_map(|(code, meal)| {
                let mut meal = meal.as_object()?.clone();
                meal.entry("code".to_owned())
                    .or_insert_with(|| Value::String(code.clone()));
                Some(Value::Object(meal))
            })
            .collect(),
        _ => Vec::new(),
    }
}
fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
    find_value(value, keys).and_then(|v| {
        v.as_str()
            .map(str::to_owned)
            .or_else(|| v.as_u64().map(|n| n.to_string()))
    })
}
fn find_number(value: &Value, keys: &[&str]) -> Option<f64> {
    find_value(value, keys).and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
}
fn field_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| value.get(*key)).and_then(|v| {
        v.as_str()
            .map(str::to_owned)
            .or_else(|| v.as_u64().map(|n| n.to_string()))
    })
}

fn address_label(value: &Value) -> String {
    for key in ["districtName", "district", "countyName", "county"] {
        if let Some(region) = value.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(label) = district_only(region) {
                return label;
            }
        }
    }

    let address = field_string(value, &["fullAddress", "address", "addressName", "name"])
        .unwrap_or_default();
    district_only(&address).unwrap_or_else(|| "已保存地址".into())
}

fn district_only(address: &str) -> Option<String> {
    let district_end = ["区", "县"]
        .iter()
        .filter_map(|suffix| address.find(suffix).map(|index| index + suffix.len()))
        .min();
    let end = district_end?;

    let prefix = &address[..end];
    let start = prefix.rfind('市').map(|index| index + '市'.len_utf8()).unwrap_or(0);
    let district = prefix[start..].trim();
    (!district.is_empty()).then(|| district.to_string())
}
fn store_label(value: &Value) -> String {
    let name = field_string(value, &["storeName", "name"]).unwrap_or_else(|| "麦当劳门店".into());
    let status = field_string(value, &["businessStatus"]).unwrap_or_default();
    if status.is_empty() {
        name
    } else {
        format!("{name}（{status}）")
    }
}
fn meal_label(value: &Value) -> String {
    let name =
        field_string(value, &["name", "productName", "mealName"]).unwrap_or_else(|| "餐品".into());
    match field_string(value, &["currentPrice", "price"]).and_then(|v| v.parse::<f64>().ok()) {
        // query-meals returns decimal yuan strings (for example "11.5").
        Some(price) => format!("{name}，{}", format_yuan(price)),
        None => name,
    }
}
fn price_summary(value: &Value) -> Value {
    json!({
        // calculate-price and order APIs return integer cents.
        "products":find_number(value,&["productPrice"]).map(cents_to_yuan),
        "delivery":find_number(value,&["deliveryPrice"]).map(cents_to_yuan),
        "packing":find_number(value,&["packingPrice"]).map(cents_to_yuan),
        "discount":find_number(value,&["discount"]).map(cents_to_yuan),
        "total":find_number(value,&["price","realTotalAmount"]).map(cents_to_yuan)
    })
}
fn cents_to_yuan(cents: f64) -> String {
    format_yuan(cents / 100.0)
}
fn format_yuan(yuan: f64) -> String {
    format!("¥{yuan:.2}")
}
fn count_selection_groups(value: &Value, minimum_key: &str, options_key: &str) -> usize {
    match value {
        Value::Object(map) => {
            let current = usize::from(
                map.contains_key(minimum_key)
                    && map.get(options_key).and_then(Value::as_array).is_some(),
            );
            current
                + map
                    .values()
                    .map(|child| count_selection_groups(child, minimum_key, options_key))
                    .sum::<usize>()
        }
        Value::Array(items) => items
            .iter()
            .map(|child| count_selection_groups(child, minimum_key, options_key))
            .sum(),
        _ => 0,
    }
}
fn count_unresolved_required_selections(value: &Value) -> usize {
    match value {
        Value::Object(map) => {
            let unresolved_modification = selection_group_is_unresolved(
                map.get("minValues"),
                map.get("values"),
                "selectedQuantity",
            );
            let unresolved_round = selection_group_is_unresolved(
                map.get("minQuantity"),
                map.get("choices"),
                "quantity",
            );
            usize::from(unresolved_modification || unresolved_round)
                + map
                    .values()
                    .map(count_unresolved_required_selections)
                    .sum::<usize>()
        }
        Value::Array(items) => items.iter().map(count_unresolved_required_selections).sum(),
        _ => 0,
    }
}
fn selection_group_is_unresolved(
    minimum: Option<&Value>,
    options: Option<&Value>,
    selected_quantity_key: &str,
) -> bool {
    let Some(minimum) = minimum.and_then(json_u64) else {
        return false;
    };
    let Some(options) = options.and_then(Value::as_array) else {
        return false;
    };
    let selected = options
        .iter()
        .filter_map(|option| option.get(selected_quantity_key).and_then(json_u64))
        .sum::<u64>();
    selected < minimum
}
fn json_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
}
fn fuzzy_product_score(query: &str, name: &str) -> usize {
    let mut seen = HashSet::new();
    let query_chars = query
        .chars()
        .filter(|character| seen.insert(*character))
        .collect::<Vec<_>>();
    if query_chars.len() < 2 {
        return 0;
    }
    let matched = query_chars
        .iter()
        .filter(|character| name.contains(**character))
        .count();
    let required = 2.max(query_chars.len().div_ceil(2));
    if matched < required {
        return 0;
    }
    let adjacent_matches = query_chars
        .windows(2)
        .filter(|pair| name.contains(&pair.iter().collect::<String>()))
        .count();
    matched * 10 + adjacent_matches * 4
}
fn canonical(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && !"，。！？、,.!?".contains(*ch))
        .collect::<String>()
        .to_lowercase()
}
fn explicit_confirmation(text: &str) -> bool {
    let text = canonical(text);
    text.contains("确认下单")
        && !["不确认下单", "别确认下单", "不要确认下单", "取消"]
            .iter()
            .any(|term| text.contains(term))
}
fn mask_id(value: &str) -> String {
    if value.chars().count() <= 8 {
        value.into()
    } else {
        let chars = value.chars().collect::<Vec<_>>();
        format!(
            "{}…{}",
            chars[..4].iter().collect::<String>(),
            chars[chars.len() - 4..].iter().collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_rejects_negation() {
        assert!(explicit_confirmation("好的，确认下单"));
        assert!(!explicit_confirmation("不要确认下单"));
        assert!(!explicit_confirmation("可以了"));
    }
    #[test]
    fn only_exposes_indexed_candidates() {
        let defs = tool_definitions();
        assert!(defs.iter().any(|v| v["name"] == "mcd_submit_order"));
        assert!(defs
            .iter()
            .all(|v| !v.to_string().contains("addressId\":{\"type")));
    }
    #[test]
    fn recursively_extracts_business_fields() {
        let value = json!({"data":{"orderDetail":{"orderId":"123","price":2230}}});
        assert_eq!(find_string(&value, &["orderId"]).as_deref(), Some("123"));
        assert_eq!(price_summary(&value)["total"], "¥22.30");
    }
    #[test]
    fn accepts_real_mcd_root_store_array() {
        let payload = json!([{"storeCode":"1001","beCode":"2001","storeName":"测试门店"}]);
        let stores = find_array(&payload, &["stores", "storeList"]).unwrap();
        assert_eq!(stores.len(), 1);
        assert_eq!(stores[0]["storeCode"], "1001");
    }
    #[test]
    fn normalizes_real_mcd_meal_map_and_full_address() {
        let payload = json!({"meals":{"520688":{"name":"薯条","currentPrice":"15.5"}}});
        let meals = find_meals(&payload);
        assert_eq!(meals.len(), 1);
        assert_eq!(meals[0]["code"], "520688");
        assert_eq!(meal_label(&meals[0]), "薯条，¥15.50");
        assert_eq!(
            address_label(&json!({"fullAddress":"杭州市测试区测试路1号"})),
            "测试区"
        );
    }
    #[test]
    fn fuzzy_search_requires_more_than_one_shared_character() {
        assert!(fuzzy_product_score("鸡腿堡", "东南亚风味椰香鸡扒堡") > 0);
        assert_eq!(fuzzy_product_score("鸡腿堡", "北非蛋风味脆鸡排麦满分"), 0);
        assert_eq!(fuzzy_product_score("鸡腿堡", "猪柳炒双蛋堡"), 0);
        assert_eq!(fuzzy_product_score("鸡腿堡", "火腿扒麦满分"), 0);
    }
    #[test]
    fn accepts_server_defaults_for_optional_modifications_and_meal_rounds() {
        let simple = json!({
            "supportModify":true,
            "rounds":[],
            "modification":{"items":[{"minValues":0,"values":[{"selectedQuantity":1}]}]}
        });
        assert_eq!(count_unresolved_required_selections(&simple), 0);

        let meal = json!({"rounds":[{
            "minQuantity":1,
            "choices":[{"quantity":1,"modification":{"items":[{
                "minValues":1,"values":[{"selectedQuantity":1},{"selectedQuantity":0}]
            }]}}]
        }]});
        assert_eq!(count_unresolved_required_selections(&meal), 0);
        assert_eq!(count_selection_groups(&meal, "minValues", "values"), 1);
    }
    #[test]
    fn blocks_only_required_groups_without_a_default() {
        let detail = json!({"rounds":[{
            "minQuantity":1,
            "choices":[{"quantity":0},{"quantity":0}]
        }]});
        assert_eq!(count_unresolved_required_selections(&detail), 1);
    }
    #[test]
    fn diagnostic_arguments_redact_sensitive_ids() {
        let safe = safe_tool_arguments(
            "delivery-query-stores",
            &json!({"beType":2,"addressId":"secret-address-id"}),
        );
        assert!(safe.get("addressId").is_none());
        assert_eq!(safe["addressIdFingerprint"].as_str().unwrap().len(), 12);
        assert!(!safe.to_string().contains("secret-address-id"));
    }
    #[test]
    fn diagnostic_response_summary_never_logs_business_payload_values() {
        let result = json!({"isError":false,"structuredContent":{"success":true,"code":200,"message":"请求成功","data":{"payH5Url":"https://secret.example/pay","addressId":"secret-address-id"}}});
        let summary = mcp_result_summary(&result).to_string();
        assert!(summary.contains("payH5Url"));
        assert!(!summary.contains("secret.example"));
        assert!(!summary.contains("secret-address-id"));
    }
}

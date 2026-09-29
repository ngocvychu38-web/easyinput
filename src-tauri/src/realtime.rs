use crate::model::{
    KeyboardActionKind, OperationResult, RealtimeCallPhase, RealtimeCallState, RealtimeVoiceConfig,
    VoiceActionMapping,
};
use crate::protocol::audio::{self, ControlAction};
use crate::{ark, diagnostics, input, mcd_order};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::{future::join_all, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    future::pending,
    net::SocketAddr,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Listener, Manager};
use tokio::{
    net::UdpSocket,
    sync::{mpsc, oneshot},
};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, http::HeaderValue, Error as WebSocketError, Message,
};

pub const ENDPOINT: &str = "wss://openspeech.bytedance.com/api/v3/duplex/realtime/dialogue";
pub const API_KEY_ACCOUNT: &str = "volcengine.realtime-voice.api-key.v1";
const MODEL: &str = "1.2.6.1";
const UDP_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(7);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);
const SPEAKER_FRAME_BYTES: usize = 960;
const SPEAKER_FRAME_DURATION: Duration = Duration::from_millis(20);
// The board normally uploads one 20 ms frame at a time. If that stream pauses,
// explicitly mute the cloud input so the service does not treat the gap as a
// dead audio source. The next board frame resumes the cloud input first.
const INPUT_AUDIO_MUTE_AFTER: Duration = Duration::from_secs(1);

type RealtimeSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest {
    pub latency_ms: u128,
    pub endpoint: String,
    pub model: String,
    pub log_id: Option<String>,
}

#[derive(Debug)]
pub enum RealtimeCommand {
    Stop,
    Interrupt,
    WakeWordMode(bool, oneshot::Sender<Result<(), String>>),
    Learning(LearningAction, oneshot::Sender<Result<(), String>>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LearningAction {
    Context { instructions: String },
    Text { text: String },
    Speak { text: String },
    Mute { muted: bool },
    Sound { enabled: bool },
    Interrupt,
    Stop,
}
impl LearningAction {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Context { instructions } if instructions.trim().is_empty() || instructions.len() > 16_000 => Err("学习提示词必须为 1–16000 字节".into()),
            Self::Text { text } | Self::Speak { text } if text.trim().is_empty() || text.len() > 4_000 => Err("学习文本必须为 1–4000 字节".into()),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningStart {
    pub owner_id: String,
    pub instructions: String,
    pub greeting: String,
}
impl LearningStart {
    pub fn validate(&self) -> Result<(), String> {
        uuid::Uuid::parse_str(&self.owner_id).map_err(|_| "无效学习会话标识")?;
        LearningAction::Context { instructions: self.instructions.clone() }.validate()?;
        if self.greeting.len() > 4_000 { return Err("学习开场文本过长".into()); }
        Ok(())
    }
}

fn learning_session_event(config: &RealtimeVoiceConfig, session_id: &str) -> serde_json::Value {
    let mut event = session_create_event(config, session_id, &[], &[], false);
    event["session"]["instructions"] = serde_json::json!(config.instructions);
    event["session"]["tools"] = serde_json::json!([]);
    event["extension"]["asr"]["extra"] = serde_json::json!({});
    event
}

pub fn validate(config: &RealtimeVoiceConfig) -> Result<(), String> {
    if config.endpoint != ENDPOINT {
        return Err("为避免 API Key 泄露，实时语音服务地址必须使用豆包官方 WSS 地址".into());
    }
    if config.model != MODEL {
        return Err("豆包实时语音 3.0 全双工模型固定为 1.2.6.1".into());
    }
    if config.voice.trim().is_empty() {
        return Err("请填写实时语音音色 ID".into());
    }
    if !(-50..=100).contains(&config.speed) {
        return Err("语速必须在 -50 到 100 之间".into());
    }
    if !(-50..=100).contains(&config.loudness) {
        return Err("音量必须在 -50 到 100 之间".into());
    }
    if config.instructions.as_bytes().len() > 16_000 {
        return Err("系统提示词过长".into());
    }
    Ok(())
}

fn authorized_request(
    config: &RealtimeVoiceConfig,
    api_key: &str,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, String> {
    let api_key = api_key.trim();
    if api_key.to_ascii_lowercase().starts_with("bearer ") {
        return Err("Realtime API Key 请直接填写密钥本身，不要添加 Bearer 前缀".into());
    }
    let mut request = config
        .endpoint
        .clone()
        .into_client_request()
        .map_err(|error| format!("实时语音服务地址无效：{error}"))?;
    request.headers_mut().insert(
        "X-Api-Key",
        HeaderValue::from_str(api_key).map_err(|_| "API Key 包含无效字符")?,
    );
    Ok(request)
}

fn handshake_error(error: WebSocketError) -> String {
    let WebSocketError::Http(response) = error else {
        return format!("豆包实时语音握手失败：{error}");
    };
    let status = response.status();
    let log_id = response
        .headers()
        .get("X-Tt-Logid")
        .and_then(|value| value.to_str().ok());
    let body = response
        .body()
        .as_deref()
        .and_then(|value| std::str::from_utf8(value).ok());
    let server_error = body
        .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
        .and_then(|value| {
            let code = value
                .pointer("/error/code")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let message = value
                .pointer("/error/message")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if message.is_empty() {
                None
            } else if code.is_empty() {
                Some(message.to_owned())
            } else {
                Some(format!("{message}（{code}）"))
            }
        });
    let mut result = format!("豆包实时语音握手失败：{status}");
    if let Some(server_error) = server_error {
        result.push_str(&format!("；服务端：{server_error}"));
    }
    if status == tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED {
        result.push_str("。请填写“新版豆包语音控制台 → API Key 管理”生成的原始 API Key；不能使用语音识别 Access Token、App ID、Resource ID 或火山方舟 API Key");
    }
    if let Some(log_id) = log_id {
        result.push_str(&format!("。LogID：{log_id}"));
    }
    result
}

fn tool_name(mapping: &VoiceActionMapping) -> String {
    format!(
        "voice_action_{}",
        mapping.id.replace('-', "").to_ascii_lowercase()
    )
}

const TOOL_USE_INSTRUCTIONS: &str = "你还可以调用 EasyInput 提供的本机工具。用户当前一句只要明确要求执行工具列表中的动作，就必须立即调用最匹配的工具，不要只用文字回答，也不要因为之前在聊其他话题而忽略。工具在整场会话的任意轮次都可以重复调用。用户说打开、启动、唤起或切换到某个已配置应用时，必须调用对应的打开应用工具。";
const MCD_INSTRUCTIONS: &str = "你可以通过 EasyInput 的麦当劳点餐工具完成麦乐送。点餐是有状态的多轮流程：开始点餐、按序号选地址、按序号选门店、搜索餐品、按序号加入餐品、算价、复述订单与费用、等待确认、创建订单、查询状态。只能使用工具返回的序号，绝不能猜测或要求用户口述内部 ID。配送地址只允许口头播报工具返回的区/县级地址标签用于区分；绝不播报或索要小区、街道、楼栋、房间号、联系人或手机号，即使其他上下文或工具数据中出现这些字段也不得复述。套餐、规格或特制选项已有完整默认值时按默认配置加入；只有必选项缺少默认值时才说明当前 MVP 暂不支持逐项配置。创建订单前必须完整播报最终价格，并且只有用户在当前一轮明确说出“确认下单”时才调用 mcd_submit_order；模糊同意、历史对话中的确认或模型自行推断都不算。支付必须由用户在浏览器二维码页自行完成。";
const COCKPIT_TOOL_NAME: &str = "cockpit_navigate";
const COCKPIT_INSTRUCTIONS: &str = "EasyInput 内置了训练营运营驾驶舱。用户明确要求打开驾驶舱、查看或切换训练营总览、期次对比、作业分析、校友会、优秀学员列表、学员画像、数据质量，或查看第 1 至第 7 期详情时，必须调用 cockpit_navigate，不要只描述操作。用户说优秀学员、优秀学员名单或优秀学员列表时，module 必须选择 outstanding-students。近似说法、同音字和常见语音识别错字应按语义选择最接近的模块；如果用户同时提到多个目标且意图不明确，先询问，不要猜测调用。";
const COCKPIT_HOTWORDS: &[&str] = &[
    "训练营驾驶舱",
    "经营总览",
    "期次对比",
    "作业分析",
    "校友会",
    "优秀学员列表",
    "学员画像",
    "数据质量",
];

fn effective_instructions(
    config: &RealtimeVoiceConfig,
    mappings: &[VoiceActionMapping],
    mcd_enabled: bool,
) -> String {
    let mut instructions = config.instructions.trim().to_owned();
    instructions.push_str(&format!("\n\n{COCKPIT_INSTRUCTIONS}"));
    if mappings.iter().any(|mapping| mapping.enabled) {
        instructions.push_str(&format!("\n\n{TOOL_USE_INSTRUCTIONS}"));
    }
    if mcd_enabled {
        instructions.push_str(&format!("\n\n{MCD_INSTRUCTIONS}"));
    }
    instructions
}

fn voice_action_hotword_context(
    mappings: &[VoiceActionMapping],
    dictionary_hotwords: &[String],
) -> Option<String> {
    let mut seen = HashSet::<String>::new();
    let mut words = Vec::<serde_json::Value>::new();
    let mut add = |raw: &str| {
        let word = raw.trim();
        if !word.is_empty() && word.chars().count() <= 100 && seen.insert(word.to_owned()) {
            words.push(serde_json::json!({"word":word}));
        }
    };
    for word in dictionary_hotwords {
        add(word);
    }
    for word in COCKPIT_HOTWORDS {
        add(word);
    }
    for mapping in mappings.iter().filter(|mapping| mapping.enabled) {
        let name = mapping.name.trim();
        add(name);
        for prefix in [
            "打开已经启动的",
            "打开已启动的",
            "切换到",
            "打开",
            "启动",
            "唤起",
            "进入",
        ] {
            if let Some(alias) = name.strip_prefix(prefix) {
                add(alias);
            }
        }
        add(&mapping.action.label);
    }
    if words.is_empty() {
        None
    } else {
        serde_json::to_string(&serde_json::json!({"hotwords":words})).ok()
    }
}

fn tool_definitions(mappings: &[VoiceActionMapping], mcd_enabled: bool) -> Vec<serde_json::Value> {
    let mut tools = mappings.iter().filter(|mapping| mapping.enabled).map(|mapping| {
        let detail = if mapping.description.trim().is_empty() { mapping.name.trim() } else { mapping.description.trim() };
        let behavior = if matches!(mapping.action.kind, KeyboardActionKind::OpenApp) {
            "当用户说打开、启动、唤起、进入或切换到该应用时必须调用；应用即使已经运行也照常调用"
        } else {
            "当用户明确要求执行该动作时必须调用，不要用文字回答替代执行"
        };
        let description = format!("执行用户在 EasyInput 中配置的本机动作“{}”。触发说明：{}。{}。", mapping.name.trim(), detail, behavior);
        let parameters = if matches!(mapping.action.kind, KeyboardActionKind::EditPtt) {
            serde_json::json!({
                "type": "object",
                "properties": { "instruction": { "type": "string", "description": "用户对当前选中文本的完整编辑要求" } },
                "required": ["instruction"]
            })
        } else {
            serde_json::json!({ "type": "object", "properties": {} })
        };
        serde_json::json!({ "type": "function", "name": tool_name(mapping), "description": description, "parameters": parameters })
    }).collect::<Vec<_>>();
    tools.push(serde_json::json!({
        "type": "function",
        "name": COCKPIT_TOOL_NAME,
        "description": "打开 EasyInput 训练营运营驾驶舱并切换到指定模块。近似说法和常见 ASR 错字可按语义映射；有多个可能目标时不要调用。",
        "parameters": {
            "type": "object",
            "properties": {
                "module": {
                    "type": "string",
                    "enum": ["overview", "cohorts", "assignments", "alumni", "outstanding-students", "learners", "data-quality", "cohort-detail"],
                    "description": "overview=经营总览，cohorts=期次对比，assignments=作业分析，alumni=校友会，outstanding-students=优秀学员列表，learners=学员画像，data-quality=数据质量，cohort-detail=单期详情"
                },
                "cohortId": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 7,
                    "description": "module 为 cohort-detail 时必填，第 1 至第 7 期"
                }
            },
            "required": ["module"]
        }
    }));
    if mcd_enabled {
        tools.extend(mcd_order::tool_definitions());
    }
    tools
}

fn session_create_event(
    config: &RealtimeVoiceConfig,
    session_id: &str,
    mappings: &[VoiceActionMapping],
    dictionary_hotwords: &[String],
    mcd_enabled: bool,
) -> serde_json::Value {
    let mut asr_extra = serde_json::json!({});
    if let Some(context) = voice_action_hotword_context(mappings, dictionary_hotwords) {
        asr_extra["context"] = serde_json::Value::String(context);
    }
    serde_json::json!({
        "type": "session.create",
        "event_id": format!("event-{}", uuid::Uuid::new_v4()),
        "session": {
            "id": session_id,
            "model": config.model,
            "instructions": effective_instructions(config, mappings, mcd_enabled),
            "audio": {
                "input": { "format": { "type": "pcm", "rate": 16000 } },
                "output": {
                    "format": { "type": "pcm_s16le", "rate": 24000 },
                    "voice": config.voice,
                    "speed": config.speed,
                    "loudness": config.loudness
                }
            },
            "tools": tool_definitions(mappings, mcd_enabled)
        },
        "extension": {
            "asr": { "extra": asr_extra },
            "tts": { "extra": {} },
            "dialog": {
                "extra": {
                    "strict_audit": config.strict_audit,
                    "enable_loudness_norm": config.enable_loudness_norm,
                    "enable_user_query_exit": config.enable_user_query_exit,
                    "enable_music": false
                }
            }
        }
    })
}

fn text_message(value: serde_json::Value) -> Result<Message, String> {
    serde_json::to_string(&value)
        .map(|value| Message::Text(value.into()))
        .map_err(|error| error.to_string())
}

fn event_error(value: &serde_json::Value) -> String {
    let code = value
        .pointer("/error/code")
        .or_else(|| value.get("status_code"))
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.as_i64().map(|value| value.to_string()))
        });
    let message = value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .and_then(|value| value.as_str())
        .unwrap_or("未知错误");
    let mut result = match code.as_deref() {
        Some(code) => format!("豆包实时语音错误（{code}）：{message}"),
        None => format!("豆包实时语音错误：{message}"),
    };
    if code.as_deref() == Some("52000033")
        || message.contains("52000033")
        || message.contains("AudioServerNoAudioInputTooLongError")
    {
        result.push_str("。开发板音频上传中断时间过长；请确认开发板仍在线后重新开始通话");
    }
    result
}

fn input_audio_state_event(event_type: &str) -> serde_json::Value {
    serde_json::json!({
        "type": event_type,
        "event_id": format!("event-{}", uuid::Uuid::new_v4())
    })
}

async fn connect_and_create(
    config: &RealtimeVoiceConfig,
    api_key: &str,
    session_id: &str,
    mappings: &[VoiceActionMapping],
    dictionary_hotwords: &[String],
    mcd_enabled: bool,
    learning: bool,
) -> Result<(RealtimeSocket, Option<String>), String> {
    validate(config)?;
    if api_key.trim().is_empty() {
        return Err("实时语音 API Key 为空".into());
    }
    diagnostics::realtime(
        Some(session_id),
        "cloud.connecting",
        serde_json::json!({"model":config.model,"enabledTools":mappings.iter().filter(|mapping|mapping.enabled).count()}),
    );
    let request = authorized_request(config, api_key)?;
    let (mut socket, response) =
        tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request))
            .await
            .map_err(|_| "连接豆包实时语音服务超时（10 秒）".to_string())?
            .map_err(handshake_error)?;
    let log_id = response
        .headers()
        .get("X-Tt-Logid")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    diagnostics::realtime(
        Some(session_id),
        "cloud.connected",
        serde_json::json!({"logId":log_id}),
    );
    let create_event = if learning { learning_session_event(config, session_id) } else { session_create_event(
        config,
        session_id,
        mappings,
        dictionary_hotwords,
        mcd_enabled,
    ) };
    let tool_summary = mappings.iter().filter(|mapping|mapping.enabled).map(|mapping|serde_json::json!({"name":mapping.name,"tool":tool_name(mapping),"kind":mapping.action.kind})).collect::<Vec<_>>();
    diagnostics::realtime(
        Some(session_id),
        "session.create.sent",
        serde_json::json!({
            "instructionCharacters":create_event.pointer("/session/instructions").and_then(|value|value.as_str()).map(|value|value.chars().count()).unwrap_or(0),
            "tools":tool_summary,
            "asrContextPresent":create_event.pointer("/extension/asr/extra/context").is_some()
        }),
    );
    socket
        .send(text_message(create_event)?)
        .await
        .map_err(|error| format!("发送 session.create 失败：{error}"))?;
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    loop {
        let incoming = tokio::time::timeout_at(deadline, socket.next())
            .await
            .map_err(|_| "等待 session.created 超时（10 秒）".to_string())?;
        let message = incoming
            .ok_or_else(|| "豆包实时语音连接在创建会话前关闭".to_string())?
            .map_err(|error| format!("接收 session.created 失败：{error}"))?;
        if let Message::Text(text) = message {
            let event: serde_json::Value = serde_json::from_str(text.as_str())
                .map_err(|error| format!("实时语音响应 JSON 无效：{error}"))?;
            match event.get("type").and_then(|value| value.as_str()) {
                Some("session.created") => {
                    diagnostics::realtime(
                        Some(session_id),
                        "session.created",
                        serde_json::json!({"logId":log_id}),
                    );
                    return Ok((socket, log_id));
                }
                Some("error") => return Err(event_error(&event)),
                _ => {}
            }
        }
    }
}

async fn close_remote_session(socket: &mut RealtimeSocket) {
    let event = serde_json::json!({
        "type": "session.close",
        "event_id": format!("event-{}", uuid::Uuid::new_v4())
    });
    if let Ok(message) = text_message(event) {
        let _ = socket.send(message).await;
    }
    let deadline = tokio::time::Instant::now() + CLOSE_TIMEOUT;
    loop {
        let incoming = match tokio::time::timeout_at(deadline, socket.next()).await {
            Ok(value) => value,
            Err(_) => break,
        };
        match incoming {
            Some(Ok(Message::Text(text))) => {
                if serde_json::from_str::<serde_json::Value>(text.as_str())
                    .ok()
                    .and_then(|event| {
                        event
                            .get("type")
                            .and_then(|value| value.as_str())
                            .map(str::to_owned)
                    })
                    .as_deref()
                    == Some("session.closed")
                {
                    break;
                }
            }
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
            Some(Ok(_)) => {}
        }
    }
    let _ = socket.close(None).await;
}

pub async fn test_connection(
    config: &RealtimeVoiceConfig,
    api_key: &str,
) -> OperationResult<ConnectionTest> {
    let started = Instant::now();
    match connect_and_create(
        config,
        api_key,
        &uuid::Uuid::new_v4().to_string(),
        &[],
        &[],
        false,
        false,
    )
    .await
    {
        Ok((mut socket, log_id)) => {
            close_remote_session(&mut socket).await;
            OperationResult::success(Some(ConnectionTest {
                latency_ms: started.elapsed().as_millis(),
                endpoint: config.endpoint.clone(),
                model: config.model.clone(),
                log_id,
            }))
        }
        Err(error) => OperationResult::failure(error),
    }
}

fn wire_session_id(session_id: &str) -> u64 {
    let parsed = uuid::Uuid::parse_str(session_id).unwrap_or_else(|_| uuid::Uuid::new_v4());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&parsed.as_bytes()[..8]);
    let value = u64::from_le_bytes(bytes);
    if value == 0 {
        1
    } else {
        value
    }
}

fn update_state(app: &AppHandle, update: impl FnOnce(&mut RealtimeCallState)) {
    let state = app.state::<crate::AppState>();
    if let Ok(mut current) = state.realtime_call.lock() {
        update(&mut current);
        let _ = app.emit("realtime-call-state", current.clone());
    };
}

pub(crate) async fn wait_for_keyboard(socket: &UdpSocket) -> Result<SocketAddr, String> {
    let deadline = tokio::time::Instant::now() + UDP_DISCOVERY_TIMEOUT;
    let mut buffer = vec![0u8; 2049];
    loop {
        let (size, peer) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer))
            .await
            .map_err(|_| {
                "7 秒内未收到开发板音频心跳；请确认键盘 Wi-Fi、电脑接收地址和端口已同步".to_string()
            })?
            .map_err(|error| format!("接收开发板心跳失败：{error}"))?;
        if audio::parse_heartbeat(&buffer[..size]).is_ok() {
            return Ok(peer);
        }
    }
}

pub(crate) async fn start_keyboard_stream(
    socket: &UdpSocket,
    peer: SocketAddr,
    session_id: u64,
) -> Result<u32, String> {
    let sequence = 1;
    socket
        .send_to(
            &audio::control_packet(ControlAction::Start, session_id, sequence),
            peer,
        )
        .await
        .map_err(|error| format!("向开发板发送录音启动命令失败：{error}"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut buffer = vec![0u8; 2049];
    loop {
        let (size, source) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer))
            .await
            .map_err(|_| "开发板未确认麦克风启动命令".to_string())?
            .map_err(|error| format!("等待开发板启动确认失败：{error}"))?;
        if source != peer {
            continue;
        }
        if let Ok(ack) = audio::parse_control_ack(&buffer[..size]) {
            if ack.action == ControlAction::Start as u8 && ack.session_id == session_id && ack.sequence == sequence {
                return if ack.status == 0 {
                    Ok(sequence)
                } else {
                    Err(format!("开发板拒绝启动麦克风（状态码 {}）", ack.status))
                };
            }
        }
    }
}

#[derive(Clone, Debug)]
struct FunctionCall {
    call_id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CockpitToolArguments {
    module: String,
    cohort_id: Option<u8>,
}

fn cockpit_command(arguments: &str) -> Result<serde_json::Value, String> {
    let arguments: CockpitToolArguments = serde_json::from_str(arguments)
        .map_err(|error| format!("驾驶舱工具参数不是有效 JSON：{error}"))?;
    let allowed = [
        "overview",
        "cohorts",
        "assignments",
        "alumni",
        "outstanding-students",
        "learners",
        "data-quality",
        "cohort-detail",
    ];
    if !allowed.contains(&arguments.module.as_str()) {
        return Err("驾驶舱模块不在允许列表中".into());
    }
    if arguments.module == "cohort-detail" {
        let cohort_id = arguments
            .cohort_id
            .filter(|value| (1..=7).contains(value))
            .ok_or_else(|| "查看单期详情时 cohortId 必须为 1 至 7".to_string())?;
        return Ok(serde_json::json!({
            "action": "navigate",
            "target": "cohort-detail",
            "parameters": { "cohortId": cohort_id }
        }));
    }
    Ok(serde_json::json!({ "action": "navigate", "target": arguments.module }))
}

async fn execute_cockpit_action(
    app: AppHandle,
    arguments: &str,
) -> Result<serde_json::Value, String> {
    let command = cockpit_command(arguments)?;
    let request_id = uuid::Uuid::new_v4().to_string();
    let response_event = format!("cockpit-command-result:{request_id}");
    let (response_tx, response_rx) = oneshot::channel::<String>();
    let listener_id = app.once(response_event.clone(), move |event| {
        let _ = response_tx.send(event.payload().to_owned());
    });
    if let Err(error) = app.emit(
        "cockpit-command-request",
        serde_json::json!({
            "requestId": request_id,
            "responseEvent": response_event,
            "command": command
        }),
    ) {
        app.unlisten(listener_id);
        return Err(format!("向 EasyInput 页面发送驾驶舱命令失败：{error}"));
    }
    let payload = match tokio::time::timeout(Duration::from_secs(12), response_rx).await {
        Ok(Ok(payload)) => payload,
        Ok(Err(_)) => return Err("驾驶舱页面未能返回命令结果".into()),
        Err(_) => {
            app.unlisten(listener_id);
            return Err("等待驾驶舱页面响应超时；请确认页面可以加载".into());
        }
    };
    let result: serde_json::Value =
        serde_json::from_str(&payload).map_err(|error| format!("驾驶舱返回了无效结果：{error}"))?;
    if result.get("ok").and_then(|value| value.as_bool()) != Some(true) {
        let message = result
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("驾驶舱拒绝了命令");
        return Err(message.to_owned());
    }
    Ok(serde_json::json!({
        "ok": true,
        "message": "驾驶舱页面已打开并完成切换",
        "result": result
    }))
}

fn canonical_voice_command(raw: &str) -> String {
    let mut command = raw
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    const PREFIXES: &[&str] = &[
        "现在请帮我",
        "现在帮我",
        "麻烦帮我",
        "麻烦你帮我",
        "请你帮我",
        "请帮我",
        "帮我",
        "麻烦你",
        "麻烦",
        "请你",
        "请",
        "给我",
        "现在",
        "直接",
        "赶紧",
        "马上",
        "来",
        "再",
    ];
    const SUFFIXES: &[&str] = &[
        "一下可以吗",
        "一下好吗",
        "一下吧",
        "听得到吗",
        "能听到吗",
        "听见了吗",
        "可以吗",
        "好不好",
        "行不行",
        "行吗",
        "好吗",
        "一下",
        "谢谢",
        "吧",
    ];
    loop {
        let mut changed = false;
        for prefix in PREFIXES {
            if let Some(value) = command.strip_prefix(prefix) {
                command = value.to_owned();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    loop {
        let mut changed = false;
        for suffix in SUFFIXES {
            if let Some(value) = command.strip_suffix(suffix) {
                command = value.to_owned();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    command
}

fn direct_action_supported(kind: &KeyboardActionKind) -> bool {
    matches!(
        kind,
        KeyboardActionKind::OpenApp
            | KeyboardActionKind::Hotkey
            | KeyboardActionKind::Copy
            | KeyboardActionKind::Paste
            | KeyboardActionKind::Cut
            | KeyboardActionKind::SelectAll
            | KeyboardActionKind::Undo
            | KeyboardActionKind::Enter
            | KeyboardActionKind::Backspace
            | KeyboardActionKind::FixedText
    )
}

fn direct_action_aliases(mapping: &VoiceActionMapping) -> HashSet<String> {
    let mut aliases = HashSet::new();
    let name = canonical_voice_command(mapping.name.trim());
    if !name.is_empty() {
        aliases.insert(name.clone());
    }
    let label = canonical_voice_command(mapping.action.label.trim());
    if matches!(mapping.action.kind, KeyboardActionKind::OpenApp) {
        const VERBS: &[&str] = &[
            "打开已经启动的",
            "打开已启动的",
            "切换到",
            "打开",
            "启动",
            "唤起",
            "进入",
        ];
        let target = VERBS
            .iter()
            .find_map(|verb| name.strip_prefix(verb))
            .filter(|value| !value.is_empty())
            .unwrap_or(&name);
        for candidate in [target, label.as_str()] {
            if candidate.is_empty() {
                continue;
            }
            for verb in VERBS {
                aliases.insert(canonical_voice_command(&format!("{verb}{candidate}")));
            }
            aliases.insert(canonical_voice_command(&format!("打开一下{candidate}")));
        }
    }
    aliases
}

fn open_app_targets(mapping: &VoiceActionMapping) -> HashSet<String> {
    if !matches!(mapping.action.kind, KeyboardActionKind::OpenApp) {
        return HashSet::new();
    }
    const VERBS: &[&str] = &[
        "打开已经启动的",
        "打开已启动的",
        "切换到",
        "打开",
        "启动",
        "唤起",
        "进入",
    ];
    let name = canonical_voice_command(mapping.name.trim());
    let mut targets = HashSet::new();
    let target = VERBS
        .iter()
        .find_map(|verb| name.strip_prefix(verb))
        .filter(|value| !value.is_empty())
        .unwrap_or(&name);
    if !target.is_empty() {
        targets.insert(target.to_owned());
    }
    let label = canonical_voice_command(mapping.action.label.trim());
    if !label.is_empty() {
        targets.insert(label);
    }
    targets
}

fn explicit_open_target(command: &str) -> Option<String> {
    const VERBS: &[&str] = &[
        "打开已经启动的",
        "打开已启动的",
        "切换到",
        "打开",
        "启动",
        "唤起",
        "进入",
    ];
    let command = canonical_voice_command(command);
    let mut target = VERBS
        .iter()
        .find_map(|verb| command.strip_prefix(verb))?
        .to_owned();
    for filler in ["一下", "下", "个", "已经启动的", "已启动的"] {
        if let Some(value) = target.strip_prefix(filler) {
            target = value.to_owned();
            break;
        }
    }
    for suffix in [
        "听得到吗",
        "能听到吗",
        "听见了吗",
        "可以吗",
        "行不行",
        "行吗",
        "好吗",
        "一下",
        "吧",
    ] {
        if let Some(value) = target.strip_suffix(suffix) {
            target = value.to_owned();
            break;
        }
    }
    (!target.is_empty()).then_some(target)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = vec![left_index + 1];
        for (right_index, right_char) in right.iter().enumerate() {
            current.push(
                (current[right_index] + 1)
                    .min(previous[right_index + 1] + 1)
                    .min(previous[right_index] + usize::from(left_char != *right_char)),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

fn strip_assistant_echo(transcript: &str, assistant: &str) -> (String, bool) {
    let assistant = canonical_voice_command(assistant);
    if assistant.chars().count() < 8 {
        return (transcript.trim().to_owned(), false);
    }
    let mut boundaries = transcript
        .char_indices()
        .filter_map(|(index, character)| {
            matches!(character, '。' | '！' | '？' | '!' | '?' | '\n')
                .then_some(index + character.len_utf8())
        })
        .collect::<Vec<_>>();
    if !boundaries.contains(&transcript.len()) {
        boundaries.push(transcript.len());
    }
    let assistant_length = assistant.chars().count();
    let mut full_matches = Vec::<(usize, usize, usize)>::new();
    let mut partial_matches = Vec::<(usize, usize)>::new();
    for boundary in boundaries {
        let candidate = canonical_voice_command(&transcript[..boundary]);
        let candidate_length = candidate.chars().count();
        if candidate_length < 8 {
            continue;
        }
        let distance = edit_distance(&candidate, &assistant);
        let maximum = candidate_length.max(assistant_length);
        if candidate_length * 10 >= assistant_length * 6 && distance * 100 <= maximum * 22 {
            full_matches.push((
                distance * 100 / maximum.max(1),
                candidate_length.abs_diff(assistant_length),
                boundary,
            ));
        }
        if candidate_length <= assistant_length {
            let assistant_prefix = assistant.chars().take(candidate_length).collect::<String>();
            let prefix_distance = edit_distance(&candidate, &assistant_prefix);
            if prefix_distance * 100 <= candidate_length * 18 {
                partial_matches.push((candidate_length, boundary));
            }
        }
    }
    let boundary = if !full_matches.is_empty() {
        full_matches.sort_by_key(|(ratio, difference, _)| (*ratio, *difference));
        Some(full_matches[0].2)
    } else {
        partial_matches
            .into_iter()
            .max_by_key(|(length, _)| *length)
            .map(|(_, boundary)| boundary)
    };
    match boundary {
        Some(boundary) => (
            transcript[boundary..]
                .trim_matches(|character: char| {
                    character.is_whitespace()
                        || matches!(character, '，' | ',' | '。' | '！' | '？' | '!' | '?')
                })
                .to_owned(),
            true,
        ),
        None => (transcript.trim().to_owned(), false),
    }
}

fn compact_voice_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace() && !character.is_ascii_punctuation() && !"，。！？；：、‘’“”《》（）【】".contains(*character))
        .collect()
}

fn is_rest_request(text: &str) -> bool {
    let compact = compact_voice_text(text);
    ["闭嘴吧", "闭嘴", "别说话", "不要说话", "请你休息一下", "休息一下", "先别说话", "先不要说话", "别回答了", "不要回答了", "安静一下", "暂停回答", "不要出声", "停止回答", "别插话", "不要插话"]
        .iter()
        .any(|phrase| compact.contains(phrase))
}

fn is_direct_subsystem_navigation_command(text: &str) -> bool {
    let targets = ["三阶魔方", "训练营驾驶舱", "麦当劳点餐", "直播工作室", "直播间", "英语学习", "驾驶舱", "麦当劳", "魔方", "直播", "英语", "点餐"];
    let open_verbs = ["打开", "开启", "启动", "进入", "切换到", "去"];
    let close_verbs = ["关闭", "关掉", "退出", "返回", "隐藏"];
    text.split(|character: char| "，。！？；、,.!?;:\n".contains(character)).any(|clause| {
        let compact = compact_voice_text(clause);
        let mut compact = compact;
        loop {
            let next = ["嗯", "啊", "诶", "呃", "那个", "然后", "好的", "好", "请问", "我想", "我说", "就是"]
                .iter().find_map(|prefix| compact.strip_prefix(prefix).filter(|rest| !rest.is_empty()));
            match next { Some(value) => compact = value.to_owned(), None => break }
        }
        let target_matches = |remainder: &str| targets.iter().any(|target| {
            let Some(mut tail) = remainder.strip_prefix(target) else { return false; };
            loop {
                let next = ["吧", "啊", "呀", "呢", "一下", "页面", "模块", "工作室", "功能"]
                    .iter()
                    .find_map(|suffix| tail.strip_prefix(suffix));
                match next { Some(rest) => tail = rest, None => break }
            }
            tail.is_empty()
        });
        if open_verbs.iter().any(|verb| compact.strip_prefix(verb).is_some_and(target_matches)) { return true; }
        if let Some(remainder) = compact.strip_prefix("不要") { if target_matches(remainder) { return true; } }
        close_verbs.iter().any(|verb| {
            compact.strip_prefix(verb).map(|remainder| {
                let remainder = ["一下", "掉", "这个", "当前"].iter().find_map(|modifier| remainder.strip_prefix(modifier)).unwrap_or(remainder);
                target_matches(remainder)
            }).unwrap_or(false)
        })
    })
}

fn is_direct_cube_control_command(text: &str) -> bool {
    let compact = compact_voice_text(text);
    compact.contains("同步重新打乱")
        || compact.contains("重新同步打乱")
        || compact.contains("同步打乱")
        || compact.contains("重新打乱")
        || compact.contains("同步打散")
        || compact.contains("打乱魔方")
        || compact.contains("魔方打乱")
        || compact.contains("一键完成七步")
        || compact.contains("一键完成7步")
        || compact.contains("一键完成魔方七步")
        || compact.contains("一起开始复原魔方")
        || compact.contains("一起复原魔方")
}

fn is_direct_live_sticker_command(text: &str) -> bool {
    let compact = compact_voice_text(text);
    if ["取消贴纸", "移除贴纸", "关闭贴纸", "摘掉面具", "去掉胡子", "恢复原样", "不要贴纸", "关闭特效", "关掉特效"].iter().any(|command| compact.contains(command)) { return true; }
    let has_target = ["面具", "面膜", "蓝脸", "脸谱", "胡子", "胡须", "猫咪", "猫猫", "小猫", "猫耳", "圣诞", "帽子和墨镜", "帽子墨镜", "夏日交友", "美颜", "磨皮", "美白", "漂亮一点"]
        .iter().any(|target| compact.contains(target));
    let has_action = ["戴", "带", "加", "变成", "换成", "打开", "来点", "来个", "使用", "用", "取消", "移除", "摘掉", "去掉", "恢复", "不要", "关闭", "关掉"]
        .iter().any(|action| compact.contains(action));
    has_target && (has_action || compact.chars().count() <= 10)
}

fn wake_command_remainder(text: &str) -> Option<String> {
    let aliases = ["八弟八弟", "巴弟巴弟", "八地八地", "巴地巴地", "八帝八帝", "巴帝巴帝", "八弟巴弟", "巴弟八弟", "八地八弟", "八弟八地", "八弟八底", "巴蒂巴迪", "巴迪巴迪", "巴蒂巴蒂", "巴迪巴蒂", "八迪八迪", "八弟巴迪", "巴弟巴迪", "巴帝巴迪", "八滴八滴", "八d八d", "8d8d"];
    let mut compact = String::new();
    let mut byte_ends = Vec::new();
    for (offset, character) in text.char_indices() {
        if !character.is_whitespace() && !character.is_ascii_punctuation() && !"，。！？；：、‘’“”《》（）【】".contains(character) {
            compact.push(character);
            byte_ends.push(offset + character.len_utf8());
        }
    }
    let compact = compact.to_lowercase();
    let end_index = aliases.iter().find_map(|phrase| compact.find(phrase).map(|start| start + phrase.chars().count()))?;
    let end = *byte_ends.get(end_index.checked_sub(1)?)?;
    let mut remainder = text[end..].trim().to_owned();
    while matches!(remainder.chars().next(), Some('，' | ',' | '。' | '.' | '！' | '!' | '？' | '?' | '、' | ':' | '：')) {
        remainder.remove(0);
        remainder = remainder.trim_start().to_owned();
    }
    Some(remainder)
}

fn open_target_match_score(spoken: &str, configured: &str) -> Option<usize> {
    if spoken == configured {
        return Some(0);
    }
    let spoken_length = spoken.chars().count();
    let configured_length = configured.chars().count();
    if spoken.is_ascii() && configured.is_ascii() && spoken_length >= 4 && configured_length >= 4 {
        let distance = edit_distance(spoken, configured);
        let limit = if spoken_length.max(configured_length) <= 6 {
            1
        } else {
            2
        };
        if distance <= limit {
            return Some(distance);
        }
    }
    if !spoken.is_ascii()
        && configured.starts_with(spoken)
        && spoken_length >= 1
        && configured_length.saturating_sub(spoken_length) <= 1
    {
        return Some(2);
    }
    None
}

fn direct_command_segments(transcript: &str) -> Vec<String> {
    let mut segmented = transcript.to_owned();
    for separator in [
        "然后", "接着", "以及", "并且", "，", ",", "、", "；", ";", "。", "\n",
    ] {
        segmented = segmented.replace(separator, "|");
    }
    for verb in ["打开", "启动", "唤起", "进入", "切换到"] {
        segmented = segmented.replace(&format!("和{verb}"), &format!("|{verb}"));
    }
    segmented
        .split('|')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn direct_voice_actions(
    transcript: &str,
    mappings: &[VoiceActionMapping],
) -> Vec<VoiceActionMapping> {
    let candidates = mappings
        .iter()
        .filter(|mapping| mapping.enabled && direct_action_supported(&mapping.action.kind))
        .map(|mapping| (mapping, direct_action_aliases(mapping)))
        .collect::<Vec<_>>();
    let mut commands = vec![canonical_voice_command(transcript)];
    commands.extend(
        direct_command_segments(transcript)
            .into_iter()
            .map(|segment| canonical_voice_command(&segment)),
    );
    commands.retain(|command| !command.is_empty());
    let mut ids = candidates
        .iter()
        .filter(|(_, aliases)| commands.iter().any(|command| aliases.contains(command)))
        .map(|(mapping, _)| mapping.id.clone())
        .collect::<HashSet<_>>();
    for spoken_target in commands
        .iter()
        .filter_map(|command| explicit_open_target(command))
    {
        let mut scored = candidates
            .iter()
            .filter(|(mapping, _)| matches!(mapping.action.kind, KeyboardActionKind::OpenApp))
            .filter_map(|(mapping, _)| {
                open_app_targets(mapping)
                    .iter()
                    .filter_map(|target| open_target_match_score(&spoken_target, target))
                    .min()
                    .map(|score| (score, mapping.id.clone()))
            })
            .collect::<Vec<_>>();
        scored.sort_by_key(|(score, _)| *score);
        if let Some((best_score, best_id)) = scored.first() {
            if scored
                .iter()
                .filter(|(score, _)| score == best_score)
                .count()
                == 1
            {
                ids.insert(best_id.clone());
            }
        }
    }
    mappings
        .iter()
        .filter(|mapping| ids.contains(&mapping.id))
        .cloned()
        .collect()
}

fn function_calls(event: &serde_json::Value) -> Result<Vec<FunctionCall>, String> {
    let items = event
        .get("items")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "Function Calling 响应缺少 items".to_string())?;
    items
        .iter()
        .map(|item| {
            let call_id = item
                .get("call_id")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "Function Calling 项缺少 call_id".to_string())?;
            let name = item
                .get("name")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "Function Calling 项缺少 name".to_string())?;
            let arguments = item
                .get("arguments")
                .and_then(|value| value.as_str())
                .unwrap_or("{}");
            Ok(FunctionCall {
                call_id: call_id.to_owned(),
                name: name.to_owned(),
                arguments: arguments.to_owned(),
            })
        })
        .collect()
}

async fn execute_voice_action(
    app: AppHandle,
    mapping: VoiceActionMapping,
    arguments: &str,
) -> Result<serde_json::Value, String> {
    let arguments: serde_json::Value = serde_json::from_str(arguments)
        .map_err(|error| format!("工具参数不是有效 JSON：{error}"))?;
    let action_name = mapping.name.trim().to_owned();
    match mapping.action.kind {
        KeyboardActionKind::OpenApp => {
            let path = mapping
                .action
                .value
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "没有配置应用路径".to_string())?;
            if !path.to_ascii_lowercase().ends_with(".app") || !std::path::Path::new(&path).is_dir()
            {
                return Err("配置的 macOS 应用已不存在".into());
            }
            tokio::task::spawn_blocking(move || {
                match std::process::Command::new("/usr/bin/open")
                    .arg(path)
                    .status()
                {
                    Ok(status) if status.success() => Ok(()),
                    Ok(status) => Err(format!("打开应用失败：{status}")),
                    Err(error) => Err(format!("无法打开应用：{error}")),
                }
            })
            .await
            .map_err(|error| format!("应用启动任务失败：{error}"))??;
            Ok(serde_json::json!({"ok":true,"action":action_name,"message":"应用已打开"}))
        }
        KeyboardActionKind::Hotkey => {
            let shortcut = mapping
                .action
                .value
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "没有配置快捷键".to_string())?;
            let display = shortcut.clone();
            tokio::task::spawn_blocking(move || input::press_shortcut(&shortcut))
                .await
                .map_err(|error| format!("快捷键任务失败：{error}"))??;
            Ok(
                serde_json::json!({"ok":true,"action":action_name,"shortcut":display,"message":"快捷键已执行"}),
            )
        }
        KeyboardActionKind::Copy
        | KeyboardActionKind::Paste
        | KeyboardActionKind::Cut
        | KeyboardActionKind::SelectAll
        | KeyboardActionKind::Undo
        | KeyboardActionKind::Enter
        | KeyboardActionKind::Backspace => {
            let shortcut = match mapping.action.kind {
                KeyboardActionKind::Copy => "Command+C",
                KeyboardActionKind::Paste => "Command+V",
                KeyboardActionKind::Cut => "Command+X",
                KeyboardActionKind::SelectAll => "Command+A",
                KeyboardActionKind::Undo => "Command+Z",
                KeyboardActionKind::Enter => "Enter",
                KeyboardActionKind::Backspace => "Backspace",
                _ => unreachable!(),
            }
            .to_owned();
            let display = shortcut.clone();
            tokio::task::spawn_blocking(move || input::press_shortcut(&shortcut))
                .await
                .map_err(|error| format!("按键任务失败：{error}"))??;
            Ok(
                serde_json::json!({"ok":true,"action":action_name,"shortcut":display,"message":"按键动作已执行"}),
            )
        }
        KeyboardActionKind::FixedText => {
            let text = mapping
                .action
                .value
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "没有配置固定文字".to_string())?;
            let count = text.chars().count();
            tokio::task::spawn_blocking(move || input::type_text(&text))
                .await
                .map_err(|error| format!("文字输入任务失败：{error}"))??;
            Ok(
                serde_json::json!({"ok":true,"action":action_name,"characters":count,"message":"固定文字已输入"}),
            )
        }
        KeyboardActionKind::EditPtt => {
            let instruction = arguments
                .get("instruction")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "语音编辑缺少 instruction".to_string())?
                .trim()
                .to_owned();
            let selection = tokio::task::spawn_blocking(input::selected_text)
                .await
                .map_err(|error| format!("读取选区任务失败：{error}"))??
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "当前应用没有可编辑的选中文本".to_string())?;
            let state = app.state::<crate::AppState>();
            let persisted = state.storage.read_config()?;
            if !persisted.ark.enabled || !persisted.ark.api_key_saved {
                return Err("请先在语音服务配置中启用火山方舟模型并保存 API Key".into());
            }
            let key = crate::read_ark_api_key(state.inner()).await?;
            let result = ark::answer(&persisted.ark, &key, &instruction, Some(&selection)).await?;
            let replacement = result.clone();
            tokio::task::spawn_blocking(move || input::replace_selected_text(&replacement))
                .await
                .map_err(|error| format!("替换选区任务失败：{error}"))??;
            Ok(
                serde_json::json!({"ok":true,"action":action_name,"message":"语音编辑已完成","result":result}),
            )
        }
        _ => Err("该映射不是允许的语音调用动作".into()),
    }
}

async fn function_result_event(
    app: AppHandle,
    session_id: String,
    mappings: Vec<VoiceActionMapping>,
    calls: Vec<FunctionCall>,
    preexecuted_tools: HashSet<String>,
    latest_user_text: String,
) -> serde_json::Value {
    let results = join_all(calls.into_iter().map(|call| {
        let app = app.clone();
        let session_id = session_id.clone();
        let preexecuted_tools = preexecuted_tools.clone();
        let latest_user_text = latest_user_text.clone();
        let mapping = mappings.iter().find(|mapping| mapping.enabled && tool_name(mapping) == call.name).cloned();
        async move {
            let payload = if call.name == COCKPIT_TOOL_NAME {
                diagnostics::realtime(Some(&session_id), "cockpit_tool.execution_started", serde_json::json!({"callId":&call.call_id,"arguments":diagnostics::text_preview(&call.arguments)}));
                match execute_cockpit_action(app.clone(), &call.arguments).await {
                    Ok(value) => {
                        update_state(&app, |state| state.last_tool_status = Some("驾驶舱已打开并完成页面切换".into()));
                        diagnostics::realtime(Some(&session_id), "cockpit_tool.execution_succeeded", serde_json::json!({"callId":&call.call_id}));
                        value
                    }
                    Err(error) => {
                        update_state(&app, |state| state.last_tool_status = Some(format!("驾驶舱控制失败：{error}")));
                        diagnostics::realtime(Some(&session_id), "cockpit_tool.execution_failed", serde_json::json!({"callId":&call.call_id,"message":&error}));
                        serde_json::json!({"ok":false,"error":{"code":"COCKPIT_CONTROL_FAILED","message":error}})
                    }
                }
            } else if call.name.starts_with(mcd_order::TOOL_PREFIX) {
                diagnostics::realtime(Some(&session_id), "mcd_tool.execution_started", serde_json::json!({"callId":&call.call_id,"tool":&call.name}));
                match mcd_order::execute(app.clone(), &session_id, &call.name, &call.arguments, &latest_user_text).await {
                    Ok(value) => {
                        update_state(&app, |state| state.last_tool_status = Some(format!("点餐步骤完成：{}", call.name)));
                        diagnostics::realtime(Some(&session_id), "mcd_tool.execution_succeeded", serde_json::json!({"callId":&call.call_id,"tool":&call.name}));
                        value
                    }
                    Err(error) => {
                        update_state(&app, |state| state.last_tool_status = Some(format!("点餐步骤失败：{error}")));
                        diagnostics::realtime(Some(&session_id), "mcd_tool.execution_failed", serde_json::json!({"callId":&call.call_id,"tool":&call.name,"message":&error}));
                        serde_json::json!({"ok":false,"error":{"code":"MCD_ORDER_FAILED","message":error}})
                    }
                }
            } else { match mapping {
                Some(mapping) => {
                    let action_name = mapping.name.trim().to_owned();
                    if preexecuted_tools.contains(&call.name) {
                        diagnostics::realtime(Some(&session_id), "tool.duplicate_execution_skipped", serde_json::json!({"callId":&call.call_id,"tool":&call.name,"action":&action_name,"reason":"local_asr_already_executed"}));
                        return serde_json::json!({
                            "call_id": call.call_id,
                            "role": "tool",
                            "content": [{"type":"input_text","text":serde_json::to_string(&serde_json::json!({"ok":true,"action":action_name,"message":"动作已由客户端在本轮执行"})).unwrap()}]
                        });
                    }
                    diagnostics::realtime(Some(&session_id), "tool.execution_started", serde_json::json!({"callId":&call.call_id,"tool":&call.name,"action":&action_name,"kind":format!("{:?}",mapping.action.kind)}));
                    match execute_voice_action(app.clone(), mapping, &call.arguments).await {
                        Ok(value) => {
                            update_state(&app, |state| state.last_tool_status = Some(format!("执行成功：{action_name}")));
                            diagnostics::realtime(Some(&session_id), "tool.execution_succeeded", serde_json::json!({"callId":&call.call_id,"tool":&call.name,"action":&action_name}));
                            value
                        }
                        Err(error) => {
                            update_state(&app, |state| state.last_tool_status = Some(format!("执行失败：{action_name} · {error}")));
                            diagnostics::realtime(Some(&session_id), "tool.execution_failed", serde_json::json!({"callId":&call.call_id,"tool":&call.name,"action":&action_name,"message":&error}));
                            serde_json::json!({"ok":false,"error":{"code":"ACTION_FAILED","message":error}})
                        }
                    }
                },
                None => {
                    update_state(&app, |state| state.last_tool_status = Some("执行失败：豆包返回了未知语音动作".into()));
                    diagnostics::realtime(Some(&session_id), "tool.unknown", serde_json::json!({"callId":&call.call_id,"tool":&call.name}));
                    serde_json::json!({"ok":false,"error":{"code":"UNKNOWN_ACTION","message":"该语音调用不存在、已禁用或配置已变化"}})
                },
            }};
            serde_json::json!({
                "call_id": call.call_id,
                "role": "tool",
                "content": [{"type":"input_text","text":serde_json::to_string(&payload).unwrap_or_else(|_| "{\"ok\":false}".into())}]
            })
        }
    })).await;
    serde_json::json!({"type":"conversation.item.create","event_id":format!("event-{}",uuid::Uuid::new_v4()),"items":results})
}

pub async fn open_session(
    app: AppHandle,
    session_id: String,
    config: RealtimeVoiceConfig,
    api_key: String,
    audio_port: u16,
    voice_actions: Vec<VoiceActionMapping>,
    mcd_enabled: bool,
    learning: bool,
    wake_word_enabled: bool,
) -> Result<(mpsc::UnboundedSender<RealtimeCommand>, oneshot::Sender<()>), String> {
    diagnostics::realtime(
        Some(&session_id),
        "session.start_requested",
        serde_json::json!({"audioPort":audio_port,"configuredMappings":voice_actions.len(),"enabledMappings":voice_actions.iter().filter(|mapping|mapping.enabled).count(),"mcdEnabled":mcd_enabled,"wakeWordEnabled":wake_word_enabled}),
    );
    let udp = UdpSocket::bind(("0.0.0.0", audio_port))
        .await
        .map_err(|error| format!("无法监听开发板音频端口 {audio_port}：{error}"))?;
    let peer = wait_for_keyboard(&udp).await?;
    diagnostics::realtime(
        Some(&session_id),
        "board.heartbeat_received",
        serde_json::json!({"ready":true}),
    );
    let dictionary_hotwords = app
        .state::<crate::AppState>()
        .storage
        .read_dictionary()
        .unwrap_or_default()
        .hotwords;
    let enabled_tool_count = if learning { 0 } else { voice_actions
        .iter()
        .filter(|mapping| mapping.enabled)
        .count()
        + 1
        + if mcd_enabled {
            mcd_order::tool_definitions().len()
        } else {
            0
        } };
    let (mut socket, log_id) = connect_and_create(
        &config,
        &api_key,
        &session_id,
        &voice_actions,
        &dictionary_hotwords,
        mcd_enabled,
        learning,
    )
    .await?;
    let wire_session = wire_session_id(&session_id);
    let mut control_sequence = match start_keyboard_stream(&udp, peer, wire_session).await {
        Ok(sequence) => {
            diagnostics::realtime(
                Some(&session_id),
                "board.start_ack",
                serde_json::json!({"sequence":sequence,"status":0}),
            );
            sequence
        }
        Err(error) => {
            diagnostics::realtime(
                Some(&session_id),
                "board.start_failed",
                serde_json::json!({"message":error}),
            );
            close_remote_session(&mut socket).await;
            return Err(error);
        }
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    update_state(&app, |state| {
        state.phase = RealtimeCallPhase::Listening;
        state.wake_word_enabled = wake_word_enabled;
        state.wake_word_armed = false;
        state.session_id = Some(session_id.clone());
        state.log_id = log_id.clone();
        state.last_tool_status = (enabled_tool_count > 0)
            .then(|| format!("已加载 {enabled_tool_count} 个语音动作，动作名称已加入实时识别热词"));
    });
    let greeting = config.greeting.trim().to_owned();
    let (ready_tx, ready_rx) = oneshot::channel();
    tauri::async_runtime::spawn(async move {
        let _ = ready_rx.await;
        let started = Instant::now();
        let (mut writer, mut reader) = socket.split();
        let (tool_result_tx, mut tool_result_rx) = mpsc::unbounded_channel::<serde_json::Value>();
        let mut handled_call_ids = HashSet::<String>::new();
        if !greeting.is_empty() {
            let _ = writer.send(text_message(serde_json::json!({"type":"speech_text_buffer.commit","event_id":format!("event-{}",uuid::Uuid::new_v4()),"speech_id":uuid::Uuid::new_v4().to_string(),"text":greeting})).unwrap()).await;
        }
        let mut udp_buffer = vec![0u8; 4096];
        let mut speaker_buffer = Vec::<u8>::new();
        let mut speaker_queue = VecDeque::<Vec<u8>>::new();
        let mut speaker_sequence = 0u32;
        let mut speaker_tick = tokio::time::interval(SPEAKER_FRAME_DURATION);
        speaker_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        speaker_tick.tick().await;
        let mut keepalive = tokio::time::interval(Duration::from_secs(1));
        let mut state_tick = tokio::time::interval(Duration::from_millis(250));
        let mut last_input_audio_at = Instant::now();
        let mut cloud_input_muted = false;
        let mut input_muted = false;
        // Milo playback must not come back through the board microphone as a
        // fresh learner turn. Keep the gate closed until the hardware queue drains.
        let mut playback_input_suppressed = false;
        let mut ignore_playback_transcription = false;
        let mut sound_enabled = true;
        let mut pending_context = VecDeque::<oneshot::Sender<Result<(), String>>>::new();
        let mut pending_interrupt = VecDeque::<oneshot::Sender<Result<(), String>>>::new();
        let mut output_cancel_pending = false;
        let mut response_active = !greeting.is_empty();
        let mut assistant_suspended = false;
        let mut wake_word_enabled = wake_word_enabled;
        let mut audio_done = false;
        let mut completed_transcripts = HashSet::<String>::new();
        let mut assistant_turn = 0u64;
        let mut terminal_error: Option<String> = None;
        let mut graceful_close = false;
        let mut close_deadline: Option<tokio::time::Instant> = None;
        let mut user_turn = 0u64;
        let mut tool_calls_this_turn = 0usize;
        let mut response_count = 0u64;
        let mut direct_preexecuted_tools = HashSet::<String>::new();
        let mut suppress_direct_response = false;
        let mut direct_cancel_sent = false;
        let mut last_assistant_text = String::new();
        loop {
            tokio::select! {
                command = rx.recv() => match command {
                    Some(RealtimeCommand::WakeWordMode(enabled, ack)) => {
                        if learning {
                            let _ = ack.send(Err("学习会话不支持实时语音唤醒待命".into()));
                            continue;
                        }
                        wake_word_enabled = enabled;
                        if !enabled { assistant_suspended = false; }
                        update_state(&app, |state| {
                            state.wake_word_enabled = enabled;
                            state.wake_word_armed = enabled && assistant_suspended;
                            state.last_tool_status = Some(if !enabled {
                                "唤醒词待命已关闭，实时语音恢复正常".into()
                            } else if assistant_suspended {
                                "助手休息中，请说“八弟八弟”唤醒".into()
                            } else {
                                "唤醒词待命已开启，说“闭嘴吧”可让助手休息".into()
                            });
                        });
                        diagnostics::realtime(Some(&session_id), "wake_word.mode_changed", serde_json::json!({"enabled":enabled,"suspended":assistant_suspended}));
                        let _ = ack.send(Ok(()));
                    }
                    Some(RealtimeCommand::Learning(action, ack)) => {
                        if !learning || close_deadline.is_some() { let _ = ack.send(Err("学习会话已结束".into())); continue; }
                        let event = match &action {
                            LearningAction::Context { instructions } => {
                                let mut next = config.clone(); next.instructions = instructions.clone();
                                let mut event = learning_session_event(&next, &session_id);
                                event["type"] = serde_json::json!("session.update");
                                event
                            }
                            LearningAction::Text { text } => serde_json::json!({"type":"conversation.item.create","event_id":uuid::Uuid::new_v4().to_string(),"items":[{"id":uuid::Uuid::new_v4().to_string(),"type":"message","role":"user","content":[{"type":"input_text","text":text}]}]}),
                            LearningAction::Speak { text } => {
                                playback_input_suppressed = true;
                                if !cloud_input_muted {
                                    if let Err(error) = writer.send(text_message(input_audio_state_event("input_audio_mute.commit")).unwrap()).await {
                                        let message = format!("暂停朗读期间的豆包音频输入失败：{error}");
                                        let _ = ack.send(Err(message.clone()));
                                        terminal_error = Some(message);
                                        break;
                                    }
                                    cloud_input_muted = true;
                                }
                                response_active = true;
                                serde_json::json!({"type":"speech_text_buffer.commit","event_id":uuid::Uuid::new_v4().to_string(),"speech_id":uuid::Uuid::new_v4().to_string(),"text":text})
                            },
                            LearningAction::Mute { muted } => {
                                input_muted = *muted;
                                cloud_input_muted = *muted;
                                input_audio_state_event(if *muted {"input_audio_mute.commit"} else {"input_audio_unmute.commit"})
                            }
                            LearningAction::Sound { enabled } => {
                                sound_enabled = *enabled;
                                if !enabled { speaker_buffer.clear(); speaker_queue.clear(); }
                                let _ = ack.send(Ok(())); continue;
                            }
                            LearningAction::Interrupt => {
                                playback_input_suppressed = false;
                                ignore_playback_transcription = false;
                                speaker_buffer.clear(); speaker_queue.clear();
                                if !response_active {
                                    update_state(&app, |state| state.phase = RealtimeCallPhase::Listening);
                                    let _ = ack.send(Ok(())); continue;
                                }
                                output_cancel_pending = true;
                                speaker_buffer.clear(); speaker_queue.clear();
                                serde_json::json!({"type":"response.cancel","event_id":uuid::Uuid::new_v4().to_string()})
                            }
                            LearningAction::Stop => {
                                speaker_buffer.clear(); speaker_queue.clear();
                                update_state(&app, |state| state.phase = RealtimeCallPhase::Closing);
                                let _ = udp.send_to(&audio::control_packet(ControlAction::Stop, wire_session, control_sequence.wrapping_add(1)), peer).await;
                                close_deadline = Some(tokio::time::Instant::now() + CLOSE_TIMEOUT);
                                serde_json::json!({"type":"session.close","event_id":uuid::Uuid::new_v4().to_string()})
                            }
                        };
                        match writer.send(text_message(event).unwrap()).await {
                            Ok(_) if matches!(action, LearningAction::Context { .. }) => pending_context.push_back(ack),
                            Ok(_) if matches!(action, LearningAction::Interrupt) => pending_interrupt.push_back(ack),
                            Ok(_) => { let _ = ack.send(Ok(())); }
                            Err(error) => { let message = format!("发送学习请求失败：{error}"); let _ = ack.send(Err(message.clone())); terminal_error = Some(message); break; }
                        }
                    }
                    Some(RealtimeCommand::Interrupt) => {
                        speaker_buffer.clear(); speaker_queue.clear();
                        diagnostics::realtime(Some(&session_id), "response.interrupt_requested", serde_json::json!({"turn":user_turn}));
                        let event = serde_json::json!({"type":"response.cancel","event_id":format!("event-{}",uuid::Uuid::new_v4())});
                        if let Err(error) = writer.send(text_message(event).unwrap()).await { terminal_error = Some(format!("发送打断请求失败：{error}")); break; }
                    }
                    Some(RealtimeCommand::Stop) | None => {
                        if close_deadline.is_none() {
                            diagnostics::realtime(Some(&session_id), "session.stop_requested", serde_json::json!({"turn":user_turn,"elapsedMs":started.elapsed().as_millis() as u64}));
                            update_state(&app, |state| state.phase = RealtimeCallPhase::Closing);
                            let _ = udp.send_to(&audio::control_packet(ControlAction::Stop, wire_session, control_sequence.wrapping_add(1)), peer).await;
                            let event = serde_json::json!({"type":"session.close","event_id":format!("event-{}",uuid::Uuid::new_v4())});
                            let _ = writer.send(text_message(event).unwrap()).await;
                            close_deadline = Some(tokio::time::Instant::now() + CLOSE_TIMEOUT);
                        }
                    }
                },
                Some(result_event) = tool_result_rx.recv(), if close_deadline.is_none() => {
                    if assistant_suspended { continue; }
                    let call_ids = result_event.get("items").and_then(|value|value.as_array()).map(|items|items.iter().filter_map(|item|item.get("call_id").and_then(|value|value.as_str()).map(str::to_owned)).collect::<Vec<_>>()).unwrap_or_default();
                    diagnostics::realtime(Some(&session_id), "tool.results_sending", serde_json::json!({"turn":user_turn,"callIds":call_ids}));
                    if let Err(error) = writer.send(text_message(result_event).unwrap()).await {
                        terminal_error = Some(format!("回传语音调用结果失败：{error}"));
                        break;
                    }
                    diagnostics::realtime(Some(&session_id), "tool.results_sent", serde_json::json!({"turn":user_turn,"count":call_ids.len()}));
                },
                result = udp.recv_from(&mut udp_buffer) => match result {
                    Ok((size, source)) if source.ip() == peer.ip() && close_deadline.is_none() => {
                        if let Ok(packet) = audio::parse_audio(&udp_buffer[..size]) {
                            if packet.session_id == wire_session && !input_muted && !playback_input_suppressed {
                                if cloud_input_muted {
                                    if let Err(error) = writer.send(text_message(input_audio_state_event("input_audio_unmute.commit")).unwrap()).await {
                                        terminal_error = Some(format!("恢复豆包音频输入失败：{error}"));
                                        break;
                                    }
                                    cloud_input_muted = false;
                                }
                                let event = serde_json::json!({"type":"input_audio_buffer.append","event_id":format!("event-{}",uuid::Uuid::new_v4()),"audio":BASE64.encode(packet.payload)});
                                if let Err(error) = writer.send(text_message(event).unwrap()).await { terminal_error = Some(format!("上传开发板音频失败：{error}")); break; }
                                last_input_audio_at = Instant::now();
                                update_state(&app, |state| state.input_packets += 1);
                            }
                        }
                    }
                    Ok(_) => {},
                    Err(error) => { terminal_error = Some(format!("接收开发板音频失败：{error}")); break; }
                },
                incoming = reader.next() => match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let event: serde_json::Value = match serde_json::from_str(text.as_str()) { Ok(value) => value, Err(error) => { terminal_error = Some(format!("实时语音响应 JSON 无效：{error}")); break; } };
                        match event.get("type").and_then(|value| value.as_str()) {
                            Some("response.canceled") => {
                                response_active = false;
                                output_cancel_pending = false;
                                playback_input_suppressed = false;
                                ignore_playback_transcription = false;
                                speaker_buffer.clear(); speaker_queue.clear();
                                update_state(&app, |state| state.phase = RealtimeCallPhase::Listening);
                                if let Some(ack) = pending_interrupt.pop_front() { let _ = ack.send(Ok(())); }
                            }
                            Some("session.updated") => { if let Some(ack) = pending_context.pop_front() { let _ = ack.send(Ok(())); } }
                            Some("conversation.item.input_audio_transcription.started") => {
                                if learning && playback_input_suppressed {
                                    ignore_playback_transcription = true;
                                    diagnostics::realtime(Some(&session_id), "asr.playback_echo_ignored", serde_json::json!({"stage":"started","turn":user_turn+1}));
                                    continue;
                                }
                                ignore_playback_transcription = false;
                                response_active = false;
                                output_cancel_pending = false;
                                while let Some(ack) = pending_interrupt.pop_front() { let _ = ack.send(Ok(())); }
                                diagnostics::realtime(Some(&session_id), "asr.started", serde_json::json!({"turn":user_turn+1}));
                                direct_preexecuted_tools.clear();
                                suppress_direct_response = false;
                                direct_cancel_sent = false;
                                speaker_buffer.clear();
                                speaker_queue.clear();
                                update_state(&app, |state| { state.phase = RealtimeCallPhase::Listening; state.user_text.clear(); state.assistant_text.clear(); });
                            }
                            Some("conversation.item.input_audio_transcription.delta") => if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
                                if ignore_playback_transcription { continue; }
                                let partial_text = {
                                    let mut partial_text = String::new();
                                    update_state(&app, |state| {
                                        state.user_text.push_str(delta);
                                        partial_text = state.user_text.clone();
                                    });
                                    partial_text
                                };
                                if wake_word_enabled && !assistant_suspended && is_rest_request(&partial_text) {
                                    assistant_suspended = true;
                                    suppress_direct_response = true;
                                    direct_cancel_sent = true;
                                    response_active = true;
                                    output_cancel_pending = true;
                                    speaker_buffer.clear();
                                    speaker_queue.clear();
                                    playback_input_suppressed = false;
                                    let cancel = serde_json::json!({"type":"response.cancel","event_id":format!("event-{}",uuid::Uuid::new_v4())});
                                    if let Err(error) = writer.send(text_message(cancel).unwrap()).await {
                                        terminal_error = Some(format!("进入语音待命时取消回答失败：{error}"));
                                        break;
                                    }
                                    diagnostics::realtime(Some(&session_id), "wake_word.suspended", serde_json::json!({"turn":user_turn+1,"transcript":diagnostics::text_preview(&partial_text),"stage":"transcription_delta"}));
                                    update_state(&app, |state| {
                                        state.wake_word_armed = true;
                                        state.last_tool_status = Some("助手休息中，请说“八弟八弟”唤醒".into());
                                        state.phase = RealtimeCallPhase::Listening;
                                    });
                                }
                            },
                            Some("conversation.item.input_audio_transcription.completed") => if let Some(text) = event.get("transcript").or_else(|| event.get("text")).and_then(|value| value.as_str()) {
                                if ignore_playback_transcription {
                                    ignore_playback_transcription = false;
                                    suppress_direct_response = true;
                                    direct_cancel_sent = false;
                                    diagnostics::realtime(Some(&session_id), "asr.playback_echo_ignored", serde_json::json!({"stage":"completed","transcript":diagnostics::text_preview(text),"turn":user_turn+1}));
                                    continue;
                                }
                                if learning {
                                    if let Some(id) = event.get("item_id").or_else(||event.get("event_id")).and_then(|v|v.as_str()) {
                                        if !completed_transcripts.insert(format!("user-{id}")) { continue; }
                                    }
                                }
                                user_turn += 1;
                                tool_calls_this_turn = 0;
                                let (mut cleaned_text, assistant_echo_removed) = strip_assistant_echo(text, &last_assistant_text);
                                diagnostics::realtime(Some(&session_id), "asr.completed", serde_json::json!({"turn":user_turn,"transcript":diagnostics::text_preview(text),"assistantEchoRemoved":assistant_echo_removed,"cleanedTranscript":diagnostics::text_preview(&cleaned_text)}));
                                if assistant_echo_removed {
                                    diagnostics::realtime(Some(&session_id), "asr.assistant_echo_removed", serde_json::json!({"turn":user_turn,"remaining":diagnostics::text_preview(&cleaned_text)}));
                                }
                                if wake_word_enabled {
                                    if assistant_suspended {
                                        if let Some(remainder) = wake_command_remainder(&cleaned_text) {
                                            assistant_suspended = false;
                                            cleaned_text = remainder;
                                            diagnostics::realtime(Some(&session_id), "wake_word.activated", serde_json::json!({"turn":user_turn,"remainder":diagnostics::text_preview(&cleaned_text)}));
                                            update_state(&app, |state| {
                                                state.wake_word_armed = false;
                                                state.last_tool_status = Some(if cleaned_text.trim().is_empty() {
                                                    "已唤醒，继续说出要处理的内容".into()
                                                } else {
                                                    "已唤醒，可以继续对话和控制软件".into()
                                                });
                                            });
                                            if cleaned_text.trim().is_empty() {
                                                suppress_direct_response = true;
                                                direct_cancel_sent = false;
                                                continue;
                                            }
                                        } else {
                                            diagnostics::realtime(Some(&session_id), "wake_word.ignored_while_suspended", serde_json::json!({"turn":user_turn}));
                                            update_state(&app, |state| {
                                                state.wake_word_armed = true;
                                                state.last_tool_status = Some("助手休息中，请说“八弟八弟”唤醒".into());
                                            });
                                            suppress_direct_response = true;
                                            direct_cancel_sent = false;
                                            continue;
                                        }
                                    } else if is_rest_request(&cleaned_text) {
                                        assistant_suspended = true;
                                        suppress_direct_response = true;
                                        direct_cancel_sent = true;
                                        response_active = true;
                                        output_cancel_pending = true;
                                        speaker_buffer.clear();
                                        speaker_queue.clear();
                                        playback_input_suppressed = false;
                                        ignore_playback_transcription = false;
                                        if let Err(error) = writer.send(text_message(serde_json::json!({"type":"response.cancel","event_id":format!("event-{}",uuid::Uuid::new_v4())})).unwrap()).await {
                                            terminal_error = Some(format!("进入语音待命时取消回答失败：{error}"));
                                            break;
                                        }
                                        diagnostics::realtime(Some(&session_id), "wake_word.suspended", serde_json::json!({"turn":user_turn,"transcript":diagnostics::text_preview(&cleaned_text)}));
                                        update_state(&app, |state| {
                                            state.wake_word_armed = true;
                                            state.last_tool_status = Some("助手休息中，请说“八弟八弟”唤醒".into());
                                            state.phase = RealtimeCallPhase::Listening;
                                        });
                                        continue;
                                    }
                                }
                                update_state(&app, |state| state.user_text = cleaned_text.clone());
                                let _ = app.emit("realtime-user-text", serde_json::json!({"text": cleaned_text}));
                                if learning && !cleaned_text.trim().is_empty() {
                                    let _ = app.emit("learning-transcript", serde_json::json!({"sessionId":session_id,"id":format!("user-{user_turn}"),"role":"user","text":cleaned_text}));
                                }
                                let subsystem_navigation = is_direct_subsystem_navigation_command(&cleaned_text);
                                let cube_control = is_direct_cube_control_command(&cleaned_text);
                                let live_sticker_control = is_direct_live_sticker_command(&cleaned_text);
                                if subsystem_navigation || cube_control || live_sticker_control {
                                    suppress_direct_response = true;
                                    direct_cancel_sent = false;
                                    let event = if cube_control { "local_action.cube_command_matched" } else if live_sticker_control { "local_action.live_sticker_matched" } else { "local_action.subsystem_navigation_matched" };
                                    diagnostics::realtime(Some(&session_id), event, serde_json::json!({"turn":user_turn,"transcript":diagnostics::text_preview(&cleaned_text)}));
                                }
                                let direct_actions = if learning { vec![] } else { direct_voice_actions(&cleaned_text, &voice_actions) };
                                if !direct_actions.is_empty() {
                                    let action_names = direct_actions.iter().map(|mapping|mapping.name.trim().to_owned()).collect::<Vec<_>>();
                                    direct_preexecuted_tools = direct_actions.iter().map(tool_name).collect();
                                    tool_calls_this_turn += direct_actions.len();
                                    suppress_direct_response = true;
                                    direct_cancel_sent = false;
                                    diagnostics::realtime(Some(&session_id), "local_action.matched", serde_json::json!({"turn":user_turn,"actions":&action_names,"tools":&direct_preexecuted_tools}));
                                    update_state(&app, |state| {
                                        state.tool_call_count += direct_actions.len() as u64;
                                        state.last_tool_status = Some(format!("本地识别，正在执行：{}", action_names.join("、")));
                                    });
                                    let direct_app = app.clone();
                                    let direct_session_id = session_id.clone();
                                    let direct_turn = user_turn;
                                    tauri::async_runtime::spawn(async move {
                                        for mapping in direct_actions {
                                            let action_name = mapping.name.trim().to_owned();
                                            let local_call_id = format!("local-{direct_turn}-{}", mapping.id);
                                            diagnostics::realtime(Some(&direct_session_id), "tool.execution_started", serde_json::json!({"turn":direct_turn,"callId":&local_call_id,"tool":tool_name(&mapping),"action":&action_name,"kind":format!("{:?}",mapping.action.kind),"route":"local_asr"}));
                                            match execute_voice_action(direct_app.clone(), mapping, "{}").await {
                                                Ok(_) => {
                                                    update_state(&direct_app, |state| state.last_tool_status = Some(format!("执行成功：{action_name}（本地语音匹配）")));
                                                    diagnostics::realtime(Some(&direct_session_id), "tool.execution_succeeded", serde_json::json!({"turn":direct_turn,"callId":&local_call_id,"action":&action_name,"route":"local_asr"}));
                                                }
                                                Err(error) => {
                                                    update_state(&direct_app, |state| state.last_tool_status = Some(format!("执行失败：{action_name} · {error}")));
                                                    diagnostics::realtime(Some(&direct_session_id), "tool.execution_failed", serde_json::json!({"turn":direct_turn,"callId":&local_call_id,"action":&action_name,"route":"local_asr","message":&error}));
                                                }
                                            }
                                        }
                                    });
                                } else if assistant_echo_removed && cleaned_text.trim().is_empty() {
                                    suppress_direct_response = true;
                                    direct_cancel_sent = false;
                                    diagnostics::realtime(Some(&session_id), "asr.echo_only_turn", serde_json::json!({"turn":user_turn,"action":"cancel_generated_response"}));
                                }
                            },
                            Some("response.output_text.delta") => if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
                                response_active = true;
                                if assistant_suspended {
                                    if !output_cancel_pending {
                                        output_cancel_pending = true;
                                        let _ = writer.send(text_message(serde_json::json!({"type":"response.cancel","event_id":format!("event-{}",uuid::Uuid::new_v4())})).unwrap()).await;
                                    }
                                    continue;
                                }
                                if output_cancel_pending { continue; }
                                if learning && !playback_input_suppressed {
                                    playback_input_suppressed = true;
                                    if !cloud_input_muted {
                                        if let Err(error) = writer.send(text_message(input_audio_state_event("input_audio_mute.commit")).unwrap()).await {
                                            terminal_error = Some(format!("暂停 Milo 播报期间的豆包音频输入失败：{error}"));
                                            break;
                                        }
                                        cloud_input_muted = true;
                                    }
                                }
                                if suppress_direct_response {
                                    if !direct_cancel_sent {
                                        let cancel = serde_json::json!({"type":"response.cancel","event_id":format!("event-{}",uuid::Uuid::new_v4())});
                                        if let Err(error) = writer.send(text_message(cancel).unwrap()).await { terminal_error = Some(format!("取消本地动作对应的模型回复失败：{error}")); break; }
                                        direct_cancel_sent = true;
                                        diagnostics::realtime(Some(&session_id), "local_action.response_canceled", serde_json::json!({"turn":user_turn,"trigger":"text_delta"}));
                                    }
                                } else {
                                    update_state(&app, |state| { state.phase = RealtimeCallPhase::Speaking; state.assistant_text.push_str(delta); });
                                }
                            },
                            Some("response.output_text.done") => if let Some(text) = event.get("text").and_then(|value| value.as_str()) {
                                diagnostics::realtime(Some(&session_id), "response.text_done", serde_json::json!({"turn":user_turn,"text":diagnostics::text_preview(text),"toolCallsThisTurn":tool_calls_this_turn,"suppressedForLocalAction":suppress_direct_response}));
                                if !suppress_direct_response && !output_cancel_pending {
                                    assistant_turn += 1;
                                    if learning { let _ = app.emit("learning-transcript", serde_json::json!({"sessionId":session_id,"id":event.get("response_id").and_then(|v|v.as_str()).unwrap_or(&format!("assistant-{assistant_turn}")),"role":"assistant","text":text})); }
                                    last_assistant_text = text.to_owned();
                                    update_state(&app, |state| state.assistant_text = text.to_owned());
                                }
                            },
                            Some("response.output_audio.started") => {
                                response_active = true;
                                audio_done = false;
                                if assistant_suspended {
                                    speaker_buffer.clear();
                                    speaker_queue.clear();
                                    if !output_cancel_pending {
                                        output_cancel_pending = true;
                                        let _ = writer.send(text_message(serde_json::json!({"type":"response.cancel","event_id":format!("event-{}",uuid::Uuid::new_v4())})).unwrap()).await;
                                    }
                                    continue;
                                }
                                if output_cancel_pending { continue; }
                                if learning && !playback_input_suppressed {
                                    playback_input_suppressed = true;
                                    if !cloud_input_muted {
                                        if let Err(error) = writer.send(text_message(input_audio_state_event("input_audio_mute.commit")).unwrap()).await {
                                            terminal_error = Some(format!("暂停 Milo 播报期间的豆包音频输入失败：{error}"));
                                            break;
                                        }
                                        cloud_input_muted = true;
                                    }
                                }
                                if suppress_direct_response {
                                    speaker_buffer.clear();
                                    speaker_queue.clear();
                                    if !direct_cancel_sent {
                                        let cancel = serde_json::json!({"type":"response.cancel","event_id":format!("event-{}",uuid::Uuid::new_v4())});
                                        if let Err(error) = writer.send(text_message(cancel).unwrap()).await { terminal_error = Some(format!("取消本地动作对应的模型播报失败：{error}")); break; }
                                        direct_cancel_sent = true;
                                        diagnostics::realtime(Some(&session_id), "local_action.response_canceled", serde_json::json!({"turn":user_turn,"trigger":"audio_started"}));
                                    }
                                    update_state(&app, |state| state.phase = RealtimeCallPhase::Listening);
                                } else { update_state(&app, |state| state.phase = RealtimeCallPhase::Speaking); }
                            },
                            Some("response.output_audio.delta") => if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
                                if !suppress_direct_response && sound_enabled && !output_cancel_pending { match BASE64.decode(delta) {
                                    Ok(bytes) => {
                                        speaker_buffer.extend_from_slice(&bytes);
                                        while speaker_buffer.len() >= SPEAKER_FRAME_BYTES {
                                            speaker_queue.push_back(speaker_buffer.drain(..SPEAKER_FRAME_BYTES).collect());
                                        }
                                    }
                                    Err(error) => { terminal_error = Some(format!("豆包下行音频 Base64 无效：{error}")); break; }
                                } }
                            },
                            Some("response.output_audio.done") => {
                                audio_done = true;
                                if !speaker_buffer.is_empty() && sound_enabled && !output_cancel_pending {
                                    speaker_buffer.resize(SPEAKER_FRAME_BYTES, 0);
                                    speaker_queue.push_back(std::mem::take(&mut speaker_buffer));
                                }
                                if speaker_queue.is_empty() {
                                    playback_input_suppressed = false;
                                    update_state(&app, |state| state.phase = RealtimeCallPhase::Listening);
                                }
                            },
                            Some("response.function_call_arguments.done") => {
                                if learning || assistant_suspended || suppress_direct_response { continue; }
                                let parsed_calls = match function_calls(&event) {
                                    Ok(calls) => calls,
                                    Err(error) => {
                                        diagnostics::realtime(Some(&session_id), "function_call.parse_failed", serde_json::json!({"turn":user_turn,"message":&error}));
                                        terminal_error = Some(error);
                                        break;
                                    }
                                };
                                let received_count = parsed_calls.len();
                                let calls = parsed_calls.into_iter().filter(|call| handled_call_ids.insert(call.call_id.clone())).collect::<Vec<_>>();
                                if received_count > calls.len() {
                                    diagnostics::realtime(Some(&session_id), "function_call.duplicate_ignored", serde_json::json!({"turn":user_turn,"count":received_count-calls.len()}));
                                }
                                if !calls.is_empty() {
                                    suppress_direct_response = false;
                                    direct_cancel_sent = false;
                                    tool_calls_this_turn += calls.len();
                                    for call in &calls {
                                        diagnostics::realtime(Some(&session_id), "function_call.received", serde_json::json!({"turn":user_turn,"callId":&call.call_id,"tool":&call.name,"arguments":diagnostics::text_preview(&call.arguments)}));
                                    }
                                    let action_names = calls.iter().filter_map(|call| {
                                        if call.name == COCKPIT_TOOL_NAME { Some("驾驶舱控制") }
                                        else { voice_actions.iter().find(|mapping| mapping.enabled && tool_name(mapping) == call.name).map(|mapping| mapping.name.trim()) }
                                    }).collect::<Vec<_>>();
                                    update_state(&app, |state| {
                                        state.tool_call_count += calls.len() as u64;
                                        state.last_tool_status = Some(if action_names.is_empty() { "豆包发起了未知语音动作".into() } else { format!("正在执行：{}", action_names.join("、")) });
                                    });
                                    let result_app = app.clone();
                                    let result_session_id = session_id.clone();
                                    let result_mappings = voice_actions.clone();
                                    let result_preexecuted_tools = direct_preexecuted_tools.clone();
                                    let latest_user_text = app.state::<crate::AppState>().realtime_call.lock().map(|state|state.user_text.clone()).unwrap_or_default();
                                    let result_tx = tool_result_tx.clone();
                                    tauri::async_runtime::spawn(async move {
                                        let event = function_result_event(result_app, result_session_id, result_mappings, calls, result_preexecuted_tools, latest_user_text).await;
                                        let _ = result_tx.send(event);
                                    });
                                }
                            }
                            Some("response.done") => {
                                response_active = false;
                                output_cancel_pending = false;
                                while let Some(ack) = pending_interrupt.pop_front() { let _ = ack.send(Ok(())); }
                                response_count += 1;
                                let outputs = event.pointer("/response/output").and_then(|value|value.as_array()).map(|items|items.iter().map(|item|serde_json::json!({"type":item.get("type"),"name":item.get("name"),"callId":item.get("call_id")})).collect::<Vec<_>>()).unwrap_or_default();
                                diagnostics::realtime(Some(&session_id), "response.done", serde_json::json!({"turn":user_turn,"response":response_count,"toolCallsThisTurn":tool_calls_this_turn,"status":event.pointer("/response/status"),"outputs":outputs}));
                                if !assistant_suspended {
                                    suppress_direct_response = false;
                                    direct_cancel_sent = false;
                                }
                            }
                            Some("session.closed") => {
                                diagnostics::realtime(Some(&session_id), "session.closed", serde_json::json!({"turn":user_turn,"responses":response_count}));
                                graceful_close = true;
                                break;
                            }
                            Some("error") => {
                                let error = event_error(&event);
                                diagnostics::realtime(Some(&session_id), "cloud.error", serde_json::json!({"turn":user_turn,"message":&error,"code":event.pointer("/error/code")}));
                                terminal_error = Some(error);
                                break;
                            }
                            Some(kind) if kind.contains("function_call") => {
                                diagnostics::realtime(Some(&session_id), "function_call.unhandled_event", serde_json::json!({"turn":user_turn,"type":kind}));
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => { if close_deadline.is_none() { terminal_error = Some("豆包实时语音连接意外关闭".into()); } else { graceful_close = true; } break; }
                    Some(Ok(_)) => {},
                    Some(Err(error)) => { terminal_error = Some(format!("接收豆包实时语音失败：{error}")); break; }
                },
                _ = speaker_tick.tick(), if close_deadline.is_none() => {
                    if let Some(frame) = speaker_queue.pop_front() {
                        speaker_sequence = speaker_sequence.wrapping_add(1);
                        match audio::speaker_packet(wire_session, speaker_sequence, &frame) {
                            Ok(packet) => if let Err(error) = udp.send_to(&packet, peer).await { terminal_error = Some(format!("向开发板扬声器发送音频失败：{error}")); break; },
                            Err(error) => { terminal_error = Some(error.to_string()); break; }
                        }
                        if audio_done && speaker_queue.is_empty() { playback_input_suppressed = false; }
                        update_state(&app, |state| { state.output_packets += 1; if audio_done && speaker_queue.is_empty() { state.phase = RealtimeCallPhase::Listening; } });
                    }
                },
                _ = keepalive.tick() => {
                    if close_deadline.is_none() {
                        control_sequence = control_sequence.wrapping_add(1);
                        let _ = udp.send_to(&audio::control_packet(ControlAction::Keepalive, wire_session, control_sequence), peer).await;
                        if !cloud_input_muted && last_input_audio_at.elapsed() >= INPUT_AUDIO_MUTE_AFTER {
                            if let Err(error) = writer.send(text_message(input_audio_state_event("input_audio_mute.commit")).unwrap()).await {
                                terminal_error = Some(format!("暂停豆包音频输入失败：{error}"));
                                break;
                            }
                            cloud_input_muted = true;
                        }
                    }
                },
                _ = state_tick.tick() => update_state(&app, |state| state.elapsed_ms = started.elapsed().as_millis() as u64),
                _ = async {
                    match close_deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => pending::<()>().await,
                    }
                } => { graceful_close = true; break; },
            }
        }
        let _ = udp
            .send_to(
                &audio::control_packet(
                    ControlAction::Stop,
                    wire_session,
                    control_sequence.wrapping_add(1),
                ),
                peer,
            )
            .await;
        if !graceful_close {
            let _ = writer.send(text_message(serde_json::json!({"type":"session.close","event_id":format!("event-{}",uuid::Uuid::new_v4())})).unwrap()).await;
        }
        let _ = writer.close().await;
        let state = app.state::<crate::AppState>();
        if let Ok(mut session) = state.realtime_session.lock() {
            *session = None;
        }
        state.mcd_orders.reset(&session_id);
        update_state(&app, |state| {
            state.elapsed_ms = started.elapsed().as_millis() as u64;
            // Learning subscribers need the terminal session id to match the final event.
            // Preserve the existing ordinary-call contract, which clears it at idle.
            state.session_id = learning.then(|| session_id.clone());
            state.learning_owner = None;
            state.error = terminal_error.clone();
            state.phase = if terminal_error.is_some() {
                RealtimeCallPhase::Error
            } else {
                RealtimeCallPhase::Idle
            };
        });
        diagnostics::realtime(
            Some(&session_id),
            "session.ended",
            serde_json::json!({"elapsedMs":started.elapsed().as_millis() as u64,"turns":user_turn,"responses":response_count,"graceful":graceful_close,"error":terminal_error}),
        );
    });
    Ok((tx, ready_tx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learning_session_isolated_from_all_desktop_tools() {
        let mut config = RealtimeVoiceConfig::default();
        config.instructions = "You are Milo. Current stage: read.".into();
        let event = learning_session_event(&config, "learning-test");
        assert_eq!(event["session"]["instructions"], config.instructions);
        assert_eq!(event["session"]["tools"], serde_json::json!([]));
        assert_eq!(event["extension"]["asr"]["extra"], serde_json::json!({}));
        assert_eq!(event["session"]["audio"]["input"]["format"]["rate"], 16000);
        assert_eq!(event["session"]["audio"]["output"]["format"]["rate"], 24000);
        let ordinary = session_create_event(&config, "ordinary", &[], &[], false);
        assert!(!ordinary["session"]["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn learning_commands_reject_invalid_owners_and_oversize_utf8() {
        let valid = LearningStart { owner_id: uuid::Uuid::new_v4().to_string(), instructions: "Milo".into(), greeting: String::new() };
        assert!(valid.validate().is_ok());
        assert!(LearningStart { owner_id: "unknown".into(), ..valid.clone() }.validate().is_err());
        assert!(LearningAction::Context { instructions: "中".repeat(6000) }.validate().is_err());
        assert!(LearningAction::Text { text: " ".into() }.validate().is_err());
        assert!(serde_json::from_value::<LearningAction>(serde_json::json!({"kind":"invoke","command":"read_key"})).is_err());
    }

    #[test]
    fn defaults_match_official_duplex_protocol() {
        let config = RealtimeVoiceConfig::default();
        assert!(validate(&config).is_ok());
        let event = session_create_event(&config, "test-session", &[], &[], false);
        assert_eq!(event["session"]["model"], "1.2.6.1");
        assert_eq!(event["session"]["audio"]["input"]["format"]["rate"], 16000);
        assert_eq!(event["session"]["audio"]["output"]["format"]["rate"], 24000);
        assert_eq!(
            event["session"]["audio"]["output"]["format"]["type"],
            "pcm_s16le"
        );
        assert_eq!(event["session"]["tools"].as_array().unwrap().len(), 1);
        assert_eq!(event["session"]["tools"][0]["name"], COCKPIT_TOOL_NAME);
        assert!(event["session"]["instructions"]
            .as_str()
            .unwrap()
            .contains("训练营运营驾驶舱"));
    }

    #[test]
    fn rejects_api_key_exfiltration_endpoint() {
        let mut config = RealtimeVoiceConfig::default();
        config.endpoint = "wss://example.com/steal".into();
        assert!(validate(&config).is_err());
    }

    #[test]
    fn explains_invalid_api_key_handshake_without_echoing_secret() {
        let response = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(401)
            .header("X-Tt-Logid", "test-log-id")
            .body(Some(
                br#"{"error":{"code":"45000010","message":"Invalid X-Api-Key"}}"#.to_vec(),
            ))
            .unwrap();
        let message = handshake_error(WebSocketError::Http(Box::new(response)));
        assert!(message.contains("Invalid X-Api-Key（45000010）"));
        assert!(message.contains("新版豆包语音控制台"));
        assert!(message.contains("test-log-id"));
    }

    #[test]
    fn trims_raw_api_key_and_rejects_bearer_prefix() {
        let config = RealtimeVoiceConfig::default();
        let request = authorized_request(&config, "  raw-key  ").unwrap();
        assert_eq!(request.headers()["X-Api-Key"], "raw-key");
        assert!(authorized_request(&config, "Bearer raw-key")
            .unwrap_err()
            .contains("不要添加 Bearer"));
    }

    #[test]
    fn explains_no_audio_timeout_and_accepts_string_error_code() {
        let event = serde_json::json!({
            "type": "error",
            "error": {
                "code": "52000033",
                "message": "AudioServerNoAudioInputTooLongError"
            }
        });
        let message = event_error(&event);
        assert!(message.contains("52000033"));
        assert!(message.contains("开发板音频上传中断时间过长"));
    }

    #[test]
    fn builds_official_audio_mute_and_unmute_events() {
        assert_eq!(
            input_audio_state_event("input_audio_mute.commit")["type"],
            "input_audio_mute.commit"
        );
        assert_eq!(
            input_audio_state_event("input_audio_unmute.commit")["type"],
            "input_audio_unmute.commit"
        );
    }

    #[test]
    fn exposes_enabled_voice_actions_as_function_tools() {
        let mapping = VoiceActionMapping {
            id: "11111111-2222-4333-8444-555555555555".into(),
            enabled: true,
            name: "打开微信".into(),
            description: "用户要求启动微信".into(),
            action: crate::model::KeyboardAction {
                kind: KeyboardActionKind::OpenApp,
                label: "微信".into(),
                value: Some("/Applications/微信.app".into()),
                host_action_id: None,
            },
        };
        let event = session_create_event(
            &RealtimeVoiceConfig::default(),
            "test-session",
            &[mapping],
            &["EasyInput".into()],
            false,
        );
        let tool = &event["session"]["tools"][0];
        assert_eq!(tool["type"], "function");
        assert_eq!(
            tool["name"],
            "voice_action_11111111222243338444555555555555"
        );
        assert!(tool["description"].as_str().unwrap().contains("打开微信"));
        assert!(event["session"]["instructions"]
            .as_str()
            .unwrap()
            .contains("任意轮次"));
        let context = event
            .pointer("/extension/asr/extra/context")
            .and_then(|value| value.as_str())
            .unwrap();
        let context: serde_json::Value = serde_json::from_str(context).unwrap();
        assert_eq!(
            context["hotwords"],
            serde_json::json!([
                {"word":"EasyInput"},
                {"word":"训练营驾驶舱"}, {"word":"经营总览"}, {"word":"期次对比"},
                {"word":"作业分析"}, {"word":"校友会"}, {"word":"优秀学员列表"}, {"word":"学员画像"}, {"word":"数据质量"},
                {"word":"打开微信"}, {"word":"微信"}
            ])
        );
    }

    #[test]
    fn exposes_stateful_mcd_tools_only_when_enabled() {
        let config = RealtimeVoiceConfig::default();
        let disabled = session_create_event(&config, "test-session", &[], &[], false);
        let enabled = session_create_event(&config, "test-session", &[], &[], true);
        let disabled_names = disabled["session"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(disabled_names, vec![COCKPIT_TOOL_NAME]);
        let names = enabled["session"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"mcd_start_delivery"));
        assert!(names.contains(&"mcd_submit_order"));
        assert!(enabled["session"]["instructions"]
            .as_str()
            .unwrap()
            .contains("当前一轮明确说出“确认下单”"));
    }

    #[test]
    fn validates_cockpit_navigation_arguments() {
        assert_eq!(
            cockpit_command(r#"{"module":"assignments"}"#).unwrap(),
            serde_json::json!({"action":"navigate","target":"assignments"})
        );
        assert_eq!(
            cockpit_command(r#"{"module":"outstanding-students"}"#).unwrap(),
            serde_json::json!({"action":"navigate","target":"outstanding-students"})
        );
        assert_eq!(
            cockpit_command(r#"{"module":"cohort-detail","cohortId":5}"#).unwrap(),
            serde_json::json!({"action":"navigate","target":"cohort-detail","parameters":{"cohortId":5}})
        );
        assert!(cockpit_command(r#"{"module":"cohort-detail","cohortId":8}"#).is_err());
        assert!(cockpit_command(r#"{"module":"unknown"}"#).is_err());
    }

    #[test]
    fn parses_parallel_function_calls() {
        let event = serde_json::json!({"items":[
            {"call_id":"call_a","name":"voice_action_a","arguments":"{}"},
            {"call_id":"call_b","name":"voice_action_b","arguments":"{\"instruction\":\"翻译\"}"}
        ]});
        let calls = function_calls(&event).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].call_id, "call_a");
        assert_eq!(calls[1].name, "voice_action_b");
    }

    fn direct_mapping(
        id: &str,
        name: &str,
        label: &str,
        kind: KeyboardActionKind,
    ) -> VoiceActionMapping {
        VoiceActionMapping {
            id: id.into(),
            enabled: true,
            name: name.into(),
            description: String::new(),
            action: crate::model::KeyboardAction {
                kind,
                label: label.into(),
                value: None,
                host_action_id: None,
            },
        }
    }

    #[test]
    fn locally_matches_exact_polite_and_open_app_alias_commands() {
        let mappings = vec![direct_mapping(
            "wechat",
            "打开微信",
            "WeChat",
            KeyboardActionKind::OpenApp,
        )];
        for transcript in [
            "打开微信。",
            "请帮我打开微信一下",
            "启动微信",
            "切换到微信吧",
            "来，打开个微信。",
            "打开微信听得到吗？",
            "打开微。",
        ] {
            let matched = direct_voice_actions(transcript, &mappings);
            assert_eq!(matched.len(), 1, "应该匹配：{transcript}");
            assert_eq!(matched[0].id, "wechat");
        }
    }

    #[test]
    fn locally_matches_multiple_explicit_commands_once_each() {
        let mappings = vec![
            direct_mapping("wechat", "打开微信", "WeChat", KeyboardActionKind::OpenApp),
            direct_mapping(
                "jianying",
                "打开剪映",
                "VideoFusion-macOS",
                KeyboardActionKind::OpenApp,
            ),
        ];
        let matched = direct_voice_actions("请帮我打开微信，然后打开剪映。", &mappings);
        assert_eq!(
            matched
                .iter()
                .map(|mapping| mapping.id.as_str())
                .collect::<Vec<_>>(),
            vec!["wechat", "jianying"]
        );
    }

    #[test]
    fn local_router_rejects_discussion_and_parameterized_editing() {
        let mappings = vec![
            direct_mapping("wechat", "打开微信", "WeChat", KeyboardActionKind::OpenApp),
            direct_mapping("edit", "润色选区", "语音编辑", KeyboardActionKind::EditPtt),
        ];
        assert!(direct_voice_actions("为什么不能打开微信？", &mappings).is_empty());
        assert!(
            direct_voice_actions("我现在没法帮你打开微信，你可以手动点开。", &mappings).is_empty()
        );
        assert!(direct_voice_actions("帮我润色选区", &mappings).is_empty());
    }

    #[test]
    fn local_router_handles_latin_asr_variants_in_the_last_clause() {
        let mappings = vec![
            direct_mapping("codex", "打开codex", "ChatGPT", KeyboardActionKind::OpenApp),
            direct_mapping(
                "teleagent",
                "打开teleagent",
                "TeleAgent",
                KeyboardActionKind::OpenApp,
            ),
        ];
        let codex = direct_voice_actions(
            "很遗憾，现在没法帮你打开 Kodex，你可以手动启动哦。打开 Kodex。",
            &mappings,
        );
        assert_eq!(
            codex
                .iter()
                .map(|mapping| mapping.id.as_str())
                .collect::<Vec<_>>(),
            vec!["codex"]
        );
        let teleagent = direct_voice_actions("你可以手动启动试试。打开 tell agent。", &mappings);
        assert_eq!(
            teleagent
                .iter()
                .map(|mapping| mapping.id.as_str())
                .collect::<Vec<_>>(),
            vec!["teleagent"]
        );
    }

    #[test]
    fn removes_full_or_partial_assistant_speaker_echo() {
        let assistant = "暂时没有可用的工具帮你打开，你可以手动点击Codex的图标来启动~";
        let (cleaned, removed) = strip_assistant_echo(
            "暂时没有可用的工具帮你打开，你可以手动点击 Codex 的图标来启动。打开 Kodex。",
            assistant,
        );
        assert!(removed);
        assert_eq!(cleaned, "打开 Kodex");

        let (cleaned, removed) = strip_assistant_echo("暂时没有可用的工具帮你打开。", assistant);
        assert!(removed);
        assert!(cleaned.is_empty());

        let (cleaned, removed) = strip_assistant_echo("为什么不能打开微信？", assistant);
        assert!(!removed);
        assert_eq!(cleaned, "为什么不能打开微信？");
    }
}

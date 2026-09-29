# 麦当劳 MCP 语音点餐集成说明

## 已接入的链路

EasyInput 现已把麦当劳外送流程注册为豆包端到端实时语音会话中的 9 个高层 Function。模型负责理解自然语言、选择下一步和播报结果；Rust 客户端负责保存会话状态、解析序号、调用麦当劳 MCP、执行下单安全校验，以及打开支付二维码页。

```text
开发板麦克风 → 豆包实时语音 → 高层 Function Calling
                                  ↓
                         EasyInput 点餐状态机
                                  ↓
                    麦当劳 MCP Streamable HTTP
                                  ↓
                  create-order → 本机浏览器二维码页
```

底层调用顺序为：

1. `delivery-query-addresses`
2. `delivery-query-stores`
3. `query-meals`
4. `query-meal-detail`
5. `query-store-coupons`
6. `calculate-price`
7. 当前轮语音确认后调用 `create-order`
8. 创建成功后打开 `payH5Url` 对应的本机二维码页
9. 支付后按需调用 `query-order`

## 使用方法

1. 在“语音服务配置”中启用并配置豆包实时语音。
2. 在“点餐”页面填写麦当劳 MCP Token，先点“测试连接”，再启用并保存。
3. 开始实时通话，说“帮我点麦当劳外送”。
4. 按语音提示选择地址、门店和餐品；存在完整默认配置的套餐或特制餐品会按默认配置加入。
5. 听完最终价格后，只有确实要创建订单时才说完整的“确认下单”。
6. 浏览器出现二维码后，由用户自行扫码支付。
7. 支付完成后可说“查一下刚才订单状态”。

## 已实现的安全约束

- MCP Token 只保存在 macOS 钥匙串，不进入 `config.json`。
- MCP 地址固定为 `https://mcp.mcd.cn`，防止 Token 被发送到第三方主机。
- 模型只能传递用户选择的序号，不能传递或编造 `addressId`、`storeCode`、`beCode` 和 `productCode`。
- 地址传给模型的语音标签只包含区/县级区域；小区、街道、楼栋、房间号、联系人和手机号不进入地址播报。完整地址仅由客户端本地会话保留，用于实际配送。
- 点餐调用按实时语音会话隔离，并由每个会话的异步锁串行化。
- 报价有效期为 2 分钟。超时必须重新算价、重新播报并再次确认。
- `create-order` 同时校验 Function 参数和当前轮真实 ASR 文本中的“确认下单”。否定表达与模糊同意不会通过。
- 每个会话只允许发起一次创建订单请求。即使网络结果不明确也不会自动重试，避免重复下单。
- 模型不会收到原始支付 URL；只获知二维码已打开、支付域名、脱敏订单号和状态摘要。
- 支付二维码页只绑定 `127.0.0.1`，使用随机路径、短时有效期和禁止缓存响应头。

## MVP 范围与剩余验证

当前版本支持账号中已保存的地址、简单单品，以及服务端已提供完整默认配置的套餐或特制餐品。以下能力暂不开放：

- 通过语音新建地址（涉及姓名、手机号和门牌号）；
- 自动选择或核销优惠券；
- 通过语音逐项修改套餐、规格、配餐、饮料、去冰、少酱等复杂配置；
- 自动支付；
- 后台高频轮询订单。

已使用本机钥匙串中的 Token 对生产 MCP 完成结构验证：地址列表返回 `fullAddress`，门店数据直接返回数组，菜单 `meals` 返回以商品编码为 key 的对象映射；客户端已经兼容这三种真实结构。2026-09-03 使用“脆薯饼 × 1”完成过一次低金额生产验证：商品 9.50 元、配送 6.00 元、打包 0.80 元、总价 16.30 元，`create-order` 成功并在浏览器打开 `m.mcd.cn` 支付二维码；未自动支付。后续首次下单联调仍应在 `create-order` 前人工核对金额、门店和地址。

金额字段的单位并不统一：`query-meals.currentPrice` 是以“元”为单位的小数字符串（例如 `"11.5"`），`calculate-price` 的 `productPrice`、`deliveryPrice`、`packingPrice` 和 `price` 则是以“分”为单位的整数。客户端分别格式化，不能共用除以 100 的逻辑。商品搜索优先使用完整关键词；没有精确结果时，模糊候选至少要覆盖一半关键词且不少于两个不同字符，并在返回中明确标记为相近餐品。

`query-meal-detail.supportModify=true` 表示餐品支持去酱、去生菜等特制，不等于用户必须选择。套餐 `rounds` 和特制 `modification` 中若已有满足最小选择数量的默认项，客户端会按服务端默认配置加入购物车；只有必选组没有默认值时才暂停并要求改选。默认配置能否计价已用生产 `calculate-price` 做过只读验证，未创建订单。

## 诊断日志

麦当劳点餐沿用实时语音 JSONL 诊断日志。可以在“通话”页面点击“导出诊断日志”，原始文件默认位于：

```text
~/Library/Application Support/pro.easyinput.desktop.intel/logs/realtime.jsonl
```

排查点餐问题时重点查找以下事件：

- `mcd.session.initializing` / `mcd.session.initialized`：MCP 初始化、协议版本和会话建立情况；
- `mcd.workflow.step_started`：每一步开始前的地址、门店、菜单、候选、购物车和报价状态；
- `mcd.mcp.request`：脱敏后的 MCP 请求参数；
- `mcd.mcp.response`：请求耗时、业务状态、数据类型、字段清单和数组长度；
- `mcd.addresses.loaded`：地址数量、脱敏地址预览、地址字段和 `addressId` 指纹；
- `mcd.address.selected`：模型实际选择的序号及对应地址指纹；
- `mcd.stores.loaded`：可配送门店数量、门店编码、业务编码和营业状态；
- `mcd.menu.loaded`：菜单结构及规范化后的餐品数量；
- `mcd.products.searched` / `mcd.cart.item_added`：搜索候选与加购结果；
- `mcd.product.detail_loaded`：餐品是否支持特制、套餐组数量、默认配置以及是否仍有未满足的必选组；
- `mcd.quote.ready`：购物车、费用明细和报价有效期；
- `mcd.order.submission_started` / `mcd.order.created`：创建订单防重状态及二维码打开结果；
- `mcd.order.queried`：支付后的订单状态查询；
- `mcd.mcp.failed` / `mcd.mcp.business_error`：网络、协议或业务错误。

日志不会记录 MCP Token、完整 `addressId`、完整订单号或原始 `payH5Url`。地址只保存脱敏预览和稳定指纹，既可对比同一地址是否被选中，也能降低诊断文件泄露隐私的风险。

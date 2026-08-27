# 实施状态与外部依赖

## 已实现并可自动验证

| 模块 | 状态 | 验证 |
|---|---|---|
| React 9 页面与 1200×800 应用外壳 | 已实现 | `npm run build` |
| Intel Tauri 配置、macOS 12 部署目标 | 已实现 | `tauri.conf.json`、Mach-O 校验脚本 |
| 配置、词库、SQLite 历史 | 已实现 | Rust 单元测试 |
| USB 配置分片与 CRC16-CCITT | 已实现 | Golden Vector 与乱序/代次测试 |
| Wi‑Fi 音频及控制报文 | 已实现解析层 | Golden Vector 测试 |
| 豆包语音配置与鉴权测试 | 已实现 | 官方域名限制、Resource ID 校验、WSS 握手、Keychain |
| USB VID/PID 自动发现 | 已实现 | 需要实板完成 I/O 验收 |
| 键盘设备控制台 | 已实现按键、麦克风、网络、音效、编程助手、更新 6 个截图状态 | 浏览器模拟及 `npm run build` |
| 开发板语音键 | 已实现 HID 持续监听、专用/兼容报告解析、去重及按住/切换会话控制 | Rust 协议测试及真实 VID/PID 设备发现 |
| 豆包语音识别 2.0 | 已修正小时版 Resource ID、配置迁移和带序号初始化首包 | 官方端点真实鉴权与二进制响应验证 |
| BLE/Wi‑Fi/音效/Agent 页面 | 已实现产品交互与无硬件预览模式 | 传输端到端待实板验收 |

## 不能由源码单独完成的发布前置项

1. EasyInput 自有账户和更新接口的请求/响应 Schema 与测试账号；语音识别已改为豆包语音官方服务。
2. EasyInput V2.0 实板，用于 USB Feature Report、BLE 写入/通知、Wi‑Fi 来源 IP、A/B 音效提交确认。
3. Developer ID Application 证书、公证账户、应用更新签名私钥与公开验证密钥。
4. macOS 12 的 CI runner 或虚拟机。

缺少这些资料时，应用维持安全失败：不会猜测认证接口、不会自动刷写固件、不会安装未签名更新。

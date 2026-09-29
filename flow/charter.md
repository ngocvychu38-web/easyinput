# EasyInput 项目宪章

> 所有 Agent 开工时首先阅读；项目范围或成功标准变化时先提议、确认，再更新。

- **项目名**：EasyInput
- **目标**：交付一套稳定、可验证的 Intel Mac 桌面客户端，把 EasyInput 硬件按键、语音输入、选区语音编辑和实时通话整合为原生 macOS 工作流。
- **范围**
  - **做**：React/Tauri 桌面客户端、USB HID 与音频协议接入、语音识别与实时通话、文本动作、本地配置/词库/历史、Keychain 安全存储、Intel Mac 构建与验证。
  - **不做**：开发板固件源码、未授权的生产签名/公证、在不可信网络中承诺安全的 Wi-Fi 音频传输。
- **约束**
  - 目标架构 `x86_64-apple-darwin`，最低 macOS 12。
  - 硬件、音频、系统权限和输入注入需要真实设备/原生应用验收，浏览器夹具不能替代。
  - 凭据不得进入仓库；敏感配置使用 macOS Keychain，并限制模型服务域名。
  - 保护用户未提交改动，跨前端/Rust 契约变更必须同步验证。
- **成功标准**
  - `npm run build`、`npm test -- --run` 与 `cargo test --manifest-path src-tauri/Cargo.toml` 通过。
  - Intel 构建与 `npm run verify:intel` 通过。
  - 核心硬件、语音和系统权限流程在真实 Intel Mac 与实板上完成端到端验收。
  - 文档索引、实施状态、接口说明和交接日志与实际产物一致。
- **角色**：拍板 = 项目所有者 / 主控 = 当前执行 Agent / 评审 = 与产出模型不同的 Agent / 其他 = 硬件与服务依赖方

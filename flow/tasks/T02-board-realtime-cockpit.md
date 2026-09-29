# T02 · 开发板实时语音直控驾驶舱

- **背景**：开发板按键、双向 PCM 与豆包实时语音已接入 EasyInput；T01 已完成驾驶舱嵌入页和父子页面消息桥，但实时语音 Function Calling 尚未向该桥发出命令。
- **目标**：打通开发板实时语音到驾驶舱页面控制的完整客户端链路，并明确固件是否需要升级。
- **输入**：现有 `src-tauri/src/realtime.rs` Function Calling 路由、`hardware-realtime-button` 事件、`src/pages/CockpitPage.tsx`、驾驶舱 `easyinput:cockpit-command` 消息桥。
- **产出**：受限 Schema 的驾驶舱内置工具；Rust 执行后发出的 Tauri 事件；EasyInput 自动切换驾驶舱并向 iframe 转发；调用结果回传；测试与固件接口/验收说明。
- **验收标准**：明确命令能控制总览、期次对比、作业、校友、学员、数据质量和第 1—7 期详情；非法模块/期次安全失败；Rust 与前端契约一致；`npm run build`、`npm test -- --run`、`cargo test --manifest-path src-tauri/Cargo.toml` 通过；文档明确客户端/固件边界。
- **验证**：`cockpit_navigate` 默认进入实时会话工具列表，模块枚举及第 1—7 期参数有 Rust 测试；EasyInput 前端构建和 13 项测试通过；Rust 68 项通过、1 项手动网络诊断按设计忽略；驾驶舱构建、6 项测试和 lint 通过。
- **状态**：完成（源码与自动化验证完成；真实开发板、Tauri WebView 和豆包在线 Function Calling 仍需实板端到端验收）

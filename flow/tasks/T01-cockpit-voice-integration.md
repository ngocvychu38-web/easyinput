# T01 · 驾驶舱语音容错与 EasyInput 集成

- **背景**：训练营驾驶舱已支持连续语音控制，但命令依赖关键词完全命中，且页面尚未出现在 EasyInput 主导航中。
- **目标**：提升相近语音文本的导航成功率，并把现有驾驶舱作为 EasyInput 内的正式页面入口。
- **输入**：`/Users/macforai/Documents/ChatGPT/训练营驾驶舱/site`、EasyInput React/Tauri 应用、用户确认的本轮需求。
- **产出**：可测试的驾驶舱模糊意图匹配器；EasyInput 驾驶舱页面、主导航入口、嵌入地址配置和必要 CSP；相关测试与文档增量。
- **验收标准**：常见同音/漏字/近似说法能切换到正确模块；候选得分接近时不执行误跳；指定期次仍可解析；EasyInput 浏览器模式能内嵌本地驾驶舱；Tauri CSP 仅放行明确的本地与生产驾驶舱来源；`npm run build`、`npm test -- --run`、驾驶舱测试与 `cargo test --manifest-path src-tauri/Cargo.toml` 通过。
- **验证**：驾驶舱 `npm test`（6 项）与 `npm run lint` 通过；EasyInput `npm run build`、`npm test -- --run`（12 项）通过；Rust 67 项通过、1 项手动网络诊断按设计忽略；本地 `3000` 与 `1420` 均返回 HTTP 200。
- **状态**：完成（真实 Intel Mac 的 Tauri WebView 麦克风授权与生产登录态另行验收）

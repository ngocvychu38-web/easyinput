# EasyInput · 协作约定（Claude Code / Codex 共用入口）

> `CLAUDE.md` 软链到本文件，两个工具读取同一份约定。项目真相源在文件里，不在对话里。
> 本文件是运行时合同：精要规则、项目约束与文档指针。完整方法论见 `flow/规范/`。

## 项目边界

- EasyInput 是一个单体 Tauri 2 桌面应用；`src/` 的 React/TypeScript 界面与 `src-tauri/` 的 Rust 本地能力共享版本、构建和发布。
- 仓库只保留一套根级 `flow/`、`docs/` 和 hooks；不要在 `src/` 或 `src-tauri/` 内复制控制面。
- 当前目标平台是 Intel Mac（`x86_64-apple-darwin`），最低 macOS 12。
- 当前仓库不包含开发板固件源码；涉及 USB HID、音频或固件配置时，必须明确客户端与固件的接口边界。

## 目录地图

- `flow/`：唯一控制层。`charter.md` 管目标，`plan.md` 管已确认计划，`进展.md` 顶部是当前交接棒，`decisions.md` 记过程决策，`踩坑记录.md` 记问题与经验，`tasks/` 放任务卡，`规范/` 放方法论详规。
- `docs/`：集中内容层，存放产品、架构、接口、教程、实施状态、评审和交付说明；入口见 `docs/README.md`。
- `src/`：React 18、TypeScript、Vite 界面、状态与前端测试。
- `src-tauri/`：Rust/Tauri 本地服务、硬件协议、系统集成、本地存储及 Rust 测试。
- `scripts/`：开发、签名、构建和架构验证脚本。
- `screenshots/`：产品验收截图；`image/` 与 `ppt-output/` 是现有素材/演示产物。
- `DESIGN.md`：跨页面的视觉与交互设计决策。

## 开工前必读

1. `flow/charter.md`
2. `flow/plan.md`（计划是契约，未确认不要偏离）
3. `flow/进展.md` 顶部一条
4. 被分配的任务卡 `flow/tasks/<...>.md`
5. 与任务直接相关的 `docs/` 文档及代码附近说明

## 常用命令

```bash
npm run dev
npm run tauri:dev
npm run build
npm test -- --run
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build:intel
npm run verify:intel
```

## 业务与安全约束

- 浏览器模式只适合界面开发；USB、Keychain、全局输入、麦克风及系统权限必须在 Tauri 应用和真实 Intel Mac 环境验证。
- 不把豆包 Access Token、火山方舟 API Key、实时语音 API Key、Wi-Fi 密码、签名身份或私钥写入仓库、日志、测试夹具或文档。
- 敏感凭据继续使用 macOS Keychain；新增模型调用必须维持服务域名限制。
- 改动前端与 Rust IPC/event 契约时，两端类型、错误处理、文档与测试必须同步。
- 不把 `dist/`、`node_modules/`、`src-tauri/target/` 等生成物当作源文件修改。
- 保留用户已有和未提交的改动；不要用破坏性 Git 命令覆盖现场。

## 工作与收工约定

- 先 plan 后 act；需要偏离时先更新 `flow/plan.md` 并取得确认。
- 一次会话聚焦一个目标，产出落文件，审稿模型与产出模型分离。
- 从根因解决问题，避免只覆盖症状的补丁。
- 收工时在 `flow/进展.md` 最上面追加一条，包含做了什么、为什么、怎么理解、产出路径、问题与解决、下一步，并在回复中同时贴出。
- 决策追加到 `flow/decisions.md`；问题与踩坑追加到 `flow/踩坑记录.md`。
- 交接只带“指针 + 增量”，不要在日志里重抄产物内容。

## 文档维护

新的设计哲学、心智模型、方向、结构、契约、长期约定或需持续参考的外部资料出现时，先提议修改点并确认，再更新相应文件。只记录从产物本身看不出的“为什么”，并保持精简。

- 工作流程：`flow/规范/工作流程.md`
- 文档维护：`flow/规范/文档维护SOP.md`
- 设计维护：`flow/规范/DESIGN维护SOP.md`
- 多子项目边界：`flow/规范/多子项目结构.md`
- hook 机制：`flow/规范/hook机制.md`

## 项目知识（durable）

- 前端与 Rust/Tauri 是同一个交付物的两层，不是两个独立项目。
- 客户端负责语音调用的本地直调、Function Calling 和动作执行；固件只需兼容现有实时通话触发与双向 PCM 协议。
- 生产发布依赖 Developer ID、公证账户和更新签名密钥；无正式签名的构建仅用于本机开发验证。

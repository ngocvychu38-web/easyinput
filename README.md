# EasyInput Intel Mac

EasyInput 0.1.29 的简体中文 Intel Mac 桌面端，目标为纯 `x86_64-apple-darwin`，最低支持 macOS 12。

## 当前实现

- 9 个唯一页面：概览/趋势、语音、历史、词库、键盘、设置、账户、帮助；键盘控制台含按键、麦克风、网络、音效、编程助手和键盘更新 6 个子页。
- Tauri 2.11.2 + React/Vite/TypeScript + Rust 应用外壳，关闭窗口时隐藏而不退出。
- 本机 `config.json`、`dictionary.json` 和 SQLite `history.db`；敏感值接口使用 macOS Keychain。
- USB HID 发现及配置分片，VID/PID、Report ID、2048 字节限制、52 字节分片和 CRC16-CCITT。
- 开发板语音键持续监听：兼容 `0x11` 专用 PTT 报告与旧固件右 Command 键盘报告，支持按住/切换两种触发模式。
- 豆包流式语音识别 2.0：默认使用 `volc.bigasr.sauc.duration`，旧版错误资源 ID 会自动迁移。
- 无硬件时可进入设备设置预览，浏览器开发模式提供可交互设备夹具；真实同步仍由 Tauri 设备适配层负责。
- `EIAU v2` 音频包、`EIHB` 心跳、`EICC` 控制包的字节级解析与 Golden Vector 测试。
- 豆包/火山引擎大模型流式语音识别配置页，支持 2.0/1.0 小时版与并发版 Resource ID、官方 WSS 握手测试和 Keychain 凭据保存。
- 录音 `sessionId`、设备 `endpointEpoch`、配置 `revision` 和长操作 `operationId` 边界。

EasyInput 自有账号协议、Developer ID、更新签名密钥和实板确认不在仓库中。语音识别改用豆包语音官方接口；Access Token 只保存在 macOS 钥匙串，并且后端拒绝把凭据发送到非官方域名。

## 开发

```bash
npm install
npm run dev
```

浏览器开发模式会使用脱敏本地夹具。启动原生应用：

```bash
npm run tauri:dev
```

## Intel 构建

```bash
rustup target add x86_64-apple-darwin  # 使用 rustup 时
npm run tauri:build:intel
npm run verify:intel
```

`src-tauri/tauri.conf.json` 已固定最低 macOS 版本 12.0，发布目标为 `.app` 和 `.dmg`。未配置 Developer ID 时只能产出本地开发包。

## 本机数据

Tauri 在 macOS 的应用数据目录中创建：

```text
~/Library/Application Support/pro.easyinput.desktop.intel/
  config.json
  dictionary.json
  history.db
```

豆包语音 Access Token 不在上述文件中，存储于登录钥匙串的 `pro.easyinput.desktop.intel / doubao-asr-access-token` 条目。

配置通过临时文件原子替换；未来版本或损坏配置不会被自动覆盖。恢复前应先调用备份流程。

## 安全边界

- Wi‑Fi 音频只应在可信局域网使用；当前固件 token 不是密码学认证。
- 固件页面只读，不刷写、不 OTA。
- 生产更新必须在配置签名公钥后才可安装，录音/同步期间禁止安装。
- 固件仓库代码未被复制；本项目仅依据公开线协议实现兼容层。

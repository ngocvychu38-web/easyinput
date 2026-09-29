# EasyInput 启动操作手册

适用项目：`/Users/macforai/Documents/ChatGPT/easyinput`

本手册覆盖 EasyInput 主程序、驾驶舱、英语学习、直播工作室和开发板语音识别。

## 一、首次准备

### 1. 检查环境

```bash
cd /Users/macforai/Documents/ChatGPT/easyinput
node -v
npm -v
cargo --version
```

Intel Mac 生产目标为 `x86_64-apple-darwin`。开发阶段可以直接使用本机架构运行。

### 2. 安装主项目依赖

```bash
cd /Users/macforai/Documents/ChatGPT/easyinput
npm install
```

直播工作室依赖单独安装：

```bash
cd /Users/macforai/Documents/ChatGPT/直播软件
npm install
```

三阶魔方服务使用现有项目源码，无需将模型密钥迁入 EasyInput。手动启动时运行 `npm run cube:dev`，服务地址为 `http://localhost:8787`；使用 `npm run tauri:dev` 时，开发脚本会自动启动该服务。

### 3. 准备配置

在 EasyInput 的设置页配置并保存：

- 豆包语音 App Key
- 豆包语音 Access Token
- 与账号匹配的 Resource ID
- 其他语音参数

凭据由应用保存到 macOS Keychain，不要写入 `.env`、代码、日志或 Git。

## 二、每次启动的推荐顺序

### 第 1 步：连接开发板

先通过 USB 或现有网络方式连接开发板，确认开发板已上电并处于可发现状态。

可以运行协议探针确认硬件链路：

```bash
cd /Users/macforai/Documents/ChatGPT/easyinput
python3 scripts/verify-live-board.py
```

看到 `startAck=true`、`stopAck=true` 和有效 `EIAU` 音频帧后，再启动直播字幕。

### 第 2 步：启动直播工作室服务

打开终端窗口 A：

```bash
cd /Users/macforai/Documents/ChatGPT/easyinput
npm run live:dev
```

服务默认地址：`http://localhost:3002`

英语学习必须使用带本地 SQLite 服务的启动方式。打开终端窗口 A2：

```bash
cd /Users/macforai/Documents/ChatGPT/英语学习
mkdir -p .runtime
PORT=3000 MILO_WORKER_PORT=3003 MILO_DB_PATH="$PWD/.runtime/milo.sqlite" npm run start:studio
```

不要用 `npm run dev` 代替上面的命令；`npm run dev` 只能提供页面预览，`/api/user-state` 会返回 503，英语学习页面会一直停在“正在读取你的小岛”。

健康检查：

```bash
curl http://localhost:3002/api/health
```

应返回包含 `"ok":true` 和 `"easyinput":true` 的 JSON。

### 启动三阶魔方

开发模式下，`npm run tauri:dev` 会在缺少服务时从相邻的 `三阶魔方09241` 项目启动 8787 服务。也可手动启动：

```bash
cd /Users/macforai/Documents/ChatGPT/easyinput
npm run cube:dev
```

健康检查：`curl http://localhost:8787/health`。服务正常后，在实时语音通话中说“打开魔方”进入双模型页面；再说“打乱魔方”触发页面右上角的同步打乱，或说“一起开始复原魔方”让 Jev 与 DeepSeek 两边同时各自运行七步复原。两边 API Key 仍在魔方自己的 AI 设置中配置。

### 第 3 步：启动 EasyInput 前端开发服务

打开终端窗口 B：

```bash
cd /Users/macforai/Documents/ChatGPT/easyinput
npm run dev -- --host 0.0.0.0
```

主前端默认地址：`http://localhost:1420`

### 第 4 步：启动 EasyInput 原生应用

开发模式打开终端窗口 C：

```bash
cd /Users/macforai/Documents/ChatGPT/easyinput
npm run tauri:dev
```

若只需要浏览器验证界面，不需要 USB、麦克风、Keychain 或开发板能力，可直接打开：

- 驾驶舱：`http://localhost:1420/?page=cockpit`
- 英语学习：`http://localhost:1420/?page=english`
- 直播：`http://localhost:1420/?page=live`

浏览器模式不能替代原生应用验证硬件语音和系统权限。

## 三、启动驾驶舱

1. 启动主前端或原生应用。
2. 打开 `http://localhost:1420/?page=cockpit`，或在左侧导航选择“驾驶舱”。
3. 等待驾驶舱页面加载完成。
4. 如果页面包含外部驾驶舱地址，确认网络和登录状态正常。
5. 语音控制前先确认 EasyInput 的语音配置已通过测试。

驾驶舱页面适合验证运营数据、导航和语音操作。页面加载超时时先检查外部页面是否能在浏览器中打开。

## 四、启动英语学习

1. 打开 `http://localhost:1420/?page=english`，或在左侧导航选择“英语学习”。
2. 选择学习内容或练习模块。
3. 需要语音输入时，在原生应用中授予麦克风权限，并确认开发板在线。
4. 先做一条短句测试，再开始连续练习。

浏览器只适合页面和交互检查；真实麦克风、开发板语音和系统权限请在 Tauri 原生应用中验证。

## 五、启动直播和硬件字幕

1. 确认直播服务已在 `3002` 端口运行。
2. 打开 `http://localhost:1420/?page=live`。
3. 点击“开始本机预览”。该操作会同时申请并启动开发板硬件字幕识别。
4. 说话后观察字幕气泡；正常情况下字幕应保持接近实时。
5. 说出已配置的语音控制词，例如“敷面膜”，验证贴纸特效。
6. 需要发布到声网时，再点击“发布到声网”。识别会话会幂等保持，不会重复启动。
7. 停止直播或关闭页面时，识别会话和开发板控制流应被释放。

直播字幕使用开发板 PCM 音频；观众音轨仍由直播页面的媒体输入提供。云端识别出现短暂断开时，客户端会自动重连。

## 六、生产构建启动

构建 Intel 应用：

```bash
cd /Users/macforai/Documents/ChatGPT/easyinput
npm run tauri:build:intel
```

应用产物：

```text
src-tauri/target/x86_64-apple-darwin/release/bundle/macos/EasyInput.app
```

双击该 `.app` 启动，或在终端运行：

```bash
open src-tauri/target/x86_64-apple-darwin/release/bundle/macos/EasyInput.app
```

使用生产应用时，直播工作室服务仍需单独启动：

```bash
cd /Users/macforai/Documents/ChatGPT/easyinput
npm run live:dev
```

## 七、常见问题

### 页面打不开

检查两个服务是否运行：

```bash
curl http://localhost:1420
curl http://localhost:3002/api/health
```

若 `1420` 无响应，重新执行 `npm run dev -- --host 0.0.0.0`；若 `3002` 无响应，重新执行 `npm run live:dev`。

### 直播字幕没有更新

按以下顺序检查：

1. 开发板是否在线。
2. `python3 scripts/verify-live-board.py` 是否能收到有效音频帧。
3. 是否在“开始本机预览”后才开始说话。
4. 豆包语音配置是否通过连接测试。
5. 是否重启过加载最新 Rust 代码的 EasyInput 应用。

### 字幕延迟较大

当前客户端已将直播音频队列限制在约 0.5 秒，并在识别服务落后时丢弃过载帧。若仍明显超过 1 秒，先重启识别会话；持续异常时保留发生时间、页面和服务日志用于进一步诊断。

### 驾驶舱或英语学习页面空白

确认使用的是主前端地址 `1420`，而不是直播服务地址 `3002`。检查浏览器开发者控制台和终端中的 Vite 输出。

## 八、停止程序

按启动顺序反向停止：

1. 在直播页面点击停止直播。
2. 关闭 EasyInput 原生应用。
3. 在终端 A、B、C 分别按 `Ctrl+C` 停止 `live:dev`、Vite 和 Tauri。

停止前不要直接拔出正在传输音频的开发板，以免留下未释放的直播会话。

## 九、开发验证命令

```bash
cd /Users/macforai/Documents/ChatGPT/easyinput
npm run build
npm test -- --run
cargo test --manifest-path src-tauri/Cargo.toml
```

直播项目单独验证：

```bash
cd /Users/macforai/Documents/ChatGPT/直播软件
npm run build
npm test
```

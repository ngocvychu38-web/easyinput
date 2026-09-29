# 驾驶舱语音与 EasyInput 集成说明

## 用户入口

EasyInput 主导航新增“驾驶舱”。开发模式与稳定应用都嵌入本机 `http://localhost:3001/dashboard`。`VITE_COCKPIT_URL` 只接受 `localhost` 或 `127.0.0.1` 的 HTTP 地址；远程地址会安全回退到本机入口。本机 3000 已由英语学习项目使用，驾驶舱固定使用 3001。

本地联调需要同时启动两个项目：

```bash
# 终端 1：训练营驾驶舱
cd "/Users/macforai/Documents/ChatGPT/训练营驾驶舱/site"
npm run dev -- --port 3001

# 终端 2：EasyInput
cd "/Users/macforai/Documents/ChatGPT/easyinput"
npm run dev
```

浏览器访问 `http://127.0.0.1:1420/?page=cockpit` 可直接进入驾驶舱，也可以从 EasyInput 主导航点击“驾驶舱”。进入后点击驾驶舱内的语音控制圆点并允许麦克风，即可连续说出指令。

## 近似语音匹配

匹配器不会要求识别文本与模块名称完全一致，处理顺序为：

1. 去掉“请、帮我、打开、切换到、页面”等口语包装词；
2. 修正常见 ASR 同音或形近错误，例如“作页分析 → 作业分析”；
3. 在模块别名中计算包含关系和编辑距离置信度；
4. 低于置信度阈值或两个候选过于接近时拒绝执行，避免误跳。

当前样例：

| 说法或识别结果 | 动作 |
|---|---|
| 打开作页分析 | 打开“作业分析” |
| 切换到学员画象 | 打开“学员画像” |
| 看一下数据制量 | 打开“数据质量与预警” |
| 打开其次对比 | 打开“期次对比” |
| 进入校友汇 | 打开“校友会” |
| 优秀学员列表 | 打开“优秀学员列表”，显示当前已提供的 45 / 126 条明细 |
| 查第五集 | 打开“第 5 期详情” |
| 打开作业还是学员 | 候选有歧义，不执行 |

## 页面通信与安全边界

EasyInput 不复制驾驶舱业务代码，而是通过受控 iframe/WebView 复用驾驶舱页面。iframe 显式声明麦克风能力；Tauri CSP 只允许本地 `localhost/127.0.0.1` 的 3000、3001 端口。

EasyInput 可以向嵌入页发送如下消息，驾驶舱只接受来自 EasyInput 本地开发地址或 Tauri 自定义协议来源、且 `event.source` 确认为父窗口的消息：

```ts
{
  type: "easyinput:cockpit-command",
  requestId: string,
  command: {
    action: "navigate" | "activateSystem" | "getState",
    target?: "overview" | "cohort-comparison" | "assignments" | "alumni" | "outstanding-students" | "learners" | "data-quality" | "cohort-detail",
    parameters?: { cohortId?: number }
  }
}
```

驾驶舱用 `easyinput:cockpit-result` 返回同一 `requestId` 的执行结果。EasyInput 的豆包实时语音现已内置 `cockpit_navigate` Function Calling：Rust 发出 `cockpit-command-request`，前端自动进入驾驶舱并转发命令；只有收到 iframe 的实际结果后，前端才通过一次性响应事件回传 Rust，随后结果才返回豆包。

除开发板实时语音外，驾驶舱内的连续 Web Speech 语音条仍可独立使用。

## 验证边界

浏览器模式可以验证导航、嵌入和 Web Speech 语音控制。Tauri WebView 的麦克风授权、生产地址登录态和 Intel Mac 原生运行仍需在真实应用环境验收。

当前固定使用本地驾驶舱最新代码，不依赖线上驾驶舱部署。使用驾驶舱前需先在本机启动 3001 服务。

开发板无需理解驾驶舱业务命令；固件兼容条件和实板步骤见 [开发板实时语音直控驾驶舱：固件要求](开发板实时语音直控驾驶舱-固件要求.md)。

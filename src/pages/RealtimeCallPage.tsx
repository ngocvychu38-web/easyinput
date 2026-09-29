import { AudioLines, CircleStop, ExternalLink, FileDown, Headphones, Mic2, PhoneCall, QrCode, Radio, ShieldCheck, Volume2, Zap } from "lucide-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { exportRealtimeDiagnostics, getRealtimeCallState, getRealtimeDiagnosticsInfo, interruptRealtimeCall, launchPaymentMvp, setRealtimeWakeWordMode, startRealtimeCall, stopRealtimeCall } from "../api";
import type { HardwareRealtimeButtonEvent, RealtimeCallPhase, RealtimeCallState, RealtimeDiagnosticsInfo } from "../types";
import { Button, SectionLabel, Toggle } from "../components/Ui";

const EMPTY_STATE: RealtimeCallState = {
  phase: "Idle", userText: "", assistantText: "", elapsedMs: 0, inputPackets: 0, outputPackets: 0, toolCallCount: 0
};

const phaseCopy: Record<RealtimeCallPhase, { label: string; hint: string }> = {
  Idle: { label: "等待通话", hint: "点击开始，或按下开发板的实时通话键" },
  Connecting: { label: "正在连接", hint: "正在连接开发板与豆包实时语音服务" },
  Listening: { label: "正在聆听", hint: "可以直接说话，模型回复时也支持语音打断" },
  Speaking: { label: "正在回答", hint: "声音正通过开发板扬声器播放" },
  Closing: { label: "正在结束", hint: "正在安全关闭云端会话与硬件音频流" },
  Error: { label: "通话异常", hint: "请根据下方提示检查配置或设备连接" }
};

function formatDuration(elapsedMs: number) {
  const total = Math.floor(elapsedMs / 1000);
  return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}

function isActive(phase: RealtimeCallPhase) {
  return phase === "Connecting" || phase === "Listening" || phase === "Speaking" || phase === "Closing";
}

function formatBytes(bytes: number) {
  if (!bytes) return "等待生成";
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}

function errorHint(message: string) {
  if (message.includes("52000033") || message.includes("AudioServerNoAudioInputTooLongError")) {
    return "开发板曾停止上传麦克风音频。新版固件会取消 5 分钟限制，并在短暂断流时自动暂停云端音频输入。";
  }
  return "确认实时语音 API Key 已保存、功能已启用，并且开发板与电脑在同一网络。";
}

export function RealtimeCallPage({ hardwareTrigger, openSettings }: { hardwareTrigger?: HardwareRealtimeButtonEvent; openSettings(): void }) {
  const [call, setCall] = useState<RealtimeCallState>(EMPTY_STATE);
  const [actionError, setActionError] = useState("");
  const [diagnostics, setDiagnostics] = useState<RealtimeDiagnosticsInfo>();
  const [exportMessage, setExportMessage] = useState("");
  const [paymentUrl, setPaymentUrl] = useState("");
  const [paymentBusy, setPaymentBusy] = useState(false);
  const [paymentMessage, setPaymentMessage] = useState("");
  const [paymentError, setPaymentError] = useState("");
  // 待命开关默认开启；只有进行中的实时通话状态才覆盖此初值。
  const [wakeWordMode, setWakeWordMode] = useState(true);
  const actionBusy = useRef(false);
  const lastHardwareSequence = useRef(0);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void getRealtimeCallState().then(value => { if (!disposed) { setCall(value); if (isActive(value.phase)) setWakeWordMode(value.wakeWordEnabled ?? true); } }).catch(reason => {
      if (!disposed) setActionError(reason instanceof Error ? reason.message : String(reason));
    });
    void getRealtimeDiagnosticsInfo().then(value => { if (!disposed) setDiagnostics(value); }).catch(() => {});
    void listen<RealtimeCallState>("realtime-call-state", event => {
      setCall(event.payload);
      setWakeWordMode(event.payload.wakeWordEnabled ?? true);
    }).then(value => {
      if (disposed) value(); else unlisten = value;
    });
    return () => { disposed = true; unlisten?.(); };
  }, []);

  useEffect(() => {
    if (isActive(call.phase)) return;
    void getRealtimeDiagnosticsInfo().then(setDiagnostics).catch(() => {});
  }, [call.phase]);

  const start = async () => {
    if (actionBusy.current) return;
    actionBusy.current = true; setActionError("");
    try {
      const result = await startRealtimeCall(wakeWordMode);
      if (!result.ok) setActionError(result.message || "实时通话启动失败");
    } catch (reason) {
      setActionError(reason instanceof Error ? reason.message : String(reason));
    } finally { actionBusy.current = false; }
  };
  const stop = async () => {
    if (actionBusy.current || call.phase === "Closing") return;
    actionBusy.current = true; setActionError("");
    try {
      const result = await stopRealtimeCall();
      if (!result.ok) setActionError(result.message || "实时通话结束失败");
    } catch (reason) {
      setActionError(reason instanceof Error ? reason.message : String(reason));
    } finally { actionBusy.current = false; }
  };
  const interrupt = async () => {
    setActionError("");
    try {
      const result = await interruptRealtimeCall();
      if (!result.ok) setActionError(result.message || "打断失败");
    } catch (reason) {
      setActionError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const changeWakeWordMode = async (enabled: boolean) => {
    setWakeWordMode(enabled);
    if (!active) return;
    setActionError("");
    try {
      const result = await setRealtimeWakeWordMode(enabled);
      if (!result.ok) {
        setWakeWordMode(!enabled);
        setActionError(result.message || "唤醒词待命状态更新失败");
      }
    } catch (reason) {
      setWakeWordMode(!enabled);
      setActionError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const exportLogs = async () => {
    setExportMessage("");
    setActionError("");
    try {
      const stamp = new Date().toISOString().replaceAll(":", "-").replace(/\.\d{3}Z$/, "");
      const path = await saveDialog({ defaultPath: `EasyInput-realtime-${stamp}.jsonl`, filters: [{ name: "JSON Lines", extensions: ["jsonl"] }] });
      if (!path) return;
      const result = await exportRealtimeDiagnostics(path);
      if (!result.ok) setActionError(result.message || "导出诊断日志失败");
      else setExportMessage(`诊断日志已导出：${result.data?.path || path}`);
      setDiagnostics(await getRealtimeDiagnosticsInfo());
    } catch (reason) {
      setActionError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const openPaymentMvp = async () => {
    if (paymentBusy) return;
    setPaymentBusy(true); setPaymentError(""); setPaymentMessage("");
    try {
      const result = await launchPaymentMvp(paymentUrl);
      if (!result.ok || !result.data) {
        setPaymentError(result.message || "支付验证页启动失败");
        return;
      }
      const domainHint = result.data.looksLikeMcdDomain ? "域名形式符合 mcd.cn" : "非 mcd.cn 域名，请核对是否为支付合作方";
      setPaymentMessage(`已在默认浏览器打开二维码页 · ${result.data.paymentHost} · ${domainHint}`);
      setPaymentUrl("");
    } catch (reason) {
      setPaymentError(reason instanceof Error ? reason.message : String(reason));
    } finally { setPaymentBusy(false); }
  };

  useEffect(() => {
    if (!hardwareTrigger?.pressed || hardwareTrigger.sequence === lastHardwareSequence.current) return;
    lastHardwareSequence.current = hardwareTrigger.sequence;
    if (isActive(call.phase)) void stop(); else void start();
  }, [hardwareTrigger]);

  const phase = phaseCopy[call.phase];
  const active = isActive(call.phase);
  return <div className={`page realtime-call-page phase-${call.phase.toLowerCase()}`}>
    <div className="realtime-call-head">
      <div><SectionLabel index="01">全双工语音</SectionLabel><h1>实时对话</h1><p>开发板麦克风收音，模型生成的语音通过开发板扬声器播放。</p></div>
      <div className="realtime-device-route"><span><Mic2 />开发板麦克风</span><i /><span><Radio />豆包实时语音</span><i /><span><Volume2 />开发板扬声器</span></div>
    </div>

    <section className="call-stage">
      <div className={`call-orb ${active ? "active" : ""}`}><Headphones /></div>
      <div className="call-phase"><i />{phase.label}</div>
      <b className="call-timer">{formatDuration(call.elapsedMs)}</b>
      <p>{phase.hint}</p>
      <div className="call-actions">
        {!active ? <Button kind="primary" onClick={() => void start()}><PhoneCall />开始实时通话</Button> : <Button kind="danger" onClick={() => void stop()} disabled={call.phase === "Closing"}><CircleStop />结束通话</Button>}
        {call.phase === "Speaking" && <Button onClick={() => void interrupt()}><Zap />打断回答</Button>}
        <Button onClick={openSettings}><AudioLines />语音服务配置</Button>
        <Button onClick={() => void exportLogs()}><FileDown />导出诊断日志</Button>
      </div>
      <div className="wake-word-setting">
        <div><b>唤醒词待命</b><span>{active && call.wakeWordEnabled ? (call.wakeWordArmed ? "助手休息中 · 说“八弟八弟”或近音唤醒" : "已开启 · 说“闭嘴吧”或“请你休息一下”可让助手待命") : wakeWordMode ? "下次通话开启 · 说“闭嘴吧”或“请你休息一下”可让助手待命" : "关闭时保持当前实时对话行为"}</span></div>
        <Toggle value={wakeWordMode} onChange={value => void changeWakeWordMode(value)} label="启用唤醒词待命" disabled={call.phase === "Connecting" || call.phase === "Closing"} />
      </div>
    </section>

    {(actionError || call.error) && <div className="voice-error">{actionError || call.error}<small>{errorHint(actionError || call.error || "")}</small></div>}
    {exportMessage && <div className="diagnostics-exported">{exportMessage}</div>}
    {call.lastToolStatus && <div className="tool-call-status"><Zap size={14} /><span>{call.lastToolStatus}</span></div>}

    <div className="conversation-grid">
      <article><header><Mic2 /><span>你说</span><small>{call.inputPackets} 帧上行</small></header><p>{call.userText || "通话开始后，识别到的内容会显示在这里。"}</p></article>
      <article><header><Volume2 /><span>大模型回答</span><small>{call.outputPackets} 帧下行</small></header><p>{call.assistantText || "模型的文字回复会显示在这里，并同步从开发板扬声器播放。"}</p></article>
    </div>
    <div className="call-diagnostics"><span>会话 {call.sessionId ? call.sessionId.slice(0, 8) : "—"}</span><span>工具调用 {call.toolCallCount ?? 0} 次</span><span>LogID {call.logId || "—"}</span><span className="diagnostic-file" title={diagnostics?.path}>诊断日志 {formatBytes(diagnostics?.bytes || 0)}</span></div>

    <section className="payment-mvp-card">
      <div className="payment-mvp-copy">
        <div><SectionLabel index="02">支付链路 MVP</SectionLabel><h2>验证 payH5Url 跨设备扫码</h2></div>
        <span><ShieldCheck />仅在本机内存中使用，不创建订单、不自动支付</span>
      </div>
      <p>粘贴一次真实、未过期的 <code>create-order.payH5Url</code>。EasyInput 会在默认浏览器显示本地生成的二维码，并保留当前设备直接打开的入口。</p>
      <div className="payment-mvp-form">
        <input
          type="url"
          value={paymentUrl}
          onChange={event => setPaymentUrl(event.target.value)}
          onKeyDown={event => { if (event.key === "Enter") void openPaymentMvp(); }}
          placeholder="https://…（真实 payH5Url，不会保存）"
          aria-label="麦当劳 payH5Url"
          autoComplete="off"
          spellCheck={false}
        />
        <Button kind="primary" onClick={() => void openPaymentMvp()} disabled={paymentBusy || !paymentUrl.trim()}><QrCode />{paymentBusy ? "正在生成" : "在浏览器显示二维码"}</Button>
      </div>
      {paymentError && <div className="payment-mvp-result error">{paymentError}</div>}
      {paymentMessage && <div className="payment-mvp-result success"><ExternalLink />{paymentMessage}</div>}
      <div className="payment-mvp-checks"><span>① 手机扫码能否打开</span><span>② 是否要求微信/支付宝环境</span><span>③ 金额与订单是否一致</span><span>④ 支付后能否正常回跳</span></div>
    </section>
  </div>;
}

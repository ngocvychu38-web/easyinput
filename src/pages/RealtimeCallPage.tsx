import { AudioLines, CircleStop, Headphones, Mic2, PhoneCall, Radio, Volume2, Zap } from "lucide-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { getRealtimeCallState, interruptRealtimeCall, startRealtimeCall, stopRealtimeCall } from "../api";
import type { HardwareRealtimeButtonEvent, RealtimeCallPhase, RealtimeCallState } from "../types";
import { Button, SectionLabel } from "../components/Ui";

const EMPTY_STATE: RealtimeCallState = {
  phase: "Idle", userText: "", assistantText: "", elapsedMs: 0, inputPackets: 0, outputPackets: 0
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

export function RealtimeCallPage({ hardwareTrigger, openSettings }: { hardwareTrigger?: HardwareRealtimeButtonEvent; openSettings(): void }) {
  const [call, setCall] = useState<RealtimeCallState>(EMPTY_STATE);
  const [actionError, setActionError] = useState("");
  const actionBusy = useRef(false);
  const lastHardwareSequence = useRef(0);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void getRealtimeCallState().then(value => { if (!disposed) setCall(value); }).catch(reason => {
      if (!disposed) setActionError(reason instanceof Error ? reason.message : String(reason));
    });
    void listen<RealtimeCallState>("realtime-call-state", event => setCall(event.payload)).then(value => {
      if (disposed) value(); else unlisten = value;
    });
    return () => { disposed = true; unlisten?.(); };
  }, []);

  const start = async () => {
    if (actionBusy.current) return;
    actionBusy.current = true; setActionError("");
    try {
      const result = await startRealtimeCall();
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

  useEffect(() => {
    if (!hardwareTrigger?.pressed || hardwareTrigger.sequence === lastHardwareSequence.current) return;
    lastHardwareSequence.current = hardwareTrigger.sequence;
    if (isActive(call.phase)) void stop(); else void start();
  }, [hardwareTrigger]);

  const phase = phaseCopy[call.phase];
  const active = isActive(call.phase);
  return <div className={`page realtime-call-page phase-${call.phase.toLowerCase()}`}>
    <div className="realtime-call-head">
      <div><SectionLabel index="01">全双工语音</SectionLabel><h1>和豆包实时对话</h1><p>开发板麦克风收音，模型生成的语音通过开发板扬声器播放。</p></div>
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
      </div>
    </section>

    {(actionError || call.error) && <div className="voice-error">{actionError || call.error}<small>确认实时语音 API Key 已保存、功能已启用，并且开发板与电脑在同一网络。</small></div>}

    <div className="conversation-grid">
      <article><header><Mic2 /><span>你说</span><small>{call.inputPackets} 帧上行</small></header><p>{call.userText || "通话开始后，识别到的内容会显示在这里。"}</p></article>
      <article><header><Volume2 /><span>豆包回答</span><small>{call.outputPackets} 帧下行</small></header><p>{call.assistantText || "模型的文字回复会显示在这里，并同步从开发板扬声器播放。"}</p></article>
    </div>
    <div className="call-diagnostics"><span>会话 {call.sessionId ? call.sessionId.slice(0, 8) : "—"}</span><span>LogID {call.logId || "—"}</span><span>按开发板“实时通话”键可开始或结束</span></div>
  </div>;
}

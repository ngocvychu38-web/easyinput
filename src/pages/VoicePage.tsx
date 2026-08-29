import { useEffect, useRef, useState } from "react";
import { Check, Clipboard, Mic, RotateCcw, Square } from "lucide-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { pushRecordingAudio, startRecording, stopRecording } from "../api";
import type { HardwareEditButtonEvent, HardwareVoiceButtonEvent, RecordingPhase, RuntimeSnapshot, SpeechSessionEvent, SpeechTranscriptEvent } from "../types";
import { Button, SectionLabel } from "../components/Ui";

type Capture = {
  stream: MediaStream;
  context: AudioContext;
  source: MediaStreamAudioSourceNode;
  processor: ScriptProcessorNode;
  silent: GainNode;
};

function to16kPcm(input: Float32Array, inputRate: number): number[] {
  const ratio = inputRate / 16_000;
  const outputLength = Math.max(1, Math.floor(input.length / ratio));
  const output = new Array<number>(outputLength);
  for (let index = 0; index < outputLength; index++) {
    const start = Math.floor(index * ratio);
    const end = Math.max(start + 1, Math.min(input.length, Math.floor((index + 1) * ratio)));
    let sum = 0;
    for (let source = start; source < end; source++) sum += input[source];
    const sample = Math.max(-1, Math.min(1, sum / (end - start)));
    output[index] = sample < 0 ? Math.round(sample * 32768) : Math.round(sample * 32767);
  }
  return output;
}

function microphoneError(reason: unknown) {
  if (reason instanceof DOMException) {
    if (reason.name === "NotAllowedError") return "没有麦克风权限。请在“系统设置 → 隐私与安全性 → 麦克风”中允许 EasyInput。";
    if (reason.name === "NotFoundError") return "没有找到可用的麦克风。";
    if (reason.name === "NotReadableError") return "麦克风正被其他应用独占，暂时无法读取。";
  }
  return reason instanceof Error ? reason.message : String(reason);
}

export function VoicePage({ runtime, refresh, hardwareTrigger, hardwareEditTrigger }: { runtime: RuntimeSnapshot; refresh(): Promise<void>; hardwareTrigger?: HardwareVoiceButtonEvent; hardwareEditTrigger?: HardwareEditButtonEvent }) {
  const [phase, setPhase] = useState<RecordingPhase>(runtime.recording.phase);
  const [sessionId, setSessionId] = useState<string>();
  const [elapsedMs, setElapsedMs] = useState(runtime.recording.elapsedMs);
  const [transcript, setTranscript] = useState(runtime.recording.partialText);
  const [definite, setDefinite] = useState(false);
  const [level, setLevel] = useState(0);
  const [error, setError] = useState(runtime.recording.error ?? "");
  const [copied, setCopied] = useState(false);
  const sessionRef = useRef<string>();
  const captureRef = useRef<Capture>();
  const sendChain = useRef<Promise<void>>(Promise.resolve());
  const timer = useRef<number>();
  const startedAt = useRef(0);
  const phaseRef = useRef<RecordingPhase>(runtime.recording.phase);
  const startInFlight = useRef(false);
  const releaseRequested = useRef(false);
  const handledHardwareSequence = useRef<number>();
  const handledEditSequence = useRef<number>();
  const [editMode,setEditMode]=useState(false);
  const [editHasSelection,setEditHasSelection]=useState(false);

  useEffect(() => { phaseRef.current = phase; }, [phase]);

  const stopLocalCapture = () => {
    window.clearInterval(timer.current);
    const capture = captureRef.current;
    captureRef.current = undefined;
    if (!capture) return;
    capture.processor.disconnect();
    capture.source.disconnect();
    capture.silent.disconnect();
    capture.stream.getTracks().forEach(track => track.stop());
    void capture.context.close();
    setLevel(0);
  };

  const finish = async () => {
    const current = sessionRef.current;
    if (!current || phaseRef.current === "Draining") return;
    phaseRef.current = "Draining";
    setPhase("Draining");
    stopLocalCapture();
    await Promise.race([
      sendChain.current,
      new Promise<void>(resolve => window.setTimeout(resolve, 600))
    ]);
    const result = await stopRecording(current);
    if (!result.ok) {
      setError(result.message ?? "结束转写失败");
      phaseRef.current = "Error";
      setPhase("Error");
      sessionRef.current = undefined;
      setSessionId(undefined);
    }
  };

  useEffect(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];
    void refresh();
    void Promise.all([
      listen<SpeechTranscriptEvent>("speech-transcript", event => {
        if (event.payload.sessionId !== sessionRef.current) return;
        setTranscript(event.payload.text);
        setDefinite(event.payload.definite);
      }),
      listen<SpeechSessionEvent>("speech-session", event => {
        if (event.payload.sessionId !== sessionRef.current) return;
        stopLocalCapture();
        setTranscript(event.payload.text);
        setElapsedMs(event.payload.durationMs);
        setError(event.payload.message ?? "");
        phaseRef.current = event.payload.phase;
        setPhase(event.payload.phase);
        setDefinite(event.payload.phase === "Idle" && Boolean(event.payload.text));
        sessionRef.current = undefined;
        setSessionId(undefined);
        setEditMode(false);
        void refresh();
      })
    ]).then(values => disposed ? values.forEach(fn => fn()) : unlisteners.push(...values));
    return () => {
      disposed = true;
      unlisteners.forEach(fn => fn());
      const current = sessionRef.current;
      stopLocalCapture();
      if (current) void stopRecording(current);
    };
  }, []);

  const begin = async (sourceKind: "Computer" | "Keyboard" | "KeyboardEdit" = "Computer") => {
    if (startInFlight.current || !["Idle", "Error"].includes(phaseRef.current)) return;
    if (!navigator.mediaDevices?.getUserMedia) { setError("当前系统 WebView 不支持麦克风采集。"); return; }
    startInFlight.current = true;
    phaseRef.current = "Preparing";
    setPhase("Preparing"); setError(""); setTranscript(""); setDefinite(false); setCopied(false); setElapsedMs(0);setEditMode(sourceKind==="KeyboardEdit");
    let stream: MediaStream | undefined;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true } });
      const result = await startRecording(sourceKind);
      if (!result.ok || !result.data) throw new Error(result.message ?? "无法启动豆包实时转写");
      const current = result.data.sessionId;
      const context = new AudioContext();
      const source = context.createMediaStreamSource(stream);
      const processor = context.createScriptProcessor(4096, 1, 1);
      const silent = context.createGain(); silent.gain.value = 0;
      sessionRef.current = current; setSessionId(current); sendChain.current = Promise.resolve();
      processor.onaudioprocess = event => {
        if (sessionRef.current !== current) return;
        const input = event.inputBuffer.getChannelData(0);
        let energy = 0; for (let index = 0; index < input.length; index++) energy += input[index] * input[index];
        setLevel(Math.min(1, Math.sqrt(energy / input.length) * 5));
        const samples = to16kPcm(input, context.sampleRate);
        sendChain.current = sendChain.current.then(async () => {
          const sent = await pushRecordingAudio(current, samples);
          if (!sent.ok) throw new Error(sent.message ?? "发送语音分片失败");
        }).catch(reason => {
          setError(reason instanceof Error ? reason.message : String(reason));
        });
      };
      source.connect(processor); processor.connect(silent); silent.connect(context.destination);
      captureRef.current = { stream, context, source, processor, silent };
      phaseRef.current = "Recording";
      startedAt.current = Date.now(); setPhase("Recording");
      timer.current = window.setInterval(() => {
        const elapsed = Date.now() - startedAt.current; setElapsedMs(elapsed);
        if (elapsed >= 180_000) void finish();
      }, 200);
      startInFlight.current = false;
      if (releaseRequested.current) { releaseRequested.current = false; void finish(); }
    } catch (reason) {
      stream?.getTracks().forEach(track => track.stop());
      if (sessionRef.current) void stopRecording(sessionRef.current);
      setError(microphoneError(reason)); setPhase("Error");
      phaseRef.current = "Error";
      startInFlight.current = false;
      sessionRef.current = undefined; setSessionId(undefined);
    }
  };

  useEffect(() => {
    if (!hardwareTrigger || handledHardwareSequence.current === hardwareTrigger.sequence) return;
    handledHardwareSequence.current = hardwareTrigger.sequence;
    const toggle = runtime.keyboardConfig.pttMode === "Toggle";
    if (toggle) {
      if (!hardwareTrigger.pressed) return;
      if (["Recording", "Preparing"].includes(phaseRef.current)) { releaseRequested.current = phaseRef.current === "Preparing"; if (phaseRef.current === "Recording") void finish(); }
      else void begin("Keyboard");
      return;
    }
    if (hardwareTrigger.pressed) { releaseRequested.current = false; void begin("Keyboard"); }
    else { releaseRequested.current = true; if (phaseRef.current === "Recording") void finish(); }
  }, [hardwareTrigger?.sequence]);

  useEffect(() => {
    if (!hardwareEditTrigger || handledEditSequence.current === hardwareEditTrigger.sequence) return;
    handledEditSequence.current = hardwareEditTrigger.sequence;
    if (hardwareEditTrigger.pressed) setEditHasSelection(hardwareEditTrigger.hasSelection);
    const toggle = runtime.keyboardConfig.pttMode === "Toggle";
    if (toggle) {
      if (!hardwareEditTrigger.pressed) return;
      if (["Recording", "Preparing"].includes(phaseRef.current)) { releaseRequested.current = phaseRef.current === "Preparing"; if (phaseRef.current === "Recording") void finish(); }
      else void begin("KeyboardEdit");
      return;
    }
    if (hardwareEditTrigger.pressed) { releaseRequested.current = false; void begin("KeyboardEdit"); }
    else { releaseRequested.current = true; if (phaseRef.current === "Recording") void finish(); }
  }, [hardwareEditTrigger?.sequence]);

  const active = phase === "Recording";
  const busy = phase === "Preparing" || phase === "Draining";
  const serviceReady = runtime.voiceService === "Connected";
  const showSpeechConfigHint = /鉴权|资源 ID|Access Token|App ID|应用 ID|令牌|Token|语音识别未启用/.test(error);
  const time = `${String(Math.floor(elapsedMs / 60_000)).padStart(2,"0")}:${String(Math.floor(elapsedMs / 1000) % 60).padStart(2,"0")}.${String(Math.floor(elapsedMs / 100) % 10)}`;
  const stateLabel = phase === "Preparing" ? "正在连接豆包语音…" : phase === "Recording" ? (editMode?"正在听取编辑问题":"正在实时转写") : phase === "Draining" ? (editMode?"正在生成方舟回答…":"正在生成最终结果…") : phase === "Error" ? (editMode?"语音编辑未完成":"转写未完成") : "准备就绪";
  const bars = Array.from({ length: 28 }, (_, index) => Math.max(4, Math.round((active ? 10 + level * 34 * (.35 + .65 * Math.abs(Math.sin(index * .72 + elapsedMs / 260))) : 4))));

  return <div className="page voice-page realtime-page">
    <div className="voice-head">
      <div><SectionLabel index="01">{editMode?"语音编辑":"实时转写"}</SectionLabel><h1>{editMode?(editHasSelection?"说出问题，回答将替换选中文本":"说出问题，回答将写入光标处"):"说话，文字会在这里出现"}</h1><p>{editMode?"先由豆包语音识别问题，再由火山方舟文本模型生成回答。":"调用已配置的豆包大模型流式语音识别，音频以 16 kHz 单声道 PCM 实时发送。"}</p></div>
      <div className="status-pills"><span><Mic size={15}/> 电脑麦克风</span>{runtime.device !== "Disconnected" && <span className="ready"><i />开发板按键已启用</span>}<span className={serviceReady ? "ready" : "offline"}><i />{serviceReady ? "豆包语音已配置" : "豆包语音未启用"}</span></div>
    </div>

    <section className={`live-transcript-card ${active ? "is-recording" : ""}`}>
      <div className="transcript-toolbar">
        <div className="live-state"><i />{stateLabel}<b>{time}</b></div>
        <div className="transcript-actions">
          <button disabled={!transcript || active || busy} onClick={() => { setTranscript(""); setDefinite(false); }}><RotateCcw size={14}/>清空</button>
          <button disabled={!transcript} onClick={async () => { await navigator.clipboard.writeText(transcript); setCopied(true); window.setTimeout(() => setCopied(false), 1500); }}>{copied ? <Check size={14}/> : <Clipboard size={14}/>} {copied ? "已复制" : "复制"}</button>
        </div>
      </div>
      <div className={`transcript-canvas ${transcript ? "has-text" : ""}`} aria-live="polite">
        {transcript ? <p>{transcript}<span className={definite ? "final-mark" : "live-caret"}>{definite ? <Check size={15}/> : ""}</span></p> : <div className="transcript-placeholder"><Mic size={25}/><b>{active ? "请开始说话" : "点击下方按钮或按开发板语音键"}</b><span>{active ? "识别结果会随说话内容实时更新" : "最长可连续录音 3 分钟"}</span></div>}
      </div>
      <div className="audio-meter" aria-label="麦克风音量">{bars.map((height,index)=><i key={index} style={{ height }} />)}</div>
    </section>

    {error && <div className="voice-error">{error}{showSpeechConfigHint && <small>如鉴权或资源 ID 不匹配，请到右上角“语音服务配置”检查参数。</small>}</div>}
    <div className="record-controls">
      <Button kind="primary" disabled={busy} onClick={() => active ? void finish() : void begin("Computer")}>{active ? <><Square size={14} fill="currentColor"/>结束并生成结果</> : busy ? stateLabel : <><Mic size={16}/>开始实时转写</>}</Button>
      <span>{sessionId ? `会话 ${sessionId.slice(0,8)}` : "转写完成后会自动保存到历史记录"}</span>
    </div>
    <div className="voice-status"><span>状态 · {stateLabel}</span><span>采样 · 16 kHz / 16-bit / 单声道</span><span>最长时长 · 03:00</span></div>
  </div>;
}

import { useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { currentMonitor, getCurrentWindow, LogicalSize, PhysicalPosition } from "@tauri-apps/api/window";
import { getRuntimeSnapshot, pushRecordingAudio, startRecording, stopRecording } from "../api";
import type { HardwareEditButtonEvent, HardwareVoiceButtonEvent, RecordingPhase, SpeechSessionEvent, SpeechTranscriptEvent } from "../types";

type SourceKind = "Keyboard" | "KeyboardEdit";
type Capture = {
  stream: MediaStream;
  context: AudioContext;
  source: MediaStreamAudioSourceNode;
  processor: ScriptProcessorNode;
  silent: GainNode;
};

const WAVE_POINTS = 72;

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
    if (reason.name === "NotAllowedError") return "请在系统设置中允许 EasyInput 使用麦克风";
    if (reason.name === "NotFoundError") return "没有找到可用的麦克风";
    if (reason.name === "NotReadableError") return "麦克风正被其他应用占用";
  }
  return reason instanceof Error ? reason.message : String(reason);
}

function statusText(phase: RecordingPhase, editMode: boolean) {
  if (phase === "Preparing") return "正在连接语音服务";
  if (phase === "Recording") return editMode ? "正在听取编辑指令" : "正在聆听";
  if (phase === "Draining") return editMode ? "正在生成并写入" : "正在识别并写入";
  if (phase === "Error") return "语音输入未完成";
  return "语音输入完成";
}

export function VoiceOverlay() {
  const [phase, setPhase] = useState<RecordingPhase>("Idle");
  const [transcript, setTranscript] = useState("");
  const [message, setMessage] = useState("");
  const [wave, setWave] = useState<number[]>(() => Array(WAVE_POINTS).fill(0.04));
  const [editMode, setEditMode] = useState(false);
  const [editHasSelection, setEditHasSelection] = useState(false);
  const [elapsedMs, setElapsedMs] = useState(0);
  const [motionPhase, setMotionPhase] = useState(0);
  const sessionRef = useRef<string>();
  const captureRef = useRef<Capture>();
  const sendChain = useRef<Promise<void>>(Promise.resolve());
  const phaseRef = useRef<RecordingPhase>("Idle");
  const startInFlight = useRef(false);
  const releaseRequested = useRef(false);
  const activeSource = useRef<SourceKind>();
  const heardEditSpeech = useRef(false);
  const lastEditSpeechAt = useRef(0);
  const silenceTimer = useRef<number>();
  const triggerMode = useRef("Hold");
  const handledVoiceSequence = useRef(0);
  const handledEditSequence = useRef(0);
  const hideTimer = useRef<number>();
  const textRef = useRef<HTMLDivElement>(null);
  const startedAtRef = useRef(0);
  const overlayWindow = useMemo(() => getCurrentWindow(), []);

  const setCurrentPhase = (next: RecordingPhase) => {
    phaseRef.current = next;
    setPhase(next);
  };

  const showOverlay = async () => {
    window.clearTimeout(hideTimer.current);
    try {
      await overlayWindow.setSize(new LogicalSize(700, 160));
      const monitor = await currentMonitor();
      if (monitor) {
        const size = await overlayWindow.outerSize();
        const x = monitor.position.x + Math.round((monitor.size.width - size.width) / 2);
        const y = monitor.position.y + monitor.size.height - size.height - Math.round(92 * monitor.scaleFactor);
        await overlayWindow.setPosition(new PhysicalPosition(x, y));
      }
      await overlayWindow.show();
    } catch {
      // A failed position adjustment must not prevent the recording itself.
      await overlayWindow.show().catch(() => undefined);
    }
  };

  const hideLater = (delay = 1800) => {
    window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => void overlayWindow.hide(), delay);
  };

  const stopLocalCapture = () => {
    window.clearInterval(silenceTimer.current);
    silenceTimer.current = undefined;
    const capture = captureRef.current;
    captureRef.current = undefined;
    if (!capture) return;
    capture.processor.disconnect();
    capture.source.disconnect();
    capture.silent.disconnect();
    capture.stream.getTracks().forEach(track => track.stop());
    void capture.context.close();
    setWave(previous => previous.map(() => 0.04));
  };

  const finish = async () => {
    const current = sessionRef.current;
    if (!current || phaseRef.current === "Draining") return;
    setCurrentPhase("Draining");
    stopLocalCapture();
    // IPC audio pushes are normally immediate. Never let one stale push keep
    // the hardware release event from closing the speech stream indefinitely.
    await Promise.race([
      sendChain.current,
      new Promise<void>(resolve => window.setTimeout(resolve, 600))
    ]);
    const result = await stopRecording(current);
    if (!result.ok) {
      sessionRef.current = undefined;
      setMessage(result.message ?? "结束语音输入失败");
      setCurrentPhase("Error");
      hideLater(5000);
    }
  };

  const begin = async (sourceKind: SourceKind) => {
    if (startInFlight.current || !["Idle", "Error"].includes(phaseRef.current)) return;
    startInFlight.current = true;
    releaseRequested.current = false;
    activeSource.current = sourceKind;
    heardEditSpeech.current = false;
    lastEditSpeechAt.current = 0;
    setEditMode(sourceKind === "KeyboardEdit");
    setTranscript("");
    setMessage("");
    setWave(Array(WAVE_POINTS).fill(0.04));
    setElapsedMs(0);
    startedAtRef.current = performance.now();
    setCurrentPhase("Preparing");
    await showOverlay();
    let stream: MediaStream | undefined;
    try {
      if (!navigator.mediaDevices?.getUserMedia) throw new Error("当前系统 WebView 不支持麦克风采集");
      stream = await navigator.mediaDevices.getUserMedia({
        audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true }
      });
      const result = await startRecording(sourceKind);
      if (!result.ok || !result.data) throw new Error(result.message ?? "无法启动语音输入");
      const current = result.data.sessionId;
      const context = new AudioContext();
      await context.resume();
      const source = context.createMediaStreamSource(stream);
      const processor = context.createScriptProcessor(4096, 1, 1);
      const silent = context.createGain();
      silent.gain.value = 0;
      sessionRef.current = current;
      sendChain.current = Promise.resolve();
      processor.onaudioprocess = event => {
        if (sessionRef.current !== current) return;
        const input = event.inputBuffer.getChannelData(0);
        let energy = 0;
        for (let index = 0; index < input.length; index++) energy += input[index] * input[index];
        const level = Math.min(1, Math.sqrt(energy / input.length) * 5);
        if (sourceKind === "KeyboardEdit" && level >= 0.09) {
          heardEditSpeech.current = true;
          lastEditSpeechAt.current = performance.now();
        }
        setWave(previous => [...previous.slice(1), Math.max(0.04, level)]);
        const samples = to16kPcm(input, context.sampleRate);
        sendChain.current = sendChain.current.then(async () => {
          const sent = await pushRecordingAudio(current, samples);
          if (!sent.ok) throw new Error(sent.message ?? "发送音频失败");
        }).catch(reason => {
          setMessage(reason instanceof Error ? reason.message : String(reason));
        });
      };
      source.connect(processor);
      processor.connect(silent);
      silent.connect(context.destination);
      captureRef.current = { stream, context, source, processor, silent };
      setCurrentPhase("Recording");
      if (sourceKind === "KeyboardEdit") {
        silenceTimer.current = window.setInterval(() => {
          if (phaseRef.current !== "Recording" || !heardEditSpeech.current) return;
          if (performance.now() - lastEditSpeechAt.current >= 1600) void finish();
        }, 160);
      }
      startInFlight.current = false;
      if (releaseRequested.current) {
        releaseRequested.current = false;
        void finish();
      }
    } catch (reason) {
      stream?.getTracks().forEach(track => track.stop());
      if (sessionRef.current) void stopRecording(sessionRef.current);
      sessionRef.current = undefined;
      activeSource.current = undefined;
      startInFlight.current = false;
      setMessage(microphoneError(reason));
      setCurrentPhase("Error");
      hideLater(6000);
    }
  };

  const handleTrigger = (pressed: boolean, sourceKind: SourceKind) => {
    if (["Preparing", "Recording", "Draining"].includes(phaseRef.current)
        && activeSource.current && activeSource.current !== sourceKind) return;
    const toggle = triggerMode.current === "Toggle";
    if (toggle) {
      if (!pressed) return;
      if (["Preparing", "Recording"].includes(phaseRef.current)) {
        releaseRequested.current = phaseRef.current === "Preparing";
        if (phaseRef.current === "Recording") void finish();
      } else {
        void begin(sourceKind);
      }
      return;
    }
    if (pressed) {
      void begin(sourceKind);
    } else {
      releaseRequested.current = true;
      if (phaseRef.current === "Recording") void finish();
    }
  };

  useEffect(() => {
    document.documentElement.classList.add("voice-overlay-document");
    document.body.classList.add("voice-overlay-document");
    void overlayWindow.setIgnoreCursorEvents(true).catch(() => undefined);
    void getRuntimeSnapshot().then(snapshot => { triggerMode.current = snapshot.keyboardConfig.pttMode; }).catch(() => undefined);
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];
    void Promise.all([
      listen<HardwareVoiceButtonEvent>("hardware-voice-button", event => {
        if (event.payload.sequence === handledVoiceSequence.current) return;
        handledVoiceSequence.current = event.payload.sequence;
        handleTrigger(event.payload.pressed, "Keyboard");
      }),
      listen<HardwareEditButtonEvent>("hardware-edit-button", event => {
        if (event.payload.sequence === handledEditSequence.current) return;
        handledEditSequence.current = event.payload.sequence;
        if (event.payload.pressed) setEditHasSelection(event.payload.hasSelection);
        handleTrigger(event.payload.pressed, "KeyboardEdit");
      }),
      listen<SpeechTranscriptEvent>("speech-transcript", event => {
        if (event.payload.sessionId !== sessionRef.current) return;
        setTranscript(event.payload.text);
      }),
      listen<SpeechSessionEvent>("speech-session", event => {
        if (event.payload.sessionId !== sessionRef.current) return;
        stopLocalCapture();
        sessionRef.current = undefined;
        activeSource.current = undefined;
        startInFlight.current = false;
        setTranscript(event.payload.text);
        setMessage(event.payload.message ?? (event.payload.text ? "已写入当前光标位置" : "没有识别到文字"));
        setElapsedMs(event.payload.durationMs);
        setCurrentPhase(event.payload.phase);
        hideLater(event.payload.phase === "Error" ? 6000 : 2200);
      })
    ]).then(values => disposed ? values.forEach(value => value()) : unlisteners.push(...values));
    return () => {
      disposed = true;
      unlisteners.forEach(value => value());
      window.clearTimeout(hideTimer.current);
      const current = sessionRef.current;
      stopLocalCapture();
      if (current) void stopRecording(current);
    };
  }, []);

  useEffect(() => {
    const element = textRef.current;
    if (element) element.scrollTo({ left: element.scrollWidth, behavior: "smooth" });
  }, [transcript]);

  useEffect(() => {
    if (phase !== "Preparing" && phase !== "Recording") return;
    const updateElapsed = () => setElapsedMs(Math.max(0, performance.now() - startedAtRef.current));
    updateElapsed();
    const timer = window.setInterval(updateElapsed, 250);
    return () => window.clearInterval(timer);
  }, [phase]);

  useEffect(() => {
    if (phase === "Idle" || phase === "Error") return;
    let frame = 0;
    let previous = 0;
    const animate = (now: number) => {
      if (now - previous >= 32) {
        const speed = phase === "Draining" ? 0.065 : 0.11;
        setMotionPhase(value => (value + speed) % (Math.PI * 2));
        previous = now;
      }
      frame = requestAnimationFrame(animate);
    };
    frame = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(frame);
  }, [phase]);

  const wavePaths = useMemo(() => {
    const width = 450;
    const center = 32;
    const stateStrength = phase === "Draining" ? 0.55 : phase === "Idle" ? 0.2 : phase === "Error" ? 0.3 : phase === "Preparing" ? 0.72 : 1;
    const recentLevel = wave.slice(-14).reduce((sum, level) => sum + level, 0) / 14;
    const activity = Math.min(1, 0.2 + Math.pow(recentLevel, 0.52) * 1.5);
    const centerPoints: [number, number][] = [];
    const upperPoints: [number, number][] = [];
    const lowerPoints: [number, number][] = [];
    const echoUpperPoints: [number, number][] = [];
    wave.forEach((level, index) => {
      const x = index * (width / (WAVE_POINTS - 1));
      const progress = index / (WAVE_POINTS - 1);
      const envelope = 0.24 + Math.sin(progress * Math.PI) * 0.76;
      const localEnergy = Math.min(1, activity * .72 + Math.pow(Math.max(0.025, level), .55) * .58);
      const signal = Math.sin(progress * Math.PI * 4.2 + motionPhase) * .58
        + Math.sin(progress * Math.PI * 8.6 - motionPhase * 1.18) * .27
        + Math.sin(progress * Math.PI * 2.1 + motionPhase * .54) * .15;
      const offset = signal * (12 + localEnergy * 21) * envelope * stateStrength;
      const halfWidth = (1.8 + localEnergy * 5.6 + Math.sin(progress * Math.PI * 3.4 + motionPhase) * 1.1) * Math.max(.62, stateStrength);
      const y = center + offset;
      centerPoints.push([x, y]);
      upperPoints.push([x, Math.max(1, y - halfWidth)]);
      lowerPoints.push([x, Math.min(63, y + halfWidth)]);
      echoUpperPoints.push([x, Math.max(1, Math.min(63, center + (Math.sin(progress * Math.PI * 4.55 - motionPhase * .7 + .9) * 14 + Math.sin(progress * Math.PI * 2.4 + motionPhase * .45) * 5) * envelope * stateStrength))]);
    });

    const toSmoothLine = (points: [number, number][], move = true) => {
      if (!points.length) return "";
      let output = `${move ? "M" : "L"}${points[0][0].toFixed(1)},${points[0][1].toFixed(1)}`;
      for (let index = 1; index < points.length - 1; index++) {
        const [x, y] = points[index];
        const [nextX, nextY] = points[index + 1];
        output += ` Q${x.toFixed(1)},${y.toFixed(1)} ${((x + nextX) / 2).toFixed(1)},${((y + nextY) / 2).toFixed(1)}`;
      }
      const last = points[points.length - 1];
      return `${output} L${last[0].toFixed(1)},${last[1].toFixed(1)}`;
    };
    const line = toSmoothLine(centerPoints);
    const ribbon = `${toSmoothLine(upperPoints)} ${toSmoothLine(lowerPoints.slice().reverse(), false)} Z`;
    return { line, ribbon, echoUpper: toSmoothLine(echoUpperPoints) };
  }, [motionPhase, phase, wave]);

  const viewState = phase === "Draining" ? "processing" : phase === "Idle" ? "done" : phase === "Error" ? "error" : "listening";
  const seconds = Math.floor(elapsedMs / 1000);
  const timeLabel = phase === "Draining" ? "AI" : phase === "Idle" ? "DONE" : phase === "Error" ? "ERROR" : `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
  const displayText = transcript || message || (editMode ? (editHasSelection ? "已读取选中文字，请说出总结、改写或翻译要求" : "未检测到选区，请说出问题或编辑要求") : "请开始说话，松开按键后自动写入");
  const hintText = phase === "Draining" ? "保持光标" : phase === "Idle" ? "输入完成" : phase === "Error" ? "请检查设置" : "松开写入";
  const highlightLength = transcript ? Math.min(8, transcript.length) : 0;
  const leadingText = highlightLength ? displayText.slice(0, -highlightLength) : displayText;
  const latestText = highlightLength ? displayText.slice(-highlightLength) : "";

  const phaseIcon = viewState === "done" ? <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m5 12.5 4.2 4.2L19 7" /></svg>
    : viewState === "processing" ? <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 12h2.5l1.6-5 3.1 10 2.4-8 1.8 6H20" /></svg>
      : viewState === "error" ? <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 7v6M12 17.2v.1" /></svg>
        : <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="8" y="3" width="8" height="12" rx="4" /><path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3M9 21h6" /></svg>;

  return <main className={`voice-overlay phase-${phase.toLowerCase()}`} data-state={viewState}>
    <div className="overlay-border-orbit" aria-hidden="true" />
    <div className="overlay-shell">
      <div className="overlay-core-wrap" aria-hidden="true">
        <div className="overlay-orbit" />
        <div className="overlay-core">{phaseIcon}</div>
      </div>

      <section className="overlay-copy">
        <header>
          <span className="overlay-dot" />
          <b>{statusText(phase, editMode)}</b>
          <span className="overlay-timer">{timeLabel}</span>
        </header>
        <div ref={textRef} className={`overlay-transcript ${transcript ? "has-text" : ""}`}>
          <span>{leadingText}</span>{latestText && <strong>{latestText}</strong>}
          {phase === "Recording" && <i />}
        </div>
        <svg className="overlay-wave" viewBox="0 0 450 64" preserveAspectRatio="none" aria-label="实时麦克风彩带">
          <defs>
            <linearGradient id="voice-ribbon-gradient" x1="0" x2="1">
              <stop offset="0" stopColor="#a84c37" stopOpacity=".16"/>
              <stop offset=".24" stopColor="#d86240"/>
              <stop offset=".52" stopColor="#efa950"/>
              <stop offset=".76" stopColor="#37b4aa"/>
              <stop offset="1" stopColor="#3687aa" stopOpacity=".16"/>
            </linearGradient>
            <linearGradient id="voice-ribbon-shine" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor="#ffffff" stopOpacity=".92"/>
              <stop offset=".48" stopColor="#ffffff" stopOpacity=".2"/>
              <stop offset="1" stopColor="#ffffff" stopOpacity=".56"/>
            </linearGradient>
            <linearGradient id="voice-thread-gradient" x1="0" x2="1"><stop offset="0" stopColor="#a84c37" stopOpacity=".08"/><stop offset=".3" stopColor="#d76c48" stopOpacity=".62"/><stop offset=".68" stopColor="#3db5aa" stopOpacity=".58"/><stop offset="1" stopColor="#3687aa" stopOpacity=".08"/></linearGradient>
          </defs>
          <path className="overlay-wave-echo echo-upper" d={wavePaths.echoUpper}/>
          <path className="overlay-ribbon-glow" d={wavePaths.ribbon} />
          <path className="overlay-ribbon" d={wavePaths.ribbon} />
          <path className="overlay-ribbon-shine" d={wavePaths.line} />
        </svg>
        {message && transcript && <footer>{message}</footer>}
      </section>

      <aside className="overlay-side" aria-hidden="true">
        <span className="overlay-brand">E</span>
        <span className="overlay-hint">{Array.from(hintText).map((character, index) => <span key={`${character}-${index}`}>{character}</span>)}</span>
      </aside>
    </div>
  </main>;
}

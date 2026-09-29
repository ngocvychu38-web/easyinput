import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AudioLines, CircleHelp, Settings, UserRound } from "lucide-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getRuntimeSnapshot } from "./api";
import type { HardwareRealtimeButtonEvent, RuntimeSnapshot } from "./types";
import { OverviewPage } from "./pages/OverviewPage";
import { VoicePage } from "./pages/VoicePage";
import { HistoryPage } from "./pages/HistoryPage";
import { DictionaryPage } from "./pages/DictionaryPage";
import { KeyboardPage } from "./pages/KeyboardPage";
import { SettingsPage } from "./pages/SettingsPage";
import { AccountPage } from "./pages/AccountPage";
import { HelpPage } from "./pages/HelpPage";
import { Onboarding } from "./components/Onboarding";
import { SpeechConfigPage } from "./pages/SpeechConfigPage";
import { RealtimeCallPage } from "./pages/RealtimeCallPage";
import { VoiceActionsPage } from "./pages/VoiceActionsPage";
import { McdPage } from "./pages/McdPage";
import { EnglishPage } from "./pages/EnglishPage";
import { CockpitPage } from "./pages/CockpitPage";
import { LivePage } from "./pages/LivePage";
import { CubePage } from "./pages/CubePage";
import "./live.css";
import "./cube.css";
import type { CockpitCommandRequest } from "./cockpit";
import { cubeCommandFromTranscript } from "./cube";
import { voiceNavigationFromTranscript } from "./navigation";
import { isLiveControlCommand } from "./live";

export type PageId = "overview" | "live" | "english" | "cockpit" | "cube" | "voice" | "call" | "mcd" | "voiceActions" | "history" | "dictionary" | "keyboard" | "speechConfig" | "settings" | "account" | "help";
const primary: { id: PageId; label: string }[] = [
  { id: "overview", label: "概览" }, { id: "voice", label: "语音" }, { id: "call", label: "通话" }, { id: "voiceActions", label: "语音调用" }, { id: "history", label: "历史" },
  { id: "dictionary", label: "词库" }, { id: "keyboard", label: "键盘" }
];
const hiddenModules: { id: PageId; label: string }[] = [
  { id: "live", label: "直播" }, { id: "mcd", label: "麦当劳点餐" },
  { id: "cockpit", label: "驾驶舱" }, { id: "english", label: "英语学习" }, { id: "cube", label: "魔方" },
];

export default function App() {
  const [page, setPage] = useState<PageId>("overview");
  const [revealedModules, setRevealedModules] = useState<PageId[]>([]);
  const pageRef = useRef(page); pageRef.current = page;
  const [runtime, setRuntime] = useState<RuntimeSnapshot>();
  const [error, setError] = useState<string>();
  const [hardwareRealtimeTrigger, setHardwareRealtimeTrigger] = useState<HardwareRealtimeButtonEvent>();
  const [cockpitCommandRequest, setCockpitCommandRequest] = useState<CockpitCommandRequest>();
  const [onboarding, setOnboarding] = useState(() => localStorage.getItem("easyinput.onboarding.completed") !== "1");

  const refresh = async () => {
    try { setRuntime(await getRuntimeSnapshot()); setError(undefined); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
  };
  const completeCockpitCommand = useCallback((requestId: string) => {
    setCockpitCommandRequest(current => current?.requestId === requestId ? undefined : current);
  }, []);
  const openModule = useCallback((module: PageId) => {
    setRevealedModules(current => current.includes(module) ? current : [...current, module]);
    setPage(module);
  }, []);
  const closeModule = useCallback((module: PageId) => {
    setRevealedModules(current => current.filter(item => item !== module));
    setPage("overview");
  }, []);
  useEffect(() => {
    let stopped = false;
    let timer: number | undefined;
    const poll = async () => {
      await refresh();
      if (!stopped) timer = window.setTimeout(poll, 2_000);
    };
    const refreshOnFocus = () => { void refresh(); };
    void poll();
    window.addEventListener("focus", refreshOnFocus);
    return () => {
      stopped = true;
      if (timer !== undefined) window.clearTimeout(timer);
      window.removeEventListener("focus", refreshOnFocus);
    };
  }, []);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];
    void Promise.all([
      listen<HardwareRealtimeButtonEvent>("hardware-realtime-button", event => { if (pageRef.current === "live") { return; } else if (pageRef.current === "english") { if (event.payload.pressed) window.dispatchEvent(new Event("easyinput:learning-hardware")); } else { setHardwareRealtimeTrigger(event.payload); setPage("call"); } }),
      listen<CockpitCommandRequest>("cockpit-command-request", event => { setCockpitCommandRequest(event.payload); openModule("cockpit"); })
      ,listen<{ text: string }>("realtime-user-text", event => {
        const text = event.payload.text.replace(/[\s“”"'‘’。，、！？；：,.!?;:]/g, "");
        const navigation = voiceNavigationFromTranscript(event.payload.text);
        if (navigation?.action === "close") { closeModule(navigation.module); return; }
        if (navigation?.action === "open") { openModule(navigation.module); return; }
        if (pageRef.current === "live" && event.payload.text.trim()) {
          window.dispatchEvent(new CustomEvent("easyinput:live-user-utterance", {
            detail: { text: event.payload.text, control: isLiveControlCommand(event.payload.text) }
          })); return;
        }
        if (pageRef.current === "cube") {
          const cubeCommand = cubeCommandFromTranscript(text);
          if (cubeCommand) { window.dispatchEvent(new CustomEvent("easyinput:cube-command", { detail: cubeCommand })); return; }
        }
      })
    ]).then(values => disposed ? values.forEach(value=>value()) : unlisteners.push(...values));
    return () => { disposed = true; unlisteners.forEach(value=>value()); };
  }, [closeModule, openModule]);

  const content = useMemo(() => {
    if (!runtime) return <div className="loading">正在读取本机配置…</div>;
    const props = { runtime, refresh };
    switch (page) {
      case "overview": return <OverviewPage {...props} navigate={target=>setPage(target)} />;
      case "live": return <LivePage />;
      case "cube": return <CubePage />;
      case "english": return <EnglishPage />;
      case "cockpit": return <CockpitPage commandRequest={cockpitCommandRequest} onCommandComplete={completeCockpitCommand} />;
      case "voice": return <VoicePage {...props} />;
      case "call": return <RealtimeCallPage hardwareTrigger={hardwareRealtimeTrigger} openSettings={() => setPage("speechConfig")} />;
      case "mcd": return <McdPage />;
      case "voiceActions": return <VoiceActionsPage />;
      case "history": return <HistoryPage />;
      case "dictionary": return <DictionaryPage />;
      case "keyboard": return <KeyboardPage {...props} />;
      case "speechConfig": return <SpeechConfigPage />;
      case "settings": return <SettingsPage {...props} />;
      case "account": return <AccountPage />;
      case "help": return <HelpPage runtime={runtime} />;
    }
  }, [page, runtime, hardwareRealtimeTrigger, cockpitCommandRequest, completeCockpitCommand]);

  return <div className="app-shell">
    {onboarding && <Onboarding onComplete={() => { localStorage.setItem("easyinput.onboarding.completed", "1"); setOnboarding(false); }} />}
    <header className="masthead">
      <div><div className="wordmark">EASY INPUT</div><div className="tagline">让输入跟上想法</div></div>
      <div className="service-line"><span>{new Intl.DateTimeFormat("zh-CN", { month: "long", day: "numeric", weekday: "short" }).format(new Date())}</span><i />{runtime?.voiceService === "Connected" ? "语音服务已连接" : "语音服务未连接"}</div>
    </header>
    <div className="rule strong" />
    <nav className="nav-row" aria-label="主导航">
      <div className="primary-nav">{[...primary, ...hiddenModules.filter(item => revealedModules.includes(item.id))].map(item => <button key={item.id} className={page === item.id ? "active" : ""} onClick={() => setPage(item.id)}>{item.label}</button>)}</div>
      <div className="utility-nav">
        <button aria-label="语音服务配置" title="语音服务配置" className={page === "speechConfig" ? "selected" : ""} onClick={() => setPage("speechConfig")}><AudioLines size={18} /></button>
        <button aria-label="设置" title="设置" className={page === "settings" ? "selected" : ""} onClick={() => setPage("settings")}><Settings size={18} /></button>
        <button aria-label="账户" title="账户" className={page === "account" ? "selected" : ""} onClick={() => setPage("account")}><UserRound size={18} /></button>
        <button aria-label="帮助" title="帮助" className={page === "help" ? "selected" : ""} onClick={() => setPage("help")}><CircleHelp size={18} /></button>
      </div>
    </nav>
    {error && <div className="error-banner">本机服务暂时不可用：{error}<button onClick={refresh}>重试</button></div>}
    <main>{content}</main>
    <footer>本日累计 {runtime?.todayChars ?? 0} 字 · 连续 1 天</footer>
  </div>;
}

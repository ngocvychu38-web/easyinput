import { useEffect, useMemo, useState } from "react";
import { AudioLines, CircleHelp, Settings, UserRound } from "lucide-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getRuntimeSnapshot } from "./api";
import type { HardwareEditButtonEvent, HardwareVoiceButtonEvent, RuntimeSnapshot } from "./types";
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

export type PageId = "overview" | "voice" | "history" | "dictionary" | "keyboard" | "speechConfig" | "settings" | "account" | "help";
const primary: { id: PageId; label: string }[] = [
  { id: "overview", label: "概览" }, { id: "voice", label: "语音" }, { id: "history", label: "历史" },
  { id: "dictionary", label: "词库" }, { id: "keyboard", label: "键盘" }
];

export default function App() {
  const [page, setPage] = useState<PageId>("overview");
  const [runtime, setRuntime] = useState<RuntimeSnapshot>();
  const [error, setError] = useState<string>();
  const [hardwareTrigger, setHardwareTrigger] = useState<HardwareVoiceButtonEvent>();
  const [hardwareEditTrigger, setHardwareEditTrigger] = useState<HardwareEditButtonEvent>();
  const [onboarding, setOnboarding] = useState(() => localStorage.getItem("easyinput.onboarding.completed") !== "1");

  const refresh = async () => {
    try { setRuntime(await getRuntimeSnapshot()); setError(undefined); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
  };
  useEffect(() => { void refresh(); }, []);
  useEffect(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];
    void Promise.all([
      listen<HardwareVoiceButtonEvent>("hardware-voice-button", event => { setHardwareTrigger(event.payload); setPage("voice"); }),
      listen<HardwareEditButtonEvent>("hardware-edit-button", event => { setHardwareEditTrigger(event.payload); setPage("voice"); })
    ]).then(values => disposed ? values.forEach(value=>value()) : unlisteners.push(...values));
    return () => { disposed = true; unlisteners.forEach(value=>value()); };
  }, []);

  const content = useMemo(() => {
    if (!runtime) return <div className="loading">正在读取本机配置…</div>;
    const props = { runtime, refresh };
    switch (page) {
      case "overview": return <OverviewPage {...props} navigate={target=>setPage(target)} />;
      case "voice": return <VoicePage {...props} hardwareTrigger={hardwareTrigger} hardwareEditTrigger={hardwareEditTrigger} />;
      case "history": return <HistoryPage />;
      case "dictionary": return <DictionaryPage />;
      case "keyboard": return <KeyboardPage {...props} />;
      case "speechConfig": return <SpeechConfigPage />;
      case "settings": return <SettingsPage {...props} />;
      case "account": return <AccountPage />;
      case "help": return <HelpPage runtime={runtime} />;
    }
  }, [page, runtime, hardwareTrigger, hardwareEditTrigger]);

  return <div className="app-shell">
    {onboarding && <Onboarding onComplete={() => { localStorage.setItem("easyinput.onboarding.completed", "1"); setOnboarding(false); }} />}
    <header className="masthead">
      <div><div className="wordmark">EASY INPUT</div><div className="tagline">让输入跟上想法</div></div>
      <div className="service-line"><span>{new Intl.DateTimeFormat("zh-CN", { month: "long", day: "numeric", weekday: "short" }).format(new Date())}</span><i />{runtime?.voiceService === "Connected" ? "语音服务已连接" : "语音服务未连接"}</div>
    </header>
    <div className="rule strong" />
    <nav className="nav-row" aria-label="主导航">
      <div className="primary-nav">{primary.map(item => <button key={item.id} className={page === item.id ? "active" : ""} onClick={() => setPage(item.id)}>{item.label}</button>)}</div>
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

import {
  AppWindow, Bluetooth, Cable, Check, ChevronDown, CircleAlert, Code2, Download, Eye, EyeOff, FolderOpen,
  HardDriveDownload, Keyboard, Lightbulb, LoaderCircle, Mic2, RefreshCw, RotateCw,
  Send, ShieldAlert, ShieldCheck, Upload, Volume2
} from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import { checkAppUpdate, listAvailableWifiNetworks, listInstalledApplications, openBluetoothSettings, openInputMonitoringSettings, pushKeyboardConfig, readKeyboardStatus } from "../api";
import type { InstalledApplication, KeyboardActionKind, KeyboardConfig, OperationResult, RuntimeSnapshot, WifiNetwork } from "../types";
import { Button, SectionLabel, Toggle } from "../components/Ui";

type DeviceTab = "keys" | "microphone" | "network" | "audio" | "agent" | "firmware";

const tabs: { id: DeviceTab; title: string; subtitle: string }[] = [
  { id: "keys", title: "按键", subtitle: "键位与旋钮" },
  { id: "microphone", title: "麦克风", subtitle: "音频来源" },
  { id: "network", title: "网络", subtitle: "Wi‑Fi 与连接" },
  { id: "audio", title: "音效", subtitle: "开机提示音" },
  { id: "agent", title: "编程助手", subtitle: "键盘灯效" },
  { id: "firmware", title: "键盘更新", subtitle: "功能与安全" }
];

const actionOptions: { kind: KeyboardActionKind; label: string }[] = [
  { kind: "VoicePtt", label: "语音输入" }, { kind: "EditPtt", label: "语音编辑" },
  { kind: "RealtimeVoice", label: "实时通话" },
  { kind: "Enter", label: "回车" }, { kind: "Backspace", label: "退格" },
  { kind: "SelectAll", label: "全选" }, { kind: "Cut", label: "剪切" },
  { kind: "Copy", label: "复制" }, { kind: "Paste", label: "粘贴" },
  { kind: "Undo", label: "撤销" },
  { kind: "FixedText", label: "固定文字" }, { kind: "OpenApp", label: "打开应用" },
  { kind: "Disabled", label: "禁用" }
];

const sounds = [
  { name: "WaytoAGI", description: "WaytoAGI 品牌开机提示", duration: "1.7 秒", default: true },
  { name: "来 WaytoAGI 学 AI 硬件", description: "完整、明确的 AI 硬件语音提示", duration: "2.8 秒" },
  { name: "又来写 bug 了", description: "轻松、有趣的开发者语音提示", duration: "2.1 秒" },
  { name: "晶亮启动", description: "清晰、轻快的双音提示", duration: "0.6 秒" },
  { name: "柔和启动", description: "较轻柔、不过分打扰的提示", duration: "0.8 秒" },
  { name: "极简启动", description: "短促、明确的单音提示", duration: "0.3 秒" }
];

function StatusDot({ tone = "ok" }: { tone?: "ok" | "warn" | "idle" }) {
  return <i className={`device-dot ${tone}`} aria-hidden="true" />;
}

function DeviceSidebar({ active, onChange, connected }: { active: DeviceTab; onChange(tab: DeviceTab): void; connected: boolean }) {
  return <aside className="keyboard-sidebar">
    <div className="keyboard-device-summary">
      <div className="bluetooth-mark"><Bluetooth size={22} /></div>
      <div><b>EasyInput AI</b><span>{connected ? "蓝牙连接" : "设备预览"}</span></div>
      <p><StatusDot tone={connected ? "ok" : "idle"} />{connected ? "键盘输入已连接" : "等待键盘连接"}</p>
      <p><StatusDot tone={connected ? "ok" : "warn"} />{connected ? "设置可用" : "可在本机预先配置"}</p>
    </div>
    <nav aria-label="键盘设置">{tabs.map(item => <button key={item.id} className={active === item.id ? "active" : ""} onClick={() => onChange(item.id)}><b>{item.title}</b><span>{item.subtitle}</span></button>)}</nav>
  </aside>;
}

function SyncCard({ busy, status, bytes, onSync }: { busy: boolean; status: string; bytes: number; onSync(): void }) {
  const permissionRequired = status.includes("输入监控") || status.includes("已阻止访问键盘 HID");
  return <aside className="sync-card">
    <div className="panel-heading"><b>同步到键盘</b><span><Check size={15} />{status || "本机已保存"}</span></div>
    <p>修改会先保存在本机，连接键盘后同步生效。</p>
    <div className="sync-space"><span>内容空间</span><b>{bytes}/2048 字节</b></div>
    <Button kind="primary" onClick={() => onSync()} disabled={busy}>{busy ? <LoaderCircle className="spin" size={17} /> : <Send size={17} />}{busy ? "正在同步" : "重新同步"}</Button>
    {permissionRequired && <Button onClick={() => void openInputMonitoringSettings()}><ShieldAlert size={16} />打开输入监控设置</Button>}
  </aside>;
}

function KeyBoardDrawing({ config, selected, onSelect }: { config: KeyboardConfig; selected: number; onSelect(index: number): void }) {
  return <div className="device-drawing" aria-label="键盘正视图">
    <div className="drawing-topline"><span>BOOT　○</span><span>OFF　▢　　BAT　▯　　USB　▢　　PWR　○ ○ ○ ○</span></div>
    <div className="drawing-keys">{config.keys.slice(0, 8).map((key, index) => <button key={index} className={selected === index ? "selected" : ""} onClick={() => onSelect(index)}><small>KEY{index + 1}</small><i>✣</i><b>{key.label}</b></button>)}</div>
    <button className="drawing-encoder"><small>ENCODER</small><b>滚动 · 上下</b></button>
    <div className="drawing-brand">AI Keyboard V2.1 <span>WaytoAGI</span></div>
  </div>;
}

function KeysPanel({ config, setConfig, sync, syncState, busy }: { config: KeyboardConfig; setConfig(value: KeyboardConfig): void; sync(): void; syncState: string; busy: boolean }) {
  const [selected, setSelected] = useState(0);
  const [applications, setApplications] = useState<InstalledApplication[]>([]);
  const [loadingApplications, setLoadingApplications] = useState(false);
  const [applicationError, setApplicationError] = useState("");
  const selectedKey = config.keys[selected];
  const patchSelectedKey = (patch: Partial<KeyboardConfig["keys"][number]>) => {
    setConfig({ ...config, keys: config.keys.map((key, index) => index === selected ? { ...key, ...patch } : key) });
  };
  const updateKey = (kind: KeyboardActionKind) => {
    const option = actionOptions.find(item => item.kind === kind)!;
    patchSelectedKey({ kind, label: option.label, value: kind === "OpenApp" ? selectedKey?.value : undefined, hostActionId: kind === "OpenApp" ? selectedKey?.hostActionId : undefined });
  };
  const loadApplications = async () => {
    if (loadingApplications) return;
    setLoadingApplications(true); setApplicationError("");
    try { setApplications(await listInstalledApplications()); }
    catch (reason) { setApplicationError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setLoadingApplications(false); }
  };
  useEffect(() => { if (selectedKey?.kind === "OpenApp" && !applications.length) void loadApplications(); }, [selected, selectedKey?.kind]);
  const selectApplication = (path: string) => {
    if (!path) { patchSelectedKey({ label: "打开应用", value: undefined }); return; }
    const application = applications.find(item => item.path === path);
    const fallbackName = path.split("/").pop()?.replace(/\.app$/i, "") || "打开应用";
    patchSelectedKey({ kind: "OpenApp", label: application?.name || fallbackName, value: path });
  };
  const browseApplication = async () => {
    setApplicationError("");
    try {
      const chosen = await openDialog({ multiple: false, directory: false, defaultPath: "/Applications", title: "选择按键要打开的应用", filters: [{ name: "macOS 应用", extensions: ["app"] }] });
      if (!chosen) return;
      const path = Array.isArray(chosen) ? chosen[0] : chosen;
      if (!path?.toLowerCase().endsWith(".app")) { setApplicationError("请选择一个 macOS .app 应用程序。"); return; }
      selectApplication(path);
      if (!applications.some(item => item.path === path)) setApplications(items => [...items, { name: path.split("/").pop()?.replace(/\.app$/i, "") || path, path }]);
    } catch (reason) { setApplicationError(reason instanceof Error ? reason.message : String(reason)); }
  };
  const bytes = new TextEncoder().encode(JSON.stringify(config)).length;
  return <>
    <div className="keyboard-content-head"><SectionLabel index="01">按键与旋钮</SectionLabel><p>选择按键或旋钮后在右侧设置；同步后会自动适配当前电脑的 macOS 按键方式。</p></div>
    <div className="platform-strip"><span>当前电脑 <b>macOS</b></span><span>键盘当前系统 <b>macOS</b></span><span>同步结果 <b>已确认</b></span><span><StatusDot />可在键盘上切换系统：长按旋钮 3 秒</span></div>
    <div className="key-config-layout">
      <section className="drawing-wrap"><div className="panel-heading"><b>设备正视图</b><span><StatusDot />与上次同步一致</span></div><KeyBoardDrawing config={config} selected={selected} onSelect={setSelected} /></section>
      <div className="key-side-stack">
        <section className="key-setting-card"><div className="panel-heading"><b><em>KEY{selected + 1}</em> 按键设置</b><span>{selectedKey?.label}</span></div><label>按下动作<div className="select-wrap"><select value={selectedKey?.kind} onChange={event => updateKey(event.target.value as KeyboardActionKind)}>{actionOptions.map(option => <option key={option.kind} value={option.kind}>{option.label}</option>)}</select><ChevronDown size={16} /></div></label>
          {selectedKey?.kind === "OpenApp" && <div className="app-picker">
            <label>要打开的应用<div className="select-wrap"><select value={selectedKey.value || ""} onChange={event => selectApplication(event.target.value)}><option value="">{loadingApplications ? "正在读取应用列表…" : "请选择应用"}</option>{selectedKey.value && !applications.some(item => item.path === selectedKey.value) && <option value={selectedKey.value}>{selectedKey.label}</option>}{applications.map(application => <option key={application.path} value={application.path}>{application.name}</option>)}</select><ChevronDown size={16} /></div></label>
            <div className="app-picker-actions"><Button onClick={() => void browseApplication()}><FolderOpen size={15} />从系统选择…</Button><button type="button" onClick={() => void loadApplications()} disabled={loadingApplications}><RefreshCw size={14} />刷新列表</button></div>
            {selectedKey.value ? <p className="selected-app-path"><AppWindow size={14} /><span title={selectedKey.value}>{selectedKey.value}</span></p> : <p className="app-picker-hint">选择后会将应用路径保存到该按键配置。</p>}
            {applicationError && <p className="form-error">{applicationError}</p>}
          </div>}
          {selectedKey?.kind === "FixedText" && <label>文字内容<textarea value={selectedKey.value || ""} onChange={event => patchSelectedKey({ value: event.target.value })} maxLength={960} placeholder="输入按键后要写入的文字" /></label>}
        </section>
        <SyncCard busy={busy} status={syncState} bytes={bytes} onSync={sync} />
      </div>
    </div>
  </>;
}

function MicrophonePanel() {
  const [source, setSource] = useState<"computer" | "keyboard">("keyboard");
  return <><div className="keyboard-content-head"><SectionLabel index="01">麦克风</SectionLabel></div><section className="soft-panel microphone-choice"><b>麦克风来源</b><p>选择录音时优先使用的麦克风；如果键盘麦克风不可用，本次会使用电脑麦克风。录音开始后不会中途切换。</p><div className="large-segmented"><button className={source === "computer" ? "active" : ""} onClick={() => setSource("computer")}><Mic2 size={17} />电脑</button><button className={source === "keyboard" ? "active" : ""} onClick={() => setSource("keyboard")}><Keyboard size={17} />键盘优先</button></div></section><section className="availability-banner"><div><b>键盘麦克风已可用</b><p>键盘和电脑已连接到同一网络。</p></div><Button>查看网络</Button></section></>;
}

function NetworkPanel({ config, setConfig, sync, syncState, busy }: { config: KeyboardConfig; setConfig(value: KeyboardConfig): void; sync(password?: string): Promise<boolean>; syncState: string; busy: boolean }) {
  const [showPassword, setShowPassword] = useState(false);
  const [password, setPassword] = useState("");
  const [networks, setNetworks] = useState<WifiNetwork[]>([]);
  const [manualSsid, setManualSsid] = useState(false);
  const [loadingNetworks, setLoadingNetworks] = useState(false);
  const [networkError, setNetworkError] = useState("");
  const [networkWarning, setNetworkWarning] = useState("");
  const [wifiInterface, setWifiInterface] = useState("");
  const bytes = new TextEncoder().encode(JSON.stringify(config)).length;
  const patchWifi = (patch: Partial<KeyboardConfig["wifi"]>) => setConfig({ ...config, wifi: { ...config.wifi, ...patch } });
  const loadNetworks = async () => {
    if (loadingNetworks) return;
    setLoadingNetworks(true); setNetworkError(""); setNetworkWarning("");
    try {
      const result = await listAvailableWifiNetworks();
      setNetworks(result.networks); setWifiInterface(result.interface); setNetworkWarning(result.warning || "");
      const selectedSsid = config.wifi.ssid || result.currentSsid || "";
      const patch: Partial<KeyboardConfig["wifi"]> = {};
      if (!config.wifi.ssid && result.currentSsid) patch.ssid = result.currentSsid;
      if (!config.wifi.audioHost && result.localIp) patch.audioHost = result.localIp;
      if (Object.keys(patch).length) patchWifi(patch);
      setManualSsid(Boolean(selectedSsid) && !result.networks.some(item => item.ssid === selectedSsid));
    } catch (reason) {
      setNetworkError(reason instanceof Error ? reason.message : String(reason));
      setManualSsid(true);
    } finally { setLoadingNetworks(false); }
  };
  useEffect(() => { void loadNetworks(); }, []);
  const selectSsid = (value: string) => {
    if (value === "__manual__") { setManualSsid(true); if (!config.wifi.ssid) patchWifi({ ssid: "", passwordSaved: false }); return; }
    setManualSsid(false);
    if (value !== config.wifi.ssid) { setPassword(""); patchWifi({ ssid: value, passwordSaved: false }); }
  };
  const changeManualSsid = (value: string) => {
    if (value !== config.wifi.ssid) setPassword("");
    patchWifi({ ssid: value, passwordSaved: false });
  };
  const saveAndSync = async () => { if (await sync(password)) setPassword(""); };
  return <>
    <div className="keyboard-content-head stacked"><SectionLabel index="01">网络与连接</SectionLabel><p>键盘麦克风与开机音效共用这份 Wi‑Fi 配置，使用时请让键盘与电脑连接到同一个路由器。</p></div>
    <section className="availability-banner"><div><b>{loadingNetworks ? "正在读取 macOS Wi‑Fi" : networks.length ? `已读取 ${networks.length} 个系统网络` : "等待选择 Wi‑Fi"}</b><p>{wifiInterface ? `系统接口 ${wifiInterface}；选择后保存在本机并同步到键盘。` : "自动读取当前网络和 macOS 已记住的网络。"}</p></div><Button onClick={() => void loadNetworks()} disabled={loadingNetworks}>{loadingNetworks ? <LoaderCircle className="spin" size={16} /> : <RefreshCw size={16} />}{loadingNetworks ? "读取中" : "刷新网络"}</Button></section>
    <div className="network-layout"><section className="network-form">
      <div className="panel-heading"><div><b>连接信息</b><p>这些信息会同步到键盘，用于键盘麦克风和无线音效</p></div><span>保存在本机</span></div>
      <div className="network-warning"><ShieldAlert size={19} /><div><b>键盘必须连接 2.4GHz Wi‑Fi，并与电脑处于同一网络</b><p>电脑使用 Wi‑Fi 时，可选择电脑当前或系统已记住的网络；电脑使用 5GHz Wi‑Fi 或网线时，请选择同一路由器的 2.4GHz Wi‑Fi。</p></div></div>
      <label>Wi‑Fi 名称<span>来自 macOS 当前网络和已记住的网络；也可手工输入其他 2.4GHz SSID。</span><div className="select-wrap wifi-select"><select value={manualSsid ? "__manual__" : config.wifi.ssid} onChange={event => selectSsid(event.target.value)}><option value="">请选择 Wi‑Fi</option>{networks.map(network => <option key={network.ssid} value={network.ssid}>{network.ssid}{network.current ? "（当前连接）" : network.configured ? "（当前配置）" : ""}</option>)}<option value="__manual__">手工输入其他网络…</option></select><ChevronDown size={16} /></div>{manualSsid && <input value={config.wifi.ssid} onChange={event => changeManualSsid(event.target.value)} placeholder="输入网络名称（SSID）" autoFocus />}</label>
      <label>电脑接收地址<span>用于接收键盘麦克风音频；首次读取网络时自动填入本机地址。</span><input value={config.wifi.audioHost} onChange={event => patchWifi({ audioHost: event.target.value })} placeholder="例如 192.168.1.10" /></label>
      <label>Wi‑Fi 密码<span>{config.wifi.passwordSaved ? "已安全保存在 EasyInput 钥匙串；留空表示不修改" : "开放网络可留空；密码不会写入本机配置文件"}</span><div className="password-wrap"><input type={showPassword ? "text" : "password"} value={password} onChange={event => setPassword(event.target.value)} placeholder={config.wifi.passwordSaved ? "已保存，输入新密码可替换" : "输入 Wi‑Fi 密码"}/><button type="button" onClick={() => setShowPassword(value => !value)} aria-label={showPassword ? "隐藏密码" : "显示密码"}>{showPassword ? <EyeOff /> : <Eye />}</button></div></label>
      {networkWarning && <p className="network-form-notice">{networkWarning}</p>}{networkError && <p className="form-error network-form-notice">自动读取失败：{networkError}。仍可手工输入网络名称。</p>}
    </section><div className="network-side"><SyncCard busy={busy} status={syncState} bytes={bytes} onSync={() => void saveAndSync()} /><section className="usage-card"><b>这份网络用于</b><div><strong>键盘麦克风</strong><p>把键盘采集的声音实时发送到这台电脑。</p></div><div className="accent"><strong>开机音效</strong><p>连接数据线时直接同步；未连接时可使用 Wi‑Fi。</p></div></section></div></div>
  </>;
}

function AudioPanel({ sync }: { sync(): void }) {
  const [selected, setSelected] = useState(0);
  const [enabled, setEnabled] = useState(true);
  const [fileName, setFileName] = useState("");
  const fileRef = useRef<HTMLInputElement>(null);
  return <><div className="keyboard-content-head stacked"><SectionLabel index="03">键盘音效</SectionLabel><p>选择内置提示音、导入自己的音频或关闭开机音效。同步后，电脑关闭时也能正常播放。</p></div><div className="audio-layout"><section>
    <div className="panel-heading"><div><b>开机提示音</b><p>支持 WAV、MP3、M4A、AAC、FLAC 和 Ogg，最长 8 秒</p></div><Volume2 size={19} /></div>
    <div className="sound-grid">{sounds.map((sound, index) => <button key={sound.name} className={selected === index && !fileName ? "selected" : ""} onClick={() => { setSelected(index); setFileName(""); }}><i /><b>{sound.name}{sound.default && <em>默认</em>}</b><span>{sound.description}</span><small>{sound.duration}</small></button>)}</div>
    <div className="sound-toggle"><div><b>开机音效</b><span>完整开机时播放已选音效，切换后需同步到键盘</span></div><small>{enabled ? "已开启" : "已关闭"}</small><Toggle value={enabled} onChange={setEnabled} label="开机音效" /></div>
    <button className={`uploaded-sound ${fileName ? "has-file" : ""}`} onClick={() => fileRef.current?.click()}><span><Upload size={18} /></span><div><b>{fileName || "导入自己的开机音效"}</b><small>EasyInput 会自动转换音频格式，无需手动处理</small></div><strong>{fileName ? "重新选择" : "选择音频"}</strong></button><input ref={fileRef} hidden type="file" accept="audio/*" onChange={event => setFileName(event.target.files?.[0]?.name || "")} />
  </section><aside className="sound-sync-card"><div className="panel-heading"><b>同步到键盘</b><Check size={18} /></div><div><span>音效状态</span><b>{enabled ? "已选择" : "已关闭"}</b></div><div><span>当前通道</span><b>连接设备后确认</b></div><Button kind="primary" onClick={() => sync()}><Send size={17} />同步开机音效</Button><p>使用语音或按键时会暂停同步，结束后继续；如果中断，当前音效不会受到影响。</p></aside></div></>;
}

function AgentPanel() {
  const [agents, setAgents] = useState<Record<string, boolean>>({ Codex: false, "Claude Code": false });
  const [testing, setTesting] = useState(false);
  const anyConnected = Object.values(agents).some(Boolean);
  const test = () => { setTesting(true); window.setTimeout(() => setTesting(false), 1800); };
  return <><div className="keyboard-content-head"><SectionLabel index="01">编程助手灯效</SectionLabel></div><section className="agent-status-head"><div><b>功能状态</b><p>在键盘灯上显示受支持编程助手的运行状态</p></div><span>{anyConnected ? <Check /> : <CircleAlert />}{anyConnected ? "已连接 Agent" : "尚未连接 Agent"}</span></section><div className="agent-cards">{Object.entries(agents).map(([name, connected]) => <section key={name}><div><b>{name}</b><p>{connected ? <Check /> : <CircleAlert />}{connected ? "已连接" : "尚未连接"}</p></div><Button kind="primary" onClick={() => setAgents(value => ({ ...value, [name]: !value[name] }))}>{connected ? "断开" : "连接"}</Button></section>)}<section><div><b>后台运行</b><p><CircleAlert />未配置</p></div></section></div><section className="agent-detail-list"><div><span><b>状态接收</b><small>接收已适配编程助手的运行状态</small></span><strong><Check />接收正常<small>{anyConnected ? "正在接收运行状态" : "尚未收到运行状态"}</small></strong></div><div><span><b>键盘灯效</b><small>把运行状态显示在键盘灯上</small></span><strong><CircleAlert />{anyConnected ? "已就绪" : "等待运行状态"}</strong></div><div><span><b>灯效测试</b><small>让键盘亮黄灯 8 秒，确认灯效是否正常</small></span><Button onClick={test} disabled={testing}><Lightbulb size={17} />{testing ? "测试中…" : "测试灯效"}</Button></div></section></>;
}

function FirmwarePanel({ currentVersion }: { currentVersion: string }) {
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<{ current: string; latest: string; available: boolean }>();
  const [message, setMessage] = useState("");
  const check = async () => { setChecking(true); setMessage(""); try { const response = await checkAppUpdate() as OperationResult<{current:string;latest:string;available:boolean}>; if (response.ok && response.data) setResult(response.data); else setMessage(response.message || "暂时无法获取更新"); } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); } finally { setChecking(false); } };
  return <div className="firmware-layout"><section className="firmware-main"><div className="firmware-title"><span><Download /></span><div><b>键盘更新</b><p>为键盘安装功能与安全更新。EasyInput App 的更新请在“帮助”中单独进行。</p></div></div><div className="version-grid"><div><span>当前版本</span><b>{currentVersion}</b></div><div><span>最新版本</span><b>{result?.latest || "尚未获取"}</b></div></div><div className="upgrade-card"><b>{result?.available ? "可升级到最新官方版本" : result ? "当前已是最新官方版本" : "检查最新官方版本"}</b><p>升级不是强制的。执行前会显示设备信息并再次确认，不会在后台自动写入。</p><div><Button kind="primary" disabled={!result?.available}><HardDriveDownload size={17} />开始升级</Button><span>点击后才会显示 BOOT 操作，你不会自动写入。</span></div></div><Button onClick={check} disabled={checking}>{checking ? <LoaderCircle className="spin" /> : <RotateCw />}{checking ? "正在检查" : "检查更新"}</Button>{message && <p className="form-error">{message}</p>}</section><aside className="safe-update"><div><ShieldCheck /><b>放心更新</b></div><ol><li><b>1. 只安装官方更新</b><p>EasyInput 会先确认更新来自官方。</p></li><li><b>2. 保留个人设置</b><p>按键设置和开机音效不会被清除。</p></li><li><b>3. 写入前确认设备</b><p>App 会先显示设备信息，经过你的确认才会写入。</p></li></ol><p><Cable />一次性升级完成后，请按 App 提示关闭电源再正常开机。</p></aside></div>;
}

export function KeyboardPage({ runtime, refresh }: { runtime: RuntimeSnapshot; refresh(): Promise<void> }) {
  const connected = runtime.device === "ConnectedUsb" || runtime.device === "ConnectedBle";
  const [preview, setPreview] = useState(false);
  const [checking, setChecking] = useState(false);
  const [openingBluetooth, setOpeningBluetooth] = useState(false);
  const [openError, setOpenError] = useState("");
  const [tab, setTab] = useState<DeviceTab>("keys");
  const [config, setConfig] = useState<KeyboardConfig>(runtime.keyboardConfig);
  const [syncing, setSyncing] = useState(false);
  const [syncState, setSyncState] = useState("");
  const detect = async () => { setChecking(true); try { await readKeyboardStatus(); await refresh(); } finally { setChecking(false); } };
  const openBluetooth = async () => { setOpeningBluetooth(true); setOpenError(""); try { const result = await openBluetoothSettings(); if (!result.ok) setOpenError(result.message ?? "无法打开系统蓝牙设置"); } catch (reason) { setOpenError(reason instanceof Error ? reason.message : String(reason)); } finally { setOpeningBluetooth(false); } };
  const sync = async (wifiPassword?: string) => { setSyncing(true); setSyncState("同步中"); try { const result = await pushKeyboardConfig(config, wifiPassword); const locallySaved = result.ok || Boolean(result.message?.includes("已保存在本机")); setSyncState(result.ok ? (connected ? "已确认" : "已保存在本机") : result.message || "同步失败"); if (wifiPassword?.trim() && locallySaved) setConfig(value => ({ ...value, wifi: { ...value.wifi, passwordSaved: true } })); if (result.ok) await refresh(); return locallySaved; } catch (error) { setSyncState(error instanceof Error ? error.message : String(error)); return false; } finally { setSyncing(false); } };
  const currentVersion = runtime.capabilities.firmwareVersion || "0.4.53";
  const activeContent = useMemo(() => {
    if (tab === "keys") return <KeysPanel config={config} setConfig={setConfig} sync={sync} syncState={syncState} busy={syncing} />;
    if (tab === "microphone") return <MicrophonePanel />;
    if (tab === "network") return <NetworkPanel config={config} setConfig={setConfig} sync={sync} syncState={syncState} busy={syncing} />;
    if (tab === "audio") return <AudioPanel sync={sync} />;
    if (tab === "agent") return <AgentPanel />;
    return <FirmwarePanel currentVersion={currentVersion} />;
  }, [tab, config, syncState, syncing, currentVersion]);
  if (!connected && !preview) return <div className="page keyboard-page"><div className="device-card"><div className="connection-side"><SectionLabel index="">键盘连接</SectionLabel><p><i />尚未连接键盘</p><div className="keyboard-illustration"><Cable /><Keyboard size={52} /><Bluetooth /></div><small>支持 USB 与系统蓝牙</small></div><div className="connect-copy"><SectionLabel index="">连接设备</SectionLabel><h1>连接 EasyInput AI</h1><p>通过 USB 或系统蓝牙连接键盘。连接成功后，按键、旋钮和设备功能会在这里自动出现。</p><div className="notice"><ShieldAlert size={17} />请通过 USB 或系统蓝牙连接键盘。</div><div className="actions"><Button kind="primary" onClick={openBluetooth} disabled={openingBluetooth}><Bluetooth size={16} />{openingBluetooth ? "正在打开…" : "打开系统蓝牙设置"}</Button><Button onClick={detect} disabled={checking}><RefreshCw size={16} />{checking ? "检测中…" : "重新检测"}</Button><Button onClick={() => setPreview(true)}><Code2 size={16} />预览设备设置</Button></div>{openError && <p className="form-error">{openError}</p>}<hr /><p className="hint">硬件未连接时仍可预览并预先配置页面；设置会保存在本机，设备接入后再同步。</p></div></div><div className="connection-guides"><div><Cable /><b>01　USB 直接连接</b><p>用 USB 数据线连接键盘和电脑，App 会自动检测连接。</p></div><div><Bluetooth /><b>02　蓝牙无线连接</b><p>在系统蓝牙设备列表中选择“EasyInput AI”。</p></div></div></div>;
  return <div className="page keyboard-page device-console"><div className="keyboard-console-grid"><DeviceSidebar active={tab} onChange={setTab} connected={connected} /><section className="keyboard-content">{preview && !connected && <div className="preview-banner"><CircleAlert size={15} />当前为无硬件预览模式，所有同步动作仅保存本机配置。<button onClick={() => setPreview(false)}>退出预览</button></div>}{activeContent}</section></div></div>;
}

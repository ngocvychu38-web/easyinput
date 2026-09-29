import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { AppWindow, ChevronDown, Copy, FolderOpen, Keyboard, LoaderCircle, Plus, RefreshCw, Save, Sparkles, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { getVoiceActionsConfig, listInstalledApplications, saveVoiceActionsConfig } from "../api";
import { Button, SectionLabel, Toggle } from "../components/Ui";
import type { InstalledApplication, KeyboardActionKind, VoiceActionMapping, VoiceActionsConfig } from "../types";

const actionOptions: { kind: KeyboardActionKind; label: string; hint: string }[] = [
  { kind: "OpenApp", label: "打开应用", hint: "打开已安装并由用户选择的 macOS 应用" },
  { kind: "Hotkey", label: "执行快捷键", hint: "向当前应用发送组合键，例如 Command+Shift+P" },
  { kind: "Copy", label: "复制", hint: "向当前应用发送 Command+C" },
  { kind: "Paste", label: "粘贴", hint: "向当前应用发送 Command+V" },
  { kind: "Cut", label: "剪切", hint: "向当前应用发送 Command+X" },
  { kind: "SelectAll", label: "全选", hint: "向当前应用发送 Command+A" },
  { kind: "Undo", label: "撤销", hint: "向当前应用发送 Command+Z" },
  { kind: "Enter", label: "回车", hint: "向当前应用发送回车键" },
  { kind: "Backspace", label: "退格", hint: "向当前应用发送退格键" },
  { kind: "FixedText", label: "输入固定文字", hint: "把预设文字写入当前光标位置" },
  { kind: "EditPtt", label: "语音编辑选区", hint: "读取当前选区，按本次语音指令处理后替换" }
];

const newMapping = (): VoiceActionMapping => ({
  id: crypto.randomUUID(), enabled: true, name: "", description: "",
  action: { kind: "OpenApp", label: "打开应用" }
});

function MappingCard({ mapping, index, applications, loadingApplications, onLoadApplications, onChange, onDelete }: {
  mapping: VoiceActionMapping; index: number; applications: InstalledApplication[]; loadingApplications: boolean;
  onLoadApplications(): Promise<void>; onChange(value: VoiceActionMapping): void; onDelete(): void;
}) {
  const [applicationError, setApplicationError] = useState("");
  const option = actionOptions.find(item => item.kind === mapping.action.kind) ?? actionOptions[0];
  const patch = (value: Partial<VoiceActionMapping>) => onChange({ ...mapping, ...value });
  const patchAction = (value: Partial<VoiceActionMapping["action"]>) => onChange({ ...mapping, action: { ...mapping.action, ...value } });
  const changeKind = (kind: KeyboardActionKind) => {
    const selected = actionOptions.find(item => item.kind === kind) ?? actionOptions[0];
    patch({ action: { kind, label: selected.label } });
    if (kind === "OpenApp") void onLoadApplications();
  };
  const selectApplication = (path: string) => {
    const application = applications.find(item => item.path === path);
    const fallback = path.split("/").pop()?.replace(/\.app$/i, "") || "打开应用";
    patchAction({ value: path || undefined, label: path ? application?.name || fallback : "打开应用" });
  };
  const browseApplication = async () => {
    setApplicationError("");
    try {
      const chosen = await openDialog({ multiple: false, directory: false, defaultPath: "/Applications", title: "选择语音要打开的应用", filters: [{ name: "macOS 应用", extensions: ["app"] }] });
      if (!chosen) return;
      const path = Array.isArray(chosen) ? chosen[0] : chosen;
      if (!path?.toLowerCase().endsWith(".app")) { setApplicationError("请选择一个 macOS .app 应用程序。"); return; }
      selectApplication(path);
    } catch (reason) { setApplicationError(reason instanceof Error ? reason.message : String(reason)); }
  };

  return <article className={`voice-action-card ${mapping.enabled ? "" : "disabled"}`}>
    <header>
      <div className="voice-action-number">{String(index + 1).padStart(2, "0")}</div>
      <div><b>{mapping.name.trim() || "未命名语音调用"}</b><span>{option.label}</span></div>
      <Toggle value={mapping.enabled} onChange={enabled => patch({ enabled })} label={`${mapping.name || `第 ${index + 1} 项`}是否启用`} />
      <button className="voice-action-delete" aria-label="删除语音调用" onClick={onDelete}><Trash2 size={16} /></button>
    </header>
    <div className="voice-action-form">
      <label>调用名称<span>告诉豆包这个动作是什么，例如“打开微信”或“复制当前内容”。</span><input value={mapping.name} maxLength={80} onChange={event => patch({ name: event.target.value })} placeholder="例如：打开微信" /></label>
      <label>语音说明<span>描述用户在什么情况下要调用；可填写多个自然说法。</span><textarea value={mapping.description} maxLength={400} onChange={event => patch({ description: event.target.value })} placeholder="例如：当用户说打开微信、启动微信或去微信时调用" /></label>
      <label>对应动作<span>{option.hint}</span><div className="select-wrap"><select value={mapping.action.kind} onChange={event => changeKind(event.target.value as KeyboardActionKind)}>{actionOptions.map(item => <option key={item.kind} value={item.kind}>{item.label}</option>)}</select><ChevronDown size={16} /></div></label>

      {mapping.action.kind === "OpenApp" && <div className="voice-action-detail app-picker">
        <label>要打开的应用<div className="select-wrap"><select value={mapping.action.value || ""} onFocus={() => void onLoadApplications()} onChange={event => selectApplication(event.target.value)}><option value="">{loadingApplications ? "正在读取应用列表…" : "请选择应用"}</option>{mapping.action.value && !applications.some(item => item.path === mapping.action.value) && <option value={mapping.action.value}>{mapping.action.label}</option>}{applications.map(application => <option key={application.path} value={application.path}>{application.name}</option>)}</select><ChevronDown size={16} /></div></label>
        <div className="app-picker-actions"><Button onClick={() => void browseApplication()}><FolderOpen size={15} />从系统选择…</Button><button onClick={() => void onLoadApplications()} disabled={loadingApplications}><RefreshCw size={14} />刷新列表</button></div>
        {mapping.action.value && <p className="selected-app-path"><AppWindow size={14} /><span title={mapping.action.value}>{mapping.action.value}</span></p>}
        {applicationError && <p className="form-error">{applicationError}</p>}
      </div>}

      {mapping.action.kind === "Hotkey" && <label className="voice-action-detail">快捷键<span>使用 Command、Shift、Option、Control 加一个按键，以 + 分隔。</span><input value={mapping.action.value || ""} onChange={event => patchAction({ value: event.target.value })} placeholder="Command+Shift+P" list="voice-shortcut-examples" /></label>}
      {mapping.action.kind === "FixedText" && <label className="voice-action-detail">固定文字<span>调用后直接写入当前光标位置。</span><textarea value={mapping.action.value || ""} maxLength={2000} onChange={event => patchAction({ value: event.target.value })} placeholder="输入要写入的文字" /></label>}
      {mapping.action.kind === "EditPtt" && <div className="voice-edit-note"><Sparkles size={16} /><div><b>语音指令会作为编辑要求</b><p>例如说“把选中的内容翻译成英文”，豆包会传入编辑要求，EasyInput 读取当前选区并使用已配置的火山方舟模型替换结果。</p></div></div>}
    </div>
  </article>;
}

export function VoiceActionsPage() {
  const [config, setConfig] = useState<VoiceActionsConfig>({ revision: 1, mappings: [] });
  const [applications, setApplications] = useState<InstalledApplication[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadingApplications, setLoadingApplications] = useState(false);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState(false);

  useEffect(() => { void getVoiceActionsConfig().then(setConfig).catch(reason => { setStatus(`读取失败：${reason instanceof Error ? reason.message : String(reason)}`); setError(true); }).finally(() => setLoading(false)); }, []);
  const enabledCount = useMemo(() => config.mappings.filter(item => item.enabled).length, [config.mappings]);
  const loadApplications = async () => {
    if (loadingApplications) return;
    setLoadingApplications(true);
    try { setApplications(await listInstalledApplications()); }
    catch (reason) { setStatus(`读取应用列表失败：${reason instanceof Error ? reason.message : String(reason)}`); setError(true); }
    finally { setLoadingApplications(false); }
  };
  const add = () => setConfig(value => ({ ...value, mappings: [...value.mappings, newMapping()] }));
  const update = (id: string, mapping: VoiceActionMapping) => setConfig(value => ({ ...value, mappings: value.mappings.map(item => item.id === id ? mapping : item) }));
  const remove = (id: string) => setConfig(value => ({ ...value, mappings: value.mappings.filter(item => item.id !== id) }));
  const save = async () => {
    setSaving(true); setStatus(""); setError(false);
    try {
      const result = await saveVoiceActionsConfig(config);
      if (!result.ok || !result.data) throw new Error(result.message || "保存失败");
      setConfig(result.data); setStatus(`已保存 ${result.data.mappings.length} 项，其中 ${result.data.mappings.filter(item => item.enabled).length} 项会在下次实时通话中提供给豆包。`);
    } catch (reason) { setStatus(reason instanceof Error ? reason.message : String(reason)); setError(true); }
    finally { setSaving(false); }
  };

  return <div className="page voice-actions-page">
    <datalist id="voice-shortcut-examples"><option value="Command+C"/><option value="Command+V"/><option value="Command+Shift+P"/><option value="Command+Option+I"/><option value="Control+Space"/></datalist>
    <div className="voice-actions-head">
      <div><SectionLabel index="01">语音调用</SectionLabel><h1>应用与动作映射</h1><p>把自然语言映射为本机应用、快捷键和语音编辑动作。列表不限制数量，启用项会在新的实时通话会话中生效。</p></div>
      <div className="voice-actions-summary"><span><b>{config.mappings.length}</b>全部映射</span><span><b>{enabledCount}</b>已启用</span></div>
    </div>
    {status && <div className={`voice-actions-status ${error ? "error" : ""}`}>{status}</div>}
    {loading ? <div className="voice-actions-empty"><LoaderCircle className="spin" size={21} />正在读取语音调用设置…</div> : config.mappings.length === 0 ? <div className="voice-actions-empty"><Sparkles size={25} /><b>还没有语音调用</b><p>先添加一项，例如“打开微信”“复制当前内容”或“把选中内容翻译成英文”。</p><Button kind="primary" onClick={add}><Plus size={16} />添加第一项</Button></div> : <div className="voice-actions-list">{config.mappings.map((mapping, index) => <MappingCard key={mapping.id} mapping={mapping} index={index} applications={applications} loadingApplications={loadingApplications} onLoadApplications={loadApplications} onChange={value => update(mapping.id, value)} onDelete={() => remove(mapping.id)} />)}</div>}
    {!loading && <div className="voice-actions-savebar"><div><Copy size={15} /><span>名称和语音说明会作为工具描述发送给豆包；应用路径与快捷键只在本机执行。</span></div><Button onClick={add}><Plus size={16} />继续添加</Button><Button kind="primary" onClick={save} disabled={saving}>{saving ? <LoaderCircle className="spin" size={16} /> : <Save size={16} />}{saving ? "正在保存" : "保存设置"}</Button></div>}
  </div>;
}

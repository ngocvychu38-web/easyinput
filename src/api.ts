import { invoke } from "@tauri-apps/api/core";
import type { ActivityDay, AppSettings, ArkConnectionTest, ArkModelConfig, DictionaryData, DictionaryExport, DictionaryImport, DoubaoConnectionTest, DoubaoSpeechConfig, HistoryEntry, InstalledApplication, KeyboardConfig, OperationResult, RealtimeCallState, RealtimeConnectionTest, RealtimeVoiceConfig, RuntimeSnapshot, WifiScanResult } from "./types";

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
let mockSnapshot: RuntimeSnapshot | undefined;

async function mockRuntime(): Promise<RuntimeSnapshot> {
  if (mockSnapshot) return structuredClone(mockSnapshot);
  const { DEFAULT_SETTINGS } = await import("./types");
  mockSnapshot = {
    version: "0.1.29", voiceService: "Connected", recording: { phase: "Idle", elapsedMs: 0, partialText: "" }, device: "ConnectedBle",
    capabilities: { config: true, microphone: true, speakerSync: true, agentLight: true, firmwareVersion: "0.4.53" },
    diagnostics: { packets: 0, bytes: 0, sequenceGaps: 0, outOfOrder: 0, rms: 0, peak: 0 }, settings: DEFAULT_SETTINGS,
    keyboardConfig: { revision: 1, targetPlatform: "MacOS", pttHotkey: "RightMeta", editPttHotkey: "RightOption", pttMode: "Hold",
      keys: ["语音输入", "语音编辑", "实时通话", "复制", "粘贴", "撤销", "全选", "打开历史"].map((label, index) => ({ kind: (["VoicePtt","EditPtt","RealtimeVoice","Copy","Paste","Undo","SelectAll","HostAction"] as const)[index], label })),
      encoder: { press: { kind: "ScrollAxisToggle", label: "切换滚动方向" }, axis: "Vertical", speed: 3, reverse: false },
      wifi: { ssid: "HUAWEI-1CRYV0", passwordSaved: true, audioHost: "192.168.1.12", audioPort: 17333 } },
    todayChars: 0, todayDurationMs: 0
  };
  return structuredClone(mockSnapshot);
}

export async function getRuntimeSnapshot() { return inTauri() ? invoke<RuntimeSnapshot>("get_runtime_snapshot") : mockRuntime(); }
export async function updateAppSettings(settings: AppSettings) {
  if (inTauri()) return invoke<OperationResult<AppSettings>>("update_app_settings", { settings });
  const snap = await mockRuntime(); mockSnapshot = { ...snap, settings: { ...settings, revision: settings.revision + 1 } };
  return { operationId: crypto.randomUUID(), ok: true, data: mockSnapshot.settings };
}
export async function startRecording(source: "Computer" | "Keyboard" | "KeyboardEdit" = "Computer"): Promise<OperationResult<{sessionId: string}>> { return inTauri() ? invoke<OperationResult<{sessionId: string}>>("start_recording", { source }) : { operationId: crypto.randomUUID(), ok: true, data: { sessionId: crypto.randomUUID() } }; }
export async function pushRecordingAudio(sessionId: string, samples: number[]): Promise<OperationResult> { return inTauri() ? invoke<OperationResult>("push_recording_audio", { sessionId, samples }) : { operationId: crypto.randomUUID(), ok: true }; }
export async function stopRecording(sessionId: string): Promise<OperationResult> { return inTauri() ? invoke<OperationResult>("stop_recording", { sessionId }) : { operationId: crypto.randomUUID(), ok: true }; }
export async function getHistoryPage(cursor?: number, limit = 20) { return inTauri() ? invoke<HistoryEntry[]>("get_history_page", { cursor, limit }) : []; }
export async function getActivityCalendar(year: number, month: number) { return inTauri() ? invoke<ActivityDay[]>("get_activity_calendar", { year, month }) : []; }
export async function deleteHistory(id: number) { return inTauri() ? invoke<OperationResult>("delete_history", { id }) : { operationId: crypto.randomUUID(), ok: true }; }
export async function getDictionary(): Promise<DictionaryData> { if(inTauri())return invoke<DictionaryData>("get_dictionary");const saved=localStorage.getItem("easyinput.dictionary");return saved?JSON.parse(saved):{version:1,hotwords:[],replacements:[]}; }
export async function saveDictionary(hotwords: string[], replacements: [string,string][]) { if(inTauri())return invoke<OperationResult>("save_dictionary",{hotwords,replacements});localStorage.setItem("easyinput.dictionary",JSON.stringify({version:1,hotwords,replacements}));return{operationId:crypto.randomUUID(),ok:true}as OperationResult; }
export async function importDictionaryFile(path: string) { return invoke<OperationResult<DictionaryImport>>("import_dictionary_file", { path }); }
export async function exportDictionaryFile(path: string, hotwords: string[]) { return invoke<OperationResult<DictionaryExport>>("export_dictionary_file", { path, hotwords }); }
export async function readKeyboardStatus() { return invokeMaybe<OperationResult>("read_ai_keyboard_status"); }
export async function openBluetoothSettings() { return invokeMaybe<OperationResult>("open_bluetooth_settings"); }
export async function openInputMonitoringSettings() { return invokeMaybe<OperationResult>("open_input_monitoring_settings"); }
export async function listInstalledApplications(): Promise<InstalledApplication[]> {
  if (inTauri()) return invoke<InstalledApplication[]>("list_installed_applications");
  return ["Safari", "Mail", "Calendar", "Notes", "Terminal", "System Settings"].map(name => ({ name, path: `/Applications/${name}.app` }));
}
export async function listAvailableWifiNetworks(): Promise<WifiScanResult> {
  if (inTauri()) return invoke<WifiScanResult>("list_available_wifi_networks");
  const snapshot = await mockRuntime();
  const ssid = snapshot.keyboardConfig.wifi.ssid;
  return { interface: "en0", currentSsid: ssid, localIp: snapshot.keyboardConfig.wifi.audioHost, networks: ssid ? [{ ssid, current: true, remembered: true, configured: true }] : [] };
}
export async function pushKeyboardConfig(config: KeyboardConfig, wifiPassword?: string) {
  const password = typeof wifiPassword === "string" ? wifiPassword : undefined;
  if (inTauri()) return invoke<OperationResult>("push_ai_keyboard_config", { config, wifiPassword: password || null });
  const snap = await mockRuntime();
  mockSnapshot = { ...snap, keyboardConfig: { ...config, revision: config.revision + 1, wifi: { ...config.wifi, passwordSaved: Boolean(password) || config.wifi.passwordSaved } } };
  return { operationId: crypto.randomUUID(), ok: true } as OperationResult;
}
export async function checkAppUpdate() {
  if (inTauri()) return invoke<OperationResult<{current:string;latest:string;available:boolean}>>("check_app_update");
  const current = (await mockRuntime()).capabilities.firmwareVersion || "0.4.53";
  return { operationId: crypto.randomUUID(), ok: true, data: { current, latest: current, available: false } } as OperationResult<{current:string;latest:string;available:boolean}>;
}
export async function login(email: string, password: string) { return invokeMaybe<OperationResult>("login", { email, password }); }
export async function logout() { return invokeMaybe<OperationResult>("logout"); }
export async function getDoubaoSpeechConfig() {
  if (inTauri()) return invoke<DoubaoSpeechConfig>("get_doubao_speech_config");
  const { DEFAULT_DOUBAO_CONFIG } = await import("./types");
  const saved = localStorage.getItem("easyinput.doubao.config");
  return saved ? { ...DEFAULT_DOUBAO_CONFIG, ...JSON.parse(saved) } : DEFAULT_DOUBAO_CONFIG;
}
export async function saveDoubaoSpeechConfig(config: DoubaoSpeechConfig, accessToken?: string) {
  if (inTauri()) return invoke<OperationResult<DoubaoSpeechConfig>>("save_doubao_speech_config", { config, accessToken: accessToken || null });
  const next = { ...config, accessTokenSaved: Boolean(accessToken) || config.accessTokenSaved };
  localStorage.setItem("easyinput.doubao.config", JSON.stringify(next));
  return { operationId: crypto.randomUUID(), ok: true, data: next };
}
export async function testDoubaoConnection(config: DoubaoSpeechConfig, accessToken?: string) {
  if (inTauri()) return invoke<OperationResult<DoubaoConnectionTest>>("test_doubao_connection", { config, accessToken: accessToken || null });
  return { operationId: crypto.randomUUID(), ok: false, message: "浏览器预览模式不能测试原生 WebSocket 鉴权" } as OperationResult<DoubaoConnectionTest>;
}
export async function getArkModelConfig() {
  if (inTauri()) return invoke<ArkModelConfig>("get_ark_model_config");
  const { DEFAULT_ARK_CONFIG } = await import("./types");
  const saved = localStorage.getItem("easyinput.ark.config");
  return saved ? { ...DEFAULT_ARK_CONFIG, ...JSON.parse(saved) } : DEFAULT_ARK_CONFIG;
}
export async function saveArkModelConfig(config: ArkModelConfig, apiKey?: string) {
  if (inTauri()) return invoke<OperationResult<ArkModelConfig>>("save_ark_model_config", { config, apiKey: apiKey || null });
  const next = { ...config, apiKeySaved: Boolean(apiKey) || config.apiKeySaved };
  localStorage.setItem("easyinput.ark.config", JSON.stringify(next));
  return { operationId: crypto.randomUUID(), ok: true, data: next } as OperationResult<ArkModelConfig>;
}
export async function testArkConnection(config: ArkModelConfig, apiKey?: string) {
  if (inTauri()) return invoke<OperationResult<ArkConnectionTest>>("test_ark_connection", { config, apiKey: apiKey || null });
  return { operationId: crypto.randomUUID(), ok: false, message: "浏览器预览模式不能测试方舟模型鉴权" } as OperationResult<ArkConnectionTest>;
}
export async function getRealtimeVoiceConfig() {
  if (inTauri()) return invoke<RealtimeVoiceConfig>("get_realtime_voice_config");
  const { DEFAULT_REALTIME_CONFIG } = await import("./types");
  const saved = localStorage.getItem("easyinput.realtime.config");
  return saved ? { ...DEFAULT_REALTIME_CONFIG, ...JSON.parse(saved) } : DEFAULT_REALTIME_CONFIG;
}
export async function saveRealtimeVoiceConfig(config: RealtimeVoiceConfig, apiKey?: string) {
  if (inTauri()) return invoke<OperationResult<RealtimeVoiceConfig>>("save_realtime_voice_config", { config, apiKey: apiKey || null });
  const next = { ...config, apiKeySaved: Boolean(apiKey) || config.apiKeySaved };
  localStorage.setItem("easyinput.realtime.config", JSON.stringify(next));
  return { operationId: crypto.randomUUID(), ok: true, data: next } as OperationResult<RealtimeVoiceConfig>;
}
export async function testRealtimeVoiceConnection(config: RealtimeVoiceConfig, apiKey?: string) {
  if (inTauri()) return invoke<OperationResult<RealtimeConnectionTest>>("test_realtime_voice_connection", { config, apiKey: apiKey || null });
  return { operationId: crypto.randomUUID(), ok: false, message: "浏览器预览模式不能测试实时语音 WebSocket 鉴权" } as OperationResult<RealtimeConnectionTest>;
}
export async function getRealtimeCallState(): Promise<RealtimeCallState> {
  if (inTauri()) return invoke<RealtimeCallState>("get_realtime_call_state");
  return { phase: "Idle", userText: "", assistantText: "", elapsedMs: 0, inputPackets: 0, outputPackets: 0 };
}
export async function startRealtimeCall() { return inTauri() ? invoke<OperationResult<{sessionId:string}>>("start_realtime_call") : { operationId: crypto.randomUUID(), ok: false, message: "浏览器预览模式不能连接开发板" } as OperationResult<{sessionId:string}>; }
export async function stopRealtimeCall() { return invokeMaybe<OperationResult>("stop_realtime_call"); }
export async function interruptRealtimeCall() { return invokeMaybe<OperationResult>("interrupt_realtime_call"); }

async function invokeMaybe<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri()) return invoke<T>(command, args);
  return { operationId: crypto.randomUUID(), ok: true } as T;
}

export type RecordingPhase = "Idle" | "Preparing" | "Recording" | "Draining" | "Error";
export interface RecordingState { phase: RecordingPhase; sessionId?: string; elapsedMs: number; partialText: string; error?: string }
export type VoiceServiceState = "Connected" | "Connecting" | "Reconnecting" | "Disconnected";
export type DeviceConnectionState = "Disconnected" | "Discovering" | "ConnectedUsb" | "ConnectedBle" | "Degraded" | "Error";
export type ConfigSyncState = "LocalOnly" | "Pending" | "Syncing" | "Confirmed" | "Failed" | "Unknown";
export interface DeviceCapabilities { config: boolean; microphone: boolean; speakerSync: boolean; agentLight: boolean; firmwareVersion?: string }
export interface AudioStreamDiagnostics { packets: number; bytes: number; sequenceGaps: number; outOfOrder: number; rms: number; peak: number; lastHeartbeatAt?: string; lastError?: string }
export type KeyboardActionKind = "VoicePtt" | "EditPtt" | "Enter" | "Backspace" | "Cut" | "SelectAll" | "Copy" | "Paste" | "Undo" | "Hotkey" | "FixedText" | "OpenApp" | "ScrollAxisToggle" | "CaretSelect" | "Disabled" | "HostAction";
export interface KeyboardAction { kind: KeyboardActionKind; label: string; value?: string; hostActionId?: string }
export interface InstalledApplication { name: string; path: string }
export interface KeyboardConfig {
  revision: number; targetPlatform: "MacOS"; pttHotkey: string; editPttHotkey: string; pttMode: "Hold" | "Toggle";
  keys: KeyboardAction[]; encoder: { press: KeyboardAction; axis: "Vertical" | "Horizontal"; speed: number; reverse: boolean };
  wifi: { ssid: string; passwordSaved: boolean; audioHost: string; audioPort: number };
}
export interface HistoryEntry { id: number; text: string; createdAt: string; durationMs: number; charCount: number; source: "Computer" | "Keyboard" | "KeyboardEdit" }
export interface ActivityDay { day: number; charCount: number; durationMs: number }
export interface DictionaryData { version: number; hotwords: string[]; replacements: [string,string][] }
export interface DictionaryImport { words: string[]; blankLines: number; duplicateLines: number }
export interface DictionaryExport { path: string; count: number }
export interface AppSettings {
  revision: number; inputHotkey: string; editHotkey: string; triggerMode: "Hold" | "Toggle"; cleanupMode: "Original" | "Smart" | "Custom";
  customCleanup: string; inputMode: "Auto" | "Direct" | "Paste"; enterToStop: boolean; overlayEnabled: boolean; livePreview: boolean;
  overlayPosition: "Top" | "Bottom" | "Cursor"; overlayOpacity: number; appearance: "System" | "Light" | "Dark"; microphoneSource: "KeyboardPreferred" | "Computer";
}
export interface DoubaoSpeechConfig {
  enabled: boolean; endpoint: string; appKey: string; resourceId: string; modelName: string; language: string;
  enableItn: boolean; enablePunc: boolean; showUtterances: boolean; accessTokenSaved: boolean;
}
export interface ArkModelConfig { enabled: boolean; endpoint: string; model: string; apiKeySaved: boolean }
export interface ArkConnectionTest { latencyMs: number; model: string }
export interface DoubaoConnectionTest { latencyMs: number; endpoint: string; resourceId: string; logId?: string }
export interface SpeechTranscriptEvent { sessionId: string; text: string; definite: boolean; sequence?: number }
export interface SpeechSessionEvent { sessionId: string; phase: RecordingPhase; text: string; durationMs: number; message?: string }
export interface HardwareVoiceButtonEvent { pressed: boolean; source: "app-report" | "keyboard-report"; sequence: number }
export interface HardwareEditButtonEvent { pressed: boolean; sequence: number; hasSelection: boolean }
export interface RuntimeSnapshot {
  version: string; voiceService: VoiceServiceState; recording: RecordingState; device: DeviceConnectionState; capabilities: DeviceCapabilities;
  diagnostics: AudioStreamDiagnostics; settings: AppSettings; keyboardConfig: KeyboardConfig; todayChars: number; todayDurationMs: number;
}
export interface OperationResult<T = undefined> { operationId: string; ok: boolean; data?: T; message?: string }

export const DEFAULT_SETTINGS: AppSettings = {
  revision: 1, inputHotkey: "RightCommand", editHotkey: "RightOption", triggerMode: "Hold", cleanupMode: "Original", customCleanup: "",
  inputMode: "Auto", enterToStop: true, overlayEnabled: true, livePreview: true, overlayPosition: "Bottom", overlayOpacity: .7,
  appearance: "System", microphoneSource: "KeyboardPreferred"
};
export const DEFAULT_DOUBAO_CONFIG: DoubaoSpeechConfig = {
  enabled: false, endpoint: "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel", appKey: "",
  resourceId: "volc.bigasr.sauc.duration", modelName: "bigmodel", language: "zh-CN",
  enableItn: true, enablePunc: true, showUtterances: true, accessTokenSaved: false
};
export const DEFAULT_ARK_CONFIG: ArkModelConfig = {
  enabled: false, endpoint: "https://ark.cn-beijing.volces.com/api/v3/responses", model: "doubao-seed-2-0-lite-260215", apiKeySaved: false
};

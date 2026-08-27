use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum RecordingPhase { Idle, Preparing, Recording, Draining, Error }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingState { pub phase: RecordingPhase, pub session_id: Option<String>, pub elapsed_ms: u64, pub partial_text: String, pub error: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum VoiceServiceState { Connected, Connecting, Reconnecting, Disconnected }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceConnectionState { Disconnected, Discovering, ConnectedUsb, ConnectedBle, Degraded, Error }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCapabilities { pub config: bool, pub microphone: bool, pub speaker_sync: bool, pub agent_light: bool, pub firmware_version: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AudioStreamDiagnostics { pub packets: u64, pub bytes: u64, pub sequence_gaps: u64, pub out_of_order: u64, pub rms: f32, pub peak: f32, pub last_heartbeat_at: Option<String>, pub last_error: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyboardActionKind { VoicePtt, EditPtt, Enter, Backspace, Cut, SelectAll, Copy, Paste, Undo, Hotkey, FixedText, OpenApp, ScrollAxisToggle, CaretSelect, Disabled, HostAction }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardAction {
    pub kind: KeyboardActionKind,
    pub label: String,
    pub value: Option<String>,
    #[serde(default)]
    pub host_action_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderConfig { pub press: KeyboardAction, pub axis: String, pub speed: u8, pub reverse: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiConfig { pub ssid: String, pub password_saved: bool, pub audio_host: String, pub audio_port: u16 }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardConfig { pub revision: u64, pub target_platform: String, pub ptt_hotkey: String, pub edit_ptt_hotkey: String, pub ptt_mode: String, pub keys: Vec<KeyboardAction>, pub encoder: EncoderConfig, pub wifi: WifiConfig }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings { pub revision: u64, pub input_hotkey: String, pub edit_hotkey: String, pub trigger_mode: String, pub cleanup_mode: String, pub custom_cleanup: String, pub input_mode: String, pub enter_to_stop: bool, pub overlay_enabled: bool, pub live_preview: bool, pub overlay_position: String, pub overlay_opacity: f32, pub appearance: String, pub microphone_source: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoubaoSpeechConfig { pub enabled: bool, pub endpoint: String, pub app_key: String, pub resource_id: String, pub model_name: String, pub language: String, pub enable_itn: bool, pub enable_punc: bool, pub show_utterances: bool, #[serde(default)] pub access_token_saved: bool }

impl Default for DoubaoSpeechConfig { fn default()->Self { Self { enabled:false,endpoint:"wss://openspeech.bytedance.com/api/v3/sauc/bigmodel".into(),app_key:String::new(),resource_id:"volc.bigasr.sauc.duration".into(),model_name:"bigmodel".into(),language:"zh-CN".into(),enable_itn:true,enable_punc:true,show_utterances:true,access_token_saved:false } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArkModelConfig { pub enabled: bool, pub endpoint: String, pub model: String, #[serde(default)] pub api_key_saved: bool }

impl Default for ArkModelConfig { fn default()->Self { Self { enabled:false,endpoint:"https://ark.cn-beijing.volces.com/api/v3/responses".into(),model:"doubao-seed-2-0-lite-260215".into(),api_key_saved:false } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot { pub version: String, pub voice_service: VoiceServiceState, pub recording: RecordingState, pub device: DeviceConnectionState, pub capabilities: DeviceCapabilities, pub diagnostics: AudioStreamDiagnostics, pub settings: AppSettings, pub keyboard_config: KeyboardConfig, pub today_chars: u64, pub today_duration_ms: u64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry { pub id: i64, pub text: String, pub created_at: String, pub duration_ms: u64, pub char_count: u64, pub source: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDay { pub day: u32, pub char_count: u64, pub duration_ms: u64 }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult<T: Serialize> { pub operation_id: String, pub ok: bool, pub data: Option<T>, pub message: Option<String> }
impl<T: Serialize> OperationResult<T> {
    pub fn success(data: Option<T>) -> Self { Self { operation_id: uuid::Uuid::new_v4().to_string(), ok: true, data, message: None } }
    pub fn failure(message: impl Into<String>) -> Self { Self { operation_id: uuid::Uuid::new_v4().to_string(), ok: false, data: None, message: Some(message.into()) } }
}

impl Default for AppSettings { fn default() -> Self { Self { revision: 1, input_hotkey:"RightCommand".into(), edit_hotkey:"RightOption".into(), trigger_mode:"Hold".into(), cleanup_mode:"Original".into(), custom_cleanup:String::new(), input_mode:"Auto".into(), enter_to_stop:true, overlay_enabled:true, live_preview:true, overlay_position:"Bottom".into(), overlay_opacity:0.7, appearance:"System".into(), microphone_source:"KeyboardPreferred".into() } } }
impl Default for KeyboardConfig { fn default() -> Self { let make=|kind: KeyboardActionKind,label: &str|KeyboardAction{kind,label:label.into(),value:None,host_action_id:None}; Self { revision:1,target_platform:"MacOS".into(),ptt_hotkey:"RightMeta".into(),edit_ptt_hotkey:"RightOption".into(),ptt_mode:"Hold".into(),keys:vec![make(KeyboardActionKind::VoicePtt,"语音输入"),make(KeyboardActionKind::EditPtt,"语音编辑"),make(KeyboardActionKind::Copy,"复制"),make(KeyboardActionKind::Paste,"粘贴"),make(KeyboardActionKind::Undo,"撤销"),make(KeyboardActionKind::SelectAll,"全选"),make(KeyboardActionKind::HostAction,"打开历史"),make(KeyboardActionKind::Disabled,"禁用")],encoder:EncoderConfig{press:make(KeyboardActionKind::ScrollAxisToggle,"切换滚动方向"),axis:"Vertical".into(),speed:3,reverse:false},wifi:WifiConfig{ssid:String::new(),password_saved:false,audio_host:String::new(),audio_port:17333} } } }

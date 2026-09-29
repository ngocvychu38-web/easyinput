mod app_catalog;
mod ark;
mod device;
mod diagnostics;
mod dictionary;
mod firmware_config;
mod input;
mod live;
mod mcd_mcp;
mod mcd_order;
mod model;
mod payment_mvp;
mod protocol;
mod realtime;
mod speech;
mod storage;
mod wifi;

use device::DeviceManager;
use model::*;
use std::{collections::HashMap, sync::Mutex};
use storage::Storage;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};

pub struct AppState {
    storage: Storage,
    live: live::State,
    recording: Mutex<RecordingState>,
    speech_sessions:
        Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<speech::StreamCommand>>>,
    speech_token: Mutex<Option<String>>,
    ark_api_key: Mutex<Option<String>>,
    realtime_api_key: Mutex<Option<String>>,
    mcd_token: Mutex<Option<String>>,
    pub(crate) mcd_orders: mcd_order::ManagerState,
    realtime_call: Mutex<RealtimeCallState>,
    realtime_start_gate: tokio::sync::Mutex<()>,
    realtime_session: Mutex<Option<tokio::sync::mpsc::UnboundedSender<realtime::RealtimeCommand>>>,
    pending_edit_context: Mutex<Option<String>>,
    edit_contexts: Mutex<HashMap<String, String>>,
    device: DeviceManager,
}
impl AppState {
    fn new(storage: Storage) -> Self {
        Self {
            storage,
            live: Mutex::new(None),
            recording: Mutex::new(RecordingState {
                phase: RecordingPhase::Idle,
                session_id: None,
                elapsed_ms: 0,
                partial_text: String::new(),
                error: None,
            }),
            speech_sessions: Mutex::new(HashMap::new()),
            speech_token: Mutex::new(None),
            ark_api_key: Mutex::new(None),
            realtime_api_key: Mutex::new(None),
            mcd_token: Mutex::new(None),
            mcd_orders: mcd_order::ManagerState::default(),
            realtime_call: Mutex::new(RealtimeCallState::default()),
            realtime_start_gate: tokio::sync::Mutex::new(()),
            realtime_session: Mutex::new(None),
            pending_edit_context: Mutex::new(None),
            edit_contexts: Mutex::new(HashMap::new()),
            device: DeviceManager::new(),
        }
    }
}

pub(crate) fn capture_edit_context(app: &tauri::AppHandle) -> bool {
    let selection = match input::selected_text() {
        Ok(value) => value.filter(|text| !text.trim().is_empty()),
        Err(error) => {
            eprintln!("EasyInput selection capture failed: {error}");
            None
        }
    };
    let present = selection.is_some();
    eprintln!(
        "EasyInput edit selection captured: present={present}, chars={}",
        selection
            .as_ref()
            .map(|text| text.chars().count())
            .unwrap_or(0)
    );
    let state = app.state::<AppState>();
    if let Ok(mut context) = state.pending_edit_context.lock() {
        *context = selection;
    };
    present
}

pub(crate) fn execute_host_action(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let canonical = uuid::Uuid::parse_str(id)
        .map_err(|_| "设备发回了无效的主机动作标识")?
        .hyphenated()
        .to_string()
        .to_lowercase();
    let state = app.state::<AppState>();
    let cfg = state.storage.read_config()?;
    let action = cfg
        .keyboard
        .keys
        .iter()
        .find(|action| {
            matches!(
                action.kind,
                KeyboardActionKind::OpenApp | KeyboardActionKind::HostAction
            ) && action
                .host_action_id
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(&canonical))
        })
        .ok_or_else(|| "设备动作在本机没有对应的应用映射".to_string())?;
    let path = action
        .value
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "设备动作对应的应用路径为空".to_string())?;
    if !std::path::Path::new(path).is_dir() {
        return Err(format!("应用已不存在：{path}"));
    }
    match std::process::Command::new("/usr/bin/open")
        .arg(path)
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("打开应用失败：{status}")),
        Err(error) => Err(format!("无法打开应用：{error}")),
    }
}

#[tauri::command]
fn get_runtime_snapshot(state: tauri::State<AppState>) -> Result<RuntimeSnapshot, String> {
    let cfg = state.storage.read_config()?;
    let (today_chars, today_duration_ms) = state.storage.today_stats()?;
    let voice_service = if cfg.speech.enabled && cfg.speech.access_token_saved {
        VoiceServiceState::Connected
    } else {
        VoiceServiceState::Disconnected
    };
    let (device, capabilities) = state.device.discover_timeout_3s();
    Ok(RuntimeSnapshot {
        version: env!("CARGO_PKG_VERSION").into(),
        voice_service,
        recording: state
            .recording
            .lock()
            .map_err(|_| "录音状态锁已损坏")?
            .clone(),
        device,
        capabilities,
        diagnostics: AudioStreamDiagnostics::default(),
        settings: cfg.settings,
        keyboard_config: cfg.keyboard,
        today_chars,
        today_duration_ms,
    })
}

#[tauri::command]
fn update_app_settings(
    state: tauri::State<AppState>,
    mut settings: AppSettings,
) -> OperationResult<AppSettings> {
    match state.storage.read_config() {
        Ok(mut cfg) => {
            settings.revision = cfg.settings.revision + 1;
            cfg.revision += 1;
            cfg.settings = settings.clone();
            match state.storage.write_config(&cfg) {
                Ok(_) => OperationResult::success(Some(settings)),
                Err(e) => OperationResult::failure(e),
            }
        }
        Err(e) => OperationResult::failure(e),
    }
}

#[tauri::command]
fn list_installed_applications() -> Result<Vec<app_catalog::InstalledApplication>, String> {
    app_catalog::list_installed_applications()
}

#[tauri::command]
async fn list_available_wifi_networks(
    state: tauri::State<'_, AppState>,
) -> Result<wifi::WifiScanResult, String> {
    let configured = state.storage.read_config()?.keyboard.wifi.ssid;
    tokio::task::spawn_blocking(move || wifi::scan(&configured))
        .await
        .map_err(|error| format!("Wi-Fi 扫描任务失败：{error}"))?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionStart {
    session_id: String,
}
#[tauri::command]
async fn start_recording(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    source: Option<String>,
) -> Result<OperationResult<SessionStart>, String> {
    let _guard = state.realtime_start_gate.lock().await;
    if state.live.lock().map_err(|_| "直播状态锁损坏")?.is_some() { return Ok(OperationResult::failure("请先停止直播字幕识别")); }
    let persisted = state.storage.read_config()?;
    let config = persisted.speech;
    if !config.enabled {
        return Ok(OperationResult::failure(
            "请先在语音服务配置中启用豆包语音识别",
        ));
    }
    let source = match source.as_deref() {
        Some("KeyboardEdit") => "KeyboardEdit",
        Some("Keyboard") => "Keyboard",
        _ => "Computer",
    };
    if source == "KeyboardEdit" && !persisted.ark.enabled {
        return Ok(OperationResult::failure(
            "请先在“语音服务配置”中启用火山方舟文本模型",
        ));
    }
    if source == "KeyboardEdit" && !persisted.ark.api_key_saved {
        return Ok(OperationResult::failure(
            "请先在“语音服务配置”中保存火山方舟 API Key",
        ));
    }
    if source.starts_with("Keyboard") && !input::request_text_input_access() {
        return Ok(OperationResult::failure(
            "请在“系统设置 → 隐私与安全性 → 辅助功能”中允许 EasyInput，然后再次按下语音键",
        ));
    }
    let edit_context = if source == "KeyboardEdit" {
        let pending = state
            .pending_edit_context
            .lock()
            .ok()
            .and_then(|mut value| value.take());
        pending
            .or_else(|| input::selected_text().ok().flatten())
            .filter(|value| !value.trim().is_empty())
    } else {
        None
    };
    let session_id = uuid::Uuid::new_v4().to_string();
    {
        let mut recording = state.recording.lock().map_err(|_| "录音状态锁已损坏")?;
        if !matches!(
            recording.phase,
            RecordingPhase::Idle | RecordingPhase::Error
        ) {
            return Ok(OperationResult::failure("已有录音会话正在进行"));
        }
        *recording = RecordingState {
            phase: RecordingPhase::Preparing,
            session_id: Some(session_id.clone()),
            elapsed_ms: 0,
            partial_text: String::new(),
            error: None,
        };
    }
    if let Some(context) = edit_context {
        state
            .edit_contexts
            .lock()
            .map_err(|_| "语音编辑上下文锁已损坏")?
            .insert(session_id.clone(), context);
    }
    let token = match read_speech_token(state.inner()).await {
        Ok(v) => v,
        Err(e) => {
            if let Ok(mut contexts) = state.edit_contexts.lock() {
                contexts.remove(&session_id);
            }
            if let Ok(mut recording) = state.recording.lock() {
                *recording = RecordingState {
                    phase: RecordingPhase::Error,
                    session_id: None,
                    elapsed_ms: 0,
                    partial_text: String::new(),
                    error: Some(e.clone()),
                }
            }
            return Ok(OperationResult::failure(e));
        }
    };
    let sender =
        match speech::open_stream(app, session_id.clone(), config, token, source.into()).await {
            Ok(v) => v,
            Err(e) => {
                if let Ok(mut contexts) = state.edit_contexts.lock() {
                    contexts.remove(&session_id);
                }
                if let Ok(mut recording) = state.recording.lock() {
                    *recording = RecordingState {
                        phase: RecordingPhase::Error,
                        session_id: None,
                        elapsed_ms: 0,
                        partial_text: String::new(),
                        error: Some(e.clone()),
                    }
                }
                return Ok(OperationResult::failure(e));
            }
        };
    state
        .speech_sessions
        .lock()
        .map_err(|_| "语音会话锁已损坏")?
        .insert(session_id.clone(), sender);
    if let Ok(mut recording) = state.recording.lock() {
        recording.phase = RecordingPhase::Recording;
    }
    Ok(OperationResult::success(Some(SessionStart { session_id })))
}

#[tauri::command]
fn push_recording_audio(
    state: tauri::State<AppState>,
    session_id: String,
    samples: Vec<i16>,
) -> OperationResult<()> {
    if samples.len() > 16_000 {
        return OperationResult::failure("单个语音分片超过 1 秒限制");
    }
    let matches = state
        .recording
        .lock()
        .map(|r| {
            r.session_id.as_deref() == Some(&session_id)
                && matches!(r.phase, RecordingPhase::Recording)
        })
        .unwrap_or(false);
    if !matches {
        return OperationResult::failure("语音分片属于已过期会话，已忽略");
    }
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        pcm.extend_from_slice(&sample.to_le_bytes())
    }
    let sender = match state
        .speech_sessions
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&session_id).cloned())
    {
        Some(v) => v,
        None => return OperationResult::failure("豆包语音会话不存在"),
    };
    match sender.send(speech::StreamCommand::Audio(pcm)) {
        Ok(_) => OperationResult::success(None),
        Err(_) => OperationResult::failure("豆包语音会话已结束"),
    }
}

#[tauri::command]
fn stop_recording(state: tauri::State<AppState>, session_id: String) -> OperationResult<()> {
    eprintln!("EasyInput stop recording received: session={session_id}");
    {
        let mut recording = match state.recording.lock() {
            Ok(v) => v,
            Err(_) => return OperationResult::failure("录音状态锁已损坏"),
        };
        if recording.session_id.as_deref() != Some(&session_id) {
            return OperationResult::failure("录音会话已过期，已忽略停止结果");
        };
        recording.phase = RecordingPhase::Draining;
    }
    let sender = match state
        .speech_sessions
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&session_id).cloned())
    {
        Some(v) => v,
        None => return OperationResult::failure("豆包语音会话不存在"),
    };
    match sender.send(speech::StreamCommand::Finish) {
        Ok(_) => OperationResult::success(None),
        Err(_) => OperationResult::failure("豆包语音会话已结束"),
    }
}

#[tauri::command]
fn get_history_page(
    state: tauri::State<AppState>,
    cursor: Option<i64>,
    limit: Option<u32>,
) -> Result<Vec<HistoryEntry>, String> {
    state.storage.history_page(cursor, limit.unwrap_or(20))
}
#[tauri::command]
fn get_activity_calendar(
    state: tauri::State<AppState>,
    year: i32,
    month: u32,
) -> Result<Vec<ActivityDay>, String> {
    state.storage.activity_month(year, month)
}
#[tauri::command]
fn delete_history(state: tauri::State<AppState>, id: i64) -> OperationResult<()> {
    match state.storage.delete_history(id) {
        Ok(_) => OperationResult::success(None),
        Err(e) => OperationResult::failure(e),
    }
}
#[tauri::command]
fn save_dictionary(
    state: tauri::State<AppState>,
    hotwords: Vec<String>,
    replacements: Vec<(String, String)>,
) -> OperationResult<()> {
    if hotwords.len() > 1000 {
        return OperationResult::failure("热词数量超过 1000 个");
    };
    match state.storage.save_dictionary(&hotwords, &replacements) {
        Ok(_) => OperationResult::success(None),
        Err(e) => OperationResult::failure(e),
    }
}

#[tauri::command]
fn get_dictionary(state: tauri::State<AppState>) -> Result<dictionary::DictionaryData, String> {
    state.storage.read_dictionary()
}

#[tauri::command]
fn import_dictionary_file(path: String) -> OperationResult<dictionary::DictionaryImport> {
    let bytes = match std::fs::read(&path) {
        Ok(v) => v,
        Err(e) => return OperationResult::failure(format!("无法读取词库文件：{e}")),
    };
    match dictionary::parse_text(&bytes) {
        Ok(data) => OperationResult::success(Some(data)),
        Err(e) => OperationResult::failure(e),
    }
}

#[tauri::command]
fn export_dictionary_file(
    path: String,
    hotwords: Vec<String>,
) -> OperationResult<dictionary::DictionaryExport> {
    let bytes = match dictionary::encode_text(&hotwords) {
        Ok(v) => v,
        Err(e) => return OperationResult::failure(e),
    };
    let mut target = std::path::PathBuf::from(path);
    if target.extension().is_none() {
        target.set_extension("txt");
    }
    match std::fs::write(&target, bytes) {
        Ok(_) => OperationResult::success(Some(dictionary::DictionaryExport {
            path: target.to_string_lossy().into_owned(),
            count: hotwords.len(),
        })),
        Err(e) => OperationResult::failure(format!("无法导出词库：{e}")),
    }
}

#[tauri::command]
fn get_ai_keyboard_connection_state(
    state: tauri::State<AppState>,
) -> OperationResult<DeviceConnectionState> {
    match state.device.discover() {
        Ok((s, _)) => OperationResult::success(Some(s)),
        Err(e) => OperationResult::failure(e),
    }
}
#[tauri::command]
fn read_ai_keyboard_status(state: tauri::State<AppState>) -> OperationResult<DeviceCapabilities> {
    match state.device.discover() {
        Ok((_, c)) => OperationResult::success(Some(c)),
        Err(e) => OperationResult::failure(e),
    }
}
#[tauri::command]
fn push_ai_keyboard_config(
    state: tauri::State<AppState>,
    mut config: KeyboardConfig,
    wifi_password: Option<String>,
) -> OperationResult<()> {
    for (index, action) in config.keys.iter().enumerate() {
        if matches!(action.kind, KeyboardActionKind::OpenApp) {
            let Some(path) = action
                .value
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            else {
                return OperationResult::failure(format!(
                    "KEY{} 的“打开应用”动作尚未选择应用",
                    index + 1
                ));
            };
            if !path.to_lowercase().ends_with(".app") {
                return OperationResult::failure(format!(
                    "KEY{} 选择的目标不是 macOS 应用程序",
                    index + 1
                ));
            }
            if !std::path::Path::new(path).is_dir() {
                return OperationResult::failure(format!(
                    "KEY{} 选择的应用已不存在，请重新选择",
                    index + 1
                ));
            }
        }
    }
    let mut persisted = match state.storage.read_config() {
        Ok(v) => v,
        Err(e) => return OperationResult::failure(e),
    };
    config.revision = persisted.keyboard.revision + 1;
    let supplied_password = wifi_password.filter(|value| !value.is_empty());
    let resolved_password = if let Some(password) = supplied_password {
        if config.wifi.ssid.trim().is_empty() {
            return OperationResult::failure("请先选择或输入 Wi-Fi 名称");
        }
        if let Err(error) = storage::set_secret(wifi::PASSWORD_ACCOUNT, &password) {
            return OperationResult::failure(format!(
                "无法将 Wi-Fi 密码写入 macOS 钥匙串：{error}"
            ));
        }
        config.wifi.password_saved = true;
        Some(password)
    } else if config.wifi.password_saved {
        match storage::get_secret(wifi::PASSWORD_ACCOUNT) {
            Ok(Some(password)) => Some(password),
            Ok(None) => {
                return OperationResult::failure("本机没有找到已保存的 Wi-Fi 密码，请重新输入")
            }
            Err(error) => {
                return OperationResult::failure(format!("无法读取已保存的 Wi-Fi 密码：{error}"))
            }
        }
    } else {
        None
    };
    let (config, bytes) = match firmware_config::prepare(config, resolved_password.as_deref()) {
        Ok(value) => value,
        Err(error) => return OperationResult::failure(error),
    };
    persisted.revision += 1;
    persisted.keyboard = config;
    if let Err(e) = state.storage.write_config(&persisted) {
        return OperationResult::failure(e);
    };
    match state.device.push_config(&bytes) {
        Ok(_) => OperationResult::success(None),
        Err(e) => OperationResult::failure(format!("配置已保存在本机，但未同步到设备：{e}")),
    }
}
#[tauri::command]
fn sync_ai_keyboard_boot_sound(
    _state: tauri::State<AppState>,
    path: String,
) -> OperationResult<()> {
    if !std::path::Path::new(&path).exists() {
        return OperationResult::failure("音频文件不存在");
    };
    OperationResult::failure("音效转码与 A/B 传输需要连接真实键盘后完成端到端验证")
}

#[tauri::command]
fn open_bluetooth_settings() -> OperationResult<()> {
    #[cfg(target_os = "macos")]
    {
        let major = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|v| String::from_utf8(v.stdout).ok())
            .and_then(|v| v.trim().split('.').next()?.parse::<u32>().ok())
            .unwrap_or(13);
        let pane = if major >= 13 {
            "x-apple.systempreferences:com.apple.BluetoothSettings"
        } else {
            "x-apple.systempreferences:com.apple.preference.Bluetooth"
        };
        match std::process::Command::new("/usr/bin/open")
            .arg(pane)
            .status()
        {
            Ok(status) if status.success() => OperationResult::success(None),
            _ => match std::process::Command::new("/usr/bin/open")
                .args(["-b", "com.apple.systempreferences"])
                .status()
            {
                Ok(status) if status.success() => OperationResult::success(None),
                Ok(status) => OperationResult::failure(format!("系统设置启动失败：{status}")),
                Err(e) => OperationResult::failure(format!("无法打开系统设置：{e}")),
            },
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        OperationResult::failure("此功能仅支持 macOS")
    }
}

#[tauri::command]
fn open_input_monitoring_settings() -> OperationResult<()> {
    #[cfg(target_os = "macos")]
    {
        if device::request_input_monitoring_access() {
            return OperationResult::success(None);
        }
        match std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
            .status()
        {
            Ok(status) if status.success() => OperationResult::success(None),
            Ok(status) => OperationResult::failure(format!("输入监控设置启动失败：{status}")),
            Err(e) => OperationResult::failure(format!("无法打开输入监控设置：{e}")),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        OperationResult::failure("此功能仅支持 macOS")
    }
}

#[tauri::command]
fn login(email: String, password: String) -> OperationResult<()> {
    if !email.contains('@') || password.is_empty() {
        return OperationResult::failure("邮箱或密码格式无效");
    };
    OperationResult::failure("生产认证接口的请求 Schema 尚未配置；没有向未知接口发送密码")
}
#[tauri::command]
fn logout() -> OperationResult<()> {
    OperationResult::success(None)
}

#[derive(serde::Serialize)]
struct UpdateInfo {
    current: String,
    latest: String,
    available: bool,
}
#[tauri::command]
fn check_app_update() -> OperationResult<UpdateInfo> {
    OperationResult::success(Some(UpdateInfo {
        current: env!("CARGO_PKG_VERSION").into(),
        latest: env!("CARGO_PKG_VERSION").into(),
        available: false,
    }))
}
#[tauri::command]
fn install_app_update(state: tauri::State<AppState>) -> OperationResult<()> {
    let busy = state
        .recording
        .lock()
        .map(|r| !matches!(r.phase, RecordingPhase::Idle))
        .unwrap_or(true);
    if busy {
        return OperationResult::failure("录音或设备同步期间不能安装更新");
    };
    OperationResult::failure("尚未配置正式更新签名公钥与发布端点")
}

#[tauri::command]
fn get_doubao_speech_config(state: tauri::State<AppState>) -> Result<DoubaoSpeechConfig, String> {
    Ok(state.storage.read_config()?.speech)
}

#[tauri::command]
fn save_doubao_speech_config(
    state: tauri::State<AppState>,
    mut config: DoubaoSpeechConfig,
    access_token: Option<String>,
) -> OperationResult<DoubaoSpeechConfig> {
    if let Err(e) = speech::validate(&config) {
        return OperationResult::failure(e);
    }
    let mut persisted = match state.storage.read_config() {
        Ok(v) => v,
        Err(e) => return OperationResult::failure(e),
    };
    let mut saved = persisted.speech.access_token_saved;
    if let Some(token) = access_token.filter(|v| !v.trim().is_empty()) {
        let token = token.trim().to_owned();
        if let Err(e) = storage::set_secret(speech::TOKEN_ACCOUNT, &token) {
            return OperationResult::failure(format!("无法写入 macOS 钥匙串：{e}"));
        }
        saved = true;
        if let Ok(mut cache) = state.speech_token.lock() {
            *cache = Some(token)
        }
    }
    if config.enabled && !saved {
        return OperationResult::failure("启用豆包语音前必须保存 Access Token");
    }
    config.access_token_saved = saved;
    persisted.revision += 1;
    persisted.speech = config.clone();
    match state.storage.write_config(&persisted) {
        Ok(_) => OperationResult::success(Some(config)),
        Err(e) => OperationResult::failure(e),
    }
}

async fn read_speech_token(state: &AppState) -> Result<String, String> {
    if let Ok(cache) = state.speech_token.lock() {
        if let Some(token) = cache.as_ref() {
            return Ok(token.clone());
        }
    }
    let token = match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::task::spawn_blocking(|| storage::get_secret(speech::TOKEN_ACCOUNT)),
    )
    .await
    {
        Ok(Ok(Ok(Some(v)))) => Ok(v),
        Ok(Ok(Ok(None))) => Err("请先在语音服务配置中填写并保存 Access Token".into()),
        Ok(Ok(Err(e))) => Err(format!("无法读取 macOS 钥匙串：{e}")),
        Ok(Err(e)) => Err(format!("读取 macOS 钥匙串任务失败：{e}")),
        Err(_) => Err("读取 macOS 钥匙串超时（30 秒），请处理系统授权提示".into()),
    }?;
    if let Ok(mut cache) = state.speech_token.lock() {
        *cache = Some(token.clone())
    }
    Ok(token)
}

pub(crate) async fn read_ark_api_key(state: &AppState) -> Result<String, String> {
    if let Ok(cache) = state.ark_api_key.lock() {
        if let Some(value) = cache.as_ref() {
            return Ok(value.clone());
        }
    }
    let value = match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::task::spawn_blocking(|| storage::get_secret(ark::API_KEY_ACCOUNT)),
    )
    .await
    {
        Ok(Ok(Ok(Some(value)))) => Ok(value),
        Ok(Ok(Ok(None))) => Err("请先在语音服务配置中填写并保存火山方舟 API Key".into()),
        Ok(Ok(Err(error))) => Err(format!("无法读取火山方舟 API Key：{error}")),
        Ok(Err(error)) => Err(format!("读取火山方舟 API Key 任务失败：{error}")),
        Err(_) => Err("读取火山方舟 API Key 超时（30 秒）".into()),
    }?;
    if let Ok(mut cache) = state.ark_api_key.lock() {
        *cache = Some(value.clone())
    }
    Ok(value)
}

#[tauri::command]
fn get_ark_model_config(state: tauri::State<AppState>) -> Result<ArkModelConfig, String> {
    Ok(state.storage.read_config()?.ark)
}

#[tauri::command]
fn save_ark_model_config(
    state: tauri::State<AppState>,
    mut config: ArkModelConfig,
    api_key: Option<String>,
) -> OperationResult<ArkModelConfig> {
    if let Err(error) = ark::validate(&config) {
        return OperationResult::failure(error);
    }
    let mut persisted = match state.storage.read_config() {
        Ok(value) => value,
        Err(error) => return OperationResult::failure(error),
    };
    let mut saved = persisted.ark.api_key_saved;
    if let Some(value) = api_key.filter(|value| !value.trim().is_empty()) {
        let value = value.trim().to_owned();
        if let Err(error) = storage::set_secret(ark::API_KEY_ACCOUNT, &value) {
            return OperationResult::failure(format!("无法写入 macOS 钥匙串：{error}"));
        }
        saved = true;
        if let Ok(mut cache) = state.ark_api_key.lock() {
            *cache = Some(value)
        }
    }
    if config.enabled && !saved {
        return OperationResult::failure("启用火山方舟模型前必须保存 API Key");
    }
    config.api_key_saved = saved;
    persisted.revision += 1;
    persisted.ark = config.clone();
    match state.storage.write_config(&persisted) {
        Ok(_) => OperationResult::success(Some(config)),
        Err(error) => OperationResult::failure(error),
    }
}

#[tauri::command]
async fn test_ark_connection(
    state: tauri::State<'_, AppState>,
    config: ArkModelConfig,
    api_key: Option<String>,
) -> Result<OperationResult<ark::ConnectionTest>, String> {
    let key = match api_key.filter(|value| !value.trim().is_empty()) {
        Some(value) => value,
        None => match read_ark_api_key(state.inner()).await {
            Ok(value) => value,
            Err(error) => return Ok(OperationResult::failure(error)),
        },
    };
    Ok(ark::test_connection(&config, &key).await)
}

async fn read_realtime_api_key(state: &AppState) -> Result<String, String> {
    if let Ok(cache) = state.realtime_api_key.lock() {
        if let Some(value) = cache.as_ref() {
            return Ok(value.clone());
        }
    }
    let value = match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::task::spawn_blocking(|| storage::get_secret(realtime::API_KEY_ACCOUNT)),
    )
    .await
    {
        Ok(Ok(Ok(Some(value)))) => Ok(value),
        Ok(Ok(Ok(None))) => Err("请先在语音服务配置中填写并保存实时语音 API Key".into()),
        Ok(Ok(Err(error))) => Err(format!("无法读取实时语音 API Key：{error}")),
        Ok(Err(error)) => Err(format!("读取实时语音 API Key 任务失败：{error}")),
        Err(_) => Err("读取实时语音 API Key 超时（30 秒）".into()),
    }?;
    if let Ok(mut cache) = state.realtime_api_key.lock() {
        *cache = Some(value.clone())
    }
    Ok(value)
}

pub(crate) async fn read_mcd_token(state: &AppState) -> Result<String, String> {
    if let Ok(cache) = state.mcd_token.lock() {
        if let Some(value) = cache.as_ref() {
            return Ok(value.clone());
        }
    }
    let value = match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::task::spawn_blocking(|| storage::get_secret(mcd_mcp::TOKEN_ACCOUNT)),
    )
    .await
    {
        Ok(Ok(Ok(Some(value)))) => Ok(value),
        Ok(Ok(Ok(None))) => Err("请先在“点餐”页面填写并保存麦当劳 MCP Token".into()),
        Ok(Ok(Err(error))) => Err(format!("无法读取麦当劳 MCP Token：{error}")),
        Ok(Err(error)) => Err(format!("读取麦当劳 MCP Token 任务失败：{error}")),
        Err(_) => Err("读取麦当劳 MCP Token 超时（30 秒）".into()),
    }?;
    if let Ok(mut cache) = state.mcd_token.lock() {
        *cache = Some(value.clone())
    }
    Ok(value)
}

#[tauri::command]
fn get_mcd_mcp_config(state: tauri::State<AppState>) -> Result<McdMcpConfig, String> {
    Ok(state.storage.read_config()?.mcd_mcp)
}

const CUBE_JEV_KEY_ACCOUNT: &str = "cube-lab.jev.api-key.v1";
const CUBE_DEEPSEEK_KEY_ACCOUNT: &str = "cube-lab.deepseek.api-key.v1";

#[tauri::command]
fn get_cube_model_keys() -> Result<std::collections::HashMap<String, Option<String>>, String> {
    let jev = storage::get_secret(CUBE_JEV_KEY_ACCOUNT)?.filter(|value| !value.is_empty());
    let deepseek = storage::get_secret(CUBE_DEEPSEEK_KEY_ACCOUNT)?.filter(|value| !value.is_empty());
    Ok([(String::from("jev"), jev), (String::from("deepseek"), deepseek)].into())
}

#[tauri::command]
fn save_cube_model_key(provider: String, key: String) -> OperationResult<()> {
    let account = match provider.as_str() {
        "jev" => CUBE_JEV_KEY_ACCOUNT,
        "deepseek" => CUBE_DEEPSEEK_KEY_ACCOUNT,
        _ => return OperationResult::failure("魔方模型类型无效"),
    };
    let key = key.trim();
    if key.len() > 4096 {
        return OperationResult::failure("魔方 API Key 长度超出限制");
    }
    match storage::set_secret(account, key) {
        Ok(()) => OperationResult::success(None),
        Err(error) => OperationResult::failure(format!("无法写入 macOS 钥匙串：{error}")),
    }
}

#[tauri::command]
fn save_mcd_mcp_config(
    state: tauri::State<AppState>,
    mut config: McdMcpConfig,
    token: Option<String>,
) -> OperationResult<McdMcpConfig> {
    if let Err(error) = mcd_mcp::validate(&config) {
        return OperationResult::failure(error);
    }
    let mut persisted = match state.storage.read_config() {
        Ok(value) => value,
        Err(error) => return OperationResult::failure(error),
    };
    let mut saved = persisted.mcd_mcp.token_saved;
    if let Some(value) = token.filter(|value| !value.trim().is_empty()) {
        let value = value.trim().to_owned();
        if let Err(error) = storage::set_secret(mcd_mcp::TOKEN_ACCOUNT, &value) {
            return OperationResult::failure(format!("无法写入 macOS 钥匙串：{error}"));
        }
        saved = true;
        if let Ok(mut cache) = state.mcd_token.lock() {
            *cache = Some(value)
        }
    }
    if config.enabled && !saved {
        return OperationResult::failure("启用麦当劳 MCP 前必须保存 Token");
    }
    config.token_saved = saved;
    persisted.revision += 1;
    persisted.mcd_mcp = config.clone();
    match state.storage.write_config(&persisted) {
        Ok(_) => OperationResult::success(Some(config)),
        Err(error) => OperationResult::failure(error),
    }
}

#[tauri::command]
async fn test_mcd_mcp_connection(
    state: tauri::State<'_, AppState>,
    config: McdMcpConfig,
    token: Option<String>,
) -> Result<OperationResult<mcd_mcp::ConnectionTest>, String> {
    let token = match token.filter(|value| !value.trim().is_empty()) {
        Some(value) => value,
        None => match read_mcd_token(state.inner()).await {
            Ok(value) => value,
            Err(error) => return Ok(OperationResult::failure(error)),
        },
    };
    Ok(mcd_mcp::test_connection(&config, &token).await)
}

#[tauri::command]
fn get_realtime_voice_config(state: tauri::State<AppState>) -> Result<RealtimeVoiceConfig, String> {
    Ok(state.storage.read_config()?.realtime_voice)
}

fn validate_voice_actions(config: &VoiceActionsConfig) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for (index, mapping) in config.mappings.iter().enumerate() {
        let position = index + 1;
        if uuid::Uuid::parse_str(&mapping.id).is_err() {
            return Err(format!("第 {position} 项的标识无效，请删除后重新添加"));
        }
        if !ids.insert(mapping.id.to_ascii_lowercase()) {
            return Err(format!("第 {position} 项与其他语音调用重复"));
        }
        if mapping.name.trim().is_empty() || mapping.name.chars().count() > 80 {
            return Err(format!(
                "第 {position} 项的调用名称不能为空且不能超过 80 个字符"
            ));
        }
        if mapping.description.chars().count() > 400 {
            return Err(format!("第 {position} 项的语音说明不能超过 400 个字符"));
        }
        match mapping.action.kind {
            KeyboardActionKind::OpenApp => {
                let path = mapping
                    .action
                    .value
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| format!("第 {position} 项尚未选择应用"))?;
                if !path.to_ascii_lowercase().ends_with(".app")
                    || !std::path::Path::new(path).is_dir()
                {
                    return Err(format!("第 {position} 项选择的应用不存在或不是 macOS 应用"));
                }
            }
            KeyboardActionKind::Hotkey => {
                let shortcut = mapping
                    .action
                    .value
                    .as_deref()
                    .ok_or_else(|| format!("第 {position} 项尚未填写快捷键"))?;
                if let Err(error) = input::validate_shortcut(shortcut) {
                    return Err(format!("第 {position} 项的快捷键无效：{error}"));
                }
            }
            KeyboardActionKind::FixedText => {
                if mapping
                    .action
                    .value
                    .as_deref()
                    .is_none_or(|value| value.is_empty() || value.chars().count() > 2000)
                {
                    return Err(format!(
                        "第 {position} 项的固定文字不能为空且不能超过 2000 个字符"
                    ));
                }
            }
            KeyboardActionKind::Copy
            | KeyboardActionKind::Paste
            | KeyboardActionKind::Cut
            | KeyboardActionKind::SelectAll
            | KeyboardActionKind::Undo
            | KeyboardActionKind::Enter
            | KeyboardActionKind::Backspace
            | KeyboardActionKind::EditPtt => {}
            _ => return Err(format!("第 {position} 项选择了不支持的语音动作")),
        }
    }
    Ok(())
}

#[tauri::command]
fn get_voice_actions_config(state: tauri::State<AppState>) -> Result<VoiceActionsConfig, String> {
    Ok(state.storage.read_config()?.voice_actions)
}

#[tauri::command]
fn save_voice_actions_config(
    state: tauri::State<AppState>,
    mut config: VoiceActionsConfig,
) -> OperationResult<VoiceActionsConfig> {
    if let Err(error) = validate_voice_actions(&config) {
        return OperationResult::failure(error);
    }
    let mut persisted = match state.storage.read_config() {
        Ok(value) => value,
        Err(error) => return OperationResult::failure(error),
    };
    config.revision = persisted.voice_actions.revision + 1;
    persisted.revision += 1;
    persisted.voice_actions = config.clone();
    match state.storage.write_config(&persisted) {
        Ok(_) => OperationResult::success(Some(config)),
        Err(error) => OperationResult::failure(error),
    }
}

#[tauri::command]
fn save_realtime_voice_config(
    state: tauri::State<AppState>,
    mut config: RealtimeVoiceConfig,
    api_key: Option<String>,
) -> OperationResult<RealtimeVoiceConfig> {
    if let Err(error) = realtime::validate(&config) {
        return OperationResult::failure(error);
    }
    let mut persisted = match state.storage.read_config() {
        Ok(value) => value,
        Err(error) => return OperationResult::failure(error),
    };
    let mut saved = persisted.realtime_voice.api_key_saved;
    if let Some(value) = api_key.filter(|value| !value.trim().is_empty()) {
        let value = value.trim().to_owned();
        if let Err(error) = storage::set_secret(realtime::API_KEY_ACCOUNT, &value) {
            return OperationResult::failure(format!("无法写入 macOS 钥匙串：{error}"));
        }
        saved = true;
        if let Ok(mut cache) = state.realtime_api_key.lock() {
            *cache = Some(value)
        }
    }
    if config.enabled && !saved {
        return OperationResult::failure("启用实时语音前必须保存 API Key");
    }
    config.api_key_saved = saved;
    persisted.revision += 1;
    persisted.realtime_voice = config.clone();
    match state.storage.write_config(&persisted) {
        Ok(_) => OperationResult::success(Some(config)),
        Err(error) => OperationResult::failure(error),
    }
}

#[tauri::command]
async fn test_realtime_voice_connection(
    state: tauri::State<'_, AppState>,
    config: RealtimeVoiceConfig,
    api_key: Option<String>,
) -> Result<OperationResult<realtime::ConnectionTest>, String> {
    let key = match api_key.filter(|value| !value.trim().is_empty()) {
        Some(value) => value,
        None => match read_realtime_api_key(state.inner()).await {
            Ok(value) => value,
            Err(error) => return Ok(OperationResult::failure(error)),
        },
    };
    Ok(realtime::test_connection(&config, &key).await)
}

#[tauri::command]
fn get_realtime_call_state(state: tauri::State<AppState>) -> Result<RealtimeCallState, String> {
    state
        .realtime_call
        .lock()
        .map(|value| value.clone())
        .map_err(|_| "实时通话状态锁已损坏".into())
}

#[tauri::command]
fn get_realtime_diagnostics_info() -> Result<diagnostics::RealtimeDiagnosticsInfo, String> {
    diagnostics::info()
}

#[tauri::command]
fn export_realtime_diagnostics(
    path: String,
) -> OperationResult<diagnostics::RealtimeDiagnosticsInfo> {
    match diagnostics::export(std::path::Path::new(&path)) {
        Ok(value) => OperationResult::success(Some(value)),
        Err(error) => OperationResult::failure(error),
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RealtimeSessionStart {
    session_id: String,
}

#[tauri::command]
async fn start_realtime_call(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    learning: Option<realtime::LearningStart>,
    wake_word_mode: Option<bool>,
) -> Result<OperationResult<RealtimeSessionStart>, String> {
    let _start_guard = state.realtime_start_gate.lock().await;
    if state.live.lock().map_err(|_| "直播状态锁损坏")?.is_some() { return Ok(OperationResult::failure("请先停止直播字幕识别")); }
    if let Some(options) = &learning { options.validate()?; }
    let persisted = state.storage.read_config()?;
    if !persisted.realtime_voice.enabled {
        return Ok(OperationResult::failure(
            "请先在语音服务配置中启用豆包实时语音",
        ));
    }
    if !persisted.realtime_voice.api_key_saved {
        return Ok(OperationResult::failure("请先保存豆包实时语音 API Key"));
    }
    if persisted.keyboard.wifi.audio_port == 0 {
        return Ok(OperationResult::failure("开发板音频端口不能为 0"));
    }
    if state
        .realtime_session
        .lock()
        .map_err(|_| "实时通话会话锁已损坏")?
        .is_some()
    {
        return Ok(OperationResult::failure("实时通话已经在运行"));
    }
    let session_id = uuid::Uuid::new_v4().to_string();
    let wake_word_enabled = wake_word_mode.unwrap_or(true) && learning.is_none();
    if let Ok(mut call) = state.realtime_call.lock() {
        *call = RealtimeCallState {
            phase: RealtimeCallPhase::Connecting,
            learning_owner: learning.as_ref().map(|value| value.owner_id.clone()),
            wake_word_enabled,
            wake_word_armed: false,
            session_id: Some(session_id.clone()),
            user_text: String::new(),
            assistant_text: String::new(),
            elapsed_ms: 0,
            input_packets: 0,
            output_packets: 0,
            tool_call_count: 0,
            last_tool_status: None,
            error: None,
            log_id: None,
        };
        let _ = app.emit("realtime-call-state", call.clone());
    }
    let key = match read_realtime_api_key(state.inner()).await {
        Ok(value) => value,
        Err(error) => {
            if let Ok(mut call) = state.realtime_call.lock() {
                call.phase = RealtimeCallPhase::Error;
                call.error = Some(error.clone());
                let _ = app.emit("realtime-call-state", call.clone());
            }
            return Ok(OperationResult::failure(error));
        }
    };
    let is_learning = learning.is_some();
    let mut config = persisted.realtime_voice;
    if let Some(options) = learning {
        config.instructions = options.instructions;
        config.greeting = options.greeting;
        // Classroom English needs deliberate articulation on the small board speaker.
        // Keep the user's configured voice, which is guaranteed to be enabled for
        // this realtime endpoint, but slow learning playback to roughly 0.7x.
        config.speed = -30;
    }
    let mcd_enabled = !is_learning && persisted.mcd_mcp.enabled && persisted.mcd_mcp.token_saved;
    let (sender, ready) = match realtime::open_session(
        app.clone(),
        session_id.clone(),
        config,
        key,
        persisted.keyboard.wifi.audio_port,
        if is_learning { vec![] } else { persisted.voice_actions.mappings },
        mcd_enabled,
        is_learning,
        wake_word_enabled,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            diagnostics::realtime(
                Some(&session_id),
                "session.start_failed",
                serde_json::json!({"message":&error}),
            );
            if let Ok(mut call) = state.realtime_call.lock() {
                call.phase = RealtimeCallPhase::Error;
                call.session_id = None;
                call.error = Some(error.clone());
                let _ = app.emit("realtime-call-state", call.clone());
            }
            return Ok(OperationResult::failure(error));
        }
    };
    *state
        .realtime_session
        .lock()
        .map_err(|_| "实时通话会话锁已损坏")? = Some(sender);
    let _ = ready.send(());
    Ok(OperationResult::success(Some(RealtimeSessionStart {
        session_id,
    })))
}

#[tauri::command]
fn stop_realtime_call(state: tauri::State<AppState>) -> OperationResult<()> {
    match state
        .realtime_session
        .lock()
        .ok()
        .and_then(|value| value.as_ref().cloned())
    {
        Some(sender) => match sender.send(realtime::RealtimeCommand::Stop) {
            Ok(_) => OperationResult::success(None),
            Err(_) => OperationResult::failure("实时通话会话已经结束"),
        },
        None => OperationResult::failure("当前没有正在进行的实时通话"),
    }
}

#[tauri::command]
fn interrupt_realtime_call(state: tauri::State<AppState>) -> OperationResult<()> {
    match state
        .realtime_session
        .lock()
        .ok()
        .and_then(|value| value.as_ref().cloned())
    {
        Some(sender) => match sender.send(realtime::RealtimeCommand::Interrupt) {
            Ok(_) => OperationResult::success(None),
            Err(_) => OperationResult::failure("实时通话会话已经结束"),
        },
        None => OperationResult::failure("当前没有正在进行的实时通话"),
    }
}

#[tauri::command]
async fn set_realtime_wake_word_mode(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<OperationResult<()>, String> {
    let sender = state
        .realtime_session
        .lock()
        .ok()
        .and_then(|value| value.as_ref().cloned());
    let Some(sender) = sender else {
        return Ok(OperationResult::failure("当前没有正在进行的实时通话"));
    };
    let (ack, result) = tokio::sync::oneshot::channel();
    if sender.send(realtime::RealtimeCommand::WakeWordMode(enabled, ack)).is_err() {
        return Ok(OperationResult::failure("实时通话会话已经结束"));
    }
    Ok(match tokio::time::timeout(std::time::Duration::from_secs(3), result).await {
        Ok(Ok(Ok(()))) => OperationResult::success(None),
        Ok(Ok(Err(error))) => OperationResult::failure(error),
        Ok(Err(_)) => OperationResult::failure("唤醒词模式更新未能确认"),
        Err(_) => OperationResult::failure("唤醒词模式更新超时"),
    })
}

#[tauri::command]
async fn learning_realtime_command(
    state: tauri::State<'_, AppState>,
    owner_id: String,
    session_id: String,
    action: realtime::LearningAction,
) -> Result<(), String> {
    action.validate()?;
    let sender = {
        let call = state.realtime_call.lock().map_err(|_| "会话状态锁已损坏")?;
        if call.learning_owner.as_deref() != Some(owner_id.as_str()) || call.session_id.as_deref() != Some(session_id.as_str()) {
            return Err("该学习页面不拥有当前实时会话".into());
        }
        state.realtime_session.lock().map_err(|_| "会话锁已损坏")?.as_ref().cloned().ok_or("学习会话已结束")?
    };
    let (ack, result) = tokio::sync::oneshot::channel();
    sender.send(realtime::RealtimeCommand::Learning(action, ack)).map_err(|_| "学习会话已结束")?;
    tokio::time::timeout(std::time::Duration::from_secs(8), result).await
        .map_err(|_| "学习请求确认超时，请结束后重新连接")?
        .map_err(|_| "学习会话已断开")?
}

#[tauri::command]
fn launch_payment_mvp(pay_h5_url: String) -> OperationResult<payment_mvp::PaymentMvpLaunch> {
    match payment_mvp::launch(&pay_h5_url) {
        Ok(value) => OperationResult::success(Some(value)),
        Err(error) => OperationResult::failure(error),
    }
}

#[tauri::command]
async fn test_doubao_connection(
    state: tauri::State<'_, AppState>,
    config: DoubaoSpeechConfig,
    access_token: Option<String>,
) -> Result<OperationResult<speech::ConnectionTest>, String> {
    let token = match access_token.filter(|v| !v.trim().is_empty()) {
        Some(v) => v,
        None => match read_speech_token(state.inner()).await {
            Ok(v) => v,
            Err(e) => return Ok(OperationResult::failure(e)),
        },
    };
    Ok(speech::test_connection(&config, &token).await)
}

pub fn run() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
            diagnostics::init(&root)?;
            let storage = Storage::open(root).map_err(|e| e.to_string())?;
            let state = AppState::new(storage);
            let device_events = state.device.event_hub();
            app.manage(state);
            device::start_voice_button_listener(app.handle().clone(), device_events);
            let icon = app.default_window_icon().cloned().ok_or("应用图标不可用")?;
            TrayIconBuilder::new()
                .icon(icon)
                .tooltip("EasyInput")
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_snapshot,
            update_app_settings,
            list_installed_applications,
            list_available_wifi_networks,
            live::start_live_speech,
            live::live_speech_command,
            live::live_rtc_token,
            start_recording,
            push_recording_audio,
            stop_recording,
            get_history_page,
            get_activity_calendar,
            delete_history,
            get_dictionary,
            save_dictionary,
            import_dictionary_file,
            export_dictionary_file,
            login,
            logout,
            get_ai_keyboard_connection_state,
            read_ai_keyboard_status,
            push_ai_keyboard_config,
            sync_ai_keyboard_boot_sound,
            open_bluetooth_settings,
            open_input_monitoring_settings,
            check_app_update,
            install_app_update,
            get_doubao_speech_config,
            save_doubao_speech_config,
            test_doubao_connection,
            get_ark_model_config,
            save_ark_model_config,
            test_ark_connection,
            get_realtime_voice_config,
            save_realtime_voice_config,
            test_realtime_voice_connection,
            get_mcd_mcp_config,
            get_cube_model_keys,
            save_cube_model_key,
            save_mcd_mcp_config,
            test_mcd_mcp_connection,
            get_voice_actions_config,
            save_voice_actions_config,
            get_realtime_call_state,
            get_realtime_diagnostics_info,
            export_realtime_diagnostics,
            start_realtime_call,
            learning_realtime_command,
            stop_realtime_call,
            interrupt_realtime_call,
            set_realtime_wake_word_mode,
            launch_payment_mvp
        ])
        .run(tauri::generate_context!())
        .expect("EasyInput 启动失败")
}

#[cfg(test)]
mod voice_action_config_tests {
    use super::*;
    #[test]
    fn accepts_an_unbounded_list_of_valid_mappings() {
        let mappings = (0..128)
            .map(|index| VoiceActionMapping {
                id: uuid::Uuid::new_v4().to_string(),
                enabled: true,
                name: format!("动作 {index}"),
                description: String::new(),
                action: KeyboardAction {
                    kind: KeyboardActionKind::Copy,
                    label: "复制".into(),
                    value: None,
                    host_action_id: None,
                },
            })
            .collect();
        assert!(validate_voice_actions(&VoiceActionsConfig {
            revision: 1,
            mappings
        })
        .is_ok());
    }
    #[test]
    fn rejects_invalid_shortcut_before_saving() {
        let mapping = VoiceActionMapping {
            id: uuid::Uuid::new_v4().to_string(),
            enabled: true,
            name: "错误快捷键".into(),
            description: String::new(),
            action: KeyboardAction {
                kind: KeyboardActionKind::Hotkey,
                label: "快捷键".into(),
                value: Some("Command+Banana".into()),
                host_action_id: None,
            },
        };
        assert!(validate_voice_actions(&VoiceActionsConfig {
            revision: 1,
            mappings: vec![mapping]
        })
        .unwrap_err()
        .contains("不支持快捷键按键"));
    }
}

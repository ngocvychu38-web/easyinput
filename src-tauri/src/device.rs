use crate::model::{DeviceCapabilities, DeviceConnectionState};
use crate::protocol::usb::{AppCommand, ConfigAck, PID, VID};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

#[cfg(target_os = "macos")]
use tauri::Emitter;

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[cfg(target_os = "macos")]
pub fn request_input_monitoring_access() -> bool {
    unsafe { CGPreflightListenEventAccess() || CGRequestListenEventAccess() }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareVoiceButtonEvent {
    pub pressed: bool,
    pub source: &'static str,
    pub sequence: u64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareEditButtonEvent { pub pressed: bool, pub sequence: u64, pub has_selection: bool }

pub(crate) struct DeviceEventHub {
    config_ack: Mutex<Option<ConfigAck>>,
    config_ack_changed: Condvar,
}

impl DeviceEventHub {
    fn new() -> Self {
        Self { config_ack: Mutex::new(None), config_ack_changed: Condvar::new() }
    }

    fn clear_config_ack(&self) {
        if let Ok(mut value) = self.config_ack.lock() { *value = None; }
    }

    fn publish_config_ack(&self, ack: ConfigAck) {
        if let Ok(mut value) = self.config_ack.lock() {
            *value = Some(ack);
            self.config_ack_changed.notify_all();
        }
    }

    fn wait_for_config_ack(&self, bytes: u16, crc: u16) -> Result<(), String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        let mut guard = self.config_ack.lock().map_err(|_| "设备回执锁已损坏")?;
        loop {
            if let Some(ack) = *guard {
                if ack.bytes == bytes && ack.crc == crc {
                    return if ack.ok && ack.saved {
                        Ok(())
                    } else {
                        Err(format!("固件拒绝保存配置（阶段 {}，校验已匹配）", ack.phase))
                    };
                }
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err("设备未返回配置保存回执；当前固件可能不是 Host Action v1 版本".into());
            }
            let timeout = deadline.saturating_duration_since(now);
            let (next, result) = self.config_ack_changed.wait_timeout(guard, timeout)
                .map_err(|_| "设备回执锁已损坏")?;
            guard = next;
            if result.timed_out() && guard.is_none() {
                return Err("设备未返回配置保存回执；当前固件可能不是 Host Action v1 版本".into());
            }
        }
    }
}

pub fn start_voice_button_listener(app: tauri::AppHandle, hub: Arc<DeviceEventHub>) {
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || {
        let trace_reports = std::env::var_os("EASYINPUT_HID_TRACE").is_some();
        let mut last_voice_state = None;
        let mut voice_sequence = 0_u64;
        let mut last_edit_state = None;
        let mut edit_sequence = 0_u64;
        let mut last_app_report: Option<(Vec<u8>, std::time::Instant)> = None;
        let mut fixed_text: Option<(u8, u8, Vec<u8>)> = None;
        loop {
            let api = match hidapi::HidApi::new() {
                Ok(value) => value,
                Err(_) => { std::thread::sleep(std::time::Duration::from_secs(2)); continue; }
            };
            let paths: Vec<_> = api.device_list()
                .filter(|device| device.vendor_id() == VID && device.product_id() == PID)
                .map(|device| device.path().to_owned())
                .collect();
            let devices: Vec<_> = paths.iter().filter_map(|path| api.open_path(path).ok()).collect();
            if devices.is_empty() {
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }
            for _ in 0..500 {
                for device in &devices {
                    let mut raw = [0_u8; 64];
                    let Ok(size) = device.read_timeout(&mut raw, 0) else { continue };
                    if size == 0 { continue; }
                    let report = &raw[..size];
                    if trace_reports {
                        eprintln!("EASYINPUT_HID_REPORT size={size} data={:02x?}", report);
                    }
                    if let Some(command) = crate::protocol::usb::app_command(report) {
                        let duplicate = last_app_report.as_ref().is_some_and(|(previous, at)| {
                            previous.as_slice() == report && at.elapsed() < std::time::Duration::from_millis(120)
                        });
                        if duplicate { continue; }
                        last_app_report = Some((report.to_vec(), std::time::Instant::now()));
                        match command {
                            AppCommand::ConfigAck(ack) => hub.publish_config_ack(ack),
                            AppCommand::HostAction(id) => {
                                if let Err(error) = crate::execute_host_action(&app, &id) {
                                    eprintln!("EasyInput host action failed: {error}");
                                }
                            }
                            AppCommand::FixedText { index, total, data } => {
                                if index == 0 { fixed_text = Some((total, 0, Vec::new())); }
                                let Some((expected_total, next, buffer)) = fixed_text.as_mut() else { continue };
                                if *expected_total != total || *next != index { fixed_text = None; continue; }
                                buffer.extend(data);
                                *next += 1;
                                if *next == *expected_total {
                                    let complete = fixed_text.take().map(|(_, _, value)| value).unwrap_or_default();
                                    if let Ok(text) = String::from_utf8(complete) {
                                        let _ = crate::input::type_text(&text);
                                    }
                                }
                            }
                            AppCommand::Hotkey { pressed, hotkey } if hotkey.eq_ignore_ascii_case("EasyInputVoice") => {
                                if last_voice_state != Some(pressed) {
                                    last_voice_state = Some(pressed);
                                    voice_sequence = voice_sequence.wrapping_add(1);
                                    let _ = app.emit("hardware-voice-button", HardwareVoiceButtonEvent { pressed, source: "app-report", sequence: voice_sequence });
                                }
                            }
                            AppCommand::Hotkey { pressed, hotkey } if hotkey.eq_ignore_ascii_case("EasyInputEdit") => {
                                if last_edit_state != Some(pressed) {
                                    last_edit_state = Some(pressed);
                                    let has_selection = if pressed {
                                        let selection = crate::input::selected_text().unwrap_or(None);
                                        let present = selection.as_ref().is_some_and(|value| !value.is_empty());
                                        crate::set_edit_context(&app, selection);
                                        present
                                    } else { false };
                                    edit_sequence = edit_sequence.wrapping_add(1);
                                    let _ = app.emit("hardware-edit-button", HardwareEditButtonEvent { pressed, sequence: edit_sequence, has_selection });
                                }
                            }
                            AppCommand::Hotkey { .. } | AppCommand::Other(_) => {}
                        }
                        continue;
                    }
                    if let Some(pressed) = crate::protocol::usb::voice_button_state(report) {
                        if last_voice_state.is_none() && !pressed {
                            last_voice_state = Some(false);
                        } else if last_voice_state != Some(pressed) {
                            last_voice_state = Some(pressed);
                            voice_sequence = voice_sequence.wrapping_add(1);
                            let _ = app.emit("hardware-voice-button", HardwareVoiceButtonEvent {
                                pressed,
                                source: "keyboard-report",
                                sequence: voice_sequence,
                            });
                        }
                    }
                    if let Some(pressed) = crate::protocol::usb::edit_button_state(report) {
                        if last_edit_state.is_none() && !pressed {
                            last_edit_state = Some(false);
                        } else if last_edit_state != Some(pressed) {
                            last_edit_state = Some(pressed);
                            let has_selection = if pressed {
                                let selection = crate::input::selected_text().unwrap_or(None);
                                let present = selection.as_ref().is_some_and(|value| !value.is_empty());
                                crate::set_edit_context(&app, selection);
                                present
                            } else { false };
                            edit_sequence = edit_sequence.wrapping_add(1);
                            let _ = app.emit("hardware-edit-button", HardwareEditButtonEvent { pressed, sequence: edit_sequence, has_selection });
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
        }
    });
    #[cfg(not(target_os = "macos"))]
    let _ = (app, hub);
}

#[derive(Debug, Clone)]
pub struct DeviceEndpoint {
    pub transport: &'static str,
    pub epoch: u64,
    pub product: String,
}

pub trait DeviceAdapter: Send + Sync {
    fn discover(&self) -> Result<Option<DeviceEndpoint>, String>;
    fn capabilities(&self) -> DeviceCapabilities;
    fn write_config(&self, _payload: &[u8], _epoch: u64) -> Result<(), String>;
}

pub struct UsbAdapter {
    epoch: AtomicU64,
    hub: Arc<DeviceEventHub>,
}

impl UsbAdapter {
    fn new(hub: Arc<DeviceEventHub>) -> Self { Self { epoch: AtomicU64::new(1), hub } }
}

impl DeviceAdapter for UsbAdapter {
    fn discover(&self) -> Result<Option<DeviceEndpoint>, String> {
        #[cfg(target_os = "macos")]
        {
            let api = hidapi::HidApi::new().map_err(|error| error.to_string())?;
            for device in api.device_list() {
                if device.vendor_id() == VID && device.product_id() == PID {
                    return Ok(Some(DeviceEndpoint {
                        transport: "USB",
                        epoch: self.epoch.load(Ordering::SeqCst),
                        product: device.product_string().unwrap_or("EasyInput AI").to_string(),
                    }));
                }
            }
        }
        Ok(None)
    }

    fn capabilities(&self) -> DeviceCapabilities {
        DeviceCapabilities { config: true, microphone: true, speaker_sync: true, agent_light: true, firmware_version: None }
    }

    fn write_config(&self, payload: &[u8], epoch: u64) -> Result<(), String> {
        if epoch != self.epoch.load(Ordering::SeqCst) { return Err("设备连接代次已过期".into()); }
        let chunks = crate::protocol::usb::split_config(payload).map_err(|error| error.to_string())?;
        let expected_bytes = payload.len() as u16;
        let expected_crc = crate::protocol::usb::crc16_ccitt(payload);
        #[cfg(target_os = "macos")]
        {
            let reports = chunks.into_iter()
                .map(|chunk| chunk.encode().map_err(|error| error.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            let api = hidapi::HidApi::new().map_err(|error| error.to_string())?;
            let mut candidates: Vec<_> = api.device_list()
                .filter(|device| device.vendor_id() == VID && device.product_id() == PID)
                .map(|device| {
                    let vendor_collection = device.usage_page() == 0xff00 && device.usage() == 0x0002;
                    let usb = matches!(device.bus_type(), hidapi::BusType::Usb);
                    ((!vendor_collection, !usb), device.path().to_owned())
                })
                .collect();
            candidates.sort_by_key(|(priority, _)| *priority);
            let mut seen = std::collections::HashSet::new();
            let mut failures = Vec::new();
            let mut permission_denied = false;
            for (_, path) in candidates {
                if !seen.insert(path.as_bytes().to_vec()) { continue; }
                let device = match api.open_path(&path) {
                    Ok(value) => value,
                    Err(error) => {
                        let detail = error.to_string();
                        permission_denied |= detail.contains("not permitted") || detail.contains("0xE00002E2");
                        failures.push(detail);
                        continue;
                    }
                };
                self.hub.clear_config_ack();
                let mut write_error = None;
                for report in &reports {
                    if let Err(error) = device.send_feature_report(report) {
                        write_error = Some(error.to_string());
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                if let Some(detail) = write_error {
                    permission_denied |= detail.contains("not permitted") || detail.contains("0xE00002E2");
                    failures.push(detail);
                } else {
                    return self.hub.wait_for_config_ack(expected_bytes, expected_crc);
                }
            }
            if permission_denied {
                let granted = request_input_monitoring_access();
                return Err(if granted {
                    "输入监控权限刚刚获准。请完全退出并重新打开 EasyInput，然后再次同步。"
                } else {
                    "macOS 已阻止访问键盘 HID。请在“系统设置 → 隐私与安全性 → 输入监控”中允许 EasyInput，完全退出并重新打开应用后再同步。"
                }.into());
            }
            return Err(format!("无法写入 EasyInput HID 配置：{}", failures.last().cloned().unwrap_or_else(|| "未找到可用的厂商自定义接口".into())));
        }
        #[allow(unreachable_code)]
        Err("当前平台不支持 USB HID".into())
    }
}

pub struct DeviceManager {
    usb: UsbAdapter,
    hub: Arc<DeviceEventHub>,
    pub endpoint_epoch: AtomicU64,
}

impl DeviceManager {
    pub fn new() -> Self {
        let hub = Arc::new(DeviceEventHub::new());
        Self { usb: UsbAdapter::new(hub.clone()), hub, endpoint_epoch: AtomicU64::new(1) }
    }

    pub(crate) fn event_hub(&self) -> Arc<DeviceEventHub> { self.hub.clone() }

    pub fn discover(&self) -> Result<(DeviceConnectionState, DeviceCapabilities), String> {
        if self.usb.discover()?.is_some() {
            return Ok((DeviceConnectionState::ConnectedUsb, self.usb.capabilities()));
        }
        Ok((DeviceConnectionState::Disconnected, DeviceCapabilities {
            config: false, microphone: false, speaker_sync: false, agent_light: false, firmware_version: None,
        }))
    }

    pub fn push_config(&self, payload: &[u8]) -> Result<(), String> {
        self.usb.write_config(payload, self.usb.epoch.load(Ordering::SeqCst))
    }

    pub fn discover_timeout_3s(&self) -> (DeviceConnectionState, DeviceCapabilities) {
        let epoch = self.endpoint_epoch.load(Ordering::SeqCst);
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let hub = Arc::new(DeviceEventHub::new());
            let manager = DeviceManager {
                usb: UsbAdapter { epoch: AtomicU64::new(epoch), hub: hub.clone() },
                hub,
                endpoint_epoch: AtomicU64::new(epoch),
            };
            let _ = sender.send(manager.discover());
        });
        match receiver.recv_timeout(std::time::Duration::from_secs(3)) {
            Ok(Ok(pair)) => pair,
            Ok(Err(_)) | Err(_) => (DeviceConnectionState::Error, DeviceCapabilities {
                config: false, microphone: false, speaker_sync: false, agent_light: false, firmware_version: None,
            }),
        }
    }
}

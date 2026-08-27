use crate::model::{KeyboardAction, KeyboardActionKind, KeyboardConfig};
use serde_json::{json, Map, Value};

fn canonical_host_action_id(action: &mut KeyboardAction) -> Result<String, String> {
    let id = match action.host_action_id.as_deref() {
        Some(value) => uuid::Uuid::parse_str(value)
            .map_err(|_| "打开应用动作的设备标识无效，请重新选择应用".to_string())?,
        None => {
            let value = uuid::Uuid::new_v4();
            action.host_action_id = Some(value.to_string());
            value
        }
    };
    Ok(id.hyphenated().to_string().to_lowercase())
}

fn hotkey(value: impl Into<String>) -> Value {
    json!({ "hotkey": value.into() })
}

fn action_value(action: &mut KeyboardAction, platform: &str) -> Result<Value, String> {
    let command_modifier = if platform.eq_ignore_ascii_case("windows") { "Ctrl" } else { "Meta" };
    match action.kind {
        KeyboardActionKind::VoicePtt => Ok(json!("voice_ptt_hold")),
        KeyboardActionKind::EditPtt => Ok(json!("edit_ptt_hold")),
        KeyboardActionKind::Enter => Ok(hotkey("Return")),
        KeyboardActionKind::Backspace => Ok(hotkey("Backspace")),
        KeyboardActionKind::Cut => Ok(hotkey(format!("{command_modifier}+X"))),
        KeyboardActionKind::SelectAll => Ok(json!("select_all")),
        KeyboardActionKind::Copy => Ok(json!("copy")),
        KeyboardActionKind::Paste => Ok(json!("paste")),
        KeyboardActionKind::Undo => {
            // Old clients represented the Backspace menu entry as Undo. Keep
            // already-saved genuine Undo actions distinct by their label.
            if action.label == "退格" { Ok(hotkey("Backspace")) } else { Ok(json!("undo")) }
        }
        KeyboardActionKind::Hotkey => {
            let value = action.value.as_deref().filter(|value| !value.trim().is_empty())
                .unwrap_or(action.label.as_str());
            let normalized = if value.eq_ignore_ascii_case("ctrl + x") || value.eq_ignore_ascii_case("ctrl+x") {
                format!("{command_modifier}+X")
            } else {
                value.replace(" + ", "+")
            };
            Ok(hotkey(normalized))
        }
        KeyboardActionKind::FixedText => {
            let text = action.value.as_deref().filter(|value| !value.is_empty())
                .ok_or_else(|| "固定文字动作尚未填写内容".to_string())?;
            if text.as_bytes().len() > 960 { return Err("固定文字超过固件 960 字节限制".into()); }
            Ok(json!({ "text": text }))
        }
        KeyboardActionKind::OpenApp => {
            Ok(json!(format!("host_action:{}", canonical_host_action_id(action)?)))
        }
        KeyboardActionKind::ScrollAxisToggle => Ok(json!("scroll_axis_toggle")),
        KeyboardActionKind::CaretSelect => Ok(json!("text_caret_select")),
        KeyboardActionKind::Disabled => Ok(json!("disabled")),
        KeyboardActionKind::HostAction => {
            if action.label == "回车" {
                Ok(hotkey("Return"))
            } else if let Some(id) = action.host_action_id.as_deref() {
                let id = uuid::Uuid::parse_str(id).map_err(|_| "主机动作标识无效".to_string())?;
                Ok(json!(format!("host_action:{}", id.hyphenated().to_string().to_lowercase())))
            } else {
                Ok(json!("history"))
            }
        }
    }
}

/// Converts the local/UI model to the firmware-owned ai_keyboard.v1 schema.
/// App paths remain local; only opaque UUIDs are written to the keyboard.
pub fn prepare(mut config: KeyboardConfig) -> Result<(KeyboardConfig, Vec<u8>), String> {
    if config.keys.len() != 8 {
        return Err(format!("按键配置必须包含 8 个按键，当前为 {} 个", config.keys.len()));
    }
    let platform = if config.target_platform.eq_ignore_ascii_case("windows") { "windows" } else { "macos" };
    let mut keys = Map::new();
    for (index, action) in config.keys.iter_mut().enumerate() {
        keys.insert(format!("KEY{}", index + 1), json!({ "press": action_value(action, platform)? }));
    }
    let encoder_press = action_value(&mut config.encoder.press, platform)?;
    let axis = if config.encoder.axis.eq_ignore_ascii_case("horizontal") { "horizontal" } else { "vertical" };
    let mode = if matches!(config.encoder.press.kind, KeyboardActionKind::CaretSelect) { "cursor" } else { "scroll" };
    let mut payload = json!({
        "schema": "ai_keyboard.v1",
        "target_platform": platform,
        "ptt_hotkey": "EasyInputVoice",
        "ptt_hotkey_source": "custom",
        "edit_ptt_hotkey": "EasyInputEdit",
        "edit_ptt_hotkey_source": "custom",
        "hotkey_mode": if config.ptt_mode.eq_ignore_ascii_case("toggle") { "toggle" } else { "hold" },
        "profiles": [{
            "id": "default",
            "keys": Value::Object(keys),
            "encoder": {
                "left": "disabled",
                "right": "disabled",
                "press": encoder_press,
                "scroll": {
                    "enabled": true,
                    "mode": mode,
                    "axis": axis,
                    "speed": config.encoder.speed.clamp(1, 5),
                    "reverse_vertical": config.encoder.reverse,
                    "reverse_horizontal": config.encoder.reverse
                }
            }
        }]
    });
    if !config.wifi.ssid.trim().is_empty() {
        payload["wifi_ssid"] = json!(config.wifi.ssid);
    }
    if !config.wifi.audio_host.trim().is_empty() {
        payload["audio_host"] = json!(config.wifi.audio_host);
    }
    payload["audio_port"] = json!(config.wifi.audio_port);
    let bytes = serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
    if bytes.len() > crate::protocol::usb::MAX_CONFIG {
        return Err(format!("键盘配置为 {} 字节，超过固件 2048 字节限制", bytes.len()));
    }
    Ok((config, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_firmware_schema_and_keeps_app_path_local() {
        let mut config = KeyboardConfig::default();
        config.keys[6] = KeyboardAction {
            kind: KeyboardActionKind::OpenApp,
            label: "Safari".into(),
            value: Some("/Applications/Safari.app".into()),
            host_action_id: None,
        };
        let (prepared, bytes) = prepare(config).unwrap();
        let payload: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["schema"], "ai_keyboard.v1");
        assert_eq!(payload["target_platform"], "macos");
        assert_eq!(payload["ptt_hotkey"], "EasyInputVoice");
        assert_eq!(payload["edit_ptt_hotkey"], "EasyInputEdit");
        assert_eq!(payload["profiles"][0]["keys"]["KEY1"]["press"], "voice_ptt_hold");
        assert_eq!(payload["profiles"][0]["keys"]["KEY3"]["press"], "copy");
        let host_action = payload["profiles"][0]["keys"]["KEY7"]["press"].as_str().unwrap();
        assert!(host_action.starts_with("host_action:"));
        assert!(!String::from_utf8(bytes).unwrap().contains("Safari.app"));
        assert!(prepared.keys[6].host_action_id.is_some());
    }
}

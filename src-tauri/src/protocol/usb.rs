use thiserror::Error;

pub const VID: u16 = 0x303A;
pub const PID: u16 = 0x1006;
pub const REPORT_CONFIG: u8 = 0x10;
pub const REPORT_APP: u8 = 0x11;
pub const REPORT_AGENT: u8 = 0x12;
pub const REPORT_STATUS: u8 = 0x13;
pub const REPORT_SPEAKER_REQUEST: u8 = 0x14;
pub const REPORT_SPEAKER_RESPONSE: u8 = 0x15;
pub const REPORT_KEYBOARD: u8 = 0x01;

pub const APP_COMMAND_FIXED_TEXT: u8 = 0x01;
pub const APP_COMMAND_HOTKEY: u8 = 0x02;
pub const APP_COMMAND_CONFIG_ACK: u8 = 0x03;
pub const APP_COMMAND_STATUS: u8 = 0x04;
pub const APP_COMMAND_HOST_ACTION: u8 = 0x05;

pub const MAX_CONFIG: usize = 2048;
pub const CHUNK_DATA: usize = 52;
const CONFIG_MAGIC: &[u8; 3] = b"S3C";
const CONFIG_VERSION: u8 = 1;
const CONFIG_PAYLOAD_HEADER_LEN: usize = 11;
const FEATURE_REPORT_LEN: usize = 64;

#[derive(Debug, Error, PartialEq)]
pub enum ProtocolError {
    #[error("配置超过 2048 字节")]
    TooLarge,
    #[error("分片过短")]
    Short,
    #[error("CRC16 校验失败")]
    CrcMismatch,
    #[error("分片元数据无效")]
    Malformed,
}

pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc = 0xFFFF_u16;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigChunk {
    pub index: u8,
    pub total: u8,
    pub total_len: u16,
    pub crc: u16,
    pub payload: Vec<u8>,
}

impl ConfigChunk {
    /// Encodes the exact 64-byte USB Feature Report accepted by firmware.
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        if self.payload.len() > CHUNK_DATA || self.total == 0 || self.index >= self.total {
            return Err(ProtocolError::Malformed);
        }
        let mut out = vec![0_u8; FEATURE_REPORT_LEN];
        out[0] = REPORT_CONFIG;
        out[1..4].copy_from_slice(CONFIG_MAGIC);
        out[4] = CONFIG_VERSION;
        out[5] = self.index;
        out[6] = self.total;
        out[7..9].copy_from_slice(&self.total_len.to_le_bytes());
        out[9] = self.payload.len() as u8;
        out[10..12].copy_from_slice(&self.crc.to_le_bytes());
        out[12..12 + self.payload.len()].copy_from_slice(&self.payload);
        Ok(out)
    }

    pub fn decode(raw: &[u8]) -> Result<Self, ProtocolError> {
        let raw = if raw.first() == Some(&REPORT_CONFIG) { &raw[1..] } else { raw };
        if raw.len() < CONFIG_PAYLOAD_HEADER_LEN {
            return Err(ProtocolError::Short);
        }
        if &raw[0..3] != CONFIG_MAGIC || raw[3] != CONFIG_VERSION {
            return Err(ProtocolError::Malformed);
        }
        let chunk_len = raw[8] as usize;
        if chunk_len > CHUNK_DATA || raw.len() < CONFIG_PAYLOAD_HEADER_LEN + chunk_len {
            return Err(ProtocolError::Malformed);
        }
        let chunk = Self {
            index: raw[4],
            total: raw[5],
            total_len: u16::from_le_bytes([raw[6], raw[7]]),
            crc: u16::from_le_bytes([raw[9], raw[10]]),
            payload: raw[11..11 + chunk_len].to_vec(),
        };
        if chunk.total == 0 || chunk.index >= chunk.total {
            return Err(ProtocolError::Malformed);
        }
        Ok(chunk)
    }
}

pub fn split_config(json: &[u8]) -> Result<Vec<ConfigChunk>, ProtocolError> {
    if json.len() > MAX_CONFIG {
        return Err(ProtocolError::TooLarge);
    }
    let total = json.len().div_ceil(CHUNK_DATA).max(1);
    if total > u8::MAX as usize {
        return Err(ProtocolError::TooLarge);
    }
    let crc = crc16_ccitt(json);
    Ok((0..total)
        .map(|index| {
            let start = index * CHUNK_DATA;
            let end = (start + CHUNK_DATA).min(json.len());
            ConfigChunk {
                index: index as u8,
                total: total as u8,
                total_len: json.len() as u16,
                crc,
                payload: json[start..end].to_vec(),
            }
        })
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigAck {
    pub phase: u8,
    pub ok: bool,
    pub bytes: u16,
    pub crc: u16,
    pub saved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppCommand {
    ConfigAck(ConfigAck),
    HostAction(String),
    FixedText { index: u8, total: u8, data: Vec<u8> },
    Hotkey { pressed: bool, hotkey: String },
    Other(u8),
}

pub fn app_command(raw: &[u8]) -> Option<AppCommand> {
    if raw.len() < 5 || raw[0] != REPORT_APP {
        return None;
    }
    let kind = raw[1];
    let index = raw[2];
    let total = raw[3];
    let len = raw[4] as usize;
    if total == 0 || index >= total || len > 59 || raw.len() < 5 + len {
        return None;
    }
    let data = &raw[5..5 + len];
    match kind {
        APP_COMMAND_CONFIG_ACK if index == 0 && total == 1 && len == 7 => {
            Some(AppCommand::ConfigAck(ConfigAck {
                phase: data[0],
                ok: data[1] == 1,
                bytes: u16::from_le_bytes([data[2], data[3]]),
                crc: u16::from_le_bytes([data[4], data[5]]),
                saved: data[6] == 1,
            }))
        }
        APP_COMMAND_HOST_ACTION if index == 0 && total == 1 && len == 36 => {
            let value = std::str::from_utf8(data).ok()?.to_owned();
            uuid::Uuid::parse_str(&value).ok()?;
            Some(AppCommand::HostAction(value))
        }
        APP_COMMAND_FIXED_TEXT => Some(AppCommand::FixedText { index, total, data: data.to_vec() }),
        APP_COMMAND_HOTKEY if len >= 2 && (data[0] == 1 || data[0] == 2) => {
            Some(AppCommand::Hotkey {
                pressed: data[0] == 1,
                hotkey: std::str::from_utf8(&data[1..]).ok()?.to_owned(),
            })
        }
        _ => Some(AppCommand::Other(kind)),
    }
}

/// Voice PTT is a standard keyboard report in the released firmware. App
/// Command kind 0x01 is fixed text and must never be treated as a PTT event.
pub fn voice_button_state(raw: &[u8]) -> Option<bool> {
    if raw.len() < 2 || raw[0] != REPORT_KEYBOARD {
        return None;
    }
    let right_meta = raw[1] & 0x80 != 0;
    let legacy_chord = raw[1] & 0x03 == 0x03
        && raw.get(3..).is_some_and(|keys| keys.contains(&0x2c));
    Some(right_meta || legacy_chord)
}

/// Edit PTT uses Right Option on macOS; older/factory mappings used
/// Ctrl+Shift+E. Both are accepted during client/firmware migration.
pub fn edit_button_state(raw: &[u8]) -> Option<bool> {
    if raw.len() < 2 || raw[0] != REPORT_KEYBOARD { return None; }
    let right_option = raw[1] & 0x40 != 0;
    let legacy_chord = raw[1] & 0x03 == 0x03
        && raw.get(3..).is_some_and(|keys| keys.contains(&0x08));
    Some(right_option || legacy_chord)
}

/// Realtime voice uses the reserved Ctrl+Shift+R chord.  Unlike voice/edit
/// PTT it is a toggle action, so the host only acts on the pressed edge.
/// Dedicated App Command reports are parsed separately by `app_command`.
pub fn realtime_button_state(raw: &[u8]) -> Option<bool> {
    if raw.len() < 2 || raw[0] != REPORT_KEYBOARD { return None; }
    let ctrl_shift = raw[1] & 0x03 == 0x03;
    let r_key = raw.get(3..).is_some_and(|keys| keys.contains(&0x15));
    Some(ctrl_shift && r_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_crc() {
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
    }

    #[test]
    fn feature_report_matches_firmware_layout() {
        let chunk = split_config(b"{}").unwrap().remove(0);
        let encoded = chunk.encode().unwrap();
        assert_eq!(encoded.len(), 64);
        assert_eq!(&encoded[0..5], &[0x10, b'S', b'3', b'C', 1]);
        assert_eq!(encoded[5], 0);
        assert_eq!(encoded[6], 1);
        assert_eq!(u16::from_le_bytes([encoded[7], encoded[8]]), 2);
        assert_eq!(encoded[9], 2);
        assert_eq!(&encoded[12..14], b"{}");
        assert_eq!(ConfigChunk::decode(&encoded).unwrap(), chunk);
    }

    #[test]
    fn split_reassembles_exact_json() {
        let data = vec![b'x'; 117];
        let chunks = split_config(&data).unwrap();
        assert_eq!(chunks.len(), 3);
        let joined: Vec<u8> = chunks.iter().flat_map(|part| part.payload.clone()).collect();
        assert_eq!(joined, data);
        assert!(chunks.iter().all(|part| part.crc == crc16_ccitt(&data)));
    }

    #[test]
    fn parses_config_ack() {
        let raw = [0x11, 0x03, 0, 1, 7, 1, 1, 0x34, 0x12, 0xcd, 0xab, 1];
        assert_eq!(
            app_command(&raw),
            Some(AppCommand::ConfigAck(ConfigAck {
                phase: 1,
                ok: true,
                bytes: 0x1234,
                crc: 0xabcd,
                saved: true,
            }))
        );
    }

    #[test]
    fn fixed_text_is_not_voice_button() {
        assert_eq!(voice_button_state(&[0x11, 0x01, 0, 1, 1, b'a']), None);
    }

    #[test]
    fn parses_keyboard_voice_buttons() {
        assert_eq!(voice_button_state(&[REPORT_KEYBOARD, 0x80, 0, 0]), Some(true));
        assert_eq!(voice_button_state(&[REPORT_KEYBOARD, 0x03, 0, 0x2c, 0, 0, 0, 0, 0]), Some(true));
        assert_eq!(voice_button_state(&[REPORT_KEYBOARD, 0, 0, 0, 0, 0, 0, 0, 0]), Some(false));
    }


    #[test]
    fn parses_keyboard_edit_buttons() {
        assert_eq!(edit_button_state(&[REPORT_KEYBOARD, 0x40, 0, 0]), Some(true));
        assert_eq!(edit_button_state(&[REPORT_KEYBOARD, 0x03, 0, 0x08, 0, 0, 0, 0, 0]), Some(true));
        assert_eq!(edit_button_state(&[REPORT_KEYBOARD, 0, 0, 0, 0, 0, 0, 0, 0]), Some(false));
    }

    #[test]
    fn parses_keyboard_realtime_toggle() {
        assert_eq!(realtime_button_state(&[REPORT_KEYBOARD, 0x03, 0, 0x15, 0, 0, 0, 0, 0]), Some(true));
        assert_eq!(realtime_button_state(&[REPORT_KEYBOARD, 0, 0, 0, 0, 0, 0, 0, 0]), Some(false));
        assert_eq!(realtime_button_state(&[REPORT_KEYBOARD, 0x03, 0, 0x08, 0, 0, 0, 0, 0]), Some(false));
    }
}

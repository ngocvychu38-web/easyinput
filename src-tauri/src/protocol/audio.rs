use thiserror::Error;

pub const AUDIO_MAGIC: &[u8; 4] = b"EIAU";
pub const HEARTBEAT_MAGIC: &[u8; 4] = b"EIHB";
pub const CONTROL_MAGIC: &[u8; 4] = b"EICC";
pub const ACK_MAGIC: &[u8; 4] = b"EICA";
pub const SPEAKER_MAGIC: &[u8; 4] = b"EISP";
pub const AUDIO_HEADER_LEN: usize = 32;
pub const CONTROL_PACKET_LEN: usize = 36;
pub const ACK_PACKET_LEN: usize = 20;
pub const SPEAKER_HEADER_LEN: usize = 32;
pub const INPUT_RATE: u32 = 16_000;
pub const INPUT_FRAME_SAMPLES: u16 = 320;
pub const OUTPUT_RATE: u32 = 24_000;
pub const OUTPUT_FRAME_SAMPLES: u16 = 480;

#[derive(Debug, Error, PartialEq)]
pub enum AudioError {
    #[error("数据包过短")]
    Short,
    #[error("magic 无效")]
    Magic,
    #[error("协议版本不支持")]
    Version,
    #[error("音频格式不支持")]
    Format,
    #[error("音频长度无效")]
    Length,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioPacket<'a> {
    pub session_id: u64,
    pub sequence: u32,
    pub sample_rate: u32,
    pub timestamp_ms: u32,
    pub frame_samples: u16,
    pub payload: &'a [u8],
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Heartbeat {
    pub streaming: bool,
    pub audio_ready: bool,
    pub session_id: u64,
    pub sequence: u32,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ControlAck {
    pub action: u8,
    pub status: u8,
    pub session_id: u64,
    pub sequence: u32,
}

#[derive(Debug, Copy, Clone)]
pub enum ControlAction { Start = 1, Stop = 2, Keepalive = 3 }

pub fn parse_audio(raw: &[u8]) -> Result<AudioPacket<'_>, AudioError> {
    if raw.len() < AUDIO_HEADER_LEN { return Err(AudioError::Short); }
    if &raw[0..4] != AUDIO_MAGIC { return Err(AudioError::Magic); }
    if raw[4] != 2 || raw[5] as usize != AUDIO_HEADER_LEN { return Err(AudioError::Version); }
    if raw[6] != 1 || raw[7] != 1 { return Err(AudioError::Format); }
    let session_id = u64::from_le_bytes(raw[8..16].try_into().unwrap());
    let sequence = u32::from_le_bytes(raw[16..20].try_into().unwrap());
    let sample_rate = u32::from_le_bytes(raw[20..24].try_into().unwrap());
    let timestamp_ms = u32::from_le_bytes(raw[24..28].try_into().unwrap());
    let frame_samples = u16::from_le_bytes(raw[28..30].try_into().unwrap());
    let payload_len = u16::from_le_bytes(raw[30..32].try_into().unwrap()) as usize;
    if sample_rate != INPUT_RATE || frame_samples != INPUT_FRAME_SAMPLES { return Err(AudioError::Format); }
    if raw.len() != AUDIO_HEADER_LEN + payload_len || payload_len != frame_samples as usize * 2 { return Err(AudioError::Length); }
    Ok(AudioPacket { session_id, sequence, sample_rate, timestamp_ms, frame_samples, payload: &raw[AUDIO_HEADER_LEN..] })
}

pub fn parse_heartbeat(raw: &[u8]) -> Result<Heartbeat, AudioError> {
    if raw.len() < 20 { return Err(AudioError::Short); }
    if &raw[0..4] != HEARTBEAT_MAGIC { return Err(AudioError::Magic); }
    if raw[4] != 1 { return Err(AudioError::Version); }
    Ok(Heartbeat {
        streaming: raw[5] & 1 != 0,
        audio_ready: raw[5] & 2 != 0,
        session_id: u64::from_le_bytes(raw[8..16].try_into().unwrap()),
        sequence: u32::from_le_bytes(raw[16..20].try_into().unwrap()),
    })
}

pub fn control_packet(action: ControlAction, session_id: u64, sequence: u32) -> [u8; CONTROL_PACKET_LEN] {
    let mut out = [0u8; CONTROL_PACKET_LEN];
    out[0..4].copy_from_slice(CONTROL_MAGIC);
    out[4] = 1;
    out[5] = action as u8;
    out[8..16].copy_from_slice(&session_id.to_le_bytes());
    out[16..20].copy_from_slice(&sequence.to_le_bytes());
    out
}

pub fn parse_control_ack(raw: &[u8]) -> Result<ControlAck, AudioError> {
    if raw.len() != ACK_PACKET_LEN { return Err(AudioError::Length); }
    if &raw[0..4] != ACK_MAGIC { return Err(AudioError::Magic); }
    if raw[4] != 1 { return Err(AudioError::Version); }
    Ok(ControlAck {
        action: raw[5],
        status: raw[6],
        session_id: u64::from_le_bytes(raw[8..16].try_into().unwrap()),
        sequence: u32::from_le_bytes(raw[16..20].try_into().unwrap()),
    })
}

pub fn speaker_packet(session_id: u64, sequence: u32, pcm: &[u8]) -> Result<Vec<u8>, AudioError> {
    if pcm.len() != OUTPUT_FRAME_SAMPLES as usize * 2 { return Err(AudioError::Length); }
    let mut packet = vec![0u8; SPEAKER_HEADER_LEN + pcm.len()];
    packet[0..4].copy_from_slice(SPEAKER_MAGIC);
    packet[4] = 1;
    packet[5] = SPEAKER_HEADER_LEN as u8;
    packet[6] = 1;
    packet[7] = 1;
    packet[8..16].copy_from_slice(&session_id.to_le_bytes());
    packet[16..20].copy_from_slice(&sequence.to_le_bytes());
    packet[20..24].copy_from_slice(&OUTPUT_RATE.to_le_bytes());
    packet[24..26].copy_from_slice(&OUTPUT_FRAME_SAMPLES.to_le_bytes());
    packet[26..28].copy_from_slice(&(pcm.len() as u16).to_le_bytes());
    packet[SPEAKER_HEADER_LEN..].copy_from_slice(pcm);
    Ok(packet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_firmware_audio_vector() {
        let mut packet = vec![0u8; 672];
        packet[0..4].copy_from_slice(AUDIO_MAGIC);
        packet[4] = 2;
        packet[5] = 32;
        packet[6] = 1;
        packet[7] = 1;
        packet[8..16].copy_from_slice(&0x0102_0304_0506_0708u64.to_le_bytes());
        packet[16..20].copy_from_slice(&7u32.to_le_bytes());
        packet[20..24].copy_from_slice(&INPUT_RATE.to_le_bytes());
        packet[24..28].copy_from_slice(&1234u32.to_le_bytes());
        packet[28..30].copy_from_slice(&INPUT_FRAME_SAMPLES.to_le_bytes());
        packet[30..32].copy_from_slice(&640u16.to_le_bytes());
        let parsed = parse_audio(&packet).unwrap();
        assert_eq!(parsed.session_id, 0x0102_0304_0506_0708);
        assert_eq!(parsed.sequence, 7);
        assert_eq!(parsed.timestamp_ms, 1234);
        assert_eq!(parsed.payload.len(), 640);
    }

    #[test]
    fn control_uses_u64_session_and_sequence() {
        let packet = control_packet(ControlAction::Start, 0x0102_0304_0506_0708, 9);
        assert_eq!(&packet[8..16], &[8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(&packet[16..20], &[9, 0, 0, 0]);
    }

    #[test]
    fn speaker_packet_is_one_twenty_millisecond_pcm_frame() {
        let pcm = vec![0x5a; 960];
        let packet = speaker_packet(42, 3, &pcm).unwrap();
        assert_eq!(&packet[0..4], SPEAKER_MAGIC);
        assert_eq!(u32::from_le_bytes(packet[20..24].try_into().unwrap()), OUTPUT_RATE);
        assert_eq!(u16::from_le_bytes(packet[24..26].try_into().unwrap()), OUTPUT_FRAME_SAMPLES);
        assert_eq!(&packet[SPEAKER_HEADER_LEN..], pcm.as_slice());
    }
}

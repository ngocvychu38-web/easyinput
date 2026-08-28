use crate::model::{OperationResult, RealtimeCallPhase, RealtimeCallState, RealtimeVoiceConfig};
use crate::protocol::audio::{self, ControlAction};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::{collections::VecDeque, future::pending, net::SocketAddr, time::{Duration, Instant}};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{net::UdpSocket, sync::mpsc};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Error as WebSocketError, Message};

pub const ENDPOINT: &str = "wss://openspeech.bytedance.com/api/v3/duplex/realtime/dialogue";
pub const API_KEY_ACCOUNT: &str = "volcengine.realtime-voice.api-key.v1";
const MODEL: &str = "1.2.6.1";
const UDP_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(7);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);
const SPEAKER_FRAME_BYTES: usize = 960;
const SPEAKER_FRAME_DURATION: Duration = Duration::from_millis(20);

type RealtimeSocket = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest { pub latency_ms: u128, pub endpoint: String, pub model: String, pub log_id: Option<String> }

#[derive(Debug)]
pub enum RealtimeCommand { Stop, Interrupt }

pub fn validate(config: &RealtimeVoiceConfig) -> Result<(), String> {
    if config.endpoint != ENDPOINT { return Err("为避免 API Key 泄露，实时语音服务地址必须使用豆包官方 WSS 地址".into()); }
    if config.model != MODEL { return Err("豆包实时语音 3.0 全双工模型固定为 1.2.6.1".into()); }
    if config.voice.trim().is_empty() { return Err("请填写实时语音音色 ID".into()); }
    if !(-50..=100).contains(&config.speed) { return Err("语速必须在 -50 到 100 之间".into()); }
    if !(-50..=100).contains(&config.loudness) { return Err("音量必须在 -50 到 100 之间".into()); }
    if config.instructions.as_bytes().len() > 16_000 { return Err("系统提示词过长".into()); }
    Ok(())
}

fn authorized_request(config: &RealtimeVoiceConfig, api_key: &str) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, String> {
    let api_key = api_key.trim();
    if api_key.to_ascii_lowercase().starts_with("bearer ") {
        return Err("Realtime API Key 请直接填写密钥本身，不要添加 Bearer 前缀".into());
    }
    let mut request = config.endpoint.clone().into_client_request().map_err(|error| format!("实时语音服务地址无效：{error}"))?;
    request.headers_mut().insert("X-Api-Key", HeaderValue::from_str(api_key).map_err(|_| "API Key 包含无效字符")?);
    Ok(request)
}

fn handshake_error(error: WebSocketError) -> String {
    let WebSocketError::Http(response) = error else {
        return format!("豆包实时语音握手失败：{error}");
    };
    let status = response.status();
    let log_id = response.headers().get("X-Tt-Logid").and_then(|value| value.to_str().ok());
    let body = response.body().as_deref().and_then(|value| std::str::from_utf8(value).ok());
    let server_error = body.and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok()).and_then(|value| {
        let code = value.pointer("/error/code").and_then(|value| value.as_str()).unwrap_or_default();
        let message = value.pointer("/error/message").and_then(|value| value.as_str()).unwrap_or_default();
        if message.is_empty() { None } else if code.is_empty() { Some(message.to_owned()) } else { Some(format!("{message}（{code}）")) }
    });
    let mut result = format!("豆包实时语音握手失败：{status}");
    if let Some(server_error) = server_error { result.push_str(&format!("；服务端：{server_error}")); }
    if status == tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED {
        result.push_str("。请填写“新版豆包语音控制台 → API Key 管理”生成的原始 API Key；不能使用语音识别 Access Token、App ID、Resource ID 或火山方舟 API Key");
    }
    if let Some(log_id) = log_id { result.push_str(&format!("。LogID：{log_id}")); }
    result
}

fn session_create_event(config: &RealtimeVoiceConfig, session_id: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "session.create",
        "event_id": format!("event-{}", uuid::Uuid::new_v4()),
        "session": {
            "id": session_id,
            "model": config.model,
            "instructions": config.instructions,
            "audio": {
                "input": { "format": { "type": "pcm", "rate": 16000 } },
                "output": {
                    "format": { "type": "pcm_s16le", "rate": 24000 },
                    "voice": config.voice,
                    "speed": config.speed,
                    "loudness": config.loudness
                }
            }
        },
        "extension": {
            "asr": { "extra": {} },
            "tts": { "extra": {} },
            "dialog": {
                "extra": {
                    "strict_audit": config.strict_audit,
                    "enable_loudness_norm": config.enable_loudness_norm,
                    "enable_user_query_exit": config.enable_user_query_exit,
                    "enable_music": false
                }
            }
        }
    })
}

fn text_message(value: serde_json::Value) -> Result<Message, String> {
    serde_json::to_string(&value).map(|value| Message::Text(value.into())).map_err(|error| error.to_string())
}

fn event_error(value: &serde_json::Value) -> String {
    let code = value.pointer("/error/code").or_else(|| value.get("status_code")).and_then(|value| value.as_i64());
    let message = value.pointer("/error/message").or_else(|| value.get("message")).and_then(|value| value.as_str()).unwrap_or("未知错误");
    match code { Some(code) => format!("豆包实时语音错误（{code}）：{message}"), None => format!("豆包实时语音错误：{message}") }
}

async fn connect_and_create(config: &RealtimeVoiceConfig, api_key: &str, session_id: &str) -> Result<(RealtimeSocket, Option<String>), String> {
    validate(config)?;
    if api_key.trim().is_empty() { return Err("实时语音 API Key 为空".into()); }
    let request = authorized_request(config, api_key)?;
    let (mut socket, response) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request)).await
        .map_err(|_| "连接豆包实时语音服务超时（10 秒）".to_string())?
        .map_err(handshake_error)?;
    let log_id = response.headers().get("X-Tt-Logid").and_then(|value| value.to_str().ok()).map(str::to_owned);
    socket.send(text_message(session_create_event(config, session_id))?).await.map_err(|error| format!("发送 session.create 失败：{error}"))?;
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    loop {
        let incoming = tokio::time::timeout_at(deadline, socket.next()).await.map_err(|_| "等待 session.created 超时（10 秒）".to_string())?;
        let message = incoming.ok_or_else(|| "豆包实时语音连接在创建会话前关闭".to_string())?.map_err(|error| format!("接收 session.created 失败：{error}"))?;
        if let Message::Text(text) = message {
            let event: serde_json::Value = serde_json::from_str(text.as_str()).map_err(|error| format!("实时语音响应 JSON 无效：{error}"))?;
            match event.get("type").and_then(|value| value.as_str()) {
                Some("session.created") => return Ok((socket, log_id)),
                Some("error") => return Err(event_error(&event)),
                _ => {}
            }
        }
    }
}

async fn close_remote_session(socket: &mut RealtimeSocket) {
    let event = serde_json::json!({
        "type": "session.close",
        "event_id": format!("event-{}", uuid::Uuid::new_v4())
    });
    if let Ok(message) = text_message(event) {
        let _ = socket.send(message).await;
    }
    let deadline = tokio::time::Instant::now() + CLOSE_TIMEOUT;
    loop {
        let incoming = match tokio::time::timeout_at(deadline, socket.next()).await {
            Ok(value) => value,
            Err(_) => break,
        };
        match incoming {
            Some(Ok(Message::Text(text))) => {
                if serde_json::from_str::<serde_json::Value>(text.as_str())
                    .ok()
                    .and_then(|event| event.get("type").and_then(|value| value.as_str()).map(str::to_owned))
                    .as_deref()
                    == Some("session.closed")
                {
                    break;
                }
            }
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
            Some(Ok(_)) => {}
        }
    }
    let _ = socket.close(None).await;
}

pub async fn test_connection(config: &RealtimeVoiceConfig, api_key: &str) -> OperationResult<ConnectionTest> {
    let started = Instant::now();
    match connect_and_create(config, api_key, &uuid::Uuid::new_v4().to_string()).await {
        Ok((mut socket, log_id)) => {
            close_remote_session(&mut socket).await;
            OperationResult::success(Some(ConnectionTest { latency_ms: started.elapsed().as_millis(), endpoint: config.endpoint.clone(), model: config.model.clone(), log_id }))
        }
        Err(error) => OperationResult::failure(error),
    }
}

fn wire_session_id(session_id: &str) -> u64 {
    let parsed = uuid::Uuid::parse_str(session_id).unwrap_or_else(|_| uuid::Uuid::new_v4());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&parsed.as_bytes()[..8]);
    let value = u64::from_le_bytes(bytes);
    if value == 0 { 1 } else { value }
}

fn update_state(app: &AppHandle, update: impl FnOnce(&mut RealtimeCallState)) {
    let state = app.state::<crate::AppState>();
    if let Ok(mut current) = state.realtime_call.lock() {
        update(&mut current);
        let _ = app.emit("realtime-call-state", current.clone());
    };
}

async fn wait_for_keyboard(socket: &UdpSocket) -> Result<SocketAddr, String> {
    let deadline = tokio::time::Instant::now() + UDP_DISCOVERY_TIMEOUT;
    let mut buffer = vec![0u8; 2049];
    loop {
        let (size, peer) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer)).await
            .map_err(|_| "7 秒内未收到开发板音频心跳；请确认键盘 Wi-Fi、电脑接收地址和端口已同步".to_string())?
            .map_err(|error| format!("接收开发板心跳失败：{error}"))?;
        if audio::parse_heartbeat(&buffer[..size]).is_ok() { return Ok(peer); }
    }
}

async fn start_keyboard_stream(socket: &UdpSocket, peer: SocketAddr, session_id: u64) -> Result<u32, String> {
    let sequence = 1;
    socket.send_to(&audio::control_packet(ControlAction::Start, session_id, sequence), peer).await.map_err(|error| format!("向开发板发送录音启动命令失败：{error}"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut buffer = vec![0u8; 2049];
    loop {
        let (size, source) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer)).await
            .map_err(|_| "开发板未确认麦克风启动命令".to_string())?
            .map_err(|error| format!("等待开发板启动确认失败：{error}"))?;
        if source != peer { continue; }
        if let Ok(ack) = audio::parse_control_ack(&buffer[..size]) {
            if ack.session_id == session_id && ack.sequence == sequence {
                return if ack.status == 0 { Ok(sequence) } else { Err(format!("开发板拒绝启动麦克风（状态码 {}）", ack.status)) };
            }
        }
    }
}

pub async fn open_session(app: AppHandle, session_id: String, config: RealtimeVoiceConfig, api_key: String, audio_port: u16) -> Result<mpsc::UnboundedSender<RealtimeCommand>, String> {
    let udp = UdpSocket::bind(("0.0.0.0", audio_port)).await.map_err(|error| format!("无法监听开发板音频端口 {audio_port}：{error}"))?;
    let peer = wait_for_keyboard(&udp).await?;
    let (mut socket, log_id) = connect_and_create(&config, &api_key, &session_id).await?;
    let wire_session = wire_session_id(&session_id);
    let mut control_sequence = match start_keyboard_stream(&udp, peer, wire_session).await {
        Ok(sequence) => sequence,
        Err(error) => {
            close_remote_session(&mut socket).await;
            return Err(error);
        }
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    update_state(&app, |state| {
        state.phase = RealtimeCallPhase::Listening;
        state.session_id = Some(session_id.clone());
        state.log_id = log_id.clone();
    });
    let greeting = config.greeting.trim().to_owned();
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let (mut writer, mut reader) = socket.split();
        if !greeting.is_empty() {
            let _ = writer.send(text_message(serde_json::json!({"type":"speech_text_buffer.commit","event_id":format!("event-{}",uuid::Uuid::new_v4()),"speech_id":uuid::Uuid::new_v4().to_string(),"text":greeting})).unwrap()).await;
        }
        let mut udp_buffer = vec![0u8; 4096];
        let mut speaker_buffer = Vec::<u8>::new();
        let mut speaker_queue = VecDeque::<Vec<u8>>::new();
        let mut speaker_sequence = 0u32;
        let mut speaker_tick = tokio::time::interval(SPEAKER_FRAME_DURATION);
        speaker_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        speaker_tick.tick().await;
        let mut keepalive = tokio::time::interval(Duration::from_secs(1));
        let mut state_tick = tokio::time::interval(Duration::from_millis(250));
        let mut terminal_error: Option<String> = None;
        let mut graceful_close = false;
        let mut close_deadline: Option<tokio::time::Instant> = None;
        loop {
            tokio::select! {
                command = rx.recv() => match command {
                    Some(RealtimeCommand::Interrupt) => {
                        let event = serde_json::json!({"type":"response.cancel","event_id":format!("event-{}",uuid::Uuid::new_v4())});
                        if let Err(error) = writer.send(text_message(event).unwrap()).await { terminal_error = Some(format!("发送打断请求失败：{error}")); break; }
                    }
                    Some(RealtimeCommand::Stop) | None => {
                        if close_deadline.is_none() {
                            update_state(&app, |state| state.phase = RealtimeCallPhase::Closing);
                            let _ = udp.send_to(&audio::control_packet(ControlAction::Stop, wire_session, control_sequence.wrapping_add(1)), peer).await;
                            let event = serde_json::json!({"type":"session.close","event_id":format!("event-{}",uuid::Uuid::new_v4())});
                            let _ = writer.send(text_message(event).unwrap()).await;
                            close_deadline = Some(tokio::time::Instant::now() + CLOSE_TIMEOUT);
                        }
                    }
                },
                result = udp.recv_from(&mut udp_buffer) => match result {
                    Ok((size, source)) if source.ip() == peer.ip() && close_deadline.is_none() => {
                        if let Ok(packet) = audio::parse_audio(&udp_buffer[..size]) {
                            if packet.session_id == wire_session {
                                let event = serde_json::json!({"type":"input_audio_buffer.append","event_id":format!("event-{}",uuid::Uuid::new_v4()),"audio":BASE64.encode(packet.payload)});
                                if let Err(error) = writer.send(text_message(event).unwrap()).await { terminal_error = Some(format!("上传开发板音频失败：{error}")); break; }
                                update_state(&app, |state| state.input_packets += 1);
                            }
                        }
                    }
                    Ok(_) => {},
                    Err(error) => { terminal_error = Some(format!("接收开发板音频失败：{error}")); break; }
                },
                incoming = reader.next() => match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let event: serde_json::Value = match serde_json::from_str(text.as_str()) { Ok(value) => value, Err(error) => { terminal_error = Some(format!("实时语音响应 JSON 无效：{error}")); break; } };
                        match event.get("type").and_then(|value| value.as_str()) {
                            Some("conversation.item.input_audio_transcription.started") => {
                                speaker_buffer.clear();
                                speaker_queue.clear();
                                update_state(&app, |state| { state.phase = RealtimeCallPhase::Listening; state.user_text.clear(); state.assistant_text.clear(); });
                            }
                            Some("conversation.item.input_audio_transcription.delta") => if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
                                update_state(&app, |state| state.user_text.push_str(delta));
                            },
                            Some("conversation.item.input_audio_transcription.completed") => if let Some(text) = event.get("transcript").or_else(|| event.get("text")).and_then(|value| value.as_str()) {
                                update_state(&app, |state| state.user_text = text.to_owned());
                            },
                            Some("response.output_text.delta") => if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
                                update_state(&app, |state| { state.phase = RealtimeCallPhase::Speaking; state.assistant_text.push_str(delta); });
                            },
                            Some("response.output_text.done") => if let Some(text) = event.get("text").and_then(|value| value.as_str()) {
                                update_state(&app, |state| state.assistant_text = text.to_owned());
                            },
                            Some("response.output_audio.started") => update_state(&app, |state| state.phase = RealtimeCallPhase::Speaking),
                            Some("response.output_audio.delta") => if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
                                match BASE64.decode(delta) {
                                    Ok(bytes) => {
                                        speaker_buffer.extend_from_slice(&bytes);
                                        while speaker_buffer.len() >= SPEAKER_FRAME_BYTES {
                                            speaker_queue.push_back(speaker_buffer.drain(..SPEAKER_FRAME_BYTES).collect());
                                        }
                                    }
                                    Err(error) => { terminal_error = Some(format!("豆包下行音频 Base64 无效：{error}")); break; }
                                }
                            },
                            Some("response.output_audio.done") => update_state(&app, |state| state.phase = RealtimeCallPhase::Listening),
                            Some("session.closed") => { graceful_close = true; break; }
                            Some("error") => { terminal_error = Some(event_error(&event)); break; }
                            _ => {}
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => { if close_deadline.is_none() { terminal_error = Some("豆包实时语音连接意外关闭".into()); } else { graceful_close = true; } break; }
                    Some(Ok(_)) => {},
                    Some(Err(error)) => { terminal_error = Some(format!("接收豆包实时语音失败：{error}")); break; }
                },
                _ = speaker_tick.tick(), if close_deadline.is_none() => {
                    if let Some(frame) = speaker_queue.pop_front() {
                        speaker_sequence = speaker_sequence.wrapping_add(1);
                        match audio::speaker_packet(wire_session, speaker_sequence, &frame) {
                            Ok(packet) => if let Err(error) = udp.send_to(&packet, peer).await { terminal_error = Some(format!("向开发板扬声器发送音频失败：{error}")); break; },
                            Err(error) => { terminal_error = Some(error.to_string()); break; }
                        }
                        update_state(&app, |state| state.output_packets += 1);
                    }
                },
                _ = keepalive.tick() => {
                    if close_deadline.is_none() {
                        control_sequence = control_sequence.wrapping_add(1);
                        let _ = udp.send_to(&audio::control_packet(ControlAction::Keepalive, wire_session, control_sequence), peer).await;
                    }
                },
                _ = state_tick.tick() => update_state(&app, |state| state.elapsed_ms = started.elapsed().as_millis() as u64),
                _ = async {
                    match close_deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => pending::<()>().await,
                    }
                } => { graceful_close = true; break; },
            }
        }
        let _ = udp.send_to(&audio::control_packet(ControlAction::Stop, wire_session, control_sequence.wrapping_add(1)), peer).await;
        if !graceful_close {
            let _ = writer.send(text_message(serde_json::json!({"type":"session.close","event_id":format!("event-{}",uuid::Uuid::new_v4())})).unwrap()).await;
        }
        let _ = writer.close().await;
        let state = app.state::<crate::AppState>();
        if let Ok(mut session) = state.realtime_session.lock() { *session = None; }
        update_state(&app, |state| {
            state.elapsed_ms = started.elapsed().as_millis() as u64;
            state.session_id = None;
            state.error = terminal_error.clone();
            state.phase = if terminal_error.is_some() { RealtimeCallPhase::Error } else { RealtimeCallPhase::Idle };
        });
    });
    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_official_duplex_protocol() {
        let config = RealtimeVoiceConfig::default();
        assert!(validate(&config).is_ok());
        let event = session_create_event(&config, "test-session");
        assert_eq!(event["session"]["model"], "1.2.6.1");
        assert_eq!(event["session"]["audio"]["input"]["format"]["rate"], 16000);
        assert_eq!(event["session"]["audio"]["output"]["format"]["rate"], 24000);
        assert_eq!(event["session"]["audio"]["output"]["format"]["type"], "pcm_s16le");
    }

    #[test]
    fn rejects_api_key_exfiltration_endpoint() {
        let mut config = RealtimeVoiceConfig::default();
        config.endpoint = "wss://example.com/steal".into();
        assert!(validate(&config).is_err());
    }

    #[test]
    fn explains_invalid_api_key_handshake_without_echoing_secret() {
        let response = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(401)
            .header("X-Tt-Logid", "test-log-id")
            .body(Some(br#"{"error":{"code":"45000010","message":"Invalid X-Api-Key"}}"#.to_vec()))
            .unwrap();
        let message = handshake_error(WebSocketError::Http(Box::new(response)));
        assert!(message.contains("Invalid X-Api-Key（45000010）"));
        assert!(message.contains("新版豆包语音控制台"));
        assert!(message.contains("test-log-id"));
    }

    #[test]
    fn trims_raw_api_key_and_rejects_bearer_prefix() {
        let config = RealtimeVoiceConfig::default();
        let request = authorized_request(&config, "  raw-key  ").unwrap();
        assert_eq!(request.headers()["X-Api-Key"], "raw-key");
        assert!(authorized_request(&config, "Bearer raw-key").unwrap_err().contains("不要添加 Bearer"));
    }
}

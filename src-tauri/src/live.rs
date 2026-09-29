//! Hardware-only caption sessions. No keyboard injection, history or assistant tools.
use crate::{
    model::{RealtimeCallPhase, RecordingPhase},
    protocol::audio::{self, ControlAction},
    AppState,
};
use serde::Serialize;
use std::{sync::Mutex, time::Duration};
use tauri::{Emitter, Manager};
use tokio::{net::UdpSocket, sync::mpsc, time::Instant};

pub enum Command {
    Heartbeat,
    Stop,
}
pub struct Lease {
    pub owner: String,
    pub id: String,
    pub tx: mpsc::Sender<Command>,
}
pub type State = Mutex<Option<Lease>>;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub session_id: String,
    pub kind: &'static str,
    pub text: String,
    pub definite: bool,
    pub utterance_id: String,
}
fn emit(app: &tauri::AppHandle, id: &str, kind: &'static str, text: String) {
    let _ = app.emit_to(
        "main",
        "live-speech",
        Event {
            session_id: id.into(),
            kind,
            text,
            definite: false,
            utterance_id: String::new(),
        },
    );
}

#[tauri::command]
pub async fn start_live_speech(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    owner: String,
) -> Result<String, String> {
    uuid::Uuid::parse_str(&owner).map_err(|_| "直播页面标识无效")?;
    let _guard = state.realtime_start_gate.lock().await;
    if state.live.lock().map_err(|_| "直播状态锁损坏")?.is_some() {
        return Err("直播识别正在运行或结束中".into());
    }
    if state
        .realtime_session
        .lock()
        .map_err(|_| "通话状态锁损坏")?
        .is_some()
        || !matches!(
            state
                .realtime_call
                .lock()
                .map_err(|_| "通话状态锁损坏")?
                .phase,
            RealtimeCallPhase::Idle | RealtimeCallPhase::Error
        )
        || !matches!(
            state.recording.lock().map_err(|_| "录音状态锁损坏")?.phase,
            RecordingPhase::Idle | RecordingPhase::Error
        )
    {
        return Err("请先结束当前语音输入或实时通话".into());
    }
    let config = state.storage.read_config()?;
    if !config.speech.enabled {
        return Err("请先在语音服务配置中启用豆包语音识别".into());
    }
    crate::speech::validate(&config.speech)?;
    let port = config.keyboard.wifi.audio_port;
    if port == 0 {
        return Err("开发板音频端口不能为 0".into());
    }
    let udp = UdpSocket::bind(("0.0.0.0", port))
        .await
        .map_err(|_| format!("开发板音频端口 {port} 已占用或无法监听"))?;
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = mpsc::channel(16);
    *state.live.lock().map_err(|_| "直播状态锁损坏")? = Some(Lease {
        owner,
        id: id.clone(),
        tx,
    });
    let session_id = id.clone();
    tauri::async_runtime::spawn(async move {
        let id = session_id;
        let wire = u64::from_le_bytes(uuid::Uuid::new_v4().as_bytes()[..8].try_into().unwrap());
        let mut peer = None;
        let mut control_sequence = 0u32;
        let result = {
            let run = async {
                let token = crate::read_speech_token(app.state::<AppState>().inner()).await?;
                emit(&app, &id, "state", "等待开发板音频心跳…".into());
                let address = crate::realtime::wait_for_keyboard(&udp).await?;
                peer = Some(address);
                control_sequence =
                    crate::realtime::start_keyboard_stream(&udp, address, wire).await?;
                // Keep the live path close to real time.  When the cloud briefly
                // falls behind, retaining every frame only makes captions stale;
                // dropping a frame is preferable to building a multi-second queue.
                let (audio_tx, audio_rx) = mpsc::channel(25); // about 0.5 seconds at 20 ms/frame
                let mut last_packet = Instant::now();
                let mut previous_sequence: Option<u32> = None;
                let capture = async {
                    let mut buffer = [0u8; 4096];
                    let mut tick = tokio::time::interval(Duration::from_secs(1));
                    loop {
                        tokio::select! {
                            packet = udp.recv_from(&mut buffer) => {
                                let (size, source) = packet.map_err(|_| "读取开发板音频失败".to_string())?;
                                if !same_device(source, address) { continue; }
                                if let Ok(packet) = audio::parse_audio(&buffer[..size]) {
                                    if packet.session_id != wire || !accept_sequence(previous_sequence, packet.sequence) { continue; }
                                    previous_sequence = Some(packet.sequence);
                                    last_packet = Instant::now();
                                    // If recognition falls behind, skip this frame
                                    // instead of terminating the live session. A
                                    // small bounded queue keeps latency from growing.
                                    let _ = audio_tx.try_send(packet.payload.to_vec());
                                }
                            }
                            _ = tick.tick() => {
                                if last_packet.elapsed() > Duration::from_secs(5) { return Err::<(), String>("开发板音频中断，请检查 Wi-Fi 后重新开启字幕".into()); }
                                control_sequence = control_sequence.wrapping_add(1);
                                udp.send_to(&audio::control_packet(ControlAction::Keepalive, wire, control_sequence), address).await.map_err(|_| "开发板保活失败".to_string())?;
                            }
                        }
                    }
                };
                let dictionary = app
                    .state::<AppState>()
                    .storage
                    .read_dictionary()
                    .unwrap_or_default();
                emit(&app, &id, "state", "开发板已连接，正在连接语音识别…".into());
                let recognize = crate::speech::caption_stream(
                    config.speech,
                    token,
                    dictionary,
                    audio_rx,
                    |caption| {
                        let _ = app.emit_to(
                            "main",
                            "live-speech",
                            Event {
                                session_id: id.clone(),
                                kind: "transcript",
                                text: caption.text,
                                definite: caption.definite,
                                utterance_id: caption.id,
                            },
                        );
                    },
                    || emit(&app, &id, "state", "开发板字幕识别中".into()),
                );
                tokio::try_join!(capture, recognize).map(|_| ())
            };
            tokio::pin!(run);
            let mut heartbeat = Instant::now();
            let mut watchdog = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    result = &mut run => break result,
                    command = rx.recv() => match command {
                        Some(Command::Heartbeat) => heartbeat = Instant::now(),
                        Some(Command::Stop) | None => break Ok(()),
                    },
                    _ = watchdog.tick() => if heartbeat.elapsed() > Duration::from_secs(20) { break Err("直播页面已断开，字幕识别已释放".into()); }
                }
            }
        };
        if let Some(address) = peer {
            // Stop is idempotent; repeat for UDP loss, without waiting on the UI.
            for _ in 0..2 {
                control_sequence = control_sequence.wrapping_add(1);
                let _ = udp
                    .send_to(
                        &audio::control_packet(ControlAction::Stop, wire, control_sequence),
                        address,
                    )
                    .await;
            }
        }
        drop(udp);
        if let Ok(mut lease) = app.state::<AppState>().live.lock() {
            if lease.as_ref().is_some_and(|lease| lease.id == id) {
                *lease = None;
            }
        }
        match result {
            Ok(()) => emit(&app, &id, "stopped", "硬件字幕已停止".into()),
            Err(error) => emit(&app, &id, "error", error),
        }
    });
    Ok(id)
}

// Firmware uses separate UDP sockets for control/heartbeat and microphone PCM.
// Pin the device IP here; the audio parser and wire session validate each frame below.
fn same_device(source: std::net::SocketAddr, control_peer: std::net::SocketAddr) -> bool {
    source.ip() == control_peer.ip()
}

fn accept_sequence(previous: Option<u32>, next: u32) -> bool {
    previous.is_none_or(|old| {
        let delta = next.wrapping_sub(old);
        delta > 0 && delta < (1 << 31)
    })
}

#[tauri::command]
pub fn live_speech_command(
    state: tauri::State<AppState>,
    owner: String,
    session_id: String,
    action: String,
) -> Result<(), String> {
    let lease = state.live.lock().map_err(|_| "直播状态锁损坏")?;
    let Some(lease) = lease.as_ref() else {
        return if action == "stop" {
            Ok(())
        } else {
            Err("字幕会话已结束".into())
        };
    };
    if lease.owner != owner || lease.id != session_id {
        return Err("页面不拥有该字幕会话".into());
    }
    let command = match action.as_str() {
        "heartbeat" => Command::Heartbeat,
        "stop" => Command::Stop,
        _ => return Err("不支持的字幕操作".into()),
    };
    lease
        .tx
        .try_send(command)
        .map_err(|_| "字幕会话繁忙或已结束".into())
}

const RTC_TOKEN: &str = "easyinput.live.rtc-token.v1";
#[tauri::command]
pub async fn live_rtc_token(token: Option<String>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        if let Some(token) = token {
            if token.len() > 8192 {
                return Err("RTC Token 过长".into());
            }
            crate::storage::set_secret(RTC_TOKEN, &token)?;
            Ok(String::new())
        } else {
            Ok(crate::storage::get_secret(RTC_TOKEN)?.unwrap_or_default())
        }
    })
    .await
    .map_err(|_| "无法访问 RTC 钥匙串".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_accepts_a_separate_device_port_but_not_another_ip() {
        let control = "192.0.2.10:40000".parse().unwrap();
        assert!(same_device("192.0.2.10:40001".parse().unwrap(), control));
        assert!(same_device(control, control));
        assert!(!same_device("192.0.2.11:40000".parse().unwrap(), control));
    }
    #[test]
    fn udp_sequence_rejects_replays_and_accepts_wrap() {
        assert!(accept_sequence(None, 8));
        assert!(accept_sequence(Some(8), 9));
        assert!(!accept_sequence(Some(8), 8));
        assert!(!accept_sequence(Some(8), 7));
        assert!(accept_sequence(Some(u32::MAX), 0));
    }
}

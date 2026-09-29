use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG_WRITE: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeDiagnosticsInfo {
    pub path: String,
    pub bytes: u64,
}

pub fn init(root: &Path) -> Result<(), String> {
    let directory = root.join("logs");
    fs::create_dir_all(&directory).map_err(|error| format!("无法创建诊断日志目录：{error}"))?;
    let _ = LOG_PATH.set(directory.join("realtime.jsonl"));
    Ok(())
}

fn current_path() -> Result<&'static PathBuf, String> {
    LOG_PATH
        .get()
        .ok_or_else(|| "实时语音诊断日志尚未初始化".to_string())
}

fn rotate(path: &Path) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() < MAX_LOG_BYTES {
        return;
    }
    let backup = PathBuf::from(format!("{}.1", path.to_string_lossy()));
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    let _ = fs::rename(path, backup);
}

pub fn realtime(session_id: Option<&str>, event: &str, fields: Value) {
    let Ok(path) = current_path() else { return };
    let Ok(_guard) = LOG_WRITE.lock() else { return };
    rotate(path);
    let record = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "component": "realtime",
        "event": event,
        "sessionId": session_id,
        "fields": fields
    });
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    if let Ok(line) = serde_json::to_string(&record) {
        let _ = writeln!(file, "{line}");
    }
}

pub fn text_preview(text: &str) -> Value {
    const LIMIT: usize = 240;
    let characters = text.chars().count();
    let preview = text.chars().take(LIMIT).collect::<String>();
    serde_json::json!({"preview":preview,"characters":characters,"truncated":characters>LIMIT})
}

pub fn info() -> Result<RealtimeDiagnosticsInfo, String> {
    let path = current_path()?;
    let bytes = fs::metadata(path).map(|value| value.len()).unwrap_or(0);
    Ok(RealtimeDiagnosticsInfo {
        path: path.to_string_lossy().into_owned(),
        bytes,
    })
}

pub fn export(destination: &Path) -> Result<RealtimeDiagnosticsInfo, String> {
    let source = current_path()?;
    if !source.exists() {
        return Err("还没有实时语音诊断日志，请先开始一次通话".into());
    }
    if destination.as_os_str().is_empty() {
        return Err("导出路径为空".into());
    }
    fs::copy(source, destination).map_err(|error| format!("导出诊断日志失败：{error}"))?;
    let bytes = fs::metadata(destination)
        .map_err(|error| error.to_string())?
        .len();
    Ok(RealtimeDiagnosticsInfo {
        path: destination.to_string_lossy().into_owned(),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_transcript_preview_without_losing_length() {
        let value = text_preview(&"剪".repeat(300));
        assert_eq!(value["characters"], 300);
        assert_eq!(value["preview"].as_str().unwrap().chars().count(), 240);
        assert_eq!(value["truncated"], true);
    }
}

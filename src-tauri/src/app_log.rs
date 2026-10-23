use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use chrono::Local;
use tauri::AppHandle;

use crate::config::data_dir;

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
static LOG_LOCK: Mutex<()> = Mutex::new(());

pub fn log_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("logs").join("s-tools.log"))
}

pub fn info(app: &AppHandle, scope: &str, message: impl AsRef<str>) {
    let _ = write(app, "INFO", scope, message.as_ref());
}

pub fn warn(app: &AppHandle, scope: &str, message: impl AsRef<str>) {
    let _ = write(app, "WARN", scope, message.as_ref());
}

pub fn error(app: &AppHandle, scope: &str, message: impl AsRef<str>) {
    let _ = write(app, "ERROR", scope, message.as_ref());
}

pub fn ensure_log_file(app: &AppHandle) -> Result<PathBuf, String> {
    let path = log_path(app)?;
    ensure_log_file_at(&path)?;
    Ok(path)
}

fn write(app: &AppHandle, level: &str, scope: &str, message: &str) -> Result<(), String> {
    let path = log_path(app)?;
    write_to_path(&path, level, scope, message)
}

fn write_to_path(path: &Path, level: &str, scope: &str, message: &str) -> Result<(), String> {
    let _guard = LOG_LOCK.lock().map_err(|_| "日志锁已损坏".to_string())?;
    ensure_log_file_at(path)?;
    rotate_if_needed(path)?;

    let normalized = message
        .replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let line = format!(
        "{} [{}] [{}] {}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        level,
        scope,
        normalized
    );
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("打开日志文件失败：{error}"))?;
    file.write_all(line.as_bytes())
        .map_err(|error| format!("写入日志文件失败：{error}"))
}

fn ensure_log_file_at(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建日志目录失败：{error}"))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(|_| ())
        .map_err(|error| format!("创建日志文件失败：{error}"))
}

fn rotate_if_needed(path: &Path) -> Result<(), String> {
    let size = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if size < MAX_LOG_BYTES {
        return Ok(());
    }
    let previous = path.with_extension("log.1");
    if previous.exists() {
        fs::remove_file(&previous).map_err(|error| format!("删除旧日志失败：{error}"))?;
    }
    fs::rename(path, &previous).map_err(|error| format!("轮换日志失败：{error}"))
}

pub fn open(app: &AppHandle) -> Result<(), String> {
    let path = ensure_log_file(app)?;
    crate::updater::reveal_path(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn writes_normalized_lines_to_the_shared_log() {
        let path = std::env::temp_dir().join(format!("stools-log-{}.log", Uuid::new_v4()));
        write_to_path(&path, "ERROR", "capture", "line one\nline two").expect("write log");
        let content = fs::read_to_string(&path).expect("read log");
        assert!(content.contains("[ERROR] [capture] line one line two"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rotates_logs_at_the_size_limit() {
        let path = std::env::temp_dir().join(format!("stools-log-{}.log", Uuid::new_v4()));
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).expect("seed log");
        write_to_path(&path, "INFO", "app", "rotated").expect("write rotated log");
        let previous = path.with_extension("log.1");
        assert!(previous.exists());
        assert!(fs::read_to_string(&path)
            .expect("read current log")
            .contains("rotated"));
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(previous);
    }
}

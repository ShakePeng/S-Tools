use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use reqwest::{Client, StatusCode};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{ipc::Channel, AppHandle, Manager};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::models::{UpdateDownloadEvent, UpdateInfo, UpdateInstallResult};

const MANIFEST_V2_URL: &str =
    "https://github.com/ShakePeng/S-Tools/releases/latest/download/latest-v2.json";
const MANIFEST_LEGACY_URL: &str =
    "https://github.com/ShakePeng/S-Tools/releases/latest/download/latest.json";
const RELEASE_DOWNLOAD_BASE: &str = "https://github.com/ShakePeng/S-Tools/releases/download";

#[derive(Debug, Deserialize)]
struct ManifestV2 {
    version: String,
    tag: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    assets: HashMap<String, AssetV2>,
    #[serde(default)]
    manual: HashMap<String, String>,
    #[serde(default, alias = "manualSizes")]
    manual_sizes: HashMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct AssetV2 {
    file: String,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ManifestLegacy {
    version: String,
    tag: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    assets: HashMap<String, String>,
    #[serde(default)]
    sizes: HashMap<String, u64>,
}

fn update_client(total_timeout: Option<Duration>) -> Result<Client, String> {
    let mut builder = Client::builder()
        .user_agent("S-Tools")
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60));
    if let Some(timeout) = total_timeout {
        builder = builder.timeout(timeout);
    }
    builder
        .build()
        .map_err(|error| format!("初始化更新客户端失败：{error}"))
}

pub async fn check_update(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let client = update_client(Some(Duration::from_secs(20)))?;
    let response = client
        .get(MANIFEST_V2_URL)
        .send()
        .await
        .map_err(|error| format!("请求更新信息失败：{error}"))?;
    let current = app.package_info().version.to_string();
    if response.status().is_success() {
        let manifest: ManifestV2 = response
            .json()
            .await
            .map_err(|error| format!("解析更新信息失败：{error}"))?;
        return Ok(update_from_v2(&manifest, &current));
    }
    if response.status() != StatusCode::NOT_FOUND {
        return Err(format!(
            "请求更新信息失败：HTTP {}",
            response.status().as_u16()
        ));
    }

    let response = client
        .get(MANIFEST_LEGACY_URL)
        .send()
        .await
        .map_err(|error| format!("请求更新信息失败：{error}"))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err("未找到更新清单，请前往 GitHub Release 手动下载".into());
    }
    let manifest: ManifestLegacy = response
        .error_for_status()
        .map_err(|error| format!("请求更新信息失败：{error}"))?
        .json()
        .await
        .map_err(|error| format!("解析更新信息失败：{error}"))?;
    Ok(update_from_legacy(&manifest, &current))
}

fn update_from_v2(manifest: &ManifestV2, current: &str) -> Option<UpdateInfo> {
    if !is_newer_version(&manifest.version, current) {
        return None;
    }
    let platform = platform_key()?;
    let tag = manifest_tag(&manifest.tag, &manifest.version);
    let manual_file = manifest.manual.get(platform);
    let asset = manifest.assets.get(platform);
    let (download_url, file_name, download_size, sha256, self_update) =
        if let Some(asset) = asset.filter(|asset| is_valid_sha256(&asset.sha256)) {
            (
                release_url(&tag, &asset.file),
                asset.file.clone(),
                asset.size,
                Some(asset.sha256.clone()),
                true,
            )
        } else {
            let file = manual_file?;
            (
                release_url(&tag, file),
                file.clone(),
                manifest.manual_sizes.get(platform).copied(),
                None,
                false,
            )
        };
    Some(UpdateInfo {
        version: manifest
            .version
            .trim()
            .trim_start_matches(['v', 'V'])
            .to_string(),
        notes: manifest.notes.clone(),
        download_url,
        file_name,
        download_size,
        sha256,
        self_update,
        manual_download_url: manual_file.map(|file| release_url(&tag, file)),
        manual_file_name: manual_file.cloned(),
        manual_download_size: manual_file
            .and_then(|_| manifest.manual_sizes.get(platform).copied()),
    })
}

fn update_from_legacy(manifest: &ManifestLegacy, current: &str) -> Option<UpdateInfo> {
    if !is_newer_version(&manifest.version, current) {
        return None;
    }
    let platform = platform_key()?;
    let file = manifest.assets.get(platform)?;
    let tag = manifest_tag(&manifest.tag, &manifest.version);
    Some(UpdateInfo {
        version: manifest
            .version
            .trim()
            .trim_start_matches(['v', 'V'])
            .to_string(),
        notes: manifest.notes.clone(),
        download_url: release_url(&tag, file),
        file_name: file.clone(),
        download_size: manifest.sizes.get(platform).copied(),
        sha256: None,
        self_update: false,
        manual_download_url: Some(release_url(&tag, file)),
        manual_file_name: Some(file.clone()),
        manual_download_size: manifest.sizes.get(platform).copied(),
    })
}

fn platform_key() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("windows-x64")
    } else if cfg!(target_os = "macos") {
        Some("macos-aarch64")
    } else {
        None
    }
}

fn release_url(tag: &str, file_name: &str) -> String {
    format!("{RELEASE_DOWNLOAD_BASE}/{tag}/{file_name}")
}

fn manifest_tag(tag: &str, version: &str) -> String {
    if tag.trim().is_empty() {
        format!("v{}", version.trim())
    } else {
        tag.trim().to_string()
    }
}

fn is_valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn is_newer_version(remote: &str, current: &str) -> bool {
    let parse = |value: &str| -> Vec<u64> {
        value
            .trim()
            .trim_start_matches(['v', 'V'])
            .split('.')
            .map(|part| {
                part.split(['-', '+'])
                    .next()
                    .unwrap_or("0")
                    .trim()
                    .parse::<u64>()
                    .unwrap_or(0)
            })
            .collect()
    };
    let remote_parts = parse(remote);
    let current_parts = parse(current);
    for index in 0..remote_parts.len().max(current_parts.len()) {
        let remote_part = remote_parts.get(index).copied().unwrap_or(0);
        let current_part = current_parts.get(index).copied().unwrap_or(0);
        if remote_part != current_part {
            return remote_part > current_part;
        }
    }
    false
}

pub async fn download_update(
    app: &AppHandle,
    url: String,
    file_name: String,
    on_event: Channel<UpdateDownloadEvent>,
    cancellation: &CancellationToken,
) -> Result<String, String> {
    let safe_name = safe_file_name(&file_name)?;
    let directory = app
        .path()
        .download_dir()
        .map_err(|error| format!("定位下载目录失败：{error}"))?;
    let target = directory.join(safe_name);
    download_to_file(&url, &target, None, &on_event, cancellation).await?;
    if cancellation.is_cancelled() {
        let _ = tokio::fs::remove_file(&target).await;
        return Err("更新下载已取消".into());
    }
    Ok(target.to_string_lossy().to_string())
}

pub async fn install_update(
    app: &AppHandle,
    url: String,
    file_name: String,
    sha256: Option<String>,
    on_event: Channel<UpdateDownloadEvent>,
    cancellation: &CancellationToken,
) -> Result<UpdateInstallResult, String> {
    let sha256 = sha256.ok_or_else(|| "更新清单缺少 SHA256，无法安全安装".to_string())?;
    if !is_valid_sha256(&sha256) {
        return Err("更新清单包含无效 SHA256".into());
    }
    #[cfg(target_os = "macos")]
    let app_bundle = match macos_app_bundle_path() {
        Ok(path) => path,
        Err(_) => {
            return Ok(UpdateInstallResult {
                installed: false,
                manual_required: true,
                saved_path: None,
            });
        }
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    return Ok(UpdateInstallResult {
        installed: false,
        manual_required: true,
        saved_path: None,
    });

    let safe_name = safe_file_name(&file_name)?;
    let temporary = std::env::temp_dir().join(format!("s-tools-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temporary);
    std::fs::create_dir_all(&temporary)
        .map_err(|error| format!("创建更新临时目录失败：{error}"))?;
    let target = temporary.join(safe_name);
    download_to_file(&url, &target, Some(&sha256), &on_event, cancellation).await?;
    if cancellation.is_cancelled() {
        let _ = tokio::fs::remove_file(&target).await;
        return Err("更新下载已取消".into());
    }

    #[cfg(target_os = "windows")]
    {
        Command::new(&target)
            .args(["/S", "/R"])
            .spawn()
            .map_err(|error| format!("启动 Windows 安装包失败：{error}"))?;
    }

    #[cfg(target_os = "macos")]
    {
        let script_path = temporary.join("install-update.sh");
        std::fs::write(&script_path, MACOS_UPDATE_SCRIPT)
            .map_err(|error| format!("创建 macOS 更新脚本失败：{error}"))?;
        Command::new("/bin/bash")
            .arg(&script_path)
            .arg(&target)
            .arg(&app_bundle)
            .arg(std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("启动 macOS 更新脚本失败：{error}"))?;
    }

    on_event
        .send(UpdateDownloadEvent::Installing)
        .map_err(|error| format!("发送安装状态失败：{error}"))?;
    schedule_exit(app.clone());
    Ok(UpdateInstallResult {
        installed: true,
        manual_required: false,
        saved_path: Some(target.to_string_lossy().to_string()),
    })
}

fn schedule_exit(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        app.exit(0);
    });
}

async fn download_to_file(
    url: &str,
    target: &Path,
    expected_sha256: Option<&str>,
    on_event: &Channel<UpdateDownloadEvent>,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    let client = update_client(None)?;
    let mut response = tokio::select! {
        _ = cancellation.cancelled() => return Err("更新下载已取消".into()),
        result = client.get(url).send() => result
            .map_err(|error| format!("下载更新失败：{error}"))?
            .error_for_status()
            .map_err(|error| format!("下载更新失败：{error}"))?,
    };
    let content_length = response.content_length();
    on_event
        .send(UpdateDownloadEvent::Started { content_length })
        .map_err(|error| format!("发送下载进度失败：{error}"))?;
    let partial = target.with_extension(format!(
        "{}.part",
        target
            .extension()
            .map(|value| value.to_string_lossy())
            .unwrap_or_default()
    ));
    let mut file = tokio::fs::File::create(&partial)
        .await
        .map_err(|error| format!("创建更新文件失败：{error}"))?;
    let mut hasher = expected_sha256.map(|_| Sha256::new());
    let mut downloaded = 0_u64;
    loop {
        let chunk = tokio::select! {
            _ = cancellation.cancelled() => {
                drop(file);
                let _ = tokio::fs::remove_file(&partial).await;
                return Err("更新下载已取消".into());
            }
            result = response.chunk() => result
                .map_err(|error| format!("下载更新失败：{error}"))?,
        };
        let Some(chunk) = chunk else {
            break;
        };
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("写入更新文件失败：{error}"))?;
        if let Some(hasher) = hasher.as_mut() {
            hasher.update(&chunk);
        }
        downloaded += chunk.len() as u64;
        on_event
            .send(UpdateDownloadEvent::Progress {
                downloaded,
                content_length,
            })
            .map_err(|error| format!("发送下载进度失败：{error}"))?;
    }
    file.flush()
        .await
        .map_err(|error| format!("写入更新文件失败：{error}"))?;
    drop(file);
    if let (Some(expected), Some(hasher)) = (expected_sha256, hasher) {
        if cancellation.is_cancelled() {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err("更新下载已取消".into());
        }
        on_event
            .send(UpdateDownloadEvent::Verifying)
            .map_err(|error| format!("发送校验状态失败：{error}"))?;
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected.trim()) {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err("更新包 SHA256 校验失败".into());
        }
    }
    if target.exists() {
        tokio::fs::remove_file(target)
            .await
            .map_err(|error| format!("替换旧更新文件失败：{error}"))?;
    }
    tokio::fs::rename(&partial, target)
        .await
        .map_err(|error| format!("保存更新文件失败：{error}"))?;
    if cancellation.is_cancelled() {
        let _ = tokio::fs::remove_file(target).await;
        return Err("更新下载已取消".into());
    }
    on_event
        .send(UpdateDownloadEvent::Finished)
        .map_err(|error| format!("发送下载进度失败：{error}"))?;
    Ok(())
}

fn safe_file_name(file_name: &str) -> Result<String, String> {
    Path::new(file_name)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "更新文件名无效".into())
}

#[cfg(target_os = "macos")]
fn macos_app_bundle_path() -> Result<PathBuf, String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("定位当前应用失败：{error}"))?;
    let bundle = executable
        .ancestors()
        .find(|part| part.extension().map(|ext| ext == "app").unwrap_or(false))
        .map(Path::to_path_buf)
        .ok_or_else(|| "当前应用不在 .app 包中，无法自动更新".to_string())?;
    let text = bundle.to_string_lossy();
    if text.contains("AppTranslocation") || bundle.starts_with("/Volumes") {
        return Err("当前应用从临时位置运行，无法自动更新".into());
    }
    let parent = bundle
        .parent()
        .ok_or_else(|| "无法定位应用安装目录".to_string())?;
    let probe = parent.join(format!(".stools-update-probe-{}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|_| "当前安装目录不可写，无法自动更新".to_string())?;
    let _ = std::fs::remove_file(probe);
    Ok(bundle)
}

#[cfg(target_os = "macos")]
const MACOS_UPDATE_SCRIPT: &str = r#"#!/bin/bash
set -u

ZIP="$1"
APP="$2"
PID="$3"
LOG="$HOME/Library/Logs/S-Tools-update.log"
mkdir -p "$(dirname "$LOG")"
exec >>"$LOG" 2>&1

echo "[$(date)] waiting for app $PID"
while kill -0 "$PID" 2>/dev/null; do
  sleep 0.2
done
sleep 0.3

WORK="$(mktemp -d "${TMPDIR:-/tmp}/stools-update.XXXXXX")" || exit 1
BACKUP="${APP}.bak.$$"
mkdir -p "$WORK/unpack"

restore_old() {
  rm -rf "$APP"
  if [ -d "$BACKUP" ]; then
    mv "$BACKUP" "$APP"
  fi
  /usr/bin/open "$APP" >/dev/null 2>&1 || true
}

if ! mv "$APP" "$BACKUP"; then
  /usr/bin/open "$APP" >/dev/null 2>&1 || true
  exit 1
fi

if ! /usr/bin/ditto -x -k "$ZIP" "$WORK/unpack"; then
  restore_old
  exit 1
fi

NEW_APP="$WORK/unpack/S-Tools.app"
if [ ! -d "$NEW_APP" ]; then
  restore_old
  exit 1
fi

if ! mv "$NEW_APP" "$APP"; then
  restore_old
  exit 1
fi

/usr/bin/xattr -dr com.apple.quarantine "$APP" >/dev/null 2>&1 || true
/usr/bin/open "$APP" >/dev/null 2>&1 || true
rm -rf "$BACKUP" "$WORK" "$ZIP"
rm -f "$0"
"#;

pub fn reveal_path(path: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn()
            .map_err(|error| format!("打开文件位置失败：{error}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|error| format!("打开文件位置失败：{error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use uuid::Uuid;

    #[test]
    fn compares_versions() {
        assert!(is_newer_version("1.0.1", "1.0.0"));
        assert!(!is_newer_version("1.0.0", "1.0.0"));
        assert!(is_newer_version("v1.1", "1.0.9"));
    }

    #[test]
    fn validates_sha() {
        assert!(is_valid_sha256(&"a".repeat(64)));
        assert!(!is_valid_sha256("abc"));
    }

    #[test]
    fn parses_v2_manifest_sizes_and_bounds() {
        let manifest: ManifestV2 = serde_json::from_str(
            r#"{
                "version": "1.1.0",
                "tag": "v1.1.0",
                "assets": {
                    "macos-aarch64": {
                        "file": "S-Tools.app.zip",
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "size": 2048
                    },
                    "windows-x64": {
                        "file": "S-Tools-setup.exe",
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "size": 2048
                    }
                },
                "manualSizes": {
                    "macos-aarch64": 4096,
                    "windows-x64": 4096
                }
            }"#,
        )
        .expect("parse v2");
        let info = update_from_v2(&manifest, "1.0.11").expect("newer update");
        assert_eq!(info.download_size, Some(2048));
        assert_eq!(info.manual_download_size, None);
        assert!(info.self_update);
    }

    #[test]
    fn manual_only_v2_manifest_uses_manual_size() {
        let manifest: ManifestV2 = serde_json::from_str(
            r#"{
                "version": "1.1.0",
                "tag": "v1.1.0",
                "assets": {},
                "manual": {
                    "macos-aarch64": "S-Tools.dmg",
                    "windows-x64": "S-Tools-setup.exe"
                },
                "manualSizes": {
                    "macos-aarch64": 4096,
                    "windows-x64": 4096
                }
            }"#,
        )
        .expect("parse manual v2");
        let info = update_from_v2(&manifest, "1.0.11").expect("newer update");
        assert!(!info.self_update);
        assert_eq!(info.download_size, Some(4096));
        assert_eq!(info.manual_download_size, Some(4096));
    }

    #[test]
    fn legacy_manifest_without_size_still_parses() {
        let manifest: ManifestLegacy = serde_json::from_str(
            r#"{
                "version": "1.1.0",
                "tag": "v1.1.0",
                "assets": {
                    "macos-aarch64": "S-Tools.dmg",
                    "windows-x64": "S-Tools-setup.exe"
                }
            }"#,
        )
        .expect("parse legacy");
        let info = update_from_legacy(&manifest, "1.0.11").expect("newer update");
        assert_eq!(info.download_size, None);
        assert_eq!(info.manual_download_size, None);
    }

    #[test]
    fn legacy_manifest_uses_available_size() {
        let manifest: ManifestLegacy = serde_json::from_str(
            r#"{
                "version": "1.1.0",
                "tag": "v1.1.0",
                "assets": {
                    "macos-aarch64": "S-Tools.dmg",
                    "windows-x64": "S-Tools-setup.exe"
                },
                "sizes": {
                    "macos-aarch64": 3072,
                    "windows-x64": 3072
                }
            }"#,
        )
        .expect("parse legacy");
        let info = update_from_legacy(&manifest, "1.0.11").expect("newer update");
        assert_eq!(info.download_size, Some(3072));
        assert_eq!(info.manual_download_size, Some(3072));
    }

    async fn serve_once(response: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            stream.write_all(&response).await.expect("write response");
            let _ = stream.shutdown().await;
        });
        (format!("http://{address}/update"), task)
    }

    fn event_channel(
        events: Arc<Mutex<Vec<UpdateDownloadEvent>>>,
        on_progress: Option<CancellationToken>,
    ) -> Channel<UpdateDownloadEvent> {
        Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                let event: UpdateDownloadEvent =
                    serde_json::from_str(&json).expect("deserialize progress event");
                if matches!(event, UpdateDownloadEvent::Progress { .. }) {
                    if let Some(token) = &on_progress {
                        token.cancel();
                    }
                }
                events.lock().expect("events lock").push(event);
            }
            Ok(())
        })
    }

    #[tokio::test]
    async fn download_reports_cumulative_progress_and_verification() {
        let (url, server) = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello"
                .to_vec(),
        )
        .await;
        let target = std::env::temp_dir().join(format!(
            "stools-update-progress-{}.zip",
            Uuid::new_v4()
        ));
        let events = Arc::new(Mutex::new(Vec::new()));
        let channel = event_channel(events.clone(), None);
        let expected = hex::encode(Sha256::digest(b"hello"));
        let cancellation = CancellationToken::new();

        download_to_file(
            &url,
            &target,
            Some(&expected),
            &channel,
            &cancellation,
        )
        .await
        .expect("download update");
        server.await.expect("server task");

        let events = events.lock().expect("events lock");
        assert!(matches!(
            events.first(),
            Some(UpdateDownloadEvent::Started {
                content_length: Some(5)
            })
        ));
        assert!(events.iter().any(|event| matches!(
            event,
            UpdateDownloadEvent::Progress {
                downloaded: 5,
                content_length: Some(5)
            }
        )));
        assert!(events
            .iter()
            .any(|event| matches!(event, UpdateDownloadEvent::Verifying)));
        assert!(matches!(
            events.last(),
            Some(UpdateDownloadEvent::Finished)
        ));
        drop(events);
        let _ = tokio::fs::remove_file(target).await;
    }

    #[tokio::test]
    async fn download_reports_bytes_when_content_length_is_missing() {
        let (url, server) = serve_once(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n"
                .to_vec(),
        )
        .await;
        let target = std::env::temp_dir().join(format!(
            "stools-update-unknown-{}.zip",
            Uuid::new_v4()
        ));
        let events = Arc::new(Mutex::new(Vec::new()));
        let channel = event_channel(events.clone(), None);
        let cancellation = CancellationToken::new();

        download_to_file(&url, &target, None, &channel, &cancellation)
            .await
            .expect("download update");
        server.await.expect("server task");

        let events = events.lock().expect("events lock");
        assert!(matches!(
            events.first(),
            Some(UpdateDownloadEvent::Started {
                content_length: None
            })
        ));
        assert!(events.iter().any(|event| matches!(
            event,
            UpdateDownloadEvent::Progress {
                downloaded: 5,
                content_length: None
            }
        )));
        drop(events);
        let _ = tokio::fs::remove_file(target).await;
    }

    #[tokio::test]
    async fn cancelling_download_removes_partial_file() {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n",
                )
                .await
                .expect("write headers");
            stream.flush().await.expect("flush response");
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let target = std::env::temp_dir().join(format!(
            "stools-update-cancel-{}.zip",
            Uuid::new_v4()
        ));
        let partial = target.with_extension("zip.part");
        let events = Arc::new(Mutex::new(Vec::new()));
        let cancellation = CancellationToken::new();
        let channel = event_channel(events.clone(), Some(cancellation.clone()));

        let result = download_to_file(
            &format!("http://{address}/update"),
            &target,
            None,
            &channel,
            &cancellation,
        )
        .await;
        server.abort();

        assert!(result.expect_err("download cancelled").contains("已取消"));
        assert!(!partial.exists());
        assert!(!target.exists());
    }
}

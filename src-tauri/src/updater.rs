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
}

#[derive(Debug, Deserialize)]
struct AssetV2 {
    file: String,
    #[serde(default)]
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct ManifestLegacy {
    version: String,
    tag: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    assets: HashMap<String, String>,
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
    let (download_url, file_name, sha256, self_update) =
        if let Some(asset) = asset.filter(|asset| is_valid_sha256(&asset.sha256)) {
            (
                release_url(&tag, &asset.file),
                asset.file.clone(),
                Some(asset.sha256.clone()),
                true,
            )
        } else {
            let file = manual_file?;
            (release_url(&tag, file), file.clone(), None, false)
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
        sha256,
        self_update,
        manual_download_url: manual_file.map(|file| release_url(&tag, file)),
        manual_file_name: manual_file.cloned(),
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
        sha256: None,
        self_update: false,
        manual_download_url: Some(release_url(&tag, file)),
        manual_file_name: Some(file.clone()),
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
) -> Result<String, String> {
    let safe_name = safe_file_name(&file_name)?;
    let directory = app
        .path()
        .download_dir()
        .map_err(|error| format!("定位下载目录失败：{error}"))?;
    let target = directory.join(safe_name);
    download_to_file(&url, &target, None, &on_event).await?;
    Ok(target.to_string_lossy().to_string())
}

pub async fn install_update(
    app: &AppHandle,
    url: String,
    file_name: String,
    sha256: Option<String>,
    on_event: Channel<UpdateDownloadEvent>,
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
    download_to_file(&url, &target, Some(&sha256), &on_event).await?;

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
) -> Result<(), String> {
    let client = update_client(None)?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("下载更新失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("下载更新失败：{error}"))?;
    on_event
        .send(UpdateDownloadEvent::Started {
            content_length: response.content_length(),
        })
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
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("下载更新失败：{error}"))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("写入更新文件失败：{error}"))?;
        if let Some(hasher) = hasher.as_mut() {
            hasher.update(&chunk);
        }
        on_event
            .send(UpdateDownloadEvent::Progress {
                chunk_length: chunk.len() as u64,
            })
            .map_err(|error| format!("发送下载进度失败：{error}"))?;
    }
    file.flush()
        .await
        .map_err(|error| format!("写入更新文件失败：{error}"))?;
    drop(file);
    if let (Some(expected), Some(hasher)) = (expected_sha256, hasher) {
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
}

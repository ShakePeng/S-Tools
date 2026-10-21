use std::{
    fs,
    path::{Path, PathBuf},
};

use tauri::{AppHandle, Manager};

use crate::models::AppConfig;

pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))
}

pub fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("config.json"))
}

pub fn load_config(app: &AppHandle) -> Result<AppConfig, String> {
    let path = config_path(app)?;
    if !path.exists() {
        let config = AppConfig::default();
        save_config(app, &config)?;
        return Ok(config);
    }
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("读取配置失败（{}）：{error}", path.display()))?;
    let mut config: AppConfig = serde_json::from_str(&content)
        .map_err(|error| format!("解析配置失败（{}）：{error}", path.display()))?;
    migrate_provider_status(&mut config);
    Ok(config)
}

pub fn save_config(app: &AppHandle, config: &AppConfig) -> Result<(), String> {
    let dir = data_dir(app)?;
    fs::create_dir_all(&dir).map_err(|error| format!("创建应用数据目录失败：{error}"))?;
    let path = dir.join("config.json");
    write_json_atomic(&path, config)
}

fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|error| format!("序列化配置失败：{error}"))?;
    fs::write(&temporary, &bytes)
        .map_err(|error| format!("写入临时配置失败（{}）：{error}", temporary.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600));
    }
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("替换旧配置失败：{error}"))?;
    }
    fs::rename(&temporary, path)
        .map_err(|error| format!("提交配置失败（{}）：{error}", path.display()))
}

fn migrate_provider_status(config: &mut AppConfig) {
    // 旧配置没有 credentials/configured 字段；凭据已不在钥匙串，需要重新填写一次。
    for provider in &mut config.providers {
        if provider.credentials.is_empty() && provider.enabled {
            provider.configured = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_legacy_provider_requires_credentials_again() {
        let mut config = AppConfig::default();
        config.providers[0].enabled = true;
        config.providers[0].configured = true;
        migrate_provider_status(&mut config);
        assert!(!config.providers[0].configured);
        assert!(!config.providers[1].configured);
    }
}

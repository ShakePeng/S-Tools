use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeySettings {
    pub translate: String,
    pub screenshot: String,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            translate: "Alt+D".into(),
            screenshot: "F1".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub schema_version: u32,
    pub hotkeys: HotkeySettings,
    pub translation_enabled: bool,
    pub screenshot_enabled: bool,
    pub target_lang: String,
    pub launch_at_login: bool,
    pub has_completed_onboarding: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            hotkeys: HotkeySettings::default(),
            translation_enabled: false,
            screenshot_enabled: false,
            target_lang: "auto".into(),
            launch_at_login: false,
            has_completed_onboarding: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub configured: bool,
    pub model: String,
    pub base_url: String,
    pub description: String,
    pub credentials: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub schema_version: u32,
    pub settings: AppSettings,
    pub providers: Vec<ProviderConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            settings: AppSettings::default(),
            providers: vec![
                ProviderConfig {
                    id: "baidu".into(),
                    name: "百度翻译".into(),
                    kind: "baidu".into(),
                    enabled: false,
                    configured: false,
                    model: String::new(),
                    base_url: String::new(),
                    description: "百度翻译开放平台，适合中文语境。".into(),
                    credentials: HashMap::new(),
                },
                ProviderConfig {
                    id: "google".into(),
                    name: "Google 翻译".into(),
                    kind: "google".into(),
                    enabled: false,
                    configured: false,
                    model: String::new(),
                    base_url: String::new(),
                    description: "Google Cloud Translation，读取系统代理。".into(),
                    credentials: HashMap::new(),
                },
                ProviderConfig {
                    id: "tencent".into(),
                    name: "腾讯云翻译".into(),
                    kind: "tencent".into(),
                    enabled: false,
                    configured: false,
                    model: String::new(),
                    base_url: String::new(),
                    description: "腾讯云机器翻译，支持 TC3 签名。".into(),
                    credentials: HashMap::new(),
                },
                ProviderConfig {
                    id: "deepseek".into(),
                    name: "DeepSeek".into(),
                    kind: "deepseek".into(),
                    enabled: false,
                    configured: false,
                    model: "deepseek-chat".into(),
                    base_url: "https://api.deepseek.com".into(),
                    description: "更自然的上下文翻译与语气润色。".into(),
                    credentials: HashMap::new(),
                },
                ProviderConfig {
                    id: "openai".into(),
                    name: "OpenAI 兼容".into(),
                    kind: "openai-compatible".into(),
                    enabled: false,
                    configured: false,
                    model: String::new(),
                    base_url: String::new(),
                    description: "接入任意兼容 Chat Completions 的服务。".into(),
                    credentials: HashMap::new(),
                },
            ],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub configured: bool,
    pub model: String,
    pub base_url: String,
    pub description: String,
    pub credentials: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub credentials: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationRequest {
    pub request_id: String,
    pub provider_id: String,
    pub text: String,
    pub source_lang: String,
    pub target_lang: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderResult {
    pub request_id: String,
    pub provider_id: String,
    pub translated_text: String,
    pub detected_lang: String,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingTranslation {
    pub text: String,
    pub source: String,
    pub notice: Option<String>,
}

impl Default for PendingTranslation {
    fn default() -> Self {
        Self {
            text: String::new(),
            source: "empty".into(),
            notice: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub accessibility: bool,
    pub screen_recording: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSession {
    pub session_id: String,
    pub monitor_id: u32,
    pub display_id: u32,
    pub image_path: String,
    pub position_x: f64,
    pub position_y: f64,
    pub width: f64,
    pub height: f64,
    pub scale_factor: f64,
    pub screen_frame: Rect,
    pub window_rects: Vec<Rect>,
    pub backing_scale: f64,
    pub foreground_app_id: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteCaptureRequest {
    pub session_id: String,
    pub data_url: String,
    pub action: String,
    #[serde(default)]
    pub save_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteCaptureResult {
    pub path: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapData {
    pub version: String,
    pub settings: AppSettings,
    pub providers: Vec<ProviderView>,
    pub permissions: PermissionStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub download_url: String,
    pub file_name: String,
    pub sha256: Option<String>,
    pub self_update: bool,
    pub manual_download_url: Option<String>,
    pub manual_file_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstallResult {
    pub installed: bool,
    pub manual_required: bool,
    pub saved_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "event")]
pub enum UpdateDownloadEvent {
    #[serde(rename = "Started")]
    Started { content_length: Option<u64> },
    #[serde(rename = "Progress")]
    Progress { chunk_length: u64 },
    #[serde(rename = "Finished")]
    Finished,
}

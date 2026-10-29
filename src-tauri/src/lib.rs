mod app_log;
mod capture;
mod config;
mod maintenance;
mod models;
mod platform;
mod selection;
mod translate;
mod updater;

use std::{
    collections::{HashMap, HashSet},
    process::Command,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    time::{Duration, Instant},
};

use models::{
    AppConfig, AppSettings, BootstrapData, CaptureSession, CompleteCaptureRequest,
    CompleteCaptureResult, PendingTranslation, PermissionStatus, ProviderInput, ProviderResult,
    ProviderView, TranslationRequest, UpdateDownloadEvent, UpdateInfo, UpdateInstallResult,
};
use tauri::{
    image::Image,
    ipc::Channel,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Runtime, State, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tokio_util::sync::CancellationToken;

const TRAY_ID: &str = "s-tools";
const AUTOSTART_ARG: &str = "--startup";
const TRANSLATION_WINDOW_WIDTH: f64 = 300.0;
const TRANSLATION_WINDOW_DEFAULT_HEIGHT: f64 = 300.0;
const TRANSLATION_WINDOW_MIN_HEIGHT: f64 = 220.0;
const TRANSLATION_WINDOW_MAX_HEIGHT: f64 = 560.0;
// macOS 显示未激活 App 的窗口时，AppKit 会先派发一次伪失焦事件；
// 宽限期用于吞掉这次事件，避免弹框刚出现就被失焦逻辑关掉。
const TRANSLATION_FOCUS_GRACE: Duration = Duration::from_millis(300);
// 显示后延迟复查焦点，兜底处理 makeKeyAndOrderFront 未生效的情况。
const TRANSLATION_FOCUS_RECHECK: Duration = Duration::from_millis(450);
// 与截图关闭后的恢复节奏保持一致。
const TRANSLATION_RESTORE_DELAY: Duration = Duration::from_millis(80);
const TRANSLATION_LOG_SCOPE: &str = "translate";
const RELEASES_URL: &str = "https://github.com/ShakePeng/S-Tools/releases";

pub struct AppState {
    config: RwLock<AppConfig>,
    sessions: Mutex<HashMap<String, CaptureSession>>,
    capture_ready: Arc<Mutex<HashSet<String>>>,
    capture_guard: capture::CaptureGuard,
    pending: Mutex<PendingTranslation>,
    /// 本轮弹框显示时刻，用于区分「显示瞬间的伪失焦」与用户真正的点击外部。
    translation_shown_at: Mutex<Option<Instant>>,
    /// 触发翻译时的前台应用 pid，关闭弹框后用它把焦点还回去。
    translation_source_app: Mutex<Option<u32>>,
    /// 本轮显示是否真正获得过焦点；获得过就不再补聚焦，避免抢回用户主动切走的焦点。
    translation_focus_seen: AtomicBool,
    cancellations: Mutex<HashMap<String, CancellationToken>>,
    update_cancellation: Mutex<Option<CancellationToken>>,
    client: reqwest::Client,
    direct_client: reqwest::Client,
    quitting: AtomicBool,
}

impl AppState {
    fn new(config: AppConfig) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent("S-Tools/1.0.11")
            .connect_timeout(Duration::from_secs(12))
            .timeout(Duration::from_secs(45))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|error| format!("初始化网络客户端失败：{error}"))?;
        let direct_client = reqwest::Client::builder()
            .user_agent("S-Tools/1.0.11")
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .no_proxy()
            .build()
            .map_err(|error| format!("初始化直连网络客户端失败：{error}"))?;
        Ok(Self {
            config: RwLock::new(config),
            sessions: Mutex::new(HashMap::new()),
            capture_ready: Arc::new(Mutex::new(HashSet::new())),
            capture_guard: capture::CaptureGuard::default(),
            pending: Mutex::new(PendingTranslation::default()),
            translation_shown_at: Mutex::new(None),
            translation_source_app: Mutex::new(None),
            translation_focus_seen: AtomicBool::new(false),
            cancellations: Mutex::new(HashMap::new()),
            update_cancellation: Mutex::new(None),
            client,
            direct_client,
            quitting: AtomicBool::new(false),
        })
    }
}

fn translation_client<'a>(
    state: &'a AppState,
    provider: &models::ProviderConfig,
) -> &'a reqwest::Client {
    if translate::provider_uses_system_proxy(provider) {
        &state.client
    } else {
        &state.direct_client
    }
}

#[tauri::command]
fn bootstrap(app: AppHandle, state: State<'_, AppState>) -> Result<BootstrapData, String> {
    let config = state
        .config
        .read()
        .map_err(|_| "配置锁已损坏".to_string())?
        .clone();
    Ok(BootstrapData {
        version: app.package_info().version.to_string(),
        settings: config.settings,
        providers: translate::provider_views(&config.providers),
        permissions: permission_status(),
    })
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut input: AppSettings,
) -> Result<AppSettings, String> {
    input.schema_version = 1;
    if input.hotkeys.translate.trim().is_empty() || input.hotkeys.screenshot.trim().is_empty() {
        return Err("快捷键不能为空".into());
    }
    if input
        .hotkeys
        .translate
        .eq_ignore_ascii_case(&input.hotkeys.screenshot)
    {
        return Err("划词翻译和截图不能使用同一个快捷键".into());
    }
    let translate_shortcut = Shortcut::from_str(&input.hotkeys.translate)
        .map_err(|error| format!("划词翻译快捷键无效：{error}"))?;
    let screenshot_shortcut = Shortcut::from_str(&input.hotkeys.screenshot)
        .map_err(|error| format!("截图快捷键无效：{error}"))?;
    if translate_shortcut.id() == screenshot_shortcut.id() {
        return Err("划词翻译和截图不能使用同一个快捷键".into());
    }
    let previous = state
        .config
        .read()
        .map_err(|_| "配置锁已损坏".to_string())?
        .clone();
    let mut config = state
        .config
        .write()
        .map_err(|_| "配置锁已损坏".to_string())?;
    config.settings = input.clone();
    config::save_config(&app, &config)?;
    drop(config);

    let apply_result =
        apply_autostart(&app, input.launch_at_login).and_then(|_| register_shortcuts(&app));
    if let Err(error) = apply_result {
        *state
            .config
            .write()
            .map_err(|_| "配置锁已损坏".to_string())? = previous.clone();
        let _ = config::save_config(&app, &previous);
        let _ = apply_autostart(&app, previous.settings.launch_at_login);
        let _ = register_shortcuts(&app);
        return Err(format!("应用设置失败，已回滚：{error}"));
    }
    Ok(input)
}

#[tauri::command]
fn save_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ProviderInput,
) -> Result<ProviderView, String> {
    let mut config = state
        .config
        .write()
        .map_err(|_| "配置锁已损坏".to_string())?;
    let provider = config
        .providers
        .iter_mut()
        .find(|provider| provider.id == input.id)
        .ok_or_else(|| "翻译服务不存在".to_string())?;
    translate::apply_provider_input(provider, &input)?;
    config::save_config(&app, &config)?;
    let view = translate::provider_views(&config.providers)
        .into_iter()
        .find(|provider| provider.id == input.id)
        .ok_or_else(|| "翻译服务不存在".to_string())?;
    drop(config);
    let _ = app.emit("app://providers-changed", ());
    Ok(view)
}

#[tauri::command]
fn set_provider_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<ProviderView, String> {
    let mut config = state
        .config
        .write()
        .map_err(|_| "配置锁已损坏".to_string())?;
    let provider = config
        .providers
        .iter_mut()
        .find(|provider| provider.id == id)
        .ok_or_else(|| "翻译服务不存在".to_string())?;
    if enabled && !provider.configured {
        return Err("请先完成该翻译服务的配置".into());
    }
    provider.enabled = enabled;
    config::save_config(&app, &config)?;
    let view = translate::provider_views(&config.providers)
        .into_iter()
        .find(|provider| provider.id == id)
        .ok_or_else(|| "翻译服务不存在".to_string())?;
    drop(config);
    let _ = app.emit("app://providers-changed", ());
    Ok(view)
}

#[tauri::command]
async fn translate(
    app: AppHandle,
    state: State<'_, AppState>,
    request: TranslationRequest,
) -> Result<ProviderResult, String> {
    ensure_translation_enabled(&app)?;
    let provider = {
        let config = state
            .config
            .read()
            .map_err(|_| "配置锁已损坏".to_string())?;
        config
            .providers
            .iter()
            .find(|provider| provider.id == request.provider_id)
            .cloned()
            .ok_or_else(|| "翻译服务不存在".to_string())?
    };
    if !provider.enabled {
        return Err("该翻译服务尚未启用".into());
    }
    if !provider.configured {
        return Err("该翻译服务尚未配置凭据".into());
    }
    let cancellation_key = format!("{}:{}", request.request_id, provider.id);
    let cancellation = CancellationToken::new();
    state
        .cancellations
        .lock()
        .map_err(|_| "取消令牌锁已损坏".to_string())?
        .insert(cancellation_key.clone(), cancellation.clone());
    let client = translation_client(&state, &provider);
    let started = Instant::now();
    app_log::info(
        &app,
        TRANSLATION_LOG_SCOPE,
        format!(
            "开始翻译 provider={} text_len={}",
            provider.id,
            request.text.chars().count()
        ),
    );
    let result = translate::perform_translation(client, &provider, &request, cancellation).await;
    if let Ok(mut tokens) = state.cancellations.lock() {
        tokens.remove(&cancellation_key);
    }
    let elapsed_ms = started.elapsed().as_millis();
    match &result {
        Ok(result) => app_log::info(
            &app,
            TRANSLATION_LOG_SCOPE,
            format!(
                "翻译完成 provider={} elapsed_ms={elapsed_ms} result_len={}",
                provider.id,
                result.translated_text.chars().count()
            ),
        ),
        Err(error) => app_log::error(
            &app,
            TRANSLATION_LOG_SCOPE,
            format!("翻译失败 provider={} elapsed_ms={elapsed_ms} error={error}", provider.id),
        ),
    }
    result
}

#[tauri::command]
fn cancel_translation(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
) -> Result<(), String> {
    let prefix = format!("{request_id}:");
    let mut tokens = state
        .cancellations
        .lock()
        .map_err(|_| "取消令牌锁已损坏".to_string())?;
    let keys = tokens
        .keys()
        .filter(|key| key.starts_with(&prefix))
        .cloned()
        .collect::<Vec<_>>();
    let cancelled = keys.len();
    for key in keys {
        let Some(token) = tokens.remove(&key) else {
            continue;
        };
        token.cancel();
    }
    if cancelled > 0 {
        app_log::info(
            &app,
            TRANSLATION_LOG_SCOPE,
            format!("取消翻译请求 count={cancelled}"),
        );
    }
    Ok(())
}

#[tauri::command]
async fn test_provider(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<ProviderResult, String> {
    let provider = {
        let config = state
            .config
            .read()
            .map_err(|_| "配置锁已损坏".to_string())?;
        config
            .providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .cloned()
            .ok_or_else(|| "翻译服务不存在".to_string())?
    };
    let request = TranslationRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        provider_id: provider.id.clone(),
        text: "Hello, S-Tools.".into(),
        source_lang: "auto".into(),
        target_lang: "zh-CN".into(),
    };
    let client = translation_client(&state, &provider);
    translate::perform_translation(client, &provider, &request, CancellationToken::new()).await
}

#[tauri::command]
fn get_pending_translation(state: State<'_, AppState>) -> Result<PendingTranslation, String> {
    state
        .pending
        .lock()
        .map(|pending| pending.clone())
        .map_err(|_| "待翻译文本锁已损坏".to_string())
}

#[tauri::command]
async fn open_translation(app: AppHandle) -> Result<(), String> {
    open_translation_workflow(app).await
}

/// 失焦事件的处置结论。抽成纯函数，便于覆盖显示瞬间的伪失焦场景。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TranslationFocusOutcome {
    IgnoreHidden,
    IgnoreGrace,
    IgnoreStale,
    Hide,
}

fn translation_focus_outcome(
    visible: bool,
    shown_for: Option<Duration>,
    still_focused: bool,
) -> TranslationFocusOutcome {
    if !visible {
        return TranslationFocusOutcome::IgnoreHidden;
    }
    if matches!(shown_for, Some(elapsed) if elapsed < TRANSLATION_FOCUS_GRACE) {
        return TranslationFocusOutcome::IgnoreGrace;
    }
    // 真正的失焦发生时窗口已经不是 key window；仍为焦点说明事件已过期。
    if still_focused {
        return TranslationFocusOutcome::IgnoreStale;
    }
    TranslationFocusOutcome::Hide
}

/// 只有用户显式关闭弹框才把焦点还给原应用；点击外部和跳转设置不抢焦点。
fn should_restore_source_app(reason: &str) -> bool {
    matches!(reason, "escape" | "button" | "close-requested")
}

fn handle_translation_focus_change(app: &AppHandle, focused: bool) {
    let Some(window) = app.get_webview_window("translate") else {
        return;
    };
    let visible = window.is_visible().unwrap_or(false);
    let state = app.state::<AppState>();
    let shown_for = state
        .translation_shown_at
        .lock()
        .ok()
        .and_then(|shown_at| *shown_at)
        .map(|at| at.elapsed());

    if focused {
        state
            .translation_focus_seen
            .store(true, std::sync::atomic::Ordering::Relaxed);
        app_log::info(
            app,
            TRANSLATION_LOG_SCOPE,
            format!(
                "翻译窗口获得焦点 visible={visible} shown_for_ms={}",
                shown_for.map(|value| value.as_millis()).unwrap_or_default()
            ),
        );
        return;
    }

    let still_focused = visible && window.is_focused().unwrap_or(false);
    match translation_focus_outcome(visible, shown_for, still_focused) {
        TranslationFocusOutcome::IgnoreHidden => app_log::info(
            app,
            TRANSLATION_LOG_SCOPE,
            "翻译窗口失焦事件：窗口已隐藏，忽略",
        ),
        TranslationFocusOutcome::IgnoreGrace => app_log::info(
            app,
            TRANSLATION_LOG_SCOPE,
            format!(
                "翻译窗口失焦事件：仍在显示宽限期内，忽略 shown_for_ms={}",
                shown_for.map(|value| value.as_millis()).unwrap_or_default()
            ),
        ),
        TranslationFocusOutcome::IgnoreStale => app_log::info(
            app,
            TRANSLATION_LOG_SCOPE,
            "翻译窗口失焦事件：窗口仍是焦点窗口，忽略过期事件",
        ),
        TranslationFocusOutcome::Hide => {
            app_log::info(
                app,
                TRANSLATION_LOG_SCOPE,
                format!(
                    "翻译窗口失焦，准备隐藏 shown_for_ms={}",
                    shown_for.map(|value| value.as_millis()).unwrap_or_default()
                ),
            );
            let _ = hide_translation_window_inner(app, "focus-lost");
        }
    }
}

fn hide_translation_window_inner(app: &AppHandle, reason: &str) -> Result<(), String> {
    let Some(window) = app.get_webview_window("translate") else {
        app_log::warn(
            app,
            TRANSLATION_LOG_SCOPE,
            format!("隐藏翻译窗口失败：窗口不存在 reason={reason}"),
        );
        return Ok(());
    };
    window
        .hide()
        .map_err(|error| format!("隐藏翻译窗口失败：{error}"))?;
    let state = app.state::<AppState>();
    if let Ok(mut shown_at) = state.translation_shown_at.lock() {
        *shown_at = None;
    }
    app_log::info(
        app,
        TRANSLATION_LOG_SCOPE,
        format!("翻译窗口已隐藏 reason={reason}"),
    );
    if should_restore_source_app(reason) {
        restore_translation_source_app(app, reason);
    } else if let Ok(mut source) = state.translation_source_app.lock() {
        // 用户已切到别的应用或本应用要打开其他窗口时，不抢回焦点。
        *source = None;
    }
    Ok(())
}

fn restore_translation_source_app(app: &AppHandle, reason: &str) {
    let state = app.state::<AppState>();
    let pid = state
        .translation_source_app
        .lock()
        .ok()
        .and_then(|mut source| source.take());
    let Some(pid) = pid else {
        app_log::info(
            app,
            TRANSLATION_LOG_SCOPE,
            format!("跳过返回原应用：未记录来源应用 reason={reason}"),
        );
        return;
    };
    if pid == std::process::id() {
        app_log::info(
            app,
            TRANSLATION_LOG_SCOPE,
            format!("跳过返回原应用：来源应用是自身 pid={pid}"),
        );
        return;
    }
    app_log::info(
        app,
        TRANSLATION_LOG_SCOPE,
        format!(
            "准备返回原应用 pid={pid} delay_ms={} reason={reason}",
            TRANSLATION_RESTORE_DELAY.as_millis()
        ),
    );
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(TRANSLATION_RESTORE_DELAY).await;
        let activated = crate::platform::activate_app(pid);
        let message = format!(
            "返回原应用 pid={pid} result={}",
            if activated { "ok" } else { "failed" }
        );
        if activated {
            app_log::info(&handle, TRANSLATION_LOG_SCOPE, message);
        } else {
            app_log::warn(&handle, TRANSLATION_LOG_SCOPE, message);
        }
    });
}

#[tauri::command]
fn hide_translation_window(app: AppHandle, reason: Option<String>) -> Result<(), String> {
    let reason = reason.unwrap_or_else(|| "unknown".into());
    hide_translation_window_inner(&app, &reason)
}

#[tauri::command]
async fn start_capture(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let main_visible = app
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    let result = async {
        ensure_screenshot_enabled(&app)?;
        capture::start_capture(
            app.clone(),
            &state.sessions,
            &state.capture_ready,
            &state.capture_guard,
        )
        .await
    }
    .await;
    if let Err(error) = &result {
        if main_visible {
            app_log::error(&app, "capture", error);
            let _ = app.emit("app://toast", error);
        }
    }
    result
}

#[tauri::command]
fn open_releases() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    Command::new("open")
        .arg(RELEASES_URL)
        .spawn()
        .map_err(|error| format!("打开 Releases 失败：{error}"))?;
    #[cfg(target_os = "windows")]
    Command::new("cmd")
        .args(["/C", "start", "", RELEASES_URL])
        .spawn()
        .map_err(|error| format!("打开 Releases 失败：{error}"))?;
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    return Err("当前平台不支持打开系统浏览器".into());
    Ok(())
}

#[tauri::command]
fn open_app_log(app: AppHandle) -> Result<(), String> {
    app_log::open(&app)
}

#[tauri::command]
fn log_capture_error(app: AppHandle, message: String) -> Result<(), String> {
    let message = message.trim();
    if message.is_empty() {
        return Ok(());
    }
    app_log::error(
        &app,
        "capture",
        &message.chars().take(4000).collect::<String>(),
    );
    Ok(())
}

#[tauri::command]
fn get_capture_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CaptureSession, String> {
    state
        .sessions
        .lock()
        .map_err(|_| "截图会话锁已损坏".to_string())?
        .get(&session_id)
        .cloned()
        .ok_or_else(|| "截图会话不存在或已过期".into())
}

#[tauri::command]
async fn get_capture_image_bytes(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<tauri::ipc::Response, String> {
    let bytes = capture::capture_image_bytes(&state.sessions, &session_id).await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
fn get_active_capture_session(state: State<'_, AppState>) -> Option<CaptureSession> {
    capture::active_session(&state.sessions)
}

#[tauri::command]
fn show_capture_window(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    capture::show_capture_window(&app, &state.sessions, &state.capture_ready, &session_id)
}

#[tauri::command]
async fn choose_capture_save_path(app: AppHandle) -> Result<Option<String>, String> {
    let directory = app
        .path()
        .picture_dir()
        .unwrap_or_else(|_| config::data_dir(&app).unwrap_or_default())
        .join("S-Tools");
    let file_name = format!(
        "S-Tools-{}.png",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::create_dir_all(&directory).ok();
        rfd::FileDialog::new()
            .set_directory(directory)
            .set_file_name(file_name)
            .add_filter("PNG 图片", &["png"])
            .save_file()
            .map(|path| path.to_string_lossy().to_string())
    })
    .await
    .map_err(|error| format!("打开保存对话框失败：{error}"))
}

#[tauri::command]
fn complete_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    request: CompleteCaptureRequest,
) -> Result<CompleteCaptureResult, String> {
    capture::complete_capture(&app, &state.sessions, &state.capture_ready, &request)
}

#[tauri::command]
fn cancel_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    capture::cleanup_session(&app, &state.sessions, &state.capture_ready, &session_id);
    Ok(())
}

#[tauri::command]
fn get_permission_status() -> PermissionStatus {
    permission_status()
}

#[tauri::command]
fn open_permission_settings(kind: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if kind == "accessibility" {
            // 仅打开设置页不会让未签名的开发版出现在权限列表中，先主动触发系统授权请求。
            selection::request_accessibility();
        }
        let anchor = match kind.as_str() {
            "accessibility" => "Privacy_Accessibility",
            "screenRecording" => "Privacy_ScreenCapture",
            _ => return Err("未知权限类型".into()),
        };
        Command::new("open")
            .arg(format!(
                "x-apple.systempreferences:com.apple.preference.security?{anchor}"
            ))
            .spawn()
            .map_err(|error| format!("打开系统设置失败：{error}"))?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = kind;
    }
    Ok(())
}

#[tauri::command]
fn open_main_window(app: AppHandle, section: Option<String>) -> Result<(), String> {
    show_main_window(&app, section.as_deref().unwrap_or("overview"))
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    apply_autostart(&app, enabled)
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let result = updater::check_update(&app).await;
    if let Err(error) = &result {
        app_log::error(&app, "update", error);
    }
    result
}

#[tauri::command]
async fn download_update(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    file_name: String,
    on_event: Channel<UpdateDownloadEvent>,
) -> Result<String, String> {
    let cancellation = begin_update_download(&state)?;
    let result = updater::download_update(&app, url, file_name, on_event, &cancellation).await;
    finish_update_download(&state);
    if let Err(error) = &result {
        app_log::error(&app, "update", error);
    }
    result
}

#[tauri::command]
async fn install_update(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    file_name: String,
    sha256: Option<String>,
    on_event: Channel<UpdateDownloadEvent>,
) -> Result<UpdateInstallResult, String> {
    let cancellation = begin_update_download(&state)?;
    let result =
        updater::install_update(&app, url, file_name, sha256, on_event, &cancellation).await;
    finish_update_download(&state);
    if let Err(error) = &result {
        app_log::error(&app, "update", error);
    }
    result
}

#[tauri::command]
fn cancel_update(state: State<'_, AppState>) -> Result<(), String> {
    let cancellation = state
        .update_cancellation
        .lock()
        .map_err(|_| "更新取消状态锁已损坏".to_string())?
        .take()
        .ok_or_else(|| "没有正在进行的更新下载".to_string())?;
    cancellation.cancel();
    Ok(())
}

fn begin_update_download(state: &AppState) -> Result<CancellationToken, String> {
    let mut cancellation = state
        .update_cancellation
        .lock()
        .map_err(|_| "更新取消状态锁已损坏".to_string())?;
    if cancellation.is_some() {
        return Err("已有更新任务正在进行".into());
    }
    let token = CancellationToken::new();
    *cancellation = Some(token.clone());
    Ok(token)
}

fn finish_update_download(state: &AppState) {
    if let Ok(mut cancellation) = state.update_cancellation.lock() {
        *cancellation = None;
    }
}

#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    updater::reveal_path(&path)
}

fn permission_status() -> PermissionStatus {
    PermissionStatus {
        accessibility: selection::accessibility_allowed(),
        screen_recording: selection::screen_recording_allowed(),
    }
}

pub fn report_capture_failure(app: &AppHandle, message: &str) {
    app_log::error(app, "capture", message);
}

fn show_main_window(app: &AppHandle, section: &str) -> Result<(), String> {
    reveal_main_window(app)?;
    let _ = app.emit("app://navigate", section);
    Ok(())
}

fn restore_app_interface(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let onboarding_completed = state
        .config
        .read()
        .map_err(|_| "配置锁已损坏".to_string())?
        .settings
        .has_completed_onboarding;
    if onboarding_completed {
        reveal_main_window(app)
    } else {
        show_onboarding(app)
    }
}

fn reveal_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(onboarding) = app.get_webview_window("onboarding") {
        let _ = onboarding.close();
    }
    if let Some(main) = app.get_webview_window("main") {
        main.show()
            .map_err(|error| format!("显示主窗口失败：{error}"))?;
        main.set_focus()
            .map_err(|error| format!("聚焦主窗口失败：{error}"))?;
    }
    Ok(())
}

fn show_onboarding(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("onboarding") {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(
        app,
        "onboarding",
        WebviewUrl::App("index.html?view=onboarding".into()),
    )
    .title("欢迎使用 S-Tools")
    .inner_size(900.0, 720.0)
    .min_inner_size(760.0, 600.0)
    .center()
    .resizable(true)
    .build()
    .map_err(|error| format!("创建引导窗口失败：{error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("聚焦引导窗口失败：{error}"))?;
    Ok(())
}

fn show_translation_window(
    app: &AppHandle,
    pending: &PendingTranslation,
    anchor: Option<tauri::PhysicalPosition<f64>>,
) -> Result<(), String> {
    let window = if let Some(window) = app.get_webview_window("translate") {
        window
    } else {
        WebviewWindowBuilder::new(
            app,
            "translate",
            WebviewUrl::App("index.html?view=translate".into()),
        )
        .title("S-Tools 划词翻译")
        .inner_size(TRANSLATION_WINDOW_WIDTH, TRANSLATION_WINDOW_DEFAULT_HEIGHT)
        .min_inner_size(TRANSLATION_WINDOW_WIDTH, TRANSLATION_WINDOW_MIN_HEIGHT)
        .max_inner_size(TRANSLATION_WINDOW_WIDTH, TRANSLATION_WINDOW_MAX_HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        // Windows 原生阴影会给无边框透明窗口额外加 1px 白边，与 CSS 轮廓叠成双边框。
        .shadow(!cfg!(target_os = "windows"))
        .visible(false)
        .build()
        .map_err(|error| format!("创建翻译窗口失败：{error}"))?
    };

    window
        .set_size(tauri::LogicalSize::new(
            TRANSLATION_WINDOW_WIDTH,
            TRANSLATION_WINDOW_DEFAULT_HEIGHT,
        ))
        .map_err(|error| format!("重置翻译窗口尺寸失败：{error}"))?;
    position_translation_window(app, &window, anchor)?;
    // 记录显示时刻，供失焦逻辑区分显示瞬间的伪失焦。
    let state = app.state::<AppState>();
    if let Ok(mut shown_at) = state.translation_shown_at.lock() {
        *shown_at = Some(Instant::now());
    }
    state.translation_focus_seen.store(false, Ordering::Relaxed);
    window
        .show()
        .map_err(|error| format!("显示翻译窗口失败：{error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("聚焦翻译窗口失败：{error}"))?;
    // macOS 显示和聚焦透明窗口时可能重置初始位置，完成后再按同一锚点定位。
    position_translation_window(app, &window, anchor)?;
    let delayed_app = app.clone();
    let delayed_window = window.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        let _ = position_translation_window(&delayed_app, &delayed_window, anchor);
    });
    app_log::info(
        app,
        TRANSLATION_LOG_SCOPE,
        format!(
            "翻译窗口已显示 size={TRANSLATION_WINDOW_WIDTH}x{TRANSLATION_WINDOW_DEFAULT_HEIGHT} anchor={} focused={}",
            anchor
                .map(|position| format!("({}, {})", position.x, position.y))
                .unwrap_or_else(|| "-".into()),
            window.is_focused().unwrap_or(false)
        ),
    );
    // 未激活 App 时 makeKeyAndOrderFront 可能不生效，延迟复查并补一次聚焦。
    let recheck_app = app.clone();
    let recheck_window = window.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(TRANSLATION_FOCUS_RECHECK).await;
        // 已经获得过焦点说明用户可能主动切走了，不再抢回焦点。
        if !recheck_window.is_visible().unwrap_or(false)
            || recheck_window.is_focused().unwrap_or(false)
            || recheck_app
                .state::<AppState>()
                .translation_focus_seen
                .load(Ordering::Relaxed)
        {
            return;
        }
        let retried = recheck_window.set_focus().is_ok();
        app_log::warn(
            &recheck_app,
            TRANSLATION_LOG_SCOPE,
            format!("翻译窗口显示后未获得焦点，重试聚焦 retried={retried}"),
        );
    });
    let _ = app.emit("translate://selection", pending.clone());
    Ok(())
}

fn position_translation_window<R: Runtime>(
    app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
    anchor: Option<tauri::PhysicalPosition<f64>>,
) -> Result<(), String> {
    position_translation_window_for_size(
        app,
        window,
        TRANSLATION_WINDOW_WIDTH,
        TRANSLATION_WINDOW_DEFAULT_HEIGHT,
        anchor,
    )
}

fn position_translation_window_for_size<R: Runtime>(
    app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
    width: f64,
    height: f64,
    anchor: Option<tauri::PhysicalPosition<f64>>,
) -> Result<(), String> {
    let cursor = anchor.unwrap_or_else(|| app.cursor_position().unwrap_or_default());
    let monitor = monitor_containing_point(app, cursor.x, cursor.y);
    let mut position_scale = 1.0;
    let (x, y) = if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        position_scale = scale;
        let work = monitor.work_area();
        translation_window_position(
            cursor.x / scale,
            cursor.y / scale,
            width,
            height,
            work.position.x as f64 / scale,
            work.position.y as f64 / scale,
            work.size.width as f64 / scale,
            work.size.height as f64 / scale,
        )
    } else {
        (120.0, 120.0)
    };
    window
        .set_position(tauri::PhysicalPosition::new(
            (x * position_scale).round() as i32,
            (y * position_scale).round() as i32,
        ))
        .map_err(|error| format!("定位翻译窗口失败：{error}"))
}

fn monitor_containing_point<R: Runtime>(
    app: &AppHandle<R>,
    x: f64,
    y: f64,
) -> Option<tauri::Monitor> {
    let monitors = app.available_monitors().ok()?;
    monitors
        .iter()
        .find(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            x >= position.x as f64
                && x < position.x as f64 + size.width as f64
                && y >= position.y as f64
                && y < position.y as f64 + size.height as f64
        })
        .cloned()
        .or_else(|| monitors.first().cloned())
}

fn clamp_translation_window_height(height: f64) -> f64 {
    if !height.is_finite() {
        return TRANSLATION_WINDOW_DEFAULT_HEIGHT;
    }
    height.clamp(TRANSLATION_WINDOW_MIN_HEIGHT, TRANSLATION_WINDOW_MAX_HEIGHT)
}

#[tauri::command]
fn resize_translation_window(app: AppHandle, height: f64) -> Result<(), String> {
    let height = clamp_translation_window_height(height);
    let Some(window) = app.get_webview_window("translate") else {
        return Ok(());
    };
    let position = window.outer_position().unwrap_or_default();
    let previous_size = window.outer_size().unwrap_or_default();
    let scale = window.scale_factor().unwrap_or(1.0);
    window
        .set_size(tauri::LogicalSize::new(TRANSLATION_WINDOW_WIDTH, height))
        .map_err(|error| format!("调整翻译窗口高度失败：{error}"))?;

    let width = (TRANSLATION_WINDOW_WIDTH * scale).round();
    let height = (height * scale).round();
    let anchor_x = position.x as f64 + previous_size.width as f64 / 2.0;
    let anchor_y = position.y as f64 + previous_size.height as f64 / 2.0;
    let Some(monitor) = monitor_containing_point(&app, anchor_x, anchor_y) else {
        return Ok(());
    };
    let work = monitor.work_area();
    let margin = 8.0 * scale;
    let left = work.position.x as f64 + margin;
    let top = work.position.y as f64 + margin;
    let right = work.position.x as f64 + work.size.width as f64 - margin;
    let bottom = work.position.y as f64 + work.size.height as f64 - margin;
    let max_x = (right - width).max(left);
    let max_y = (bottom - height).max(top);
    let x = (position.x as f64).clamp(left, max_x);
    let y = if position.y as f64 + height > bottom {
        bottom - height
    } else {
        position.y as f64
    }
    .clamp(top, max_y);
    window
        .set_position(tauri::PhysicalPosition::new(
            x.round() as i32,
            y.round() as i32,
        ))
        .map_err(|error| format!("校正翻译窗口位置失败：{error}"))
}

fn translation_window_position(
    cursor_x: f64,
    cursor_y: f64,
    window_width: f64,
    window_height: f64,
    work_x: f64,
    work_y: f64,
    work_width: f64,
    work_height: f64,
) -> (f64, f64) {
    const GAP_X: f64 = 0.0;
    const GAP_Y: f64 = 0.0;
    const MARGIN: f64 = 8.0;

    let left = work_x + MARGIN;
    let top = work_y + MARGIN;
    let right = work_x + work_width - MARGIN;
    let bottom = work_y + work_height - MARGIN;
    let max_x = (right - window_width).max(left);
    let max_y = (bottom - window_height).max(top);

    let preferred_x = cursor_x + GAP_X;
    let x = if preferred_x + window_width <= right {
        preferred_x
    } else {
        cursor_x - GAP_X - window_width
    };
    let preferred_y = cursor_y + GAP_Y;
    let y = if preferred_y + window_height <= bottom {
        preferred_y
    } else {
        cursor_y - GAP_Y - window_height
    };

    (x.clamp(left, max_x), y.clamp(top, max_y))
}

/// 记录触发翻译时的前台应用，弹框关闭后用它把焦点还回去。
fn record_translation_source_app(app: &AppHandle) {
    let pid = crate::platform::frontmost_app_id().filter(|pid| *pid != std::process::id());
    let state = app.state::<AppState>();
    match state.translation_source_app.lock() {
        Ok(mut source) => *source = pid,
        Err(_) => {
            app_log::warn(app, TRANSLATION_LOG_SCOPE, "来源应用锁已损坏，无法记录来源应用");
            return;
        }
    }
    match pid {
        Some(pid) => app_log::info(
            app,
            TRANSLATION_LOG_SCOPE,
            format!("记录来源应用 pid={pid}"),
        ),
        None => app_log::warn(
            app,
            TRANSLATION_LOG_SCOPE,
            "未取到来源应用，关闭弹框后不会恢复焦点",
        ),
    }
}

async fn open_translation_workflow(app: AppHandle) -> Result<(), String> {
    ensure_translation_enabled(&app)?;
    record_translation_source_app(&app);
    let anchor = app.cursor_position().ok();
    let captured = tauri::async_runtime::spawn_blocking(selection::capture_selection)
        .await
        .map_err(|error| format!("读取选区任务失败：{error}"))?;
    let (text, truncate_notice) = selection::truncate_selection(captured.text);
    let notice = truncate_notice.or(captured.notice);
    let pending = PendingTranslation {
        text,
        source: captured.source,
        notice,
    };
    // 只记录长度，不记录选中文本与译文。
    app_log::info(
        &app,
        TRANSLATION_LOG_SCOPE,
        format!(
            "读取选区完成 source={} text_len={} notice={}",
            pending.source,
            pending.text.chars().count(),
            pending.notice.as_deref().unwrap_or("-")
        ),
    );
    if let Some(state) = app.try_state::<AppState>() {
        *state
            .pending
            .lock()
            .map_err(|_| "待翻译文本锁已损坏".to_string())? = pending.clone();
    }
    show_translation_window(&app, &pending, anchor)
}

fn ensure_translation_enabled(app: &AppHandle) -> Result<(), String> {
    let enabled = app
        .state::<AppState>()
        .config
        .read()
        .map(|config| config.settings.translation_enabled)
        .unwrap_or(false);
    if enabled {
        Ok(())
    } else {
        Err("划词翻译功能尚未开启，请先到工作台打开开关".into())
    }
}

fn should_start_translation(state: ShortcutState) -> bool {
    state == ShortcutState::Released
}

fn should_start_capture(state: ShortcutState) -> bool {
    state == ShortcutState::Pressed
}

fn ensure_screenshot_enabled(app: &AppHandle) -> Result<(), String> {
    let enabled = app
        .state::<AppState>()
        .config
        .read()
        .map(|config| config.settings.screenshot_enabled)
        .unwrap_or(false);
    if enabled {
        Ok(())
    } else {
        Err("截图功能尚未开启，请先到工作台打开开关".into())
    }
}

fn launched_from_autostart(args: &[String]) -> bool {
    args.iter().any(|arg| arg == AUTOSTART_ARG)
}

#[cfg(windows)]
const WINDOWS_RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const WINDOWS_AUTOSTART_NAME: &str = "S-Tools";

#[cfg(all(windows, not(debug_assertions)))]
fn windows_autostart_matches(registered: &str, executable: &str, expected_args: &[&str]) -> bool {
    let normalize = |value: &str| {
        value
            .trim()
            .trim_matches('"')
            .replace('/', "\\")
            .to_lowercase()
    };
    let command = registered.trim();
    let (registered_path, registered_args) = if let Some(rest) = command.strip_prefix('"') {
        match rest.split_once('"') {
            Some((path, args)) => (path, args.trim()),
            None => return false,
        }
    } else {
        match command.split_once(char::is_whitespace) {
            Some((path, args)) => (path, args.trim()),
            None => (command, ""),
        }
    };
    normalize(registered_path) == normalize(executable)
        && normalize(registered_args) == normalize(&expected_args.join(" "))
}

#[cfg(all(windows, not(debug_assertions)))]
fn read_windows_autostart() -> Result<Option<String>, String> {
    use winreg::enums::KEY_READ;

    let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey_with_flags(WINDOWS_RUN_KEY, KEY_READ)
        .map_err(|error| format!("读取 Windows 开机启动项失败：{error}"))?;
    match key.get_value::<String, _>(WINDOWS_AUTOSTART_NAME) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("读取 Windows 开机启动项失败：{error}")),
    }
}

#[cfg(windows)]
fn remove_windows_autostart() -> Result<(), String> {
    use winreg::enums::KEY_WRITE;

    let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey_with_flags(WINDOWS_RUN_KEY, KEY_WRITE)
        .map_err(|error| format!("打开 Windows 开机启动项失败：{error}"))?;
    match key.delete_value(WINDOWS_AUTOSTART_NAME) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("移除 Windows 开机启动项失败：{error}")),
    }
}

fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    // 开发版没有前端服务器，绝不能写入开机启动项，否则登录后会显示连接失败页。
    #[cfg(all(windows, debug_assertions))]
    {
        let _ = (app, enabled);
        return remove_windows_autostart();
    }

    #[cfg(not(all(windows, debug_assertions)))]
    {
        use tauri_plugin_autostart::ManagerExt;

        let manager = app.autolaunch();

        #[cfg(windows)]
        {
            let executable = std::env::current_exe()
                .map_err(|error| format!("定位 S-Tools 可执行文件失败：{error}"))?
                .display()
                .to_string();
            let registered = read_windows_autostart()?;
            if enabled {
                // 旧版曾把 debug 路径写进同名启动项，因此必须校验路径和静默参数。
                let matches = registered
                    .as_deref()
                    .map(|value| windows_autostart_matches(value, &executable, &[AUTOSTART_ARG]))
                    .unwrap_or(false);
                if !matches {
                    manager
                        .enable()
                        .map_err(|error| format!("开启开机启动失败：{error}"))?;
                }
                return Ok(());
            }
            remove_windows_autostart()?;
            return Ok(());
        }

        #[cfg(not(windows))]
        {
            if matches!(manager.is_enabled(), Ok(current) if current == enabled) {
                return Ok(());
            }
            if enabled {
                manager
                    .enable()
                    .map_err(|error| format!("开启开机启动失败：{error}"))
            } else {
                match manager.disable() {
                    Ok(()) => Ok(()),
                    Err(_) if matches!(manager.is_enabled(), Ok(false)) => Ok(()),
                    Err(error) => Err(format!("关闭开机启动失败：{error}")),
                }
            }
        }
    }
}

fn register_shortcuts(app: &AppHandle) -> Result<(), String> {
    let settings = app
        .state::<AppState>()
        .config
        .read()
        .map(|config| config.settings.clone())
        .map_err(|_| "配置锁已损坏".to_string())?;
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    if settings.translation_enabled {
        manager
            .on_shortcut(settings.hotkeys.translate.as_str(), |app, _, event| {
                if should_start_translation(event.state) {
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = open_translation_workflow(handle.clone()).await {
                            let _ = handle.emit("app://toast", error);
                        }
                    });
                }
            })
            .map_err(|error| format!("注册划词翻译快捷键失败：{error}"))?;
    }
    if settings.screenshot_enabled {
        manager
            .on_shortcut(settings.hotkeys.screenshot.as_str(), |app, _, event| {
                if should_start_capture(event.state) {
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let state = handle.state::<AppState>();
                        if let Err(error) = capture::start_capture(
                            handle.clone(),
                            &state.sessions,
                            &state.capture_ready,
                            &state.capture_guard,
                        )
                        .await
                        {
                            report_capture_failure(&handle, &error);
                        }
                    });
                }
            })
            .map_err(|error| format!("注册截图快捷键失败：{error}"))?;
    }
    Ok(())
}

#[tauri::command]
fn suspend_shortcuts(app: AppHandle) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|error| format!("暂停全局快捷键失败：{error}"))
}

#[tauri::command]
fn resume_shortcuts(app: AppHandle) -> Result<(), String> {
    register_shortcuts(&app)
}

#[tauri::command]
fn validate_hotkey(app: AppHandle, value: String) -> Result<(), String> {
    let shortcut =
        Shortcut::from_str(&value).map_err(|error| format!("快捷键格式无效：{error}"))?;
    let manager = app.global_shortcut();
    manager
        .register(shortcut)
        .map_err(|error| format!("该快捷键无法注册，可能被系统或其他应用占用：{error}"))?;
    manager
        .unregister(shortcut)
        .map_err(|error| format!("释放测试快捷键失败：{error}"))
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let translate = MenuItemBuilder::with_id("translate", "划词翻译").build(app)?;
    let screenshot = MenuItemBuilder::with_id("screenshot", "区域截图").build(app)?;
    let settings = MenuItemBuilder::with_id("settings", "设置").build(app)?;
    let logs = MenuItemBuilder::with_id("logs", "打开日志").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出 S-Tools").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&translate, &screenshot, &settings, &logs, &quit])
        .build()?;
    #[cfg(target_os = "macos")]
    let icon_bytes = include_bytes!("../icons/tray-template.png");
    #[cfg(not(target_os = "macos"))]
    let icon_bytes = include_bytes!("../icons/tray.png");
    let icon = Image::from_bytes(icon_bytes).unwrap_or_else(|_| {
        app.default_window_icon()
            .expect("window icon is required")
            .clone()
    });
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "translate" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = open_translation_workflow(handle.clone()).await {
                        let _ = show_main_window(&handle, "overview");
                        let _ = handle.emit("app://toast", error);
                    }
                });
            }
            "screenshot" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = ensure_screenshot_enabled(&handle) {
                        report_capture_failure(&handle, &error);
                        return;
                    }
                    let state = handle.state::<AppState>();
                    if let Err(error) = capture::start_capture(
                        handle.clone(),
                        &state.sessions,
                        &state.capture_ready,
                        &state.capture_guard,
                    )
                    .await
                    {
                        report_capture_failure(&handle, &error);
                    }
                });
            }
            "settings" => {
                let _ = show_main_window(app, "settings");
            }
            "logs" => {
                if let Err(error) = app_log::open(app) {
                    let _ = show_main_window(app, "settings");
                    let _ = app.emit("app://toast", error);
                }
            }
            "quit" => {
                app.state::<AppState>()
                    .quitting
                    .store(true, Ordering::Relaxed);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = reveal_main_window(tray.app_handle());
            }
        });
    #[cfg(target_os = "macos")]
    let builder = builder.icon_as_template(true);
    builder.build(app)?;
    Ok(())
}

fn start_update_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        loop {
            match updater::check_update(&app).await {
                Ok(Some(info)) => {
                    let _ = app.emit("update://available", info);
                }
                Ok(None) => {}
                Err(error) => app_log::error(&app, "update", format!("自动检查更新失败：{error}")),
            }
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = restore_app_interface(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = config::data_dir(&handle)?;
            maintenance::remove_legacy_history(&data_dir)?;
            let config = config::load_config(&handle).map_err(|error| error.to_string())?;
            let settings = config.settings.clone();
            let state = AppState::new(config).map_err(|error| error.to_string())?;
            app.manage(state);
            app_log::info(
                &handle,
                "app",
                format!("启动 S-Tools {}", handle.package_info().version),
            );

            #[cfg(target_os = "macos")]
            let _ = app
                .handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory);

            setup_tray(app)?;
            #[cfg(all(target_os = "macos", debug_assertions))]
            if settings.translation_enabled && !selection::accessibility_allowed() {
                let permission_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(800)).await;
                    let main_handle = permission_handle.clone();
                    let _ = permission_handle.run_on_main_thread(move || {
                        let _ = show_main_window(&main_handle, "translation");
                        selection::request_accessibility();
                    });
                });
            }
            let _ = capture::cleanup_stale_temp(&handle);
            register_shortcuts(&handle).map_err(|error| error.to_string())?;
            apply_autostart(&handle, settings.launch_at_login)
                .map_err(|error| error.to_string())?;

            let startup_args: Vec<String> = std::env::args().skip(1).collect();
            let launched_from_autostart = launched_from_autostart(&startup_args);
            if settings.has_completed_onboarding && launched_from_autostart {
                // 开机启动只驻留托盘；用户点击托盘或快捷键时再显示窗口。
            } else if !settings.has_completed_onboarding {
                show_onboarding(&handle).map_err(|error| error.to_string())?;
            } else {
                reveal_main_window(&handle).map_err(|error| error.to_string())?;
            }
            start_update_loop(handle);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let label = window.label();
                if label == "main"
                    || label == "onboarding"
                    || label == "translate"
                    || label == capture::CAPTURE_WINDOW_LABEL
                {
                    api.prevent_close();
                    if label == "translate" {
                        let _ = hide_translation_window_inner(window.app_handle(), "close-requested");
                    } else {
                        let _ = window.hide();
                    }
                }
            }
            tauri::WindowEvent::Focused(focused) if window.label() == "translate" => {
                handle_translation_focus_change(window.app_handle(), *focused);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            save_settings,
            save_provider,
            set_provider_enabled,
            translate,
            cancel_translation,
            test_provider,
            get_pending_translation,
            open_translation,
            hide_translation_window,
            resize_translation_window,
            start_capture,
            get_capture_session,
            get_active_capture_session,
            get_capture_image_bytes,
            show_capture_window,
            choose_capture_save_path,
            complete_capture,
            cancel_capture,
            get_permission_status,
            open_permission_settings,
            open_main_window,
            set_autostart,
            suspend_shortcuts,
            resume_shortcuts,
            validate_hotkey,
            check_update,
            download_update,
            install_update,
            cancel_update,
            reveal_path,
            open_releases,
            open_app_log,
            log_capture_error,
        ])
        .build(tauri::generate_context!())
        .expect("error while building S-Tools")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } = event
            {
                if !has_visible_windows {
                    let _ = restore_app_interface(app);
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_translation_blur_events_during_show_grace_period() {
        // 显示后 100ms 内的失焦事件是 AppKit 的伪失焦，不能隐藏弹框。
        assert_eq!(
            translation_focus_outcome(true, Some(Duration::from_millis(100)), false),
            TranslationFocusOutcome::IgnoreGrace
        );
        assert_eq!(
            translation_focus_outcome(true, None, false),
            TranslationFocusOutcome::Hide
        );
        assert_eq!(
            translation_focus_outcome(true, Some(TRANSLATION_FOCUS_GRACE), false),
            TranslationFocusOutcome::Hide
        );
    }

    #[test]
    fn ignores_translation_blur_events_for_hidden_or_still_focused_windows() {
        assert_eq!(
            translation_focus_outcome(false, Some(Duration::from_secs(5)), false),
            TranslationFocusOutcome::IgnoreHidden
        );
        // 事件送达时窗口仍是 key window，说明这是一条过期事件。
        assert_eq!(
            translation_focus_outcome(true, Some(Duration::from_secs(5)), true),
            TranslationFocusOutcome::IgnoreStale
        );
        assert_eq!(
            translation_focus_outcome(true, Some(Duration::from_secs(5)), false),
            TranslationFocusOutcome::Hide
        );
    }

    #[test]
    fn restores_source_app_only_for_explicit_closes() {
        assert!(should_restore_source_app("escape"));
        assert!(should_restore_source_app("button"));
        assert!(should_restore_source_app("close-requested"));
        assert!(!should_restore_source_app("focus-lost"));
        assert!(!should_restore_source_app("settings"));
        assert!(!should_restore_source_app("accessibility"));
        assert!(!should_restore_source_app("unknown"));
    }

    #[test]
    fn shortcut_events_use_release_for_translation_and_press_for_capture() {
        assert!(!should_start_translation(ShortcutState::Pressed));
        assert!(should_start_translation(ShortcutState::Released));
        assert!(should_start_capture(ShortcutState::Pressed));
        assert!(!should_start_capture(ShortcutState::Released));
    }

    #[test]
    fn detects_launch_from_autostart_argument() {
        assert!(launched_from_autostart(&["--startup".into()]));
        assert!(launched_from_autostart(&[
            "--other".into(),
            "--startup".into()
        ]));
        assert!(!launched_from_autostart(&[]));
        assert!(!launched_from_autostart(&["--startup=false".into()]));
    }

    #[cfg(all(windows, not(debug_assertions)))]
    #[test]
    fn validates_windows_autostart_executable_and_arguments() {
        let expected_executable = r"C:\Program Files\S-Tools\S-Tools.exe";
        assert!(windows_autostart_matches(
            r#""C:\Program Files\S-Tools\S-Tools.exe" --startup"#,
            expected_executable,
            &["--startup"],
        ));
        assert!(!windows_autostart_matches(
            r"D:\studyspace\S-Tools\src-tauri\target\debug\s-tools.exe",
            expected_executable,
            &["--startup"],
        ));
        assert!(!windows_autostart_matches(
            r#""C:\Program Files\S-Tools\S-Tools.exe""#,
            expected_executable,
            &["--startup"],
        ));
    }

    #[test]
    fn translation_window_flips_and_clamps_inside_work_area() {
        assert_eq!(
            translation_window_position(100.0, 100.0, 440.0, 360.0, 0.0, 0.0, 800.0, 600.0),
            (100.0, 100.0)
        );
        assert_eq!(
            translation_window_position(700.0, 500.0, 440.0, 360.0, 0.0, 0.0, 800.0, 600.0),
            (260.0, 140.0)
        );
        assert_eq!(
            translation_window_position(-1700.0, 200.0, 440.0, 360.0, -1920.0, 0.0, 1920.0, 1080.0,),
            (-1700.0, 200.0)
        );
        assert_eq!(
            translation_window_position(150.0, 100.0, 440.0, 360.0, 0.0, 0.0, 300.0, 200.0),
            (8.0, 8.0)
        );
        assert_eq!(
            translation_window_position(700.0, 500.0, 300.0, 560.0, 0.0, 0.0, 800.0, 600.0),
            (400.0, 8.0)
        );
    }

    #[test]
    fn translation_window_height_is_clamped() {
        assert_eq!(clamp_translation_window_height(100.0), 220.0);
        assert_eq!(clamp_translation_window_height(320.0), 320.0);
        assert_eq!(clamp_translation_window_height(900.0), 560.0);
        assert_eq!(clamp_translation_window_height(f64::NAN), 300.0);
    }

    #[test]
    fn feature_switches_default_to_disabled() {
        let settings: AppSettings = serde_json::from_str(
            r#"{
                "schemaVersion": 1,
                "theme": "light",
                "hotkeys": {"translate": "Alt+D", "screenshot": "F1"}
            }"#,
        )
        .expect("legacy settings");
        assert!(!settings.translation_enabled);
        assert!(!settings.screenshot_enabled);
    }

    #[test]
    fn parses_control_command_a() {
        let shortcut = Shortcut::from_str("Super+Control+A").expect("valid accelerator");
        assert!(shortcut
            .mods
            .contains(tauri_plugin_global_shortcut::Modifiers::SUPER));
        assert!(shortcut
            .mods
            .contains(tauri_plugin_global_shortcut::Modifiers::CONTROL));
    }

    #[test]
    fn tray_icon_has_visible_pixels() {
        for bytes in [
            include_bytes!("../icons/tray.png").as_slice(),
            include_bytes!("../icons/tray-template.png").as_slice(),
        ] {
            let image = image::load_from_memory(bytes).expect("tray icon");
            assert!(image.to_rgba8().pixels().any(|pixel| pixel.0[3] > 0));
        }
    }
}

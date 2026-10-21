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
    time::Duration,
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
const TRANSLATION_WINDOW_WIDTH: f64 = 300.0;
const TRANSLATION_WINDOW_DEFAULT_HEIGHT: f64 = 300.0;
const TRANSLATION_WINDOW_MIN_HEIGHT: f64 = 220.0;
const TRANSLATION_WINDOW_MAX_HEIGHT: f64 = 560.0;

pub struct AppState {
    config: RwLock<AppConfig>,
    sessions: Mutex<HashMap<String, CaptureSession>>,
    capture_ready: Arc<Mutex<HashSet<String>>>,
    pending: Mutex<PendingTranslation>,
    cancellations: Mutex<HashMap<String, CancellationToken>>,
    client: reqwest::Client,
    direct_client: reqwest::Client,
    quitting: AtomicBool,
}

impl AppState {
    fn new(config: AppConfig) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent("S-Tools/1.0.3")
            .connect_timeout(Duration::from_secs(12))
            .timeout(Duration::from_secs(45))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|error| format!("初始化网络客户端失败：{error}"))?;
        let direct_client = reqwest::Client::builder()
            .user_agent("S-Tools/1.0.3")
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
            pending: Mutex::new(PendingTranslation::default()),
            cancellations: Mutex::new(HashMap::new()),
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
    translate::provider_views(&config.providers)
        .into_iter()
        .find(|provider| provider.id == id)
        .ok_or_else(|| "翻译服务不存在".to_string())
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
    let result = translate::perform_translation(client, &provider, &request, cancellation).await;
    if let Ok(mut tokens) = state.cancellations.lock() {
        tokens.remove(&cancellation_key);
    }
    result
}

#[tauri::command]
fn cancel_translation(state: State<'_, AppState>, request_id: String) -> Result<(), String> {
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
    for key in keys {
        let Some(token) = tokens.remove(&key) else {
            continue;
        };
        token.cancel();
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

#[tauri::command]
fn hide_translation_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("translate") {
        window
            .hide()
            .map_err(|error| format!("隐藏翻译窗口失败：{error}"))?;
    }
    Ok(())
}

#[tauri::command]
async fn start_capture(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    ensure_screenshot_enabled(&app)?;
    capture::start_capture(app, &state.sessions, &state.capture_ready).await
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
    capture::complete_capture(
        &app,
        &state.sessions,
        &state.capture_ready,
        &request,
    )
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
    updater::check_update(&app).await
}

#[tauri::command]
async fn download_update(
    app: AppHandle,
    url: String,
    file_name: String,
    on_event: Channel<UpdateDownloadEvent>,
) -> Result<String, String> {
    updater::download_update(&app, url, file_name, on_event).await
}

#[tauri::command]
async fn install_update(
    app: AppHandle,
    url: String,
    file_name: String,
    sha256: Option<String>,
    on_event: Channel<UpdateDownloadEvent>,
) -> Result<UpdateInstallResult, String> {
    updater::install_update(&app, url, file_name, sha256, on_event).await
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

fn show_main_window(app: &AppHandle, section: &str) -> Result<(), String> {
    if let Some(onboarding) = app.get_webview_window("onboarding") {
        let _ = onboarding.close();
    }
    if let Some(main) = app.get_webview_window("main") {
        main.show()
            .map_err(|error| format!("显示主窗口失败：{error}"))?;
        main.set_focus()
            .map_err(|error| format!("聚焦主窗口失败：{error}"))?;
        let _ = app.emit("app://navigate", section);
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
        .inner_size(
            TRANSLATION_WINDOW_WIDTH,
            TRANSLATION_WINDOW_DEFAULT_HEIGHT,
        )
        .min_inner_size(TRANSLATION_WINDOW_WIDTH, TRANSLATION_WINDOW_MIN_HEIGHT)
        .max_inner_size(TRANSLATION_WINDOW_WIDTH, TRANSLATION_WINDOW_MAX_HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .shadow(true)
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
    height.clamp(
        TRANSLATION_WINDOW_MIN_HEIGHT,
        TRANSLATION_WINDOW_MAX_HEIGHT,
    )
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
        .set_size(tauri::LogicalSize::new(
            TRANSLATION_WINDOW_WIDTH,
            height,
        ))
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

async fn open_translation_workflow(app: AppHandle) -> Result<(), String> {
    ensure_translation_enabled(&app)?;
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

fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
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
                        )
                        .await
                        {
                            let _ = handle.emit("app://toast", error);
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
    let quit = MenuItemBuilder::with_id("quit", "退出 S-Tools").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&translate, &screenshot, &settings, &quit])
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
                        let _ = show_main_window(&handle, "overview");
                        let _ = handle.emit("app://toast", error);
                        return;
                    }
                    let state = handle.state::<AppState>();
                    if let Err(error) = capture::start_capture(
                        handle.clone(),
                        &state.sessions,
                        &state.capture_ready,
                    )
                    .await
                    {
                        let _ = handle.emit("app://toast", error);
                    }
                });
            }
            "settings" => {
                let _ = show_main_window(app, "settings");
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
                let _ = show_main_window(tray.app_handle(), "overview");
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
            if let Ok(Some(info)) = updater::check_update(&app).await {
                let _ = app.emit("update://available", info);
            }
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = show_main_window(app, "overview");
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
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
            let _ = capture::ensure_capture_window(&handle);
            register_shortcuts(&handle).map_err(|error| error.to_string())?;
            apply_autostart(&handle, settings.launch_at_login)
                .map_err(|error| error.to_string())?;

            if !settings.has_completed_onboarding {
                show_onboarding(&handle).map_err(|error| error.to_string())?;
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
                    let _ = window.hide();
                }
            }
            tauri::WindowEvent::Focused(false) if window.label() == "translate" => {
                let _ = window.hide();
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
            reveal_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running S-Tools");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_events_use_release_for_translation_and_press_for_capture() {
        assert!(!should_start_translation(ShortcutState::Pressed));
        assert!(should_start_translation(ShortcutState::Released));
        assert!(should_start_capture(ShortcutState::Pressed));
        assert!(!should_start_capture(ShortcutState::Released));
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
            translation_window_position(
                -1700.0,
                200.0,
                440.0,
                360.0,
                -1920.0,
                0.0,
                1920.0,
                1080.0,
            ),
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

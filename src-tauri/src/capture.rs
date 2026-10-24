use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD, Engine};
use image::ImageEncoder;
#[cfg(target_os = "macos")]
use tauri::LogicalPosition;
use tauri::LogicalSize;
#[cfg(not(target_os = "macos"))]
use tauri::PhysicalPosition;
use tauri::{window::Color, AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use uuid::Uuid;
use xcap::Monitor;

use crate::{
    app_log,
    config::data_dir,
    models::{CaptureSession, CompleteCaptureRequest, CompleteCaptureResult, Rect},
};

pub const CAPTURE_WINDOW_LABEL: &str = "capture-overlay";
const CAPTURE_READY_TIMEOUT: Duration = Duration::from_secs(8);

/// Keeps one capture workflow in flight, including the period before a session exists.
#[derive(Default)]
pub struct CaptureGuard {
    active: AtomicBool,
}

impl CaptureGuard {
    pub fn try_begin(&self) -> bool {
        !self.active.swap(true, Ordering::AcqRel)
    }

    pub fn finish(&self) {
        self.active.store(false, Ordering::Release);
    }
}

pub struct CapturePermit<'a> {
    guard: &'a CaptureGuard,
}

impl Drop for CapturePermit<'_> {
    fn drop(&mut self) {
        self.guard.finish();
    }
}

impl CaptureGuard {
    pub fn acquire(&self) -> Option<CapturePermit<'_>> {
        self.try_begin().then_some(CapturePermit { guard: self })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistingCapture {
    Visible,
    Hidden,
}

pub fn existing_capture_state(
    session_exists: bool,
    window_visible: bool,
) -> Option<ExistingCapture> {
    if !session_exists {
        None
    } else if window_visible {
        Some(ExistingCapture::Visible)
    } else {
        Some(ExistingCapture::Hidden)
    }
}

pub fn ensure_capture_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(CAPTURE_WINDOW_LABEL) {
        crate::platform::set_capture_window_level(&window);
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(
        app,
        CAPTURE_WINDOW_LABEL,
        WebviewUrl::App("index.html?view=capture".into()),
    )
    .title("S-Tools 截图")
    .inner_size(800.0, 600.0)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .background_color(Color(11, 12, 14, 255))
    .accept_first_mouse(true)
    .visible(false)
    .build()
    .map_err(|error| format!("创建截图覆盖窗口失败：{error}"))?;
    crate::platform::set_capture_window_level(&window);
    Ok(window)
}

pub async fn start_capture(
    app: AppHandle,
    sessions: &Mutex<HashMap<String, CaptureSession>>,
    ready: &Arc<Mutex<HashSet<String>>>,
    guard: &CaptureGuard,
) -> Result<(), String> {
    if !crate::selection::screen_recording_allowed() {
        crate::selection::request_screen_recording();
        app_log::warn(&app, "capture", "屏幕录制权限未授予");
        return Err("需要授予屏幕录制权限后才能截图".into());
    }
    let Some(_permit) = guard.acquire() else {
        app_log::warn(&app, "capture", "截图任务正在启动或运行，忽略重复触发");
        return Err("截图任务正在启动或运行，请稍候".into());
    };
    // 窗口可以复用，但旧会话必须先清干净；否则前端可能先看到上一轮截屏。
    let has_session = !sessions
        .lock()
        .map_err(|_| "截图会话锁已损坏".to_string())?
        .is_empty();
    let visible = app
        .get_webview_window(CAPTURE_WINDOW_LABEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if let Some(ExistingCapture::Visible) = existing_capture_state(has_session, visible) {
        app_log::warn(&app, "capture", "已有可见截图任务，忽略本次触发");
        return Err("已有截图任务正在运行".into());
    }
    if has_session {
        let stale_ids = sessions
            .lock()
            .map_err(|_| "截图会话锁已损坏".to_string())?
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        app_log::warn(
            &app,
            "capture",
            format!("清理 {} 个不可见截图会话后重新开始", stale_ids.len()),
        );
        for session_id in stale_ids {
            cleanup_session(&app, sessions, ready, &session_id);
        }
    }
    let window = ensure_capture_window(&app).map_err(|error| {
        app_log::error(&app, "capture", &error);
        error
    })?;
    cleanup_stale_temp(&app)?;
    let foreground_app_id = crate::platform::frontmost_app_id();
    app_log::info(&app, "capture", "开始抓取当前显示器");

    let session_result: Result<CaptureSession, String> = async {
        let cursor = app
            .cursor_position()
            .map_err(|error| format!("读取鼠标位置失败：{error}"))?;
        #[cfg(target_os = "macos")]
        let primary_scale = app
            .primary_monitor()
            .ok()
            .flatten()
            .map(|monitor| monitor.scale_factor())
            .unwrap_or(1.0);
        #[cfg(not(target_os = "macos"))]
        let primary_scale = 1.0;
        let app_for_capture = app.clone();
        let session = tauri::async_runtime::spawn_blocking(move || {
            let mut monitors =
                Monitor::all().map_err(|error| format!("读取显示器失败：{error}"))?;
            let (cursor_x, cursor_y) = monitor_cursor_position(cursor.x, cursor.y, primary_scale);
            let monitor_index = monitors
                .iter()
                .position(|monitor| monitor_contains_cursor(monitor, cursor_x, cursor_y))
                .unwrap_or(0);
            if monitors.is_empty() {
                return Err("没有检测到可截图的显示器".to_string());
            }
            let monitor = monitors.swap_remove(monitor_index);
            let monitor_id = monitor.id().unwrap_or_default();
            let monitor_x = monitor.x().unwrap_or_default() as f64;
            let monitor_y = monitor.y().unwrap_or_default() as f64;
            let monitor_width = monitor.width().unwrap_or_default() as f64;
            let monitor_height = monitor.height().unwrap_or_default() as f64;
            if monitor_width <= 0.0 || monitor_height <= 0.0 {
                return Err("显示器尺寸无效".to_string());
            }
            let backing_scale = monitor.scale_factor().unwrap_or(1.0).max(0.1) as f64;
            let coordinates_are_physical = cfg!(target_os = "windows");
            let (logical_x, logical_y, logical_width, logical_height) = logical_display_frame(
                monitor_x,
                monitor_y,
                monitor_width,
                monitor_height,
                backing_scale,
                coordinates_are_physical,
            );
            #[cfg(target_os = "macos")]
            let (window_x, window_y) = (logical_x, logical_y);
            #[cfg(not(target_os = "macos"))]
            let (window_x, window_y) = {
                let (x, y) = physical_monitor_position(
                    monitor_x,
                    monitor_y,
                    backing_scale,
                    coordinates_are_physical,
                );
                (x as f64, y as f64)
            };
            let window_rects = crate::platform::window_rects_for_display(
                monitor_x,
                monitor_y,
                monitor_width,
                monitor_height,
                backing_scale,
            );

            let session_id = Uuid::new_v4().to_string();
            let directory = data_dir(&app_for_capture)?.join("captures").join("tmp");
            std::fs::create_dir_all(&directory)
                .map_err(|error| format!("创建截图临时目录失败：{error}"))?;
            let image_path = directory.join(format!("{session_id}.png"));
            #[cfg(target_os = "macos")]
            if !capture_display_to_png(monitor_index + 1, &image_path) {
                capture_monitor_to_png(&monitor, &image_path)?;
            }
            #[cfg(not(target_os = "macos"))]
            capture_monitor_to_png(&monitor, &image_path)?;

            Ok::<_, String>(CaptureSession {
                session_id,
                monitor_id,
                display_id: monitor_id,
                image_path: image_path.to_string_lossy().to_string(),
                position_x: window_x,
                position_y: window_y,
                width: logical_width,
                height: logical_height,
                scale_factor: backing_scale,
                screen_frame: Rect {
                    x: logical_x,
                    y: logical_y,
                    width: logical_width,
                    height: logical_height,
                },
                window_rects,
                backing_scale,
                foreground_app_id,
            })
        })
        .await
        .map_err(|error| format!("截图任务失败：{error}"))??;
        Ok(session)
    }
    .await;

    let session = match session_result {
        Ok(session) => session,
        Err(error) => {
            restore_foreground_app(foreground_app_id);
            app_log::error(&app, "capture", &error);
            return Err(error);
        }
    };

    if let Err(error) = sessions
        .lock()
        .map(|mut sessions| {
            sessions.insert(session.session_id.clone(), session.clone());
        })
        .map_err(|_| "截图会话锁已损坏".to_string())
    {
        let _ = std::fs::remove_file(&session.image_path);
        restore_foreground_app(foreground_app_id);
        app_log::error(&app, "capture", &error);
        return Err(error);
    }
    if let Err(error) = ready
        .lock()
        .map(|mut sessions| {
            sessions.remove(&session.session_id);
        })
        .map_err(|_| "截图就绪状态锁已损坏".to_string())
    {
        let _ = sessions
            .lock()
            .map(|mut sessions| sessions.remove(&session.session_id));
        let _ = std::fs::remove_file(&session.image_path);
        restore_foreground_app(foreground_app_id);
        app_log::error(&app, "capture", &error);
        return Err(error);
    }

    let position_result = {
        #[cfg(target_os = "macos")]
        {
            window
                .set_position(LogicalPosition::new(session.position_x, session.position_y))
                .and_then(|_| window.set_size(LogicalSize::new(session.width, session.height)))
        }
        #[cfg(not(target_os = "macos"))]
        {
            window
                .set_position(PhysicalPosition::new(
                    session.position_x.round() as i32,
                    session.position_y.round() as i32,
                ))
                .and_then(|_| window.set_size(LogicalSize::new(session.width, session.height)))
        }
    };
    if let Err(error) = position_result {
        cleanup_session(&app, sessions, ready, &session.session_id);
        let message = format!("设置截图窗口失败：{error}");
        app_log::error(&app, "capture", &message);
        return Err(message);
    }
    #[cfg(windows)]
    if let Err(error) = fit_capture_window_to_display(
        &window,
        session.position_x.round() as i32,
        session.position_y.round() as i32,
        (session.screen_frame.width * session.backing_scale).round() as i32,
        (session.screen_frame.height * session.backing_scale).round() as i32,
    ) {
        // 校准失败时保留 Tauri 已设置的窗口尺寸，避免一次 Win32 抖动打断截图。
        app_log::warn(&app, "capture", format!("校准截图窗口失败：{error}"));
    }
    app_log::info(
        &app,
        "capture",
        format!("已抓图，等待截图界面就绪：{}", session.session_id),
    );
    start_ready_watchdog(app.clone(), session.session_id.clone(), ready.clone());
    // WebView 重建与事件监听存在天然竞态，前端挂载后还会主动拉取当前会话。
    let _ = window.emit("capture://started", session.clone());
    Ok(())
}

#[cfg(target_os = "macos")]
fn capture_display_to_png(display_number: usize, path: &Path) -> bool {
    let display_argument = format!("-D{display_number}");
    let Ok(status) = std::process::Command::new("/usr/sbin/screencapture")
        .args(["-x", "-t", "png", display_argument.as_str()])
        .arg(path)
        .status()
    else {
        return false;
    };
    status.success()
        && std::fs::metadata(path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false)
}

fn capture_monitor_to_png(monitor: &Monitor, path: &Path) -> Result<(), String> {
    let image = monitor
        .capture_image()
        .map_err(|error| format!("捕获屏幕失败：{error}"))?;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(
        &mut bytes,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Sub,
    )
    .write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|error| format!("编码截图失败：{error}"))?;
    std::fs::write(path, bytes).map_err(|error| format!("保存临时截图失败：{error}"))
}

fn fit_capture_window_to_display(
    window: &tauri::WebviewWindow,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<(), String> {
    // Windows 专属窗口边界处理放在独立模块，避免非 Windows 目标解析 hwnd 等平台 API。
    #[cfg(windows)]
    return crate::platform::fit_capture_window_to_display(window, x, y, width, height);
    #[cfg(not(windows))]
    {
        let _ = (window, x, y, width, height);
        Ok(())
    }
}

fn start_ready_watchdog(app: AppHandle, session_id: String, ready: Arc<Mutex<HashSet<String>>>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CAPTURE_READY_TIMEOUT).await;
        let some_state = app.try_state::<crate::AppState>();
        let is_ready = ready
            .lock()
            .map(|sessions| sessions.contains(&session_id))
            .unwrap_or(false);
        let is_visible = app
            .get_webview_window(CAPTURE_WINDOW_LABEL)
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false);
        if is_ready && is_visible {
            return;
        }
        let still_exists = some_state
            .as_ref()
            .and_then(|state| state.sessions.lock().ok())
            .map(|sessions| sessions.contains_key(&session_id))
            .unwrap_or(false);
        if !still_exists {
            return;
        }
        app_log::warn(
            &app,
            "capture",
            format!(
                "截图界面 {} 秒未就绪（ready={is_ready}, visible={is_visible}）",
                CAPTURE_READY_TIMEOUT.as_secs()
            ),
        );
        if let Some(state) = some_state.as_ref() {
            cleanup_session(&app, &state.sessions, &state.capture_ready, &session_id);
            app_log::error(
                &app,
                "capture",
                "截图界面未能正常显示，已自动结束本次任务，请重试",
            );
            return;
        }
    });
}

pub fn show_capture_window(
    app: &AppHandle,
    sessions: &Mutex<HashMap<String, CaptureSession>>,
    ready: &Arc<Mutex<HashSet<String>>>,
    session_id: &str,
) -> Result<(), String> {
    if !sessions
        .lock()
        .map_err(|_| "截图会话锁已损坏".to_string())?
        .contains_key(session_id)
    {
        return Err("截图会话不存在或已过期".into());
    }
    let window = app
        .get_webview_window(CAPTURE_WINDOW_LABEL)
        .ok_or_else(|| "截图覆盖窗口不存在".to_string())?;
    crate::platform::set_capture_window_level(&window);
    ready
        .lock()
        .map_err(|_| "截图就绪状态锁已损坏".to_string())?
        .insert(session_id.to_string());
    if let Err(error) = window.show().and_then(|_| window.set_focus()) {
        if let Ok(mut sessions) = ready.lock() {
            sessions.remove(session_id);
        }
        return Err(format!("显示截图窗口失败：{error}"));
    }
    app_log::info(app, "capture", format!("截图界面已显示：{session_id}"));
    Ok(())
}

pub async fn capture_image_bytes(
    sessions: &Mutex<HashMap<String, CaptureSession>>,
    session_id: &str,
) -> Result<Vec<u8>, String> {
    let path = sessions
        .lock()
        .map_err(|_| "截图会话锁已损坏".to_string())?
        .get(session_id)
        .map(|session| PathBuf::from(&session.image_path))
        .ok_or_else(|| "截图会话不存在或已过期".to_string())?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|error| format!("读取截图图像失败：{error}"))?;
    if bytes.len() > 60 * 1024 * 1024 {
        return Err("截图图像过大".into());
    }
    Ok(bytes)
}

pub fn active_session(sessions: &Mutex<HashMap<String, CaptureSession>>) -> Option<CaptureSession> {
    sessions
        .lock()
        .ok()
        .and_then(|sessions| sessions.values().next().cloned())
}

pub fn cleanup_stale_temp(app: &AppHandle) -> Result<(), String> {
    let directory = data_dir(app)?.join("captures").join("tmp");
    if !directory.exists() {
        return Ok(());
    }
    for entry in
        std::fs::read_dir(&directory).map_err(|error| format!("读取截图临时目录失败：{error}"))?
    {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(())
}

fn monitor_contains_cursor(monitor: &Monitor, cursor_x: f64, cursor_y: f64) -> bool {
    let x = monitor.x().unwrap_or_default() as f64;
    let y = monitor.y().unwrap_or_default() as f64;
    let width = monitor.width().unwrap_or_default() as f64;
    let height = monitor.height().unwrap_or_default() as f64;
    point_inside_monitor(cursor_x, cursor_y, x, y, width, height)
}

fn point_inside_monitor(
    point_x: f64,
    point_y: f64,
    monitor_x: f64,
    monitor_y: f64,
    monitor_width: f64,
    monitor_height: f64,
) -> bool {
    point_x >= monitor_x
        && point_x < monitor_x + monitor_width
        && point_y >= monitor_y
        && point_y < monitor_y + monitor_height
}

fn monitor_cursor_position(cursor_x: f64, cursor_y: f64, primary_scale: f64) -> (f64, f64) {
    #[cfg(target_os = "macos")]
    return (
        cursor_x / primary_scale.max(0.1),
        cursor_y / primary_scale.max(0.1),
    );
    #[cfg(not(target_os = "macos"))]
    return (cursor_x, cursor_y);
}

fn logical_display_frame(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
    coordinates_are_physical: bool,
) -> (f64, f64, f64, f64) {
    if coordinates_are_physical {
        (x / scale, y / scale, width / scale, height / scale)
    } else {
        (x, y, width, height)
    }
}

#[cfg(any(not(target_os = "macos"), test))]
fn physical_monitor_position(
    x: f64,
    y: f64,
    scale: f64,
    coordinates_are_physical: bool,
) -> (i32, i32) {
    if coordinates_are_physical {
        (x.round() as i32, y.round() as i32)
    } else {
        ((x * scale).round() as i32, (y * scale).round() as i32)
    }
}

fn restore_foreground_app(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        crate::platform::activate_app(pid);
    });
}

pub fn complete_capture(
    app: &AppHandle,
    sessions: &Mutex<HashMap<String, CaptureSession>>,
    ready: &Arc<Mutex<HashSet<String>>>,
    request: &CompleteCaptureRequest,
) -> Result<CompleteCaptureResult, String> {
    if !sessions
        .lock()
        .map_err(|_| "截图会话锁已损坏".to_string())?
        .contains_key(&request.session_id)
    {
        return Err("截图会话不存在或已过期".into());
    }

    let encoded = request
        .data_url
        .split_once(',')
        .map(|(_, value)| value)
        .ok_or_else(|| "截图数据格式无效".to_string())?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|error| format!("解码截图失败：{error}"))?;
    if bytes.len() > 60 * 1024 * 1024 {
        return Err("截图数据过大，无法保存".into());
    }
    if !matches!(request.action.as_str(), "copy" | "save") {
        return Err("未知截图操作".into());
    }
    let image =
        image::load_from_memory(&bytes).map_err(|error| format!("读取截图图像失败：{error}"))?;
    let file_name = format!(
        "S-Tools-{}.png",
        chrono::Local::now().format("%Y%m%d-%H%M%S-%3f")
    );

    let mut output_path = None;
    let message;
    if request.action == "copy" {
        copy_image(&image)?;
        message = "截图已复制到剪贴板".into();
    } else {
        let output = if let Some(path) = request.save_path.as_deref() {
            let path = PathBuf::from(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("创建保存目录失败：{error}"))?;
            }
            save_image(&image, &path)?;
            path
        } else {
            save_to_pictures(app, &image, &file_name)?
        };
        message = format!("截图已保存到 {}", output.display());
        output_path = Some(output.to_string_lossy().to_string());
    }

    cleanup_session(app, sessions, ready, &request.session_id);
    Ok(CompleteCaptureResult {
        path: output_path,
        message,
    })
}

pub fn copy_image(image: &image::DynamicImage) -> Result<(), String> {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("访问剪贴板失败：{error}"))?;
    clipboard
        .set_image(arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: Cow::Owned(rgba.into_raw()),
        })
        .map_err(|error| format!("复制截图到剪贴板失败：{error}"))
}

fn save_image(image: &image::DynamicImage, path: &Path) -> Result<(), String> {
    image
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|error| format!("保存截图失败：{error}"))
}

fn save_to_pictures(
    app: &AppHandle,
    image: &image::DynamicImage,
    file_name: &str,
) -> Result<PathBuf, String> {
    let base = app
        .path()
        .picture_dir()
        .unwrap_or_else(|_| data_dir(app).unwrap_or_default().join("exports"));
    let directory = base.join("S-Tools");
    std::fs::create_dir_all(&directory).map_err(|error| format!("创建图片目录失败：{error}"))?;
    let target = unique_path(directory.join(file_name));
    save_image(image, &target)?;
    Ok(target)
}

fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "S-Tools".into());
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "png".into());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for index in 2..1000 {
        let candidate = parent.join(format!("{stem}-{index}.{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    path
}

pub fn hide_capture_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(CAPTURE_WINDOW_LABEL) {
        let _ = window.hide();
    }
}

pub fn cleanup_session(
    app: &AppHandle,
    sessions: &Mutex<HashMap<String, CaptureSession>>,
    ready: &Arc<Mutex<HashSet<String>>>,
    session_id: &str,
) {
    let mut foreground_app_id = None;
    if let Ok(mut map) = sessions.lock() {
        if let Some(session) = map.remove(session_id) {
            let _ = std::fs::remove_file(session.image_path);
            foreground_app_id = session.foreground_app_id;
        }
    }
    if let Ok(mut sessions) = ready.lock() {
        sessions.remove(session_id);
    }
    hide_capture_window(app);
    app_log::info(app, "capture", format!("截图会话已结束：{session_id}"));
    let _ = app.emit("capture://ended", ());
    restore_foreground_app(foreground_app_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CaptureSession;

    #[test]
    #[ignore = "requires an interactive desktop with screen recording permission"]
    fn capture_primary_monitor_smoke_test() {
        let monitors = Monitor::all().expect("monitor list");
        let monitor = monitors.first().expect("at least one monitor");
        let image = monitor.capture_image().expect("capture screen");
        assert!(image.width() > 0);
        assert!(image.height() > 0);
    }

    #[tokio::test]
    async fn capture_image_bytes_returns_raw_png_payload() {
        let path = std::env::temp_dir().join(format!("stools-test-{}.png", Uuid::new_v4()));
        let payload = b"not-a-real-png-but-valid-for-ipc-test";
        std::fs::write(&path, payload).expect("write fixture");
        let session_id = "test-session".to_string();
        let sessions = Mutex::new(HashMap::from([(
            session_id.clone(),
            CaptureSession {
                session_id: session_id.clone(),
                monitor_id: 1,
                display_id: 1,
                image_path: path.to_string_lossy().to_string(),
                position_x: 0.0,
                position_y: 0.0,
                width: 100.0,
                height: 100.0,
                scale_factor: 1.0,
                screen_frame: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
                window_rects: Vec::new(),
                backing_scale: 1.0,
                foreground_app_id: None,
            },
        )]));
        let data = capture_image_bytes(&sessions, &session_id)
            .await
            .expect("raw bytes");
        assert_eq!(data, payload);
        assert_eq!(
            active_session(&sessions).map(|session| session.session_id),
            Some(session_id)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn converts_retina_monitor_frame_to_logical_window_geometry() {
        assert_eq!(
            logical_display_frame(0.0, 0.0, 2304.0, 1296.0, 2.0, false),
            (0.0, 0.0, 2304.0, 1296.0)
        );
        assert_eq!(physical_monitor_position(0.0, 0.0, 2.0, false), (0, 0));
    }

    #[test]
    fn keeps_visible_capture_sessions_and_recovers_hidden_ones() {
        assert_eq!(existing_capture_state(false, false), None);
        assert_eq!(
            existing_capture_state(true, true),
            Some(ExistingCapture::Visible)
        );
        assert_eq!(
            existing_capture_state(true, false),
            Some(ExistingCapture::Hidden)
        );
    }

    #[test]
    fn capture_guard_rejects_concurrent_starts_and_releases_on_drop() {
        let guard = CaptureGuard::default();
        let permit = guard.acquire().expect("first capture starts");
        assert!(guard.acquire().is_none());

        drop(permit);
        assert!(guard.acquire().is_some());
    }

    #[test]
    fn converts_windows_scaled_monitor_frame_and_position() {
        assert_eq!(
            logical_display_frame(1920.0, 0.0, 2560.0, 1440.0, 1.25, true),
            (1536.0, 0.0, 2048.0, 1152.0)
        );
        assert_eq!(
            physical_monitor_position(1920.0, 0.0, 1.25, true),
            (1920, 0)
        );
    }

    #[test]
    fn converts_primary_scale_cursor_before_monitor_lookup() {
        #[cfg(target_os = "macos")]
        assert_eq!(monitor_cursor_position(6000.0, 240.0, 2.0), (3000.0, 120.0));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(monitor_cursor_position(3000.0, 240.0, 2.0), (3000.0, 240.0));
    }

    #[test]
    fn matches_secondary_monitor_with_mixed_scale_coordinates() {
        assert!(point_inside_monitor(
            3000.0, 300.0, 2304.0, 114.0, 1920.0, 1080.0
        ));
        assert!(!point_inside_monitor(
            6000.0, 300.0, 2304.0, 114.0, 1920.0, 1080.0
        ));
    }
}

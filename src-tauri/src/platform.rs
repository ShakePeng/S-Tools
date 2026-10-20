use crate::models::Rect;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn stools_frontmost_app_pid() -> u32;
    fn stools_activate_app_pid(pid: u32);
    fn stools_window_rects_json(
        display_x: f64,
        display_y: f64,
        display_width: f64,
        display_height: f64,
    ) -> *mut std::os::raw::c_char;
    fn stools_free_string(value: *mut std::os::raw::c_char);
}

pub fn frontmost_app_id() -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        let pid = unsafe { stools_frontmost_app_pid() };
        return (pid > 0).then_some(pid);
    }
    #[cfg(target_os = "windows")]
    {
        return windows_frontmost_pid();
    }
    #[allow(unreachable_code)]
    None
}

pub fn activate_app(pid: u32) {
    #[cfg(target_os = "macos")]
    unsafe {
        stools_activate_app_pid(pid);
    }
    #[cfg(target_os = "windows")]
    windows_activate_pid(pid);
}

pub fn window_rects_for_display(
    display_x: f64,
    display_y: f64,
    display_width: f64,
    display_height: f64,
    _display_scale: f64,
) -> Vec<Rect> {
    #[cfg(target_os = "macos")]
    {
        let pointer = unsafe {
            stools_window_rects_json(display_x, display_y, display_width, display_height)
        };
        if pointer.is_null() {
            return Vec::new();
        }
        let text = unsafe { std::ffi::CStr::from_ptr(pointer) }
            .to_string_lossy()
            .to_string();
        unsafe { stools_free_string(pointer) };
        return serde_json::from_str(&text).unwrap_or_default();
    }
    #[cfg(target_os = "windows")]
    {
        return windows_window_rects(
            display_x,
            display_y,
            display_width,
            display_height,
            _display_scale,
        );
    }
    #[allow(unreachable_code)]
    Vec::new()
}

#[cfg(target_os = "windows")]
fn windows_frontmost_pid() -> Option<u32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };
    unsafe {
        let window = GetForegroundWindow();
        if window == 0 {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, &mut pid);
        (pid > 0).then_some(pid)
    }
}

#[cfg(target_os = "windows")]
fn windows_activate_pid(pid: u32) {
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    };

    static TARGET_PID: AtomicU32 = AtomicU32::new(0);
    TARGET_PID.store(pid, Ordering::Relaxed);

    unsafe extern "system" fn callback(window: isize, _: isize) -> i32 {
        if !unsafe { IsWindowVisible(window) } {
            return 1;
        }
        let mut owner = 0u32;
        unsafe {
            GetWindowThreadProcessId(window, &mut owner);
        }
        if owner == TARGET_PID.load(Ordering::Relaxed) {
            unsafe {
                SetForegroundWindow(window);
            }
            return 0;
        }
        1
    }

    unsafe {
        EnumWindows(Some(callback), 0);
    }
}

#[cfg(target_os = "windows")]
fn windows_window_rects(
    display_x: f64,
    display_y: f64,
    display_width: f64,
    display_height: f64,
    display_scale: f64,
) -> Vec<Rect> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };

    #[derive(Default)]
    struct Context {
        own_pid: u32,
        display_x: f64,
        display_y: f64,
        display_width: f64,
        display_height: f64,
        display_scale: f64,
        rectangles: Vec<Rect>,
    }

    unsafe extern "system" fn callback(window: isize, parameter: isize) -> i32 {
        let context = unsafe { &mut *(parameter as *mut Context) };
        if !unsafe { IsWindowVisible(window) } || unsafe { IsIconic(window) } {
            return 1;
        }
        if unsafe { GetWindowTextLengthW(window) } == 0 {
            return 1;
        }
        let mut class_name = [0u16; 128];
        let class_length = unsafe { GetClassNameW(window, class_name.as_mut_ptr(), 128) };
        if class_length > 0 {
            let class_name = String::from_utf16_lossy(&class_name[..class_length as usize]);
            if is_system_window_class(&class_name) {
                return 1;
            }
        }
        let mut owner = 0u32;
        unsafe {
            GetWindowThreadProcessId(window, &mut owner);
        }
        if owner == context.own_pid {
            return 1;
        }
        let ex_style = unsafe { GetWindowLongW(window, GWL_EXSTYLE) } as u32;
        if ex_style & WS_EX_TOOLWINDOW != 0 {
            return 1;
        }
        let mut raw = unsafe { std::mem::zeroed() };
        if unsafe { GetWindowRect(window, &mut raw) } == 0 {
            return 1;
        }
        if let Some(rect) = clip_and_scale_window_rect(
            Rect {
                x: raw.left as f64,
                y: raw.top as f64,
                width: (raw.right - raw.left) as f64,
                height: (raw.bottom - raw.top) as f64,
            },
            Rect {
                x: context.display_x,
                y: context.display_y,
                width: context.display_width,
                height: context.display_height,
            },
            context.display_scale,
        ) {
            context.rectangles.push(rect);
        }
        1
    }

    let mut context = Context {
        own_pid: std::process::id(),
        display_x,
        display_y,
        display_width,
        display_height,
        display_scale,
        rectangles: Vec::new(),
    };
    let pointer = &mut context as *mut Context as isize;
    unsafe {
        EnumWindows(Some(callback), pointer);
    }
    context.rectangles
}

#[cfg(any(target_os = "windows", test))]
fn clip_and_scale_window_rect(window: Rect, display: Rect, display_scale: f64) -> Option<Rect> {
    let scale = if display_scale.is_finite() && display_scale > 0.0 {
        display_scale
    } else {
        1.0
    };
    let visible_left = window.x.max(display.x);
    let visible_top = window.y.max(display.y);
    let visible_right = (window.x + window.width).min(display.x + display.width);
    let visible_bottom = (window.y + window.height).min(display.y + display.height);
    let width = visible_right - visible_left;
    let height = visible_bottom - visible_top;
    if width < 8.0 * scale || height < 8.0 * scale {
        return None;
    }
    Some(Rect {
        x: (visible_left - display.x) / scale,
        y: (visible_top - display.y) / scale,
        width: width / scale,
        height: height / scale,
    })
}

#[cfg(any(target_os = "windows", test))]
fn is_system_window_class(class_name: &str) -> bool {
    matches!(
        class_name,
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "Windows.UI.Core.CoreWindow"
            | "XamlExplorerHostIslandWindow"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_windows_physical_window_rect_to_local_logical_points() {
        let rect = clip_and_scale_window_rect(
            Rect {
                x: 400.0,
                y: 350.0,
                width: 1200.0,
                height: 900.0,
            },
            Rect {
                x: 100.0,
                y: 50.0,
                width: 3000.0,
                height: 2000.0,
            },
            1.5,
        )
        .expect("visible window");
        assert_eq!(
            rect,
            Rect {
                x: 200.0,
                y: 200.0,
                width: 800.0,
                height: 600.0,
            }
        );
    }

    #[test]
    fn preserves_negative_secondary_display_origin() {
        let rect = clip_and_scale_window_rect(
            Rect {
                x: -1800.0,
                y: 100.0,
                width: 1000.0,
                height: 600.0,
            },
            Rect {
                x: -1920.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            1.0,
        )
        .expect("visible window");
        assert_eq!(
            rect,
            Rect {
                x: 120.0,
                y: 100.0,
                width: 1000.0,
                height: 600.0,
            }
        );
    }

    #[test]
    fn filters_desktop_and_taskbar_window_classes() {
        assert!(is_system_window_class("Progman"));
        assert!(is_system_window_class("Shell_TrayWnd"));
        assert!(!is_system_window_class("Chrome_WidgetWin_1"));
    }
}

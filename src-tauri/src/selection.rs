#[derive(Clone, Debug)]
pub struct CapturedSelection {
    pub text: String,
    pub source: String,
    pub notice: Option<String>,
}

#[cfg(target_os = "macos")]
pub fn accessibility_allowed() -> bool {
    unsafe { stools_ax_is_trusted() }
}

#[cfg(not(target_os = "macos"))]
pub fn accessibility_allowed() -> bool {
    true
}

#[cfg(target_os = "macos")]
pub fn screen_recording_allowed() -> bool {
    unsafe { stools_screen_recording_allowed() }
}

#[cfg(not(target_os = "macos"))]
pub fn screen_recording_allowed() -> bool {
    true
}

#[cfg(target_os = "macos")]
pub fn request_screen_recording() {
    unsafe { stools_request_screen_recording() }
}

#[cfg(not(target_os = "macos"))]
pub fn request_screen_recording() {}

pub fn capture_selection() -> CapturedSelection {
    #[cfg(target_os = "macos")]
    {
        if let Some(text) = macos_selected_text() {
            return CapturedSelection {
                text,
                source: "accessibility".into(),
                notice: None,
            };
        }
        if let Some(text) = macos_copy_selected_text() {
            return CapturedSelection {
                text,
                source: "clipboard".into(),
                notice: Some("已通过复制读取选区，原剪贴板已尽力恢复".into()),
            };
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(text) = windows_copy_selected_text() {
            return CapturedSelection {
                text,
                source: "clipboard".into(),
                notice: Some("已通过复制读取选区，原文本剪贴板已恢复".into()),
            };
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(text) = clipboard_selected_text_fallback() {
            return CapturedSelection {
                text,
                source: "clipboard".into(),
                notice: Some("已通过剪贴板读取选区".into()),
            };
        }
    }

    CapturedSelection {
        text: String::new(),
        source: "empty".into(),
        notice: Some("未能读取选中文本，请检查辅助功能权限或直接输入".into()),
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn stools_ax_is_trusted() -> bool;
    fn stools_screen_recording_allowed() -> bool;
    fn stools_request_screen_recording();
    fn stools_selected_text_ax() -> *mut std::os::raw::c_char;
    fn stools_copy_selected_text() -> *mut std::os::raw::c_char;
    fn stools_free_string(value: *mut std::os::raw::c_char);
}

#[cfg(target_os = "macos")]
fn take_ffi_string(pointer: *mut std::os::raw::c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let value = unsafe { std::ffi::CStr::from_ptr(pointer) }
        .to_string_lossy()
        .trim()
        .to_string();
    unsafe { stools_free_string(pointer) };
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(target_os = "macos")]
fn macos_selected_text() -> Option<String> {
    take_ffi_string(unsafe { stools_selected_text_ax() })
}

#[cfg(target_os = "macos")]
fn macos_copy_selected_text() -> Option<String> {
    take_ffi_string(unsafe { stools_copy_selected_text() })
}

#[cfg(target_os = "windows")]
fn windows_copy_selected_text() -> Option<String> {
    use std::{
        mem::size_of,
        thread,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_C, VK_CONTROL,
    };

    let mut clipboard = arboard::Clipboard::new().ok()?;
    let previous_text = clipboard.get_text().ok();
    let previous_image = clipboard.get_image().ok();
    // 不认识的剪贴板格式没有可靠恢复手段，此时宁可不使用复制降级，也不能清空用户剪贴板。
    if previous_text.is_none() && previous_image.is_none() {
        return None;
    }
    let sentinel = format!("__s_tools_clipboard_{}__", uuid::Uuid::new_v4());
    clipboard.set_text(&sentinel).ok()?;

    #[allow(non_snake_case)]
    unsafe {
        let mut inputs: [INPUT; 4] = std::mem::zeroed();
        inputs[0].r#type = INPUT_KEYBOARD;
        inputs[0].Anonymous.ki = KEYBDINPUT {
            wVk: VK_CONTROL as u16,
            wScan: 0,
            dwFlags: 0,
            time: 0,
            dwExtraInfo: 0,
        };
        inputs[1].r#type = INPUT_KEYBOARD;
        inputs[1].Anonymous.ki = KEYBDINPUT {
            wVk: VK_C as u16,
            wScan: 0,
            dwFlags: 0,
            time: 0,
            dwExtraInfo: 0,
        };
        inputs[2].r#type = INPUT_KEYBOARD;
        inputs[2].Anonymous.ki = KEYBDINPUT {
            wVk: VK_C as u16,
            wScan: 0,
            dwFlags: KEYEVENTF_KEYUP,
            time: 0,
            dwExtraInfo: 0,
        };
        inputs[3].r#type = INPUT_KEYBOARD;
        inputs[3].Anonymous.ki = KEYBDINPUT {
            wVk: VK_CONTROL as u16,
            wScan: 0,
            dwFlags: KEYEVENTF_KEYUP,
            time: 0,
            dwExtraInfo: 0,
        };
        let sent = SendInput(4, inputs.as_ptr(), size_of::<INPUT>() as i32);
        if sent != 4 {
            restore_windows_clipboard(&mut clipboard, previous_text, previous_image);
            return None;
        }
    }

    let deadline = Instant::now() + Duration::from_millis(800);
    let mut captured = None;
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
        if let Ok(value) = clipboard.get_text() {
            if value != sentinel && !value.is_empty() {
                captured = Some(value);
                break;
            }
        }
    }
    restore_windows_clipboard(&mut clipboard, previous_text, previous_image);
    captured
}

#[cfg(target_os = "windows")]
fn restore_windows_clipboard(
    clipboard: &mut arboard::Clipboard,
    text: Option<String>,
    image: Option<arboard::ImageData<'static>>,
) {
    if let Some(image) = image {
        let _ = clipboard.set_image(image);
    } else if let Some(text) = text {
        let _ = clipboard.set_text(text);
    } else {
        let _ = clipboard.clear();
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn clipboard_selected_text_fallback() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

pub fn truncate_selection(value: String) -> (String, Option<String>) {
    const MAX_CHARS: usize = 8000;
    if value.chars().count() <= MAX_CHARS {
        return (value, None);
    }
    (
        value.chars().take(MAX_CHARS).collect(),
        Some("选中文本过长，已截取前 8000 个字符".into()),
    )
}

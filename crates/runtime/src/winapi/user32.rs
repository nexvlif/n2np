use crate::gui;
use crate::utf;

winapi_api!(
    "user32.dll",
    "MessageBoxA",
    pub unsafe extern "win64" fn message_box_a(
        _window: usize,
        text: *const u8,
        caption: *const u8,
        message_type: u32,
    ) -> i32 {
        let text = utf::read_ansi(text);
        let caption = utf::read_ansi(caption);
        gui::message_box(&text, &caption, message_type)
    }
);

winapi_api!(
    "user32.dll",
    "MessageBoxW",
    pub unsafe extern "win64" fn message_box_w(
        _window: usize,
        text: *const u16,
        caption: *const u16,
        message_type: u32,
    ) -> i32 {
        let text = utf::read_wide(text);
        let caption = utf::read_wide(caption);
        gui::message_box(&text, &caption, message_type)
    }
);

winapi_api!(
    "user32.dll",
    "GetDesktopWindow",
    pub unsafe extern "win64" fn get_desktop_window() -> usize {
        0x10001
    }
);

winapi_api!(
    "user32.dll",
    "PostQuitMessage",
    pub unsafe extern "win64" fn post_quit_message(code: i32) {
        log::info!("[n2np-user32] PostQuitMessage({code})");
    }
);

winapi_api!(
    "user32.dll",
    "GetSystemMetrics",
    pub unsafe extern "win64" fn get_system_metrics(index: i32) -> i32 {
        match index {
            0 => 1920,
            1 => 1080,
            4 => 24,
            5 => 24,
            6 => 1,
            7 => 1,
            11 => 32,
            15 => 32,
            16 => 32,
            20 => 4,
            21 => 4,
            43 => 4,
            44 => 4,
            45 => 8,
            80 => 1,
            _ => 0,
        }
    }
);

winapi_api!(
    "user32.dll",
    "GetDoubleClickTime",
    pub unsafe extern "win64" fn get_double_click_time() -> u32 {
        500
    }
);

winapi_api!(
    "user32.dll",
    "MessageBeep",
    pub unsafe extern "win64" fn message_beep(_message_type: u32) -> i32 {
        1
    }
);

winapi_api!(
    "user32.dll",
    "GetDC",
    pub unsafe extern "win64" fn get_dc(_window: usize) -> usize {
        0
    }
);

winapi_api!(
    "user32.dll",
    "ReleaseDC",
    pub unsafe extern "win64" fn release_dc(_window: usize, _dc: usize) -> i32 {
        1
    }
);

winapi_api!(
    "user32.dll",
    "SetCursorPos",
    pub unsafe extern "win64" fn set_cursor_pos(_x: i32, _y: i32) -> i32 {
        1
    }
);

winapi_api!(
    "user32.dll",
    "GetCursorPos",
    pub unsafe extern "win64" fn get_cursor_pos(_point: *mut i32) -> i32 {
        1
    }
);

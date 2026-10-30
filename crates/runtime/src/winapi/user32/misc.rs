//! Small stubs and queries that do not need the message queue.

winapi_api!(
    "user32.dll",
    "LoadCursorA",
    pub unsafe extern "win64" fn load_cursor_a(_instance: usize, name: *const u8) -> usize {
        let value = name as usize;
        if value <= 0xFFFF {
            0x0001_0000 + value
        } else {
            0x0001_0000
        }
    }
);

winapi_api!(
    "user32.dll",
    "LoadCursorW",
    pub unsafe extern "win64" fn load_cursor_w(_instance: usize, name: *const u16) -> usize {
        let value = name as usize;
        if value <= 0xFFFF {
            0x0001_0000 + value
        } else {
            0x0001_0000
        }
    }
);

winapi_api!(
    "user32.dll",
    "LoadIconA",
    pub unsafe extern "win64" fn load_icon_a(_instance: usize, name: *const u8) -> usize {
        let value = name as usize;
        0x0002_0000 + (value & 0xFFFF)
    }
);

winapi_api!(
    "user32.dll",
    "LoadIconW",
    pub unsafe extern "win64" fn load_icon_w(_instance: usize, name: *const u16) -> usize {
        let value = name as usize;
        0x0002_0000 + (value & 0xFFFF)
    }
);

winapi_api!(
    "user32.dll",
    "GetFocus",
    pub unsafe extern "win64" fn get_focus() -> usize {
        0
    }
);

winapi_api!(
    "user32.dll",
    "SetFocus",
    pub unsafe extern "win64" fn set_focus(_hwnd: usize) -> usize {
        0
    }
);

winapi_api!(
    "user32.dll",
    "SetCapture",
    pub unsafe extern "win64" fn set_capture(_hwnd: usize) -> usize {
        0
    }
);

winapi_api!(
    "user32.dll",
    "ReleaseCapture",
    pub unsafe extern "win64" fn release_capture() -> i32 {
        1
    }
);

winapi_api!(
    "user32.dll",
    "GetKeyState",
    pub unsafe extern "win64" fn get_key_state(_key: i32) -> i16 {
        0
    }
);

winapi_api!(
    "user32.dll",
    "GetAsyncKeyState",
    pub unsafe extern "win64" fn get_async_key_state(_key: i32) -> i16 {
        0
    }
);

winapi_api!(
    "user32.dll",
    "GetParent",
    pub unsafe extern "win64" fn get_parent(_hwnd: usize) -> usize {
        0
    }
);

winapi_api!(
    "user32.dll",
    "GetWindow",
    pub unsafe extern "win64" fn get_window(_hwnd: usize, _command: u32) -> usize {
        0
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

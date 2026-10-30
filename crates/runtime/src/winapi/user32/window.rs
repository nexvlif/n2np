//! Window lifecycle, geometry, text, and window-long APIs.

use super::*;

winapi_api!(
    "user32.dll",
    "CreateWindowExA",
    pub unsafe extern "win64" fn create_window_ex_a(
        _ex_style: u32,
        class_name: *const u8,
        window_name: *const u8,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        _parent: usize,
        _menu: usize,
        _instance: usize,
        param: *mut c_void,
    ) -> usize {
        create_window(
            class_name_a(class_name),
            utf::read_ansi(window_name),
            style,
            x,
            y,
            width,
            height,
            param,
        )
    }
);

winapi_api!(
    "user32.dll",
    "CreateWindowExW",
    pub unsafe extern "win64" fn create_window_ex_w(
        _ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        _parent: usize,
        _menu: usize,
        _instance: usize,
        param: *mut c_void,
    ) -> usize {
        create_window(
            class_name_w(class_name),
            utf::read_wide(window_name),
            style,
            x,
            y,
            width,
            height,
            param,
        )
    }
);

winapi_api!(
    "user32.dll",
    "DestroyWindow",
    pub unsafe extern "win64" fn destroy_window(hwnd: usize) -> i32 {
        destroy_window_impl(hwnd)
    }
);

winapi_api!(
    "user32.dll",
    "ShowWindow",
    pub unsafe extern "win64" fn show_window(hwnd: usize, command: i32) -> i32 {
        match gui::show_window(hwnd, command) {
            Some(previous) => i32::from(previous),
            None => 0,
        }
    }
);

winapi_api!(
    "user32.dll",
    "ShowWindowAsync",
    pub unsafe extern "win64" fn show_window_async(hwnd: usize, command: i32) -> i32 {
        show_window(hwnd, command)
    }
);

winapi_api!(
    "user32.dll",
    "UpdateWindow",
    pub unsafe extern "win64" fn update_window(hwnd: usize) -> i32 {
        i32::from(gui::invalidate(hwnd))
    }
);

winapi_api!(
    "user32.dll",
    "InvalidateRect",
    pub unsafe extern "win64" fn invalidate_rect(
        hwnd: usize,
        _rect: *const i32,
        _erase: i32,
    ) -> i32 {
        i32::from(gui::invalidate(hwnd))
    }
);

winapi_api!(
    "user32.dll",
    "ValidateRect",
    pub unsafe extern "win64" fn validate_rect(_hwnd: usize, _rect: *const i32) -> i32 {
        1
    }
);

winapi_api!(
    "user32.dll",
    "GetClientRect",
    pub unsafe extern "win64" fn get_client_rect(hwnd: usize, rect: *mut i32) -> i32 {
        let (width, height) = gui::client_size(hwnd).unwrap_or((0, 0));
        write_rect(rect, 0, 0, width, height);
        1
    }
);

winapi_api!(
    "user32.dll",
    "GetWindowRect",
    pub unsafe extern "win64" fn get_window_rect(hwnd: usize, rect: *mut i32) -> i32 {
        let (width, height) = gui::client_size(hwnd).unwrap_or((0, 0));
        write_rect(rect, 0, 0, width, height);
        1
    }
);

winapi_api!(
    "user32.dll",
    "GetWindowTextA",
    pub unsafe extern "win64" fn get_window_text_a(
        hwnd: usize,
        text: *mut u8,
        max_count: i32,
    ) -> i32 {
        let value = gui::window_text(hwnd).unwrap_or_default();
        let cap = max_count.max(0) as usize;
        utf::write_ansi(text, &value, cap) as i32
    }
);

winapi_api!(
    "user32.dll",
    "GetWindowTextW",
    pub unsafe extern "win64" fn get_window_text_w(
        hwnd: usize,
        text: *mut u16,
        max_count: i32,
    ) -> i32 {
        let value = gui::window_text(hwnd).unwrap_or_default();
        let cap = max_count.max(0) as usize;
        utf::write_wide(text, &value, cap) as i32
    }
);

winapi_api!(
    "user32.dll",
    "GetWindowTextLengthA",
    pub unsafe extern "win64" fn get_window_text_length_a(hwnd: usize) -> i32 {
        gui::window_text(hwnd)
            .map(|value| value.len() as i32)
            .unwrap_or(0)
    }
);

winapi_api!(
    "user32.dll",
    "GetWindowTextLengthW",
    pub unsafe extern "win64" fn get_window_text_length_w(hwnd: usize) -> i32 {
        gui::window_text(hwnd)
            .map(|value| value.encode_utf16().count() as i32)
            .unwrap_or(0)
    }
);

winapi_api!(
    "user32.dll",
    "SetWindowTextA",
    pub unsafe extern "win64" fn set_window_text_a(hwnd: usize, text: *const u8) -> i32 {
        let value = utf::read_ansi(text);
        i32::from(gui::set_window_text(hwnd, &value))
    }
);

winapi_api!(
    "user32.dll",
    "SetWindowTextW",
    pub unsafe extern "win64" fn set_window_text_w(hwnd: usize, text: *const u16) -> i32 {
        let value = utf::read_wide(text);
        i32::from(gui::set_window_text(hwnd, &value))
    }
);

winapi_api!(
    "user32.dll",
    "GetWindowLongPtrA",
    pub unsafe extern "win64" fn get_window_long_ptr_a(hwnd: usize, index: i32) -> isize {
        gui::get_window_long(hwnd, index)
    }
);

winapi_api!(
    "user32.dll",
    "GetWindowLongPtrW",
    pub unsafe extern "win64" fn get_window_long_ptr_w(hwnd: usize, index: i32) -> isize {
        gui::get_window_long(hwnd, index)
    }
);

winapi_api!(
    "user32.dll",
    "SetWindowLongPtrA",
    pub unsafe extern "win64" fn set_window_long_ptr_a(
        hwnd: usize,
        index: i32,
        value: isize,
    ) -> isize {
        gui::set_window_long(hwnd, index, value)
    }
);

winapi_api!(
    "user32.dll",
    "SetWindowLongPtrW",
    pub unsafe extern "win64" fn set_window_long_ptr_w(
        hwnd: usize,
        index: i32,
        value: isize,
    ) -> isize {
        gui::set_window_long(hwnd, index, value)
    }
);

winapi_api!(
    "user32.dll",
    "GetWindowLongA",
    pub unsafe extern "win64" fn get_window_long_a(hwnd: usize, index: i32) -> isize {
        gui::get_window_long(hwnd, index)
    }
);

winapi_api!(
    "user32.dll",
    "GetWindowLongW",
    pub unsafe extern "win64" fn get_window_long_w(hwnd: usize, index: i32) -> isize {
        gui::get_window_long(hwnd, index)
    }
);

winapi_api!(
    "user32.dll",
    "SetWindowLongA",
    pub unsafe extern "win64" fn set_window_long_a(hwnd: usize, index: i32, value: isize) -> isize {
        gui::set_window_long(hwnd, index, value)
    }
);

winapi_api!(
    "user32.dll",
    "SetWindowLongW",
    pub unsafe extern "win64" fn set_window_long_w(hwnd: usize, index: i32, value: isize) -> isize {
        gui::set_window_long(hwnd, index, value)
    }
);

winapi_api!(
    "user32.dll",
    "AdjustWindowRect",
    pub unsafe extern "win64" fn adjust_window_rect(
        rect: *mut i32,
        _style: u32,
        _menu: i32,
    ) -> i32 {
        let _ = rect;
        1
    }
);

winapi_api!(
    "user32.dll",
    "AdjustWindowRectEx",
    pub unsafe extern "win64" fn adjust_window_rect_ex(
        rect: *mut i32,
        _style: u32,
        _menu: i32,
        _ex_style: u32,
    ) -> i32 {
        let _ = rect;
        1
    }
);

winapi_api!(
    "user32.dll",
    "MoveWindow",
    pub unsafe extern "win64" fn move_window(
        hwnd: usize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        repaint: i32,
    ) -> i32 {
        let moved = gui::move_window(hwnd, x, y, width, height);
        if moved && repaint != 0 {
            gui::invalidate(hwnd);
        }
        i32::from(moved)
    }
);

winapi_api!(
    "user32.dll",
    "SetWindowPos",
    pub unsafe extern "win64" fn set_window_pos(
        hwnd: usize,
        _after: usize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32 {
        i32::from(gui::set_window_pos(hwnd, x, y, width, height, flags))
    }
);

winapi_api!(
    "user32.dll",
    "IsWindow",
    pub unsafe extern "win64" fn is_window(hwnd: usize) -> i32 {
        i32::from(gui::is_window(hwnd))
    }
);

winapi_api!(
    "user32.dll",
    "IsWindowVisible",
    pub unsafe extern "win64" fn is_window_visible(hwnd: usize) -> i32 {
        i32::from(gui::is_window_visible(hwnd))
    }
);

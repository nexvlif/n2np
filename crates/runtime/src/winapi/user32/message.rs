//! Message queue, dispatch, and Send/Post message APIs.

use super::*;

winapi_api!(
    "user32.dll",
    "GetMessageA",
    pub unsafe extern "win64" fn get_message_a(
        msg: *mut u8,
        hwnd: usize,
        min: u32,
        max: u32,
    ) -> i32 {
        get_message_impl(msg, hwnd, min, max)
    }
);

winapi_api!(
    "user32.dll",
    "GetMessageW",
    pub unsafe extern "win64" fn get_message_w(
        msg: *mut u8,
        hwnd: usize,
        min: u32,
        max: u32,
    ) -> i32 {
        get_message_impl(msg, hwnd, min, max)
    }
);

winapi_api!(
    "user32.dll",
    "PeekMessageA",
    pub unsafe extern "win64" fn peek_message_a(
        msg: *mut u8,
        hwnd: usize,
        min: u32,
        max: u32,
        flags: u32,
    ) -> i32 {
        peek_message_impl(msg, hwnd, min, max, flags)
    }
);

winapi_api!(
    "user32.dll",
    "PeekMessageW",
    pub unsafe extern "win64" fn peek_message_w(
        msg: *mut u8,
        hwnd: usize,
        min: u32,
        max: u32,
        flags: u32,
    ) -> i32 {
        peek_message_impl(msg, hwnd, min, max, flags)
    }
);

winapi_api!(
    "user32.dll",
    "TranslateMessage",
    pub unsafe extern "win64" fn translate_message(msg: *const u8) -> i32 {
        translate_message_impl(msg)
    }
);

winapi_api!(
    "user32.dll",
    "DispatchMessageA",
    pub unsafe extern "win64" fn dispatch_message_a(msg: *const u8) -> isize {
        dispatch_message_impl(msg)
    }
);

winapi_api!(
    "user32.dll",
    "DispatchMessageW",
    pub unsafe extern "win64" fn dispatch_message_w(msg: *const u8) -> isize {
        dispatch_message_impl(msg)
    }
);

winapi_api!(
    "user32.dll",
    "PostMessageA",
    pub unsafe extern "win64" fn post_message_a(
        hwnd: usize,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> i32 {
        i32::from(gui::post_message(hwnd, message, wparam, lparam))
    }
);

winapi_api!(
    "user32.dll",
    "PostMessageW",
    pub unsafe extern "win64" fn post_message_w(
        hwnd: usize,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> i32 {
        i32::from(gui::post_message(hwnd, message, wparam, lparam))
    }
);

winapi_api!(
    "user32.dll",
    "SendMessageA",
    pub unsafe extern "win64" fn send_message_a(
        hwnd: usize,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        match gui::window_proc(hwnd) {
            Some(proc_) if proc_ != 0 => {
                gui::call_window_proc(proc_, hwnd, message, wparam, lparam)
            }
            _ => def_window_proc(hwnd, message, wparam, lparam),
        }
    }
);

winapi_api!(
    "user32.dll",
    "SendMessageW",
    pub unsafe extern "win64" fn send_message_w(
        hwnd: usize,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        send_message_a(hwnd, message, wparam, lparam)
    }
);

winapi_api!(
    "user32.dll",
    "DefWindowProcA",
    pub unsafe extern "win64" fn def_window_proc_a(
        hwnd: usize,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        def_window_proc(hwnd, message, wparam, lparam)
    }
);

winapi_api!(
    "user32.dll",
    "DefWindowProcW",
    pub unsafe extern "win64" fn def_window_proc_w(
        hwnd: usize,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        def_window_proc(hwnd, message, wparam, lparam)
    }
);

winapi_api!(
    "user32.dll",
    "PostQuitMessage",
    pub unsafe extern "win64" fn post_quit_message(code: i32) {
        log::info!("[n2np-user32] PostQuitMessage({code})");
        gui::post_quit(code);
    }
);

winapi_api!(
    "user32.dll",
    "SetTimer",
    pub unsafe extern "win64" fn set_timer(
        hwnd: usize,
        id: usize,
        elapse: u32,
        callback: usize,
    ) -> usize {
        if elapse == 0 {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        gui::set_timer(hwnd, id, elapse, callback)
    }
);

winapi_api!(
    "user32.dll",
    "KillTimer",
    pub unsafe extern "win64" fn kill_timer(hwnd: usize, id: usize) -> i32 {
        i32::from(gui::kill_timer(hwnd, id))
    }
);

winapi_api!(
    "user32.dll",
    "GetMessagePos",
    pub unsafe extern "win64" fn get_message_pos() -> u32 {
        0
    }
);

winapi_api!(
    "user32.dll",
    "GetMessageTime",
    pub unsafe extern "win64" fn get_message_time() -> i32 {
        0
    }
);

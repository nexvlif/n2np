use crate::error::{self, ERROR_INVALID_PARAMETER};
use crate::gui;
use crate::utf;
use std::ffi::c_void;

const WM_NCCREATE: u32 = 0x0081;
const WM_CREATE: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;
const WM_PAINT: u32 = 0x000F;
const WM_CLOSE: u32 = 0x0010;
const WM_QUIT: u32 = 0x0012;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_KEYDOWN: u32 = 0x0100;
const WM_CHAR: u32 = 0x0102;

const PM_REMOVE: u32 = 0x0001;

const PAINTSTRUCT_SIZE: usize = 72;

unsafe fn read_usize(base: *const u8, offset: usize) -> usize {
    if base.is_null() {
        return 0;
    }
    std::ptr::read_unaligned(base.add(offset) as *const usize)
}

unsafe fn class_name_a(ptr: *const u8) -> Option<String> {
    let value = ptr as usize;
    if value == 0 {
        return None;
    }
    if value <= 0xFFFF {
        return gui::class_name_for_atom(value as u16);
    }
    Some(utf::read_ansi(ptr))
}

unsafe fn class_name_w(ptr: *const u16) -> Option<String> {
    let value = ptr as usize;
    if value == 0 {
        return None;
    }
    if value <= 0xFFFF {
        return gui::class_name_for_atom(value as u16);
    }
    Some(utf::read_wide(ptr))
}

fn write_msg(msg: *mut u8, message: gui::Msg) {
    unsafe {
        std::ptr::write_unaligned(msg as *mut gui::Msg, message);
    }
}

unsafe fn read_msg(msg: *const u8) -> gui::Msg {
    if msg.is_null() {
        return gui::Msg::default();
    }
    std::ptr::read_unaligned(msg as *const gui::Msg)
}

fn write_rect(rect: *mut i32, left: i32, top: i32, right: i32, bottom: i32) {
    if rect.is_null() {
        return;
    }
    unsafe {
        *rect = left;
        *rect.add(1) = top;
        *rect.add(2) = right;
        *rect.add(3) = bottom;
    }
}

unsafe fn create_window(
    class_name: Option<String>,
    title: String,
    style: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    param: *mut c_void,
) -> usize {
    let Some(class_name) = class_name else {
        error::set_last_error(ERROR_INVALID_PARAMETER);
        return 0;
    };
    let Some((hwnd, proc_)) = gui::create_window(
        &class_name,
        &title,
        style,
        x,
        y,
        width,
        height,
        param as isize,
    ) else {
        error::set_last_error(ERROR_INVALID_PARAMETER);
        return 0;
    };
    if proc_ != 0 {
        let result = gui::call_window_proc(proc_, hwnd, WM_CREATE, 0, 0);
        if result == -1 {
            gui::destroy_window(hwnd);
            return 0;
        }
    }
    hwnd
}

unsafe fn destroy_window_impl(hwnd: usize) -> i32 {
    let Some(proc_) = gui::window_proc(hwnd) else {
        return 0;
    };
    if proc_ != 0 {
        gui::call_window_proc(proc_, hwnd, WM_DESTROY, 0, 0);
    }
    gui::destroy_window(hwnd);
    1
}

fn def_window_proc(hwnd: usize, message: u32, wparam: usize, lparam: isize) -> isize {
    match message {
        WM_CLOSE => {
            unsafe {
                destroy_window_impl(hwnd);
            }
            0
        }
        WM_PAINT => {
            if let Some(hdc) = gui::begin_paint(hwnd) {
                gui::end_paint(hdc);
            }
            0
        }
        WM_ERASEBKGND => 1,
        WM_NCCREATE => 1,
        WM_CREATE => 0,
        _ => {
            let _ = (wparam, lparam);
            0
        }
    }
}

unsafe fn get_message_impl(msg: *mut u8, hwnd: usize, min: u32, max: u32) -> i32 {
    if msg.is_null() {
        error::set_last_error(ERROR_INVALID_PARAMETER);
        return -1;
    }
    match gui::wait_message(hwnd, min, max) {
        Some(message) => {
            let quit = message.message == WM_QUIT;
            write_msg(msg, message);
            if quit {
                0
            } else {
                1
            }
        }
        None => {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            -1
        }
    }
}

unsafe fn peek_message_impl(msg: *mut u8, hwnd: usize, min: u32, max: u32, flags: u32) -> i32 {
    if msg.is_null() {
        error::set_last_error(ERROR_INVALID_PARAMETER);
        return 0;
    }
    let remove = flags & PM_REMOVE != 0;
    match gui::peek_message(hwnd, min, max, remove) {
        Some(message) => {
            write_msg(msg, message);
            1
        }
        None => 0,
    }
}

unsafe fn translate_message_impl(msg: *const u8) -> i32 {
    let message = read_msg(msg);
    if message.message != WM_KEYDOWN {
        return 0;
    }
    let key = message.wparam;
    let character = if (0x20..=0x7e).contains(&key) {
        key
    } else {
        match key {
            0x08 | 0x09 | 0x0D | 0x1B => key,
            _ => return 0,
        }
    };
    if gui::post_message(message.hwnd, WM_CHAR, character, 0) {
        1
    } else {
        0
    }
}

unsafe fn dispatch_message_impl(msg: *const u8) -> isize {
    let message = read_msg(msg);
    if message.hwnd == 0 {
        return 0;
    }
    match gui::window_proc(message.hwnd) {
        Some(proc_) if proc_ != 0 => gui::call_window_proc(
            proc_,
            message.hwnd,
            message.message,
            message.wparam,
            message.lparam,
        ),
        _ => def_window_proc(
            message.hwnd,
            message.message,
            message.wparam,
            message.lparam,
        ),
    }
}

mod class;
mod message;
mod misc;
mod paint;
mod window;

pub use class::*;
pub use message::*;
pub use misc::*;
pub use paint::*;
pub use window::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_layout_matches_win64() {
        assert_eq!(std::mem::size_of::<gui::Msg>(), 48);
        assert_eq!(std::mem::offset_of!(gui::Msg, message), 8);
        assert_eq!(std::mem::offset_of!(gui::Msg, wparam), 16);
        assert_eq!(std::mem::offset_of!(gui::Msg, lparam), 24);
    }

    #[test]
    fn default_window_proc_erases_background() {
        assert_eq!(def_window_proc(0, WM_ERASEBKGND, 0, 0), 1);
        assert_eq!(def_window_proc(0, WM_NCCREATE, 0, 0), 1);
    }
}

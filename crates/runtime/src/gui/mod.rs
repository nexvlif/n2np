#[cfg(target_os = "linux")]
mod wayland;

#[cfg(target_os = "linux")]
pub use wayland::*;

#[cfg(not(target_os = "linux"))]
pub const WM_QUIT: u32 = 0x0012;

#[cfg(not(target_os = "linux"))]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Msg {
    pub hwnd: usize,
    pub message: u32,
    pub _pad: u32,
    pub wparam: usize,
    pub lparam: isize,
    pub time: u32,
    pub x: i32,
    pub y: i32,
    pub private: u32,
}

#[cfg(target_os = "linux")]
pub fn message_box(text: &str, caption: &str, utype: u32) -> i32 {
    wayland::message_box(text, caption, utype)
}

#[cfg(not(target_os = "linux"))]
pub fn message_box(text: &str, caption: &str, _utype: u32) -> i32 {
    log::warn!("[n2np-gui] GUI backend is only available on Linux/Wayland");
    println!("[{caption}] {text}");
    1
}

#[cfg(not(target_os = "linux"))]
mod stubs {
    use super::Msg;

    pub fn register_class(
        _name: &str,
        _proc_: usize,
        _background: usize,
        _cursor: usize,
        _icon: usize,
    ) -> Option<u16> {
        None
    }
    pub fn unregister_class(_name: &str) -> bool {
        false
    }
    pub fn class_name_for_atom(_atom: u16) -> Option<String> {
        None
    }
    pub fn class_proc(_name: &str) -> Option<usize> {
        None
    }
    pub fn create_window(
        _class_name: &str,
        _title: &str,
        _style: u32,
        _x: i32,
        _y: i32,
        _width: i32,
        _height: i32,
        _user_data: isize,
    ) -> Option<(usize, usize)> {
        None
    }
    pub fn destroy_window(_hwnd: usize) -> bool {
        false
    }
    pub fn window_proc(_hwnd: usize) -> Option<usize> {
        None
    }
    pub fn set_window_proc(_hwnd: usize, _proc_: usize) -> Option<usize> {
        None
    }
    pub fn show_window(_hwnd: usize, _command: i32) -> Option<bool> {
        None
    }
    pub fn is_window(_hwnd: usize) -> bool {
        false
    }
    pub fn is_window_visible(_hwnd: usize) -> bool {
        false
    }
    pub fn invalidate(_hwnd: usize) -> bool {
        false
    }
    pub fn post_message(_hwnd: usize, _message: u32, _wparam: usize, _lparam: isize) -> bool {
        false
    }
    pub fn post_quit(_code: i32) {}
    pub fn wait_message(_hwnd: usize, _min: u32, _max: u32) -> Option<Msg> {
        None
    }
    pub fn peek_message(_hwnd: usize, _min: u32, _max: u32, _remove: bool) -> Option<Msg> {
        None
    }
    pub fn begin_paint(_hwnd: usize) -> Option<usize> {
        None
    }
    pub fn end_paint(_hdc: usize) {}
    pub fn get_dc(_hwnd: usize) -> Option<usize> {
        None
    }
    pub fn release_dc(_hdc: usize) -> bool {
        false
    }
    pub fn client_size(_hwnd: usize) -> Option<(i32, i32)> {
        None
    }
    pub fn text_out(_hdc: usize, _x: i32, _y: i32, _text: &str) -> bool {
        false
    }
    pub fn fill_rect(
        _hdc: usize,
        _color: u32,
        _x: i32,
        _y: i32,
        _width: i32,
        _height: i32,
    ) -> bool {
        false
    }
    pub fn set_text_color(_hdc: usize, _color: u32) -> Option<u32> {
        None
    }
    pub fn get_text_color(_hdc: usize) -> Option<u32> {
        None
    }
    pub fn set_background_color(_hdc: usize, _color: u32) -> Option<u32> {
        None
    }
    pub fn get_background_color(_hdc: usize) -> Option<u32> {
        None
    }
    pub fn set_background_mode(_hdc: usize, _mode: i32) -> Option<i32> {
        None
    }
    pub fn get_background_mode(_hdc: usize) -> Option<i32> {
        None
    }
    pub fn window_text(_hwnd: usize) -> Option<String> {
        None
    }
    pub fn set_window_text(_hwnd: usize, _text: &str) -> bool {
        false
    }
    pub fn get_window_long(_hwnd: usize, _index: i32) -> isize {
        0
    }
    pub fn set_window_long(_hwnd: usize, _index: i32, _value: isize) -> isize {
        0
    }
    pub fn move_window(_hwnd: usize, _x: i32, _y: i32, _width: i32, _height: i32) -> bool {
        false
    }
    pub fn set_window_pos(
        _hwnd: usize,
        _x: i32,
        _y: i32,
        _width: i32,
        _height: i32,
        _flags: u32,
    ) -> bool {
        false
    }
    pub fn colorref_to_pixel(color: u32) -> u32 {
        color
    }
    pub fn system_color(_index: u32) -> u32 {
        0x00ff_ffff
    }
    pub fn default_background() -> u32 {
        0x00ff_ffff
    }
    pub fn call_window_proc(
        _proc_: usize,
        _hwnd: usize,
        _message: u32,
        _wparam: usize,
        _lparam: isize,
    ) -> isize {
        0
    }
}

#[cfg(not(target_os = "linux"))]
pub use stubs::*;

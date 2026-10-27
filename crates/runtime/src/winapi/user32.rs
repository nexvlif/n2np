use crate::gui;

fn read_ansi(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    unsafe {
        while len < 65536 && *ptr.add(len) != 0 {
            len += 1;
        }
        let bytes = std::slice::from_raw_parts(ptr, len);
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn read_wide(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    unsafe {
        while len < 65536 && *ptr.add(len) != 0 {
            len += 1;
        }
        let units = std::slice::from_raw_parts(ptr, len);
        String::from_utf16_lossy(units)
    }
}

pub unsafe extern "win64" fn message_box_a(
    _hwnd: usize,
    text: *const u8,
    caption: *const u8,
    utype: u32,
) -> i32 {
    let text = read_ansi(text);
    let caption = read_ansi(caption);
    gui::message_box(&text, &caption, utype)
}

pub unsafe extern "win64" fn message_box_w(
    _hwnd: usize,
    text: *const u16,
    caption: *const u16,
    utype: u32,
) -> i32 {
    let text = read_wide(text);
    let caption = read_wide(caption);
    gui::message_box(&text, &caption, utype)
}

pub unsafe extern "win64" fn get_desktop_window() -> usize {
    0x10001
}

pub unsafe extern "win64" fn post_quit_message(code: i32) {
    log::info!("[n2np-user32] PostQuitMessage({code})");
}

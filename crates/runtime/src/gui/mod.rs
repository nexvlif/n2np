#[cfg(target_os = "linux")]
mod x11;

#[cfg(target_os = "linux")]
pub fn message_box(text: &str, caption: &str, utype: u32) -> i32 {
    x11::message_box(text, caption, utype).unwrap_or_else(|err| {
        log::error!("[n2np-gui] MessageBox failed: {err}");
        1
    })
}

#[cfg(not(target_os = "linux"))]
pub fn message_box(text: &str, caption: &str, _utype: u32) -> i32 {
    log::warn!("[n2np-gui] GUI backend is only available on Linux/WSLg");
    println!("[{caption}] {text}");
    1
}

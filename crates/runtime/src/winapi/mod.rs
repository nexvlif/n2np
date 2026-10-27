pub mod kernel32;
pub mod user32;

fn addr(f: *const ()) -> usize {
    f as usize
}

pub fn resolve(dll: &str, name: &str) -> Option<usize> {
    let dll = dll.rsplit(['\\', '/']).next().unwrap_or(dll);
    let dll = dll.trim().to_ascii_lowercase();
    let dll = if dll.contains('.') {
        dll
    } else {
        format!("{dll}.dll")
    };

    match dll.as_str() {
        "kernel32.dll" => match name {
            "ExitProcess" => Some(addr(kernel32::exit_process as *const ())),
            "GetStdHandle" => Some(addr(kernel32::get_std_handle as *const ())),
            "WriteFile" => Some(addr(kernel32::write_file as *const ())),
            "LoadLibraryA" => Some(addr(kernel32::load_library_a as *const ())),
            "GetProcAddress" => Some(addr(kernel32::get_proc_address as *const ())),
            "VirtualProtect" => Some(addr(kernel32::virtual_protect as *const ())),
            "GetModuleHandleA" => Some(addr(kernel32::get_module_handle_a as *const ())),
            "GetModuleHandleW" => Some(addr(kernel32::get_module_handle_w as *const ())),
            "GetCommandLineA" => Some(addr(kernel32::get_command_line_a as *const ())),
            "Sleep" => Some(addr(kernel32::sleep as *const ())),
            _ => None,
        },
        "user32.dll" => match name {
            "MessageBoxA" => Some(addr(user32::message_box_a as *const ())),
            "MessageBoxW" => Some(addr(user32::message_box_w as *const ())),
            "GetDesktopWindow" => Some(addr(user32::get_desktop_window as *const ())),
            "PostQuitMessage" => Some(addr(user32::post_quit_message as *const ())),
            _ => None,
        },
        _ => None,
    }
}

pub fn resolve_ordinal(dll: &str, _ordinal: u16) -> Option<usize> {
    log::warn!("[n2np-thunk] ordinal imports are not implemented for {dll}");
    None
}

pub unsafe extern "win64" fn missing_thunk() -> usize {
    log::warn!("[n2np-thunk] called an unimplemented Windows API thunk");
    0
}

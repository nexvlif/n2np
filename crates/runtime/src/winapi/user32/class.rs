//! Window class registration and MessageBox entry points.

use super::*;

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
    "RegisterClassA",
    pub unsafe extern "win64" fn register_class_a(class: *const u8) -> u16 {
        if class.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        let proc_ = read_usize(class, 8);
        let icon = read_usize(class, 32);
        let cursor = read_usize(class, 40);
        let background = read_usize(class, 48);
        let name = utf::read_ansi(read_usize(class, 64) as *const u8);
        if name.is_empty() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        gui::register_class(&name, proc_, background, cursor, icon).unwrap_or(0)
    }
);

winapi_api!(
    "user32.dll",
    "RegisterClassW",
    pub unsafe extern "win64" fn register_class_w(class: *const u8) -> u16 {
        if class.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        let proc_ = read_usize(class, 8);
        let icon = read_usize(class, 32);
        let cursor = read_usize(class, 40);
        let background = read_usize(class, 48);
        let name = utf::read_wide(read_usize(class, 64) as *const u16);
        if name.is_empty() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        gui::register_class(&name, proc_, background, cursor, icon).unwrap_or(0)
    }
);

winapi_api!(
    "user32.dll",
    "RegisterClassExA",
    pub unsafe extern "win64" fn register_class_ex_a(class: *const u8) -> u16 {
        register_class_a(class)
    }
);

winapi_api!(
    "user32.dll",
    "RegisterClassExW",
    pub unsafe extern "win64" fn register_class_ex_w(class: *const u8) -> u16 {
        register_class_w(class)
    }
);

winapi_api!(
    "user32.dll",
    "UnregisterClassA",
    pub unsafe extern "win64" fn unregister_class_a(class: *const u8, _instance: usize) -> i32 {
        let Some(name) = class_name_a(class) else {
            return 0;
        };
        i32::from(gui::unregister_class(&name))
    }
);

winapi_api!(
    "user32.dll",
    "UnregisterClassW",
    pub unsafe extern "win64" fn unregister_class_w(class: *const u16, _instance: usize) -> i32 {
        let Some(name) = class_name_w(class) else {
            return 0;
        };
        i32::from(gui::unregister_class(&name))
    }
);

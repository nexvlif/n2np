//! Input translation between Wayland/evdev and Win32 virtual keys.

/// Map a Linux evdev keycode (as delivered by `wl_keyboard.key`) to a Win32
/// virtual-key code. Only the keys needed by small GUI programs are covered.
pub fn keycode_to_virtual_key(keycode: u32) -> usize {
    match keycode {
        1 => 0x1B,   // Escape
        14 => 0x08,  // Backspace
        15 => 0x09,  // Tab
        28 => 0x0D,  // Enter
        57 => 0x20,  // Space
        111 => 0x2E, // Delete
        2..=11 => b'1' as usize + (keycode - 2) as usize,
        16..=25 => b'Q' as usize + (keycode - 16) as usize,
        30..=38 => b'A' as usize + (keycode - 30) as usize,
        44..=50 => b'Z' as usize + (keycode - 44) as usize,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_keycodes() {
        assert_eq!(keycode_to_virtual_key(1), 0x1B);
        assert_eq!(keycode_to_virtual_key(28), 0x0D);
        assert_eq!(keycode_to_virtual_key(30), b'A' as usize);
        assert_eq!(keycode_to_virtual_key(44), b'Z' as usize);
        assert_eq!(keycode_to_virtual_key(2), b'1' as usize);
    }
}

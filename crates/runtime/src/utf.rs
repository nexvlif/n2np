const MAX_STRING: usize = 1 << 20;

pub unsafe fn read_ansi(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while len < MAX_STRING && *ptr.add(len) != 0 {
        len += 1;
    }
    let bytes = std::slice::from_raw_parts(ptr, len);
    String::from_utf8_lossy(bytes).into_owned()
}

pub unsafe fn read_ansi_slice(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    let bytes = std::slice::from_raw_parts(ptr, len.min(MAX_STRING));
    String::from_utf8_lossy(bytes).into_owned()
}

pub unsafe fn read_wide(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while len < MAX_STRING && *ptr.add(len) != 0 {
        len += 1;
    }
    let units = std::slice::from_raw_parts(ptr, len);
    String::from_utf16_lossy(units)
}

pub unsafe fn read_wide_slice(ptr: *const u16, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    let units = std::slice::from_raw_parts(ptr, len.min(MAX_STRING));
    String::from_utf16_lossy(units)
}

pub fn wide_with_nul(text: &str) -> Vec<u16> {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0);
    units
}

pub fn ansi_with_nul(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

pub fn ansi_to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

pub fn wide_to_ansi(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

pub unsafe fn write_ansi(dst: *mut u8, text: &str, cap: usize) -> usize {
    if dst.is_null() || cap == 0 {
        return text.len() + 1;
    }
    let bytes = text.as_bytes();
    let copy = bytes.len().min(cap.saturating_sub(1));
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, copy);
    *dst.add(copy) = 0;
    if copy < bytes.len() {
        copy
    } else {
        bytes.len()
    }
}

pub unsafe fn write_wide(dst: *mut u16, text: &str, cap: usize) -> usize {
    let units: Vec<u16> = text.encode_utf16().collect();
    if dst.is_null() || cap == 0 {
        return units.len() + 1;
    }
    let copy = units.len().min(cap.saturating_sub(1));
    std::ptr::copy_nonoverlapping(units.as_ptr(), dst, copy);
    *dst.add(copy) = 0;
    if copy < units.len() {
        copy
    } else {
        units.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_round_trip() {
        let bytes = ansi_with_nul("hello");
        let text = unsafe { read_ansi(bytes.as_ptr()) };
        assert_eq!(text, "hello");
    }

    #[test]
    fn wide_round_trip() {
        let units = wide_with_nul("n2np");
        let text = unsafe { read_wide(units.as_ptr()) };
        assert_eq!(text, "n2np");
    }

    #[test]
    fn write_truncates_and_terminates() {
        let mut buffer = [0xAAu8; 4];
        let written = unsafe { write_ansi(buffer.as_mut_ptr(), "hello", buffer.len()) };
        assert_eq!(written, 3);
        assert_eq!(&buffer, b"hel\0");
    }
}

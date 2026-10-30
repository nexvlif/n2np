//! Painting and device-context APIs.

use super::*;

winapi_api!(
    "user32.dll",
    "BeginPaint",
    pub unsafe extern "win64" fn begin_paint(hwnd: usize, paint: *mut u8) -> usize {
        let Some(hdc) = gui::begin_paint(hwnd) else {
            return 0;
        };
        if !paint.is_null() {
            std::ptr::write_bytes(paint, 0, PAINTSTRUCT_SIZE);
            std::ptr::write_unaligned(paint as *mut usize, hdc);
            std::ptr::write_unaligned(paint.add(8) as *mut i32, 0);
            let (width, height) = gui::client_size(hwnd).unwrap_or((0, 0));
            write_rect(paint.add(12) as *mut i32, 0, 0, width, height);
        }
        hdc
    }
);

winapi_api!(
    "user32.dll",
    "EndPaint",
    pub unsafe extern "win64" fn end_paint(_hwnd: usize, paint: *const u8) -> i32 {
        if !paint.is_null() {
            let hdc = std::ptr::read_unaligned(paint as *const usize);
            gui::end_paint(hdc);
        }
        1
    }
);

winapi_api!(
    "user32.dll",
    "GetDC",
    pub unsafe extern "win64" fn get_dc(hwnd: usize) -> usize {
        gui::get_dc(hwnd).unwrap_or(0)
    }
);

winapi_api!(
    "user32.dll",
    "ReleaseDC",
    pub unsafe extern "win64" fn release_dc(_hwnd: usize, dc: usize) -> i32 {
        i32::from(gui::release_dc(dc))
    }
);

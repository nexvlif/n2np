use crate::error;
use std::ffi::c_void;
use std::ptr;

winapi_api!(
    "ntdll.dll",
    "RtlZeroMemory",
    pub unsafe extern "win64" fn rtl_zero_memory(destination: *mut c_void, length: usize) {
        if !destination.is_null() && length != 0 {
            ptr::write_bytes(destination as *mut u8, 0, length);
        }
    }
);

winapi_api!(
    "ntdll.dll",
    "RtlFillMemory",
    pub unsafe extern "win64" fn rtl_fill_memory(
        destination: *mut c_void,
        length: usize,
        fill: u8,
    ) {
        if !destination.is_null() && length != 0 {
            ptr::write_bytes(destination as *mut u8, fill, length);
        }
    }
);

winapi_api!(
    "ntdll.dll",
    "RtlCopyMemory",
    pub unsafe extern "win64" fn rtl_copy_memory(
        destination: *mut c_void,
        source: *const c_void,
        length: usize,
    ) {
        if !destination.is_null() && !source.is_null() && length != 0 {
            ptr::copy_nonoverlapping(source as *const u8, destination as *mut u8, length);
        }
    }
);

winapi_api!(
    "ntdll.dll",
    "RtlMoveMemory",
    pub unsafe extern "win64" fn rtl_move_memory(
        destination: *mut c_void,
        source: *const c_void,
        length: usize,
    ) {
        if !destination.is_null() && !source.is_null() && length != 0 {
            ptr::copy(source as *const u8, destination as *mut u8, length);
        }
    }
);

winapi_api!(
    "ntdll.dll",
    "RtlCompareMemory",
    pub unsafe extern "win64" fn rtl_compare_memory(
        left: *const c_void,
        right: *const c_void,
        length: usize,
    ) -> usize {
        if left.is_null() || right.is_null() || length == 0 {
            return 0;
        }
        let left = std::slice::from_raw_parts(left as *const u8, length);
        let right = std::slice::from_raw_parts(right as *const u8, length);
        left.iter().zip(right).take_while(|(a, b)| a == b).count()
    }
);

winapi_api!(
    "ntdll.dll",
    "RtlSetLastWin32Error",
    pub unsafe extern "win64" fn rtl_set_last_win32_error(code: u32) {
        error::set_last_error(code)
    }
);

winapi_api!(
    "ntdll.dll",
    "RtlGetLastWin32Error",
    pub unsafe extern "win64" fn rtl_get_last_win32_error() -> u32 {
        error::get_last_error()
    }
);

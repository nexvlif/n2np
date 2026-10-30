use crate::memory;
use std::ffi::c_void;
use std::io::Write;
use std::ptr;

winapi_api!(
    "msvcrt.dll",
    "malloc",
    pub unsafe extern "win64" fn malloc(size: usize) -> *mut c_void {
        memory::alloc(size, 16, false, memory::PAGE_READWRITE) as *mut c_void
    }
);

winapi_api!(
    "msvcrt.dll",
    "calloc",
    pub unsafe extern "win64" fn calloc(count: usize, size: usize) -> *mut c_void {
        memory::alloc(count.saturating_mul(size), 16, true, memory::PAGE_READWRITE) as *mut c_void
    }
);

winapi_api!(
    "msvcrt.dll",
    "realloc",
    pub unsafe extern "win64" fn realloc(pointer: *mut c_void, size: usize) -> *mut c_void {
        memory::realloc(pointer as *mut u8, size) as *mut c_void
    }
);

winapi_api!(
    "msvcrt.dll",
    "free",
    pub unsafe extern "win64" fn free(pointer: *mut c_void) {
        memory::free(pointer as *mut u8);
    }
);

winapi_api!(
    "msvcrt.dll",
    "_aligned_malloc",
    pub unsafe extern "win64" fn aligned_malloc(size: usize, alignment: usize) -> *mut c_void {
        memory::alloc(size, alignment, false, memory::PAGE_READWRITE) as *mut c_void
    }
);

winapi_api!(
    "msvcrt.dll",
    "_aligned_free",
    pub unsafe extern "win64" fn aligned_free(pointer: *mut c_void) {
        memory::free(pointer as *mut u8);
    }
);

winapi_api!(
    "msvcrt.dll",
    "memcpy",
    pub unsafe extern "win64" fn memcpy(
        destination: *mut c_void,
        source: *const c_void,
        length: usize,
    ) -> *mut c_void {
        if !destination.is_null() && !source.is_null() && length != 0 {
            ptr::copy_nonoverlapping(source as *const u8, destination as *mut u8, length);
        }
        destination
    }
);

winapi_api!(
    "msvcrt.dll",
    "memmove",
    pub unsafe extern "win64" fn memmove(
        destination: *mut c_void,
        source: *const c_void,
        length: usize,
    ) -> *mut c_void {
        if !destination.is_null() && !source.is_null() && length != 0 {
            ptr::copy(source as *const u8, destination as *mut u8, length);
        }
        destination
    }
);

winapi_api!(
    "msvcrt.dll",
    "memset",
    pub unsafe extern "win64" fn memset(
        destination: *mut c_void,
        value: i32,
        length: usize,
    ) -> *mut c_void {
        if !destination.is_null() && length != 0 {
            ptr::write_bytes(destination as *mut u8, value as u8, length);
        }
        destination
    }
);

winapi_api!(
    "msvcrt.dll",
    "memcmp",
    pub unsafe extern "win64" fn memcmp(
        left: *const c_void,
        right: *const c_void,
        length: usize,
    ) -> i32 {
        if left.is_null() || right.is_null() || length == 0 {
            return 0;
        }
        let left = std::slice::from_raw_parts(left as *const u8, length);
        let right = std::slice::from_raw_parts(right as *const u8, length);
        left.cmp(right) as i32
    }
);

winapi_api!(
    "msvcrt.dll",
    "strlen",
    pub unsafe extern "win64" fn strlen(text: *const u8) -> usize {
        crate::utf::read_ansi(text).len()
    }
);

winapi_api!(
    "msvcrt.dll",
    "strcpy",
    pub unsafe extern "win64" fn strcpy(destination: *mut u8, source: *const u8) -> *mut u8 {
        let text = crate::utf::read_ansi(source);
        ptr::copy_nonoverlapping(text.as_ptr(), destination, text.len());
        *destination.add(text.len()) = 0;
        destination
    }
);

winapi_api!(
    "msvcrt.dll",
    "strncpy",
    pub unsafe extern "win64" fn strncpy(
        destination: *mut u8,
        source: *const u8,
        count: usize,
    ) -> *mut u8 {
        let text = crate::utf::read_ansi(source);
        let copy = text.len().min(count);
        ptr::copy_nonoverlapping(text.as_ptr(), destination, copy);
        if copy < count {
            ptr::write_bytes(destination.add(copy), 0, count - copy);
        }
        destination
    }
);

winapi_api!(
    "msvcrt.dll",
    "strcat",
    pub unsafe extern "win64" fn strcat(destination: *mut u8, source: *const u8) -> *mut u8 {
        let mut text = crate::utf::read_ansi(destination);
        text.push_str(&crate::utf::read_ansi(source));
        ptr::copy_nonoverlapping(text.as_ptr(), destination, text.len());
        *destination.add(text.len()) = 0;
        destination
    }
);

winapi_api!(
    "msvcrt.dll",
    "strcmp",
    pub unsafe extern "win64" fn strcmp(left: *const u8, right: *const u8) -> i32 {
        crate::utf::read_ansi(left).cmp(&crate::utf::read_ansi(right)) as i32
    }
);

winapi_api!(
    "msvcrt.dll",
    "strncmp",
    pub unsafe extern "win64" fn strncmp(left: *const u8, right: *const u8, count: usize) -> i32 {
        let left = crate::utf::read_ansi(left);
        let right = crate::utf::read_ansi(right);
        left.as_bytes()
            .iter()
            .take(count)
            .cmp(right.as_bytes().iter().take(count)) as i32
    }
);

winapi_api!(
    "msvcrt.dll",
    "strchr",
    pub unsafe extern "win64" fn strchr(text: *const u8, value: i32) -> *mut u8 {
        let text_ref = crate::utf::read_ansi(text);
        match text_ref
            .as_bytes()
            .iter()
            .position(|byte| *byte == value as u8)
        {
            Some(index) => text.add(index) as *mut u8,
            None => ptr::null_mut(),
        }
    }
);

winapi_api!(
    "msvcrt.dll",
    "strrchr",
    pub unsafe extern "win64" fn strrchr(text: *const u8, value: i32) -> *mut u8 {
        let text_ref = crate::utf::read_ansi(text);
        match text_ref
            .as_bytes()
            .iter()
            .rposition(|byte| *byte == value as u8)
        {
            Some(index) => text.add(index) as *mut u8,
            None => ptr::null_mut(),
        }
    }
);

winapi_api!(
    "msvcrt.dll",
    "strstr",
    pub unsafe extern "win64" fn strstr(text: *const u8, needle: *const u8) -> *mut u8 {
        let text_ref = crate::utf::read_ansi(text);
        let needle_ref = crate::utf::read_ansi(needle);
        match text_ref.find(&needle_ref) {
            Some(index) => text.add(index) as *mut u8,
            None => ptr::null_mut(),
        }
    }
);

winapi_api!(
    "msvcrt.dll",
    "atoi",
    pub unsafe extern "win64" fn atoi(text: *const u8) -> i32 {
        crate::utf::read_ansi(text).trim().parse().unwrap_or(0)
    }
);

winapi_api!(
    "msvcrt.dll",
    "atol",
    pub unsafe extern "win64" fn atol(text: *const u8) -> i64 {
        crate::utf::read_ansi(text).trim().parse().unwrap_or(0)
    }
);

winapi_api!(
    "msvcrt.dll",
    "abs",
    pub unsafe extern "win64" fn abs(value: i32) -> i32 {
        value.wrapping_abs()
    }
);

winapi_api!(
    "msvcrt.dll",
    "labs",
    pub unsafe extern "win64" fn labs(value: i64) -> i64 {
        value.wrapping_abs()
    }
);

winapi_api!(
    "msvcrt.dll",
    "puts",
    pub unsafe extern "win64" fn puts(text: *const u8) -> i32 {
        let value = crate::utf::read_ansi(text);
        let mut stdout = std::io::stdout();
        if writeln!(stdout, "{value}").is_ok() {
            0
        } else {
            -1
        }
    }
);

winapi_api!(
    "msvcrt.dll",
    "putchar",
    pub unsafe extern "win64" fn putchar(value: i32) -> i32 {
        let byte = [value as u8];
        let mut stdout = std::io::stdout();
        if stdout.write_all(&byte).is_ok() {
            value
        } else {
            -1
        }
    }
);

winapi_api!(
    "msvcrt.dll",
    "exit",
    pub unsafe extern "win64" fn exit(code: i32) -> ! {
        if let Err(err) = crate::trace::flush_output() {
            log::warn!("[n2np-trace] failed to write runtime profile: {err}");
        }
        std::process::exit(code)
    }
);

winapi_api!(
    "msvcrt.dll",
    "_exit",
    pub unsafe extern "win64" fn _exit(code: i32) -> ! {
        std::process::exit(code)
    }
);

winapi_api!(
    "msvcrt.dll",
    "abort",
    pub unsafe extern "win64" fn abort() -> ! {
        std::process::abort()
    }
);

winapi_api!(
    "msvcrt.dll",
    "_initterm",
    pub unsafe extern "win64" fn _initterm(start: *mut usize, end: *mut usize) {
        let mut current = start;
        while current < end {
            let function = *current;
            if function != 0 {
                let function: unsafe extern "win64" fn() = std::mem::transmute(function);
                function();
            }
            current = current.add(1);
        }
    }
);

winapi_api!(
    "msvcrt.dll",
    "_initterm_e",
    pub unsafe extern "win64" fn _initterm_e(start: *mut usize, end: *mut usize) -> i32 {
        let mut current = start;
        while current < end {
            let function = *current;
            if function != 0 {
                let function: unsafe extern "win64" fn() -> i32 = std::mem::transmute(function);
                let result = function();
                if result != 0 {
                    return result;
                }
            }
            current = current.add(1);
        }
        0
    }
);

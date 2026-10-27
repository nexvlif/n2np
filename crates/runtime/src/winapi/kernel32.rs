use std::ffi::c_void;
use std::io::{self, Write};

const STD_INPUT_HANDLE: u32 = 0xFFFF_FFF6;
const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;
const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4;

static COMMAND_LINE: &[u8] = b"n2np\0";

pub unsafe extern "win64" fn get_std_handle(nstd_handle: u32) -> usize {
    match nstd_handle {
        STD_OUTPUT_HANDLE => 1,
        STD_ERROR_HANDLE => 2,
        STD_INPUT_HANDLE => 0,
        _ => usize::MAX,
    }
}

pub unsafe extern "win64" fn write_file(
    handle: usize,
    buffer: *const u8,
    bytes_to_write: u32,
    bytes_written: *mut u32,
    _overlapped: *mut c_void,
) -> i32 {
    if buffer.is_null() {
        return 0;
    }
    let data = std::slice::from_raw_parts(buffer, bytes_to_write as usize);
    let result = match handle {
        1 => io::stdout().write_all(data).and_then(|_| io::stdout().flush()),
        2 => io::stderr().write_all(data).and_then(|_| io::stderr().flush()),
        _ => Ok(()),
    };
    if result.is_err() {
        return 0;
    }
    if !bytes_written.is_null() {
        *bytes_written = bytes_to_write;
    }
    1
}

pub unsafe extern "win64" fn exit_process(code: u32) -> ! {
    std::process::exit(code as i32);
}

pub unsafe extern "win64" fn load_library_a(_name: *const u8) -> usize {
    0x1000_0000
}

pub unsafe extern "win64" fn get_proc_address(_module: usize, _proc: *const u8) -> usize {
    0
}

pub unsafe extern "win64" fn virtual_protect(
    _addr: usize,
    _size: usize,
    _new_protect: u32,
    old_protect: *mut u32,
) -> i32 {
    if !old_protect.is_null() {
        *old_protect = 0x20;
    }
    1
}

pub unsafe extern "win64" fn get_module_handle_a(_name: *const u8) -> usize {
    0x1400_0000
}

pub unsafe extern "win64" fn get_module_handle_w(_name: *const u16) -> usize {
    0x1400_0000
}

pub unsafe extern "win64" fn get_command_line_a() -> *const u8 {
    COMMAND_LINE.as_ptr()
}

pub unsafe extern "win64" fn sleep(ms: u32) {
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}

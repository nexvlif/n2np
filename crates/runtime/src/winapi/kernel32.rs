use std::io::{self, Write};

const STD_INPUT_HANDLE: i64 = -10;
const STD_OUTPUT_HANDLE: i64 = -11;
const STD_ERROR_HANDLE: i64 = -12;

pub fn get_std_handle(nstd_handle: i64) -> i64 {
    match nstd_handle {
        STD_OUTPUT_HANDLE => 1,
        STD_ERROR_HANDLE => 2,
        STD_INPUT_HANDLE => 0,
        _ => -1,
    }
}

pub fn write_file(handle: i64, buffer: &[u8]) -> i32 {
    if handle == 1 {
        let _ = io::stdout().write_all(buffer);
        let _ = io::stdout().flush();
        buffer.len() as i32
    } else if handle == 2 {
        let _ = io::stderr().write_all(buffer);
        let _ = io::stderr().flush();
        buffer.len() as i32
    } else {
        0
    }
}

pub fn exit_process(code: u32) -> ! {
    std::process::exit(code as i32);
}
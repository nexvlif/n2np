use std::cell::Cell;

pub const ERROR_SUCCESS: u32 = 0;
pub const ERROR_FILE_NOT_FOUND: u32 = 2;
pub const ERROR_PATH_NOT_FOUND: u32 = 3;
pub const ERROR_ACCESS_DENIED: u32 = 5;
pub const ERROR_INVALID_HANDLE: u32 = 6;
pub const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
pub const ERROR_INVALID_PARAMETER: u32 = 87;
pub const ERROR_BROKEN_PIPE: u32 = 109;
pub const ERROR_DISK_FULL: u32 = 112;
pub const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
pub const ERROR_INVALID_NAME: u32 = 123;
pub const ERROR_MOD_NOT_FOUND: u32 = 126;
pub const ERROR_PROC_NOT_FOUND: u32 = 127;
pub const ERROR_ALREADY_EXISTS: u32 = 183;
pub const ERROR_ENVVAR_NOT_FOUND: u32 = 203;
pub const ERROR_DIRECTORY: u32 = 267;
pub const ERROR_OPERATION_ABORTED: u32 = 995;

thread_local! {
    static LAST_ERROR: Cell<u32> = const { Cell::new(ERROR_SUCCESS) };
}

pub fn set_last_error(code: u32) {
    LAST_ERROR.with(|cell| cell.set(code));
}

pub fn get_last_error() -> u32 {
    LAST_ERROR.with(Cell::get)
}

pub fn from_io(err: &std::io::Error) -> u32 {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound => ERROR_FILE_NOT_FOUND,
        ErrorKind::PermissionDenied => ERROR_ACCESS_DENIED,
        ErrorKind::AlreadyExists => ERROR_ALREADY_EXISTS,
        ErrorKind::InvalidInput | ErrorKind::InvalidData => ERROR_INVALID_PARAMETER,
        ErrorKind::WriteZero | ErrorKind::BrokenPipe => ERROR_BROKEN_PIPE,
        ErrorKind::StorageFull => ERROR_DISK_FULL,
        ErrorKind::OutOfMemory => ERROR_NOT_ENOUGH_MEMORY,
        _ => err
            .raw_os_error()
            .map(|v| v as u32)
            .unwrap_or(ERROR_INVALID_PARAMETER),
    }
}

pub fn set_from_io(err: &std::io::Error) {
    set_last_error(from_io(err));
}

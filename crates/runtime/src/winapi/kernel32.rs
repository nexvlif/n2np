use crate::error::{
    self, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_DIRECTORY, ERROR_ENVVAR_NOT_FOUND,
    ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_HANDLE, ERROR_INVALID_PARAMETER, ERROR_MOD_NOT_FOUND,
    ERROR_NOT_ENOUGH_MEMORY, ERROR_PROC_NOT_FOUND,
};
use crate::handles::{
    self, EventState, HandleKind, MutexState, SemaphoreState, ThreadState, INVALID_HANDLE_VALUE,
};
use crate::memory;
use crate::modules;
use crate::utf;
use crate::winapi;
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const CREATE_NEW: u32 = 1;
const CREATE_ALWAYS: u32 = 2;
const OPEN_EXISTING: u32 = 3;
const OPEN_ALWAYS: u32 = 4;
const TRUNCATE_EXISTING: u32 = 5;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFF_FFFF;
const FILE_BEGIN: u32 = 0;
const FILE_CURRENT: u32 = 1;
const FILE_END: u32 = 2;
const INVALID_SET_FILE_POINTER: u32 = 0xFFFF_FFFF;
const FILE_TYPE_DISK: u32 = 1;
const FILE_TYPE_CHAR: u32 = 2;
const STILL_ACTIVE: u32 = 259;
const INFINITE: u32 = 0xFFFF_FFFF;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 0x102;
const WAIT_FAILED: u32 = 0xFFFF_FFFF;
const GMEM_FIXED: u32 = 0;
const GMEM_ZEROINIT: u32 = 0x40;
const LMEM_FIXED: u32 = 0;
const LMEM_ZEROINIT: u32 = 0x40;
const HEAP_NO_SERIALIZE: u32 = 0x0000_0001;
const STARTUPINFO_SIZE: usize = 104;

static START: OnceLock<Instant> = OnceLock::new();
static CMDLINE: OnceLock<(Vec<u8>, Vec<u16>)> = OnceLock::new();
static ENV_A_BLOCKS: OnceLock<Mutex<HashMap<usize, Vec<u8>>>> = OnceLock::new();
static ENV_W_BLOCKS: OnceLock<Mutex<HashMap<usize, Vec<u16>>>> = OnceLock::new();
static NEXT_TLS: AtomicU32 = AtomicU32::new(1);
static NEXT_THREAD_ID: AtomicU32 = AtomicU32::new(1);
static PROCESS_HEAP: usize = 0x00B0_0000;
static CRITICAL_SECTIONS: OnceLock<Mutex<HashMap<usize, Arc<CriticalSectionState>>>> =
    OnceLock::new();

#[derive(Default)]
struct CriticalSectionInner {
    owner: u32,
    recursion: u32,
    spin: u32,
}

struct CriticalSectionState {
    inner: Mutex<CriticalSectionInner>,
    ready: Condvar,
}

impl CriticalSectionState {
    fn new() -> Self {
        Self {
            inner: Mutex::new(CriticalSectionInner::default()),
            ready: Condvar::new(),
        }
    }

    fn acquire(&self, thread: u32) -> u32 {
        let mut inner = self.inner.lock().unwrap();
        while inner.owner != 0 && inner.owner != thread {
            inner = self.ready.wait(inner).unwrap();
        }
        if inner.owner == thread {
            inner.recursion = inner.recursion.saturating_add(1);
        } else {
            inner.owner = thread;
            inner.recursion = 1;
        }
        inner.recursion
    }

    fn try_acquire(&self, thread: u32) -> Option<u32> {
        let mut inner = self.inner.lock().unwrap();
        if inner.owner != 0 && inner.owner != thread {
            return None;
        }
        if inner.owner == thread {
            inner.recursion = inner.recursion.saturating_add(1);
        } else {
            inner.owner = thread;
            inner.recursion = 1;
        }
        Some(inner.recursion)
    }

    fn release(&self, thread: u32) -> Option<u32> {
        let mut inner = self.inner.lock().unwrap();
        if inner.owner != thread {
            return None;
        }
        inner.recursion = inner.recursion.saturating_sub(1);
        if inner.recursion == 0 {
            inner.owner = 0;
            self.ready.notify_one();
        }
        Some(inner.recursion)
    }

    fn set_spin(&self, spin: u32) -> u32 {
        let mut inner = self.inner.lock().unwrap();
        std::mem::replace(&mut inner.spin, spin)
    }
}

fn critical_sections() -> &'static Mutex<HashMap<usize, Arc<CriticalSectionState>>> {
    CRITICAL_SECTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn critical_section_for(section: usize) -> Arc<CriticalSectionState> {
    let mut sections = critical_sections().lock().unwrap();
    sections
        .entry(section)
        .or_insert_with(|| Arc::new(CriticalSectionState::new()))
        .clone()
}

thread_local! {
    static THREAD_ID: Cell<u32> = const { Cell::new(0) };
    static TLS_VALUES: std::cell::RefCell<HashMap<u32, usize>> =
        std::cell::RefCell::new(HashMap::new());
}

fn started() -> Instant {
    *START.get_or_init(Instant::now)
}

fn current_thread_id() -> u32 {
    THREAD_ID.with(|cell| {
        let value = cell.get();
        if value != 0 {
            return value;
        }
        let value = NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed).max(1);
        cell.set(value);
        value
    })
}

fn allocate_thread_id() -> u32 {
    NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed).max(1)
}

fn command_line() -> &'static (Vec<u8>, Vec<u16>) {
    CMDLINE.get_or_init(|| {
        let line = std::env::args()
            .map(|arg| {
                if arg.contains(' ') {
                    format!("\"{arg}\"")
                } else {
                    arg
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        (utf::ansi_with_nul(&line), utf::wide_with_nul(&line))
    })
}

fn prefix_path() -> PathBuf {
    crate::prefix::active_root()
}

pub fn host_path(text: &str) -> PathBuf {
    let text = text.trim().trim_matches('"');
    if text.is_empty() {
        return PathBuf::new();
    }
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && (bytes[0] as char).is_ascii_alphabetic() {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let rest = text[2..].trim_start_matches(['\\', '/']).replace('\\', "/");
        if drive == 'z' {
            return PathBuf::from(format!("/{rest}"));
        }
        return prefix_path().join(format!("drive_{drive}")).join(rest);
    }
    PathBuf::from(text.replace('\\', "/"))
}

unsafe fn ansi_path(ptr: *const u8) -> Option<PathBuf> {
    if ptr.is_null() {
        return None;
    }
    Some(host_path(&utf::read_ansi(ptr)))
}

unsafe fn wide_path(ptr: *const u16) -> Option<PathBuf> {
    if ptr.is_null() {
        return None;
    }
    Some(host_path(&utf::read_wide(ptr)))
}

fn find_env(name: &str) -> Option<String> {
    std::env::vars()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

fn env_block_a() -> *const u8 {
    let mut block = Vec::new();
    for (key, value) in std::env::vars() {
        block.extend_from_slice(format!("{key}={value}").as_bytes());
        block.push(0);
    }
    block.push(0);
    let pointer = block.as_ptr();
    ENV_A_BLOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(pointer as usize, block);
    pointer
}

fn env_block_w() -> *const u16 {
    let mut block = Vec::new();
    for (key, value) in std::env::vars() {
        block.extend(format!("{key}={value}").encode_utf16());
        block.push(0);
    }
    block.push(0);
    let pointer = block.as_ptr();
    ENV_W_BLOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(pointer as usize, block);
    pointer
}

fn expand_env(input: &str) -> String {
    let mut out = String::new();
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                if !name.is_empty() {
                    if let Some(value) = find_env(name) {
                        out.push_str(&value);
                    } else {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                } else {
                    out.push_str("%%");
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

unsafe fn copy_out_a(dst: *mut u8, cap: u32, text: &str) -> u32 {
    let needed = text.len() + 1;
    if dst.is_null() {
        error::set_last_error(ERROR_INSUFFICIENT_BUFFER);
        return needed as u32;
    }
    if cap as usize >= needed {
        utf::write_ansi(dst, text, cap as usize);
        return text.len() as u32;
    }
    error::set_last_error(ERROR_INSUFFICIENT_BUFFER);
    needed as u32
}

unsafe fn copy_out_w(dst: *mut u16, cap: u32, text: &str) -> u32 {
    let units = text.encode_utf16().count();
    let needed = units + 1;
    if dst.is_null() {
        error::set_last_error(ERROR_INSUFFICIENT_BUFFER);
        return needed as u32;
    }
    if cap as usize >= needed {
        utf::write_wide(dst, text, cap as usize);
        return units as u32;
    }
    error::set_last_error(ERROR_INSUFFICIENT_BUFFER);
    needed as u32
}

fn write_handle(handle: usize, data: &[u8]) -> io::Result<()> {
    if let Some(index) = handles::std_index(handle) {
        return match index {
            1 => io::stdout()
                .write_all(data)
                .and_then(|_| io::stdout().flush()),
            2 => io::stderr()
                .write_all(data)
                .and_then(|_| io::stderr().flush()),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "not writable")),
        };
    }
    handles::with_mut(handle, |kind| match kind {
        HandleKind::File { file, .. } => file.write_all(data).and_then(|_| file.flush()),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file")),
    })
    .unwrap_or_else(|| {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid handle",
        ))
    })
}

fn read_handle(handle: usize, data: &mut [u8]) -> io::Result<usize> {
    if let Some(index) = handles::std_index(handle) {
        return match index {
            0 => io::stdin().read(data),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "not readable")),
        };
    }
    handles::with_mut(handle, |kind| match kind {
        HandleKind::File { file, .. } => file.read(data),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file")),
    })
    .unwrap_or_else(|| {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid handle",
        ))
    })
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + i64::from(m <= 2), m as u32, d as u32)
}

fn system_time_parts() -> (u16, u16, u16, u16, u16, u16, u16, u16) {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let dow = (days + 4).rem_euclid(7) as u16;
    (
        year as u16,
        month as u16,
        dow,
        day as u16,
        (rem / 3600) as u16,
        ((rem % 3600) / 60) as u16,
        (rem % 60) as u16,
        (duration.subsec_millis()) as u16,
    )
}

fn filetime_now() -> u64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (duration.as_secs() + 11_644_473_600) * 10_000_000 + u64::from(duration.subsec_nanos()) / 100
}

unsafe fn write_system_time(dst: *mut u16) {
    if dst.is_null() {
        return;
    }
    let (year, month, dow, day, hour, minute, second, millis) = system_time_parts();
    let values = [year, month, dow, day, hour, minute, second, millis];
    ptr::copy_nonoverlapping(values.as_ptr(), dst, values.len());
}

fn open_file(path: &PathBuf, access: u32, disposition: u32) -> io::Result<File> {
    let read = access & GENERIC_READ != 0 || access == 0;
    let write = access & GENERIC_WRITE != 0;
    let mut options = OpenOptions::new();
    options.read(read).write(write);
    match disposition {
        CREATE_NEW => {
            options.create_new(true);
        }
        CREATE_ALWAYS => {
            options.create(true).truncate(true);
        }
        OPEN_EXISTING => {}
        OPEN_ALWAYS => {
            options.create(true);
        }
        TRUNCATE_EXISTING => {
            options.truncate(true);
        }
        _ => {}
    }
    options.open(path)
}

fn create_file(path: PathBuf, access: u32, disposition: u32, flags: u32) -> usize {
    if path.as_os_str().is_empty() {
        error::set_last_error(ERROR_INVALID_PARAMETER);
        return INVALID_HANDLE_VALUE;
    }
    let metadata = fs::metadata(&path);
    if let Ok(metadata) = &metadata {
        if metadata.is_dir() {
            if disposition == OPEN_EXISTING && flags & FILE_FLAG_BACKUP_SEMANTICS != 0 {
                return handles::insert(HandleKind::Directory { path });
            }
            error::set_last_error(ERROR_ACCESS_DENIED);
            return INVALID_HANDLE_VALUE;
        }
    }
    match open_file(&path, access, disposition) {
        Ok(file) => handles::insert(HandleKind::File { file, path }),
        Err(err) => {
            error::set_from_io(&err);
            INVALID_HANDLE_VALUE
        }
    }
}

fn module_path(handle: usize) -> Option<String> {
    if let Some(path) = modules::path_for(handle) {
        return Some(path);
    }
    let name = modules::name_for(handle)?;
    Some(format!("C:\\windows\\system32\\{name}"))
}

fn wait_timeout_ms(milliseconds: u32) -> Option<Duration> {
    if milliseconds == INFINITE {
        None
    } else {
        Some(Duration::from_millis(u64::from(milliseconds)))
    }
}

fn create_event(manual_reset: bool, initial: bool) -> usize {
    handles::insert(HandleKind::Event(Arc::new(EventState::new(
        manual_reset,
        initial,
    ))))
}

fn create_mutex(initial_owner: bool) -> usize {
    let owner = initial_owner.then(current_thread_id);
    handles::insert(HandleKind::Mutex(Arc::new(MutexState::new(owner))))
}

fn create_semaphore(initial: u32, maximum: u32) -> usize {
    handles::insert(HandleKind::Semaphore(Arc::new(SemaphoreState::new(
        initial, maximum,
    ))))
}

pub fn register_main_module(path: &str, handle: usize) {
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("main.exe");
    modules::register(name, handle);
    modules::set_main(handle);
    modules::set_path(handle, path);
}

winapi_api!(
    "kernel32.dll",
    "GetStdHandle",
    pub unsafe extern "win64" fn get_std_handle(nstd_handle: u32) -> usize {
        handles::std_handle(nstd_handle)
    }
);

winapi_api!(
    "kernel32.dll",
    "SetStdHandle",
    pub unsafe extern "win64" fn set_std_handle(nstd_handle: u32, handle: usize) -> i32 {
        if handles::set_std_handle(nstd_handle, handle) {
            1
        } else {
            0
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "WriteFile",
    pub unsafe extern "win64" fn write_file(
        handle: usize,
        buffer: *const u8,
        bytes_to_write: u32,
        bytes_written: *mut u32,
        _overlapped: *mut c_void,
    ) -> i32 {
        if !bytes_written.is_null() {
            *bytes_written = 0;
        }
        if buffer.is_null() && bytes_to_write != 0 {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        let data = if bytes_to_write == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(buffer, bytes_to_write as usize)
        };
        match write_handle(handle, data) {
            Ok(()) => {
                if !bytes_written.is_null() {
                    *bytes_written = bytes_to_write;
                }
                1
            }
            Err(err) => {
                error::set_from_io(&err);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "ReadFile",
    pub unsafe extern "win64" fn read_file(
        handle: usize,
        buffer: *mut u8,
        bytes_to_read: u32,
        bytes_read: *mut u32,
        _overlapped: *mut c_void,
    ) -> i32 {
        if !bytes_read.is_null() {
            *bytes_read = 0;
        }
        if buffer.is_null() && bytes_to_read != 0 {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        let data = if bytes_to_read == 0 {
            &mut [][..]
        } else {
            std::slice::from_raw_parts_mut(buffer, bytes_to_read as usize)
        };
        match read_handle(handle, data) {
            Ok(read) => {
                if !bytes_read.is_null() {
                    *bytes_read = read as u32;
                }
                1
            }
            Err(err) => {
                error::set_from_io(&err);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "CreateFileA",
    pub unsafe extern "win64" fn create_file_a(
        file_name: *const u8,
        access: u32,
        _share: u32,
        _security: *mut c_void,
        disposition: u32,
        flags: u32,
        _template: usize,
    ) -> usize {
        match ansi_path(file_name) {
            Some(path) => create_file(path, access, disposition, flags),
            None => {
                error::set_last_error(ERROR_INVALID_PARAMETER);
                INVALID_HANDLE_VALUE
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "CreateFileW",
    pub unsafe extern "win64" fn create_file_w(
        file_name: *const u16,
        access: u32,
        _share: u32,
        _security: *mut c_void,
        disposition: u32,
        flags: u32,
        _template: usize,
    ) -> usize {
        match wide_path(file_name) {
            Some(path) => create_file(path, access, disposition, flags),
            None => {
                error::set_last_error(ERROR_INVALID_PARAMETER);
                INVALID_HANDLE_VALUE
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "CloseHandle",
    pub unsafe extern "win64" fn close_handle(handle: usize) -> i32 {
        if handle == INVALID_HANDLE_VALUE || handle == 0 {
            error::set_last_error(ERROR_INVALID_HANDLE);
            return 0;
        }
        if handles::remove(handle).is_some() {
            1
        } else {
            error::set_last_error(ERROR_INVALID_HANDLE);
            0
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GetFileSize",
    pub unsafe extern "win64" fn get_file_size(handle: usize, high: *mut u32) -> u32 {
        if !high.is_null() {
            *high = 0;
        }
        let result = handles::with(handle, |kind| kind.path().cloned())
            .flatten()
            .and_then(|path| fs::metadata(path).ok())
            .map(|metadata| metadata.len());
        match result {
            Some(size) if size <= u64::from(u32::MAX) => size as u32,
            Some(_) => {
                if !high.is_null() {
                    *high = 1;
                }
                0
            }
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                INVALID_SET_FILE_POINTER
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GetFileSizeEx",
    pub unsafe extern "win64" fn get_file_size_ex(handle: usize, size: *mut i64) -> i32 {
        if size.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        let result = handles::with(handle, |kind| kind.path().cloned())
            .flatten()
            .and_then(|path| fs::metadata(path).ok())
            .map(|metadata| metadata.len() as i64);
        match result {
            Some(value) => {
                *size = value;
                1
            }
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "SetFilePointer",
    pub unsafe extern "win64" fn set_file_pointer(
        handle: usize,
        low: u32,
        high: *mut u32,
        method: u32,
    ) -> u32 {
        let high_value = if high.is_null() { 0 } else { *high };
        let distance = ((high_value as u64) << 32 | u64::from(low)) as i64;
        let seek = match method {
            FILE_BEGIN => SeekFrom::Start(distance as u64),
            FILE_CURRENT => SeekFrom::Current(distance),
            FILE_END => SeekFrom::End(distance),
            _ => {
                error::set_last_error(ERROR_INVALID_PARAMETER);
                return INVALID_SET_FILE_POINTER;
            }
        };
        let result = handles::with_mut(handle, |kind| match kind {
            HandleKind::File { file, .. } => file.seek(seek).ok(),
            _ => None,
        })
        .flatten();
        match result {
            Some(position) => {
                if !high.is_null() {
                    *high = (position >> 32) as u32;
                }
                position as u32
            }
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                INVALID_SET_FILE_POINTER
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "SetFilePointerEx",
    pub unsafe extern "win64" fn set_file_pointer_ex(
        handle: usize,
        distance: i64,
        new_position: *mut i64,
        method: u32,
    ) -> i32 {
        let seek = match method {
            FILE_BEGIN => {
                if distance < 0 {
                    error::set_last_error(ERROR_INVALID_PARAMETER);
                    return 0;
                }
                SeekFrom::Start(distance as u64)
            }
            FILE_CURRENT => SeekFrom::Current(distance),
            FILE_END => SeekFrom::End(distance),
            _ => {
                error::set_last_error(ERROR_INVALID_PARAMETER);
                return 0;
            }
        };
        let result = handles::with_mut(handle, |kind| match kind {
            HandleKind::File { file, .. } => file.seek(seek).ok(),
            _ => None,
        })
        .flatten();
        match result {
            Some(position) => {
                if !new_position.is_null() {
                    *new_position = position as i64;
                }
                1
            }
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "SetEndOfFile",
    pub unsafe extern "win64" fn set_end_of_file(handle: usize) -> i32 {
        let result = handles::with_mut(handle, |kind| match kind {
            HandleKind::File { file, .. } => file
                .stream_position()
                .and_then(|position| file.set_len(position))
                .map(|_| ())
                .map_err(|err| err),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file")),
        });
        match result {
            Some(Ok(())) => 1,
            Some(Err(err)) => {
                error::set_from_io(&err);
                0
            }
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "FlushFileBuffers",
    pub unsafe extern "win64" fn flush_file_buffers(handle: usize) -> i32 {
        let result = handles::with_mut(handle, |kind| match kind {
            HandleKind::File { file, .. } => file.flush(),
            HandleKind::Directory { .. } => Ok(()),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file")),
        });
        match result {
            Some(Ok(())) => 1,
            Some(Err(err)) => {
                error::set_from_io(&err);
                0
            }
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GetFileType",
    pub unsafe extern "win64" fn get_file_type(handle: usize) -> u32 {
        if handles::std_index(handle).is_some() {
            return FILE_TYPE_CHAR;
        }
        match handles::with(handle, |kind| match kind {
            HandleKind::File { .. } | HandleKind::Directory { .. } => FILE_TYPE_DISK,
            _ => 0,
        }) {
            Some(value) => value,
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "DeleteFileA",
    pub unsafe extern "win64" fn delete_file_a(path: *const u8) -> i32 {
        delete_file(host_path(&utf::read_ansi(path)))
    }
);

winapi_api!(
    "kernel32.dll",
    "DeleteFileW",
    pub unsafe extern "win64" fn delete_file_w(path: *const u16) -> i32 {
        delete_file(host_path(&utf::read_wide(path)))
    }
);

fn delete_file(path: PathBuf) -> i32 {
    match fs::remove_file(path) {
        Ok(()) => 1,
        Err(err) => {
            error::set_from_io(&err);
            0
        }
    }
}

winapi_api!(
    "kernel32.dll",
    "CopyFileA",
    pub unsafe extern "win64" fn copy_file_a(
        source: *const u8,
        target: *const u8,
        fail_if_exists: i32,
    ) -> i32 {
        copy_file(
            host_path(&utf::read_ansi(source)),
            host_path(&utf::read_ansi(target)),
            fail_if_exists != 0,
        )
    }
);

winapi_api!(
    "kernel32.dll",
    "CopyFileW",
    pub unsafe extern "win64" fn copy_file_w(
        source: *const u16,
        target: *const u16,
        fail_if_exists: i32,
    ) -> i32 {
        copy_file(
            host_path(&utf::read_wide(source)),
            host_path(&utf::read_wide(target)),
            fail_if_exists != 0,
        )
    }
);

fn copy_file(source: PathBuf, target: PathBuf, fail_if_exists: bool) -> i32 {
    if fail_if_exists && target.exists() {
        error::set_last_error(ERROR_ALREADY_EXISTS);
        return 0;
    }
    match fs::copy(&source, &target) {
        Ok(_) => 1,
        Err(err) => {
            error::set_from_io(&err);
            0
        }
    }
}

winapi_api!(
    "kernel32.dll",
    "MoveFileA",
    pub unsafe extern "win64" fn move_file_a(source: *const u8, target: *const u8) -> i32 {
        move_file(
            host_path(&utf::read_ansi(source)),
            host_path(&utf::read_ansi(target)),
        )
    }
);

winapi_api!(
    "kernel32.dll",
    "MoveFileW",
    pub unsafe extern "win64" fn move_file_w(source: *const u16, target: *const u16) -> i32 {
        move_file(
            host_path(&utf::read_wide(source)),
            host_path(&utf::read_wide(target)),
        )
    }
);

fn move_file(source: PathBuf, target: PathBuf) -> i32 {
    match fs::rename(&source, &target) {
        Ok(()) => 1,
        Err(err) => {
            error::set_from_io(&err);
            0
        }
    }
}

winapi_api!(
    "kernel32.dll",
    "CreateDirectoryA",
    pub unsafe extern "win64" fn create_directory_a(
        path: *const u8,
        _security: *mut c_void,
    ) -> i32 {
        create_directory(host_path(&utf::read_ansi(path)))
    }
);

winapi_api!(
    "kernel32.dll",
    "CreateDirectoryW",
    pub unsafe extern "win64" fn create_directory_w(
        path: *const u16,
        _security: *mut c_void,
    ) -> i32 {
        create_directory(host_path(&utf::read_wide(path)))
    }
);

fn create_directory(path: PathBuf) -> i32 {
    match fs::create_dir(&path) {
        Ok(()) => 1,
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            error::set_last_error(ERROR_ALREADY_EXISTS);
            0
        }
        Err(err) => {
            error::set_from_io(&err);
            0
        }
    }
}

winapi_api!(
    "kernel32.dll",
    "RemoveDirectoryA",
    pub unsafe extern "win64" fn remove_directory_a(path: *const u8) -> i32 {
        remove_directory(host_path(&utf::read_ansi(path)))
    }
);

winapi_api!(
    "kernel32.dll",
    "RemoveDirectoryW",
    pub unsafe extern "win64" fn remove_directory_w(path: *const u16) -> i32 {
        remove_directory(host_path(&utf::read_wide(path)))
    }
);

fn remove_directory(path: PathBuf) -> i32 {
    match fs::remove_dir(path) {
        Ok(()) => 1,
        Err(err) if err.kind() == io::ErrorKind::NotADirectory => {
            error::set_last_error(ERROR_DIRECTORY);
            0
        }
        Err(err) => {
            error::set_from_io(&err);
            0
        }
    }
}

winapi_api!(
    "kernel32.dll",
    "GetFileAttributesA",
    pub unsafe extern "win64" fn get_file_attributes_a(path: *const u8) -> u32 {
        get_file_attributes(host_path(&utf::read_ansi(path)))
    }
);

winapi_api!(
    "kernel32.dll",
    "GetFileAttributesW",
    pub unsafe extern "win64" fn get_file_attributes_w(path: *const u16) -> u32 {
        get_file_attributes(host_path(&utf::read_wide(path)))
    }
);

fn get_file_attributes(path: PathBuf) -> u32 {
    match fs::metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() {
                FILE_ATTRIBUTE_DIRECTORY
            } else {
                FILE_ATTRIBUTE_NORMAL
            }
        }
        Err(err) => {
            error::set_from_io(&err);
            INVALID_FILE_ATTRIBUTES
        }
    }
}

winapi_api!(
    "kernel32.dll",
    "SetFileAttributesA",
    pub unsafe extern "win64" fn set_file_attributes_a(_path: *const u8, _attributes: u32) -> i32 {
        1
    }
);
winapi_api!(
    "kernel32.dll",
    "SetFileAttributesW",
    pub unsafe extern "win64" fn set_file_attributes_w(_path: *const u16, _attributes: u32) -> i32 {
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "GetCurrentDirectoryA",
    pub unsafe extern "win64" fn get_current_directory_a(size: u32, buffer: *mut u8) -> u32 {
        let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        copy_out_a(buffer, size, &dir.to_string_lossy())
    }
);

winapi_api!(
    "kernel32.dll",
    "GetCurrentDirectoryW",
    pub unsafe extern "win64" fn get_current_directory_w(size: u32, buffer: *mut u16) -> u32 {
        let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        copy_out_w(buffer, size, &dir.to_string_lossy())
    }
);

winapi_api!(
    "kernel32.dll",
    "SetCurrentDirectoryA",
    pub unsafe extern "win64" fn set_current_directory_a(path: *const u8) -> i32 {
        set_current_directory(host_path(&utf::read_ansi(path)))
    }
);

winapi_api!(
    "kernel32.dll",
    "SetCurrentDirectoryW",
    pub unsafe extern "win64" fn set_current_directory_w(path: *const u16) -> i32 {
        set_current_directory(host_path(&utf::read_wide(path)))
    }
);

fn set_current_directory(path: PathBuf) -> i32 {
    match std::env::set_current_dir(path) {
        Ok(()) => 1,
        Err(err) => {
            error::set_from_io(&err);
            0
        }
    }
}

winapi_api!(
    "kernel32.dll",
    "GetFullPathNameA",
    pub unsafe extern "win64" fn get_full_path_name_a(
        file_name: *const u8,
        buffer_length: u32,
        buffer: *mut u8,
        file_part: *mut *mut u8,
    ) -> u32 {
        let text = utf::read_ansi(file_name);
        let path = host_path(&text);
        let full = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        };
        let full_text = full.to_string_lossy().into_owned();
        if !file_part.is_null() {
            let offset = full_text
                .rfind(['/', '\\'])
                .map(|index| index + 1)
                .unwrap_or(0);
            *file_part = buffer.add(offset);
        }
        copy_out_a(buffer, buffer_length, &full_text)
    }
);

winapi_api!(
    "kernel32.dll",
    "GetFullPathNameW",
    pub unsafe extern "win64" fn get_full_path_name_w(
        file_name: *const u16,
        buffer_length: u32,
        buffer: *mut u16,
        file_part: *mut *mut u16,
    ) -> u32 {
        let text = utf::read_wide(file_name);
        let path = host_path(&text);
        let full = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        };
        let full_text = full.to_string_lossy().into_owned();
        if !file_part.is_null() {
            let offset = full_text.encode_utf16().count()
                - full_text
                    .rsplit(['/', '\\'])
                    .next()
                    .map(|part| part.encode_utf16().count())
                    .unwrap_or(0);
            *file_part = buffer.add(offset);
        }
        copy_out_w(buffer, buffer_length, &full_text)
    }
);

winapi_api!(
    "kernel32.dll",
    "GetTempPathA",
    pub unsafe extern "win64" fn get_temp_path_a(size: u32, buffer: *mut u8) -> u32 {
        let temp = std::env::var("TMPDIR")
            .or_else(|_| std::env::var("TEMP"))
            .or_else(|_| std::env::var("TMP"))
            .unwrap_or_else(|_| "/tmp".to_string());
        let temp = temp.trim_end_matches(['/', '\\']).to_string() + "/";
        copy_out_a(buffer, size, &temp)
    }
);

winapi_api!(
    "kernel32.dll",
    "GetTempPathW",
    pub unsafe extern "win64" fn get_temp_path_w(size: u32, buffer: *mut u16) -> u32 {
        let temp = std::env::var("TMPDIR")
            .or_else(|_| std::env::var("TEMP"))
            .or_else(|_| std::env::var("TMP"))
            .unwrap_or_else(|_| "/tmp".to_string());
        let temp = temp.trim_end_matches(['/', '\\']).to_string() + "/";
        copy_out_w(buffer, size, &temp)
    }
);

winapi_api!(
    "kernel32.dll",
    "GetTempFileNameA",
    pub unsafe extern "win64" fn get_temp_file_name_a(
        path_name: *const u8,
        prefix: *const u8,
        unique: u32,
        buffer: *mut u8,
    ) -> u32 {
        let dir = host_path(&utf::read_ansi(path_name));
        let prefix = utf::read_ansi(prefix);
        for offset in 0..0xFFFFu32 {
            let number = unique.wrapping_add(offset) & 0xFFFF;
            let candidate = dir.join(format!("{prefix}{number:04x}.tmp"));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(_) => {
                    let text = candidate.to_string_lossy().into_owned();
                    utf::write_ansi(buffer, &text, 260);
                    return number;
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(err) => {
                    error::set_from_io(&err);
                    return 0;
                }
            }
        }
        error::set_last_error(ERROR_ALREADY_EXISTS);
        0
    }
);

winapi_api!(
    "kernel32.dll",
    "GetTempFileNameW",
    pub unsafe extern "win64" fn get_temp_file_name_w(
        path_name: *const u16,
        prefix: *const u16,
        unique: u32,
        buffer: *mut u16,
    ) -> u32 {
        let dir = host_path(&utf::read_wide(path_name));
        let prefix = utf::read_wide(prefix);
        for offset in 0..0xFFFFu32 {
            let number = unique.wrapping_add(offset) & 0xFFFF;
            let candidate = dir.join(format!("{prefix}{number:04x}.tmp"));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(_) => {
                    let text = candidate.to_string_lossy().into_owned();
                    utf::write_wide(buffer, &text, 260);
                    return number;
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(err) => {
                    error::set_from_io(&err);
                    return 0;
                }
            }
        }
        error::set_last_error(ERROR_ALREADY_EXISTS);
        0
    }
);

winapi_api!(
    "kernel32.dll",
    "GetModuleFileNameA",
    pub unsafe extern "win64" fn get_module_file_name_a(
        module: usize,
        buffer: *mut u8,
        size: u32,
    ) -> u32 {
        let handle = if module == 0 {
            modules::main_handle()
        } else {
            module
        };
        match module_path(handle) {
            Some(path) => copy_out_a(buffer, size, &path),
            None => {
                error::set_last_error(ERROR_MOD_NOT_FOUND);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GetModuleFileNameW",
    pub unsafe extern "win64" fn get_module_file_name_w(
        module: usize,
        buffer: *mut u16,
        size: u32,
    ) -> u32 {
        let handle = if module == 0 {
            modules::main_handle()
        } else {
            module
        };
        match module_path(handle) {
            Some(path) => copy_out_w(buffer, size, &path),
            None => {
                error::set_last_error(ERROR_MOD_NOT_FOUND);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GetModuleHandleA",
    pub unsafe extern "win64" fn get_module_handle_a(name: *const u8) -> usize {
        if name.is_null() {
            return modules::main_handle();
        }
        let name = utf::read_ansi(name);
        if name.is_empty() {
            return modules::main_handle();
        }
        match modules::handle_for(&name) {
            Some(handle) => handle,
            None => {
                error::set_last_error(ERROR_MOD_NOT_FOUND);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GetModuleHandleW",
    pub unsafe extern "win64" fn get_module_handle_w(name: *const u16) -> usize {
        if name.is_null() {
            return modules::main_handle();
        }
        let name = utf::read_wide(name);
        if name.is_empty() {
            return modules::main_handle();
        }
        match modules::handle_for(&name) {
            Some(handle) => handle,
            None => {
                error::set_last_error(ERROR_MOD_NOT_FOUND);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GetModuleHandleExA",
    pub unsafe extern "win64" fn get_module_handle_ex_a(
        flags: u32,
        name: *const u8,
        module: *mut usize,
    ) -> i32 {
        if flags & 4 != 0 || module.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        let handle = get_module_handle_a(name);
        if handle == 0 {
            return 0;
        }
        *module = handle;
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "GetModuleHandleExW",
    pub unsafe extern "win64" fn get_module_handle_ex_w(
        flags: u32,
        name: *const u16,
        module: *mut usize,
    ) -> i32 {
        if flags & 4 != 0 || module.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        let handle = get_module_handle_w(name);
        if handle == 0 {
            return 0;
        }
        *module = handle;
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "LoadLibraryA",
    pub unsafe extern "win64" fn load_library_a(name: *const u8) -> usize {
        if name.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        let name = utf::read_ansi(name);
        match modules::load_library(&name) {
            Some(handle) => handle,
            None => {
                error::set_last_error(ERROR_MOD_NOT_FOUND);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "LoadLibraryW",
    pub unsafe extern "win64" fn load_library_w(name: *const u16) -> usize {
        if name.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        let name = utf::read_wide(name);
        match modules::load_library(&name) {
            Some(handle) => handle,
            None => {
                error::set_last_error(ERROR_MOD_NOT_FOUND);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GetProcAddress",
    pub unsafe extern "win64" fn get_proc_address(module: usize, name: *const u8) -> usize {
        if module == 0 || name.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        let is_ordinal = (name as usize) < 0x1_0000;
        let (lookup, display) = if is_ordinal {
            let ordinal = name as usize;
            (format!("#{ordinal}"), format!("#{ordinal}"))
        } else {
            let value = utf::read_ansi(name);
            (value.clone(), value)
        };
        let resolved = if modules::is_builtin_handle(module) {
            let dll = modules::name_for(module).unwrap_or_default();
            if is_ordinal {
                winapi::resolve_ordinal(&dll, lookup.trim_start_matches('#').parse().unwrap_or(0))
            } else {
                winapi::resolve(&dll, &lookup)
            }
        } else {
            modules::resolve_export(module, &lookup)
        };
        match resolved {
            Some(address) => address,
            None => {
                log::warn!("[n2np-kernel32] unresolved GetProcAddress({module:#x}, {display})");
                error::set_last_error(ERROR_PROC_NOT_FOUND);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "FreeLibrary",
    pub unsafe extern "win64" fn free_library(_module: usize) -> i32 {
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "GetCommandLineA",
    pub unsafe extern "win64" fn get_command_line_a() -> *const u8 {
        command_line().0.as_ptr()
    }
);

winapi_api!(
    "kernel32.dll",
    "GetCommandLineW",
    pub unsafe extern "win64" fn get_command_line_w() -> *const u16 {
        command_line().1.as_ptr()
    }
);

winapi_api!(
    "kernel32.dll",
    "GetEnvironmentVariableA",
    pub unsafe extern "win64" fn get_environment_variable_a(
        name: *const u8,
        buffer: *mut u8,
        size: u32,
    ) -> u32 {
        let name = utf::read_ansi(name);
        match find_env(&name) {
            Some(value) => copy_out_a(buffer, size, &value),
            None => {
                error::set_last_error(ERROR_ENVVAR_NOT_FOUND);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GetEnvironmentVariableW",
    pub unsafe extern "win64" fn get_environment_variable_w(
        name: *const u16,
        buffer: *mut u16,
        size: u32,
    ) -> u32 {
        let name = utf::read_wide(name);
        match find_env(&name) {
            Some(value) => copy_out_w(buffer, size, &value),
            None => {
                error::set_last_error(ERROR_ENVVAR_NOT_FOUND);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GetEnvironmentStringsA",
    pub unsafe extern "win64" fn get_environment_strings_a() -> *const u8 {
        env_block_a()
    }
);

winapi_api!(
    "kernel32.dll",
    "GetEnvironmentStringsW",
    pub unsafe extern "win64" fn get_environment_strings_w() -> *const u16 {
        env_block_w()
    }
);

winapi_api!(
    "kernel32.dll",
    "FreeEnvironmentStringsA",
    pub unsafe extern "win64" fn free_environment_strings_a(block: *const u8) -> i32 {
        ENV_A_BLOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .remove(&(block as usize));
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "FreeEnvironmentStringsW",
    pub unsafe extern "win64" fn free_environment_strings_w(block: *const u16) -> i32 {
        ENV_W_BLOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .remove(&(block as usize));
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "ExpandEnvironmentStringsA",
    pub unsafe extern "win64" fn expand_environment_strings_a(
        source: *const u8,
        buffer: *mut u8,
        size: u32,
    ) -> u32 {
        let expanded = expand_env(&utf::read_ansi(source));
        copy_out_a(buffer, size, &expanded)
    }
);

winapi_api!(
    "kernel32.dll",
    "ExpandEnvironmentStringsW",
    pub unsafe extern "win64" fn expand_environment_strings_w(
        source: *const u16,
        buffer: *mut u16,
        size: u32,
    ) -> u32 {
        let expanded = expand_env(&utf::read_wide(source));
        copy_out_w(buffer, size, &expanded)
    }
);

winapi_api!(
    "kernel32.dll",
    "GetLastError",
    pub unsafe extern "win64" fn get_last_error() -> u32 {
        error::get_last_error()
    }
);

winapi_api!(
    "kernel32.dll",
    "SetLastError",
    pub unsafe extern "win64" fn set_last_error(code: u32) {
        error::set_last_error(code)
    }
);

winapi_api!(
    "kernel32.dll",
    "InitializeCriticalSection",
    pub unsafe extern "win64" fn initialize_critical_section(section: *mut u8) {
        if section.is_null() {
            return;
        }
        ptr::write_bytes(section, 0, 40);
        ptr::write_unaligned(section.add(8) as *mut i32, -1);
        let state = critical_section_for(section as usize);
        state.set_spin(0);
    }
);

winapi_api!(
    "kernel32.dll",
    "InitializeCriticalSectionAndSpinCount",
    pub unsafe extern "win64" fn initialize_critical_section_and_spin_count(
        section: *mut u8,
        spin_count: u32,
    ) -> i32 {
        initialize_critical_section(section);
        set_critical_section_spin_count(section, spin_count);
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "InitializeCriticalSectionEx",
    pub unsafe extern "win64" fn initialize_critical_section_ex(
        section: *mut u8,
        spin_count: u32,
        _flags: u32,
    ) -> i32 {
        initialize_critical_section_and_spin_count(section, spin_count)
    }
);

winapi_api!(
    "kernel32.dll",
    "EnterCriticalSection",
    pub unsafe extern "win64" fn enter_critical_section(section: *mut u8) {
        if section.is_null() {
            return;
        }
        let thread = current_thread_id();
        let state = critical_section_for(section as usize);
        let recursion = state.acquire(thread);
        ptr::write_unaligned(section.add(8) as *mut i32, -(recursion as i32));
        ptr::write_unaligned(section.add(12) as *mut u32, recursion);
        ptr::write_unaligned(section.add(16) as *mut u64, thread as u64);
    }
);

winapi_api!(
    "kernel32.dll",
    "LeaveCriticalSection",
    pub unsafe extern "win64" fn leave_critical_section(section: *mut u8) {
        if section.is_null() {
            return;
        }
        let thread = current_thread_id();
        let state = critical_section_for(section as usize);
        let Some(recursion) = state.release(thread) else {
            return;
        };
        let owner = if recursion == 0 { 0 } else { thread };
        ptr::write_unaligned(section.add(8) as *mut i32, -(recursion as i32));
        ptr::write_unaligned(section.add(12) as *mut u32, recursion);
        ptr::write_unaligned(section.add(16) as *mut u64, owner as u64);
    }
);

winapi_api!(
    "kernel32.dll",
    "TryEnterCriticalSection",
    pub unsafe extern "win64" fn try_enter_critical_section(section: *mut u8) -> i32 {
        if section.is_null() {
            return 0;
        }
        let thread = current_thread_id();
        let state = critical_section_for(section as usize);
        let Some(recursion) = state.try_acquire(thread) else {
            return 0;
        };
        ptr::write_unaligned(section.add(8) as *mut i32, -(recursion as i32));
        ptr::write_unaligned(section.add(12) as *mut u32, recursion);
        ptr::write_unaligned(section.add(16) as *mut u64, thread as u64);
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "DeleteCriticalSection",
    pub unsafe extern "win64" fn delete_critical_section(section: *mut u8) {
        if section.is_null() {
            return;
        }
        critical_sections()
            .lock()
            .unwrap()
            .remove(&(section as usize));
        ptr::write_bytes(section, 0, 40);
    }
);

winapi_api!(
    "kernel32.dll",
    "SetCriticalSectionSpinCount",
    pub unsafe extern "win64" fn set_critical_section_spin_count(
        section: *mut u8,
        spin_count: u32,
    ) -> u32 {
        if section.is_null() {
            return 0;
        }
        let state = critical_section_for(section as usize);
        let previous = state.set_spin(spin_count);
        ptr::write_unaligned(section.add(32) as *mut u32, spin_count);
        previous
    }
);

winapi_api!(
    "kernel32.dll",
    "GetCurrentProcess",
    pub unsafe extern "win64" fn get_current_process() -> usize {
        usize::MAX
    }
);

winapi_api!(
    "kernel32.dll",
    "GetCurrentProcessId",
    pub unsafe extern "win64" fn get_current_process_id() -> u32 {
        std::process::id()
    }
);

winapi_api!(
    "kernel32.dll",
    "GetCurrentThread",
    pub unsafe extern "win64" fn get_current_thread() -> usize {
        usize::MAX - 1
    }
);

winapi_api!(
    "kernel32.dll",
    "GetCurrentThreadId",
    pub unsafe extern "win64" fn get_current_thread_id() -> u32 {
        current_thread_id()
    }
);

winapi_api!(
    "kernel32.dll",
    "ExitProcess",
    pub unsafe extern "win64" fn exit_process(code: u32) -> ! {
        if let Err(err) = crate::trace::flush_output() {
            log::warn!("[n2np-trace] failed to write runtime profile: {err}");
        }
        std::process::exit(code as i32)
    }
);

winapi_api!(
    "kernel32.dll",
    "TerminateProcess",
    pub unsafe extern "win64" fn terminate_process(_process: usize, code: u32) -> i32 {
        if let Err(err) = crate::trace::flush_output() {
            log::warn!("[n2np-trace] failed to write runtime profile: {err}");
        }
        std::process::exit(code as i32)
    }
);

winapi_api!(
    "kernel32.dll",
    "Sleep",
    pub unsafe extern "win64" fn sleep(milliseconds: u32) {
        std::thread::sleep(Duration::from_millis(u64::from(milliseconds)))
    }
);

winapi_api!(
    "kernel32.dll",
    "SleepEx",
    pub unsafe extern "win64" fn sleep_ex(milliseconds: u32, _alertable: i32) -> u32 {
        std::thread::sleep(Duration::from_millis(u64::from(milliseconds)));
        0
    }
);

winapi_api!(
    "kernel32.dll",
    "GetTickCount",
    pub unsafe extern "win64" fn get_tick_count() -> u32 {
        started().elapsed().as_millis() as u32
    }
);

winapi_api!(
    "kernel32.dll",
    "GetTickCount64",
    pub unsafe extern "win64" fn get_tick_count64() -> u64 {
        started().elapsed().as_millis() as u64
    }
);

winapi_api!(
    "kernel32.dll",
    "QueryPerformanceCounter",
    pub unsafe extern "win64" fn query_performance_counter(counter: *mut i64) -> i32 {
        if counter.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        *counter = started().elapsed().as_nanos() as i64;
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "QueryPerformanceFrequency",
    pub unsafe extern "win64" fn query_performance_frequency(frequency: *mut i64) -> i32 {
        if frequency.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        *frequency = 1_000_000_000;
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "GetSystemTimeAsFileTime",
    pub unsafe extern "win64" fn get_system_time_as_file_time(file_time: *mut u32) {
        if file_time.is_null() {
            return;
        }
        let value = filetime_now();
        *file_time = value as u32;
        *file_time.add(1) = (value >> 32) as u32;
    }
);

winapi_api!(
    "kernel32.dll",
    "GetSystemTimePreciseAsFileTime",
    pub unsafe extern "win64" fn get_system_time_precise_as_file_time(file_time: *mut u32) {
        if file_time.is_null() {
            return;
        }
        let value = filetime_now();
        *file_time = value as u32;
        *file_time.add(1) = (value >> 32) as u32;
    }
);

winapi_api!(
    "kernel32.dll",
    "GetSystemTime",
    pub unsafe extern "win64" fn get_system_time(system_time: *mut u16) {
        write_system_time(system_time)
    }
);

winapi_api!(
    "kernel32.dll",
    "GetLocalTime",
    pub unsafe extern "win64" fn get_local_time(system_time: *mut u16) {
        write_system_time(system_time)
    }
);

winapi_api!(
    "kernel32.dll",
    "GetVersion",
    pub unsafe extern "win64" fn get_version() -> u32 {
        0x0A00_0000
    }
);

winapi_api!(
    "kernel32.dll",
    "GetVersionExA",
    pub unsafe extern "win64" fn get_version_ex_a(info: *mut u32) -> i32 {
        if info.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        *info.add(1) = 10;
        *info.add(2) = 0;
        *info.add(3) = 19045;
        *info.add(4) = 2;
        ptr::write_bytes(info.add(5) as *mut u8, 0, 128);
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "GetVersionExW",
    pub unsafe extern "win64" fn get_version_ex_w(info: *mut u32) -> i32 {
        if info.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        *info.add(1) = 10;
        *info.add(2) = 0;
        *info.add(3) = 19045;
        *info.add(4) = 2;
        ptr::write_bytes(info.add(5) as *mut u8, 0, 256);
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "GetSystemDirectoryA",
    pub unsafe extern "win64" fn get_system_directory_a(buffer: *mut u8, size: u32) -> u32 {
        copy_out_a(buffer, size, "C:\\windows\\system32")
    }
);

winapi_api!(
    "kernel32.dll",
    "GetSystemDirectoryW",
    pub unsafe extern "win64" fn get_system_directory_w(buffer: *mut u16, size: u32) -> u32 {
        copy_out_w(buffer, size, "C:\\windows\\system32")
    }
);

winapi_api!(
    "kernel32.dll",
    "GetWindowsDirectoryA",
    pub unsafe extern "win64" fn get_windows_directory_a(buffer: *mut u8, size: u32) -> u32 {
        copy_out_a(buffer, size, "C:\\windows")
    }
);

winapi_api!(
    "kernel32.dll",
    "GetWindowsDirectoryW",
    pub unsafe extern "win64" fn get_windows_directory_w(buffer: *mut u16, size: u32) -> u32 {
        copy_out_w(buffer, size, "C:\\windows")
    }
);

winapi_api!(
    "kernel32.dll",
    "GetComputerNameA",
    pub unsafe extern "win64" fn get_computer_name_a(buffer: *mut u8, size: *mut u32) -> i32 {
        copy_name_a("n2np", buffer, size)
    }
);

winapi_api!(
    "kernel32.dll",
    "GetComputerNameW",
    pub unsafe extern "win64" fn get_computer_name_w(buffer: *mut u16, size: *mut u32) -> i32 {
        copy_name_w("n2np", buffer, size)
    }
);

winapi_api!(
    "kernel32.dll",
    "GetUserNameA",
    pub unsafe extern "win64" fn get_user_name_a(buffer: *mut u8, size: *mut u32) -> i32 {
        copy_name_a("n2np", buffer, size)
    }
);

winapi_api!(
    "kernel32.dll",
    "GetUserNameW",
    pub unsafe extern "win64" fn get_user_name_w(buffer: *mut u16, size: *mut u32) -> i32 {
        copy_name_w("n2np", buffer, size)
    }
);

unsafe fn copy_name_a(value: &str, buffer: *mut u8, size: *mut u32) -> i32 {
    if size.is_null() {
        error::set_last_error(ERROR_INVALID_PARAMETER);
        return 0;
    }
    let needed = value.len() as u32 + 1;
    if buffer.is_null() || *size < needed {
        *size = needed;
        error::set_last_error(ERROR_INSUFFICIENT_BUFFER);
        return 0;
    }
    utf::write_ansi(buffer, value, *size as usize);
    *size = value.len() as u32;
    1
}

unsafe fn copy_name_w(value: &str, buffer: *mut u16, size: *mut u32) -> i32 {
    if size.is_null() {
        error::set_last_error(ERROR_INVALID_PARAMETER);
        return 0;
    }
    let units = value.encode_utf16().count() as u32;
    let needed = units + 1;
    if buffer.is_null() || *size < needed {
        *size = needed;
        error::set_last_error(ERROR_INSUFFICIENT_BUFFER);
        return 0;
    }
    utf::write_wide(buffer, value, *size as usize);
    *size = units;
    1
}

winapi_api!(
    "kernel32.dll",
    "GetStartupInfoA",
    pub unsafe extern "win64" fn get_startup_info_a(info: *mut u8) {
        if info.is_null() {
            return;
        }
        ptr::write_bytes(info, 0, STARTUPINFO_SIZE);
        *(info as *mut u32) = STARTUPINFO_SIZE as u32;
    }
);

winapi_api!(
    "kernel32.dll",
    "GetStartupInfoW",
    pub unsafe extern "win64" fn get_startup_info_w(info: *mut u8) {
        if info.is_null() {
            return;
        }
        ptr::write_bytes(info, 0, STARTUPINFO_SIZE);
        *(info as *mut u32) = STARTUPINFO_SIZE as u32;
    }
);

winapi_api!(
    "kernel32.dll",
    "CreateThread",
    pub unsafe extern "win64" fn create_thread(
        _attributes: *mut c_void,
        _stack_size: usize,
        start_address: usize,
        parameter: *mut c_void,
        flags: u32,
        thread_id: *mut u32,
    ) -> usize {
        const CREATE_SUSPENDED: u32 = 0x0000_0004;
        if start_address == 0 {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        let start: unsafe extern "win64" fn(*mut c_void) -> u32 =
            std::mem::transmute(start_address);
        let parameter = parameter as usize;
        let state = Arc::new(ThreadState::with_suspended(flags & CREATE_SUSPENDED != 0));
        let handle = handles::insert(HandleKind::Thread(state.clone()));
        let id = allocate_thread_id();
        if !thread_id.is_null() {
            *thread_id = id;
        }
        std::thread::spawn(move || {
            THREAD_ID.with(|cell| cell.set(id));
            state.wait_until_started();
            let code = unsafe { start(parameter as *mut c_void) };
            state.finish(code);
        });
        handle
    }
);

winapi_api!(
    "kernel32.dll",
    "ResumeThread",
    pub unsafe extern "win64" fn resume_thread(handle: usize) -> u32 {
        match handles::with(handle, |kind| match kind {
            HandleKind::Thread(state) => state.resume(),
            _ => u32::MAX,
        }) {
            Some(value) => value,
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                u32::MAX
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "WaitForSingleObject",
    pub unsafe extern "win64" fn wait_for_single_object(handle: usize, milliseconds: u32) -> u32 {
        match handles::wait(handle, wait_timeout_ms(milliseconds), current_thread_id()) {
            handles::WaitOutcome::Signaled => WAIT_OBJECT_0,
            handles::WaitOutcome::Timeout => WAIT_TIMEOUT,
            handles::WaitOutcome::Invalid => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                WAIT_FAILED
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "WaitForMultipleObjects",
    pub unsafe extern "win64" fn wait_for_multiple_objects(
        count: u32,
        handles_ptr: *const usize,
        wait_all: i32,
        milliseconds: u32,
    ) -> u32 {
        if handles_ptr.is_null() || count == 0 || count > 64 {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return WAIT_FAILED;
        }
        if count == 1 {
            return wait_for_single_object(*handles_ptr, milliseconds);
        }
        let list = std::slice::from_raw_parts(handles_ptr, count as usize);
        let thread_id = current_thread_id();
        let deadline = wait_timeout_ms(milliseconds).map(|timeout| Instant::now() + timeout);
        loop {
            if wait_all != 0 {
                if list.iter().all(|handle| handles::is_signaled(*handle)) {
                    for handle in list {
                        handles::consume_signal(*handle, thread_id);
                    }
                    return WAIT_OBJECT_0;
                }
            } else if let Some(index) = list.iter().position(|handle| handles::is_signaled(*handle))
            {
                handles::consume_signal(list[index], thread_id);
                return WAIT_OBJECT_0 + index as u32;
            }
            if let Some(deadline) = deadline {
                if Instant::now() >= deadline {
                    return WAIT_TIMEOUT;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GetExitCodeThread",
    pub unsafe extern "win64" fn get_exit_code_thread(handle: usize, code: *mut u32) -> i32 {
        if code.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        match handles::with(handle, |kind| match kind {
            HandleKind::Thread(state) => {
                *code = if state.is_done() {
                    state.exit_code()
                } else {
                    STILL_ACTIVE
                };
                1
            }
            _ => 0,
        }) {
            Some(value) => value,
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "CreateMutexA",
    pub unsafe extern "win64" fn create_mutex_a(
        _attributes: *mut c_void,
        initial_owner: i32,
        _name: *const u8,
    ) -> usize {
        create_mutex(initial_owner != 0)
    }
);

winapi_api!(
    "kernel32.dll",
    "CreateMutexW",
    pub unsafe extern "win64" fn create_mutex_w(
        _attributes: *mut c_void,
        initial_owner: i32,
        _name: *const u16,
    ) -> usize {
        create_mutex(initial_owner != 0)
    }
);

winapi_api!(
    "kernel32.dll",
    "ReleaseMutex",
    pub unsafe extern "win64" fn release_mutex(handle: usize) -> i32 {
        let thread_id = current_thread_id();
        match handles::with(handle, |kind| match kind {
            HandleKind::Mutex(state) => i32::from(state.release(thread_id)),
            _ => 0,
        }) {
            Some(value) => value,
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "CreateSemaphoreA",
    pub unsafe extern "win64" fn create_semaphore_a(
        _attributes: *mut c_void,
        initial: i32,
        maximum: i32,
        _name: *const u8,
    ) -> usize {
        create_semaphore(initial.max(0) as u32, maximum.max(1) as u32)
    }
);

winapi_api!(
    "kernel32.dll",
    "CreateSemaphoreW",
    pub unsafe extern "win64" fn create_semaphore_w(
        _attributes: *mut c_void,
        initial: i32,
        maximum: i32,
        _name: *const u16,
    ) -> usize {
        create_semaphore(initial.max(0) as u32, maximum.max(1) as u32)
    }
);

winapi_api!(
    "kernel32.dll",
    "ReleaseSemaphore",
    pub unsafe extern "win64" fn release_semaphore(
        handle: usize,
        release_count: i32,
        previous: *mut i32,
    ) -> i32 {
        if release_count <= 0 {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        match handles::with(handle, |kind| match kind {
            HandleKind::Semaphore(state) => match state.release(release_count as u32) {
                Some(value) => {
                    if !previous.is_null() {
                        *previous = value as i32;
                    }
                    1
                }
                None => {
                    error::set_last_error(ERROR_INVALID_PARAMETER);
                    0
                }
            },
            _ => 0,
        }) {
            Some(value) => value,
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "CreateEventA",
    pub unsafe extern "win64" fn create_event_a(
        _attributes: *mut c_void,
        manual_reset: i32,
        initial_state: i32,
        _name: *const u8,
    ) -> usize {
        create_event(manual_reset != 0, initial_state != 0)
    }
);

winapi_api!(
    "kernel32.dll",
    "CreateEventW",
    pub unsafe extern "win64" fn create_event_w(
        _attributes: *mut c_void,
        manual_reset: i32,
        initial_state: i32,
        _name: *const u16,
    ) -> usize {
        create_event(manual_reset != 0, initial_state != 0)
    }
);

winapi_api!(
    "kernel32.dll",
    "SetEvent",
    pub unsafe extern "win64" fn set_event(handle: usize) -> i32 {
        match handles::with(handle, |kind| match kind {
            HandleKind::Event(state) => {
                state.set();
                1
            }
            _ => 0,
        }) {
            Some(value) => value,
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "ResetEvent",
    pub unsafe extern "win64" fn reset_event(handle: usize) -> i32 {
        match handles::with(handle, |kind| match kind {
            HandleKind::Event(state) => {
                state.reset();
                1
            }
            _ => 0,
        }) {
            Some(value) => value,
            None => {
                error::set_last_error(ERROR_INVALID_HANDLE);
                0
            }
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "TlsAlloc",
    pub unsafe extern "win64" fn tls_alloc() -> u32 {
        const TLS_MINIMUM_AVAILABLE: u32 = 1088;
        const TLS_OUT_OF_INDEXES: u32 = 0xFFFF_FFFF;
        let index = NEXT_TLS.fetch_add(1, Ordering::Relaxed);
        if index >= TLS_MINIMUM_AVAILABLE {
            NEXT_TLS.store(TLS_MINIMUM_AVAILABLE, Ordering::Relaxed);
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return TLS_OUT_OF_INDEXES;
        }
        index
    }
);

winapi_api!(
    "kernel32.dll",
    "TlsFree",
    pub unsafe extern "win64" fn tls_free(index: u32) -> i32 {
        if index == 0 || index >= 1088 {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        TLS_VALUES.with(|values| values.borrow_mut().remove(&index));
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "TlsGetValue",
    pub unsafe extern "win64" fn tls_get_value(index: u32) -> usize {
        if index == 0 || index >= 1088 {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        TLS_VALUES.with(|values| values.borrow().get(&index).copied().unwrap_or(0))
    }
);

winapi_api!(
    "kernel32.dll",
    "TlsSetValue",
    pub unsafe extern "win64" fn tls_set_value(index: u32, value: usize) -> i32 {
        if index == 0 || index >= 1088 {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        TLS_VALUES.with(|values| {
            values.borrow_mut().insert(index, value);
        });
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "VirtualAlloc",
    pub unsafe extern "win64" fn virtual_alloc(
        _address: *mut c_void,
        size: usize,
        _allocation_type: u32,
        protect: u32,
    ) -> *mut c_void {
        if size == 0 {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return ptr::null_mut();
        }
        let pointer = memory::alloc(size, 4096, true, protect);
        if pointer.is_null() {
            error::set_last_error(ERROR_NOT_ENOUGH_MEMORY);
        }
        pointer as *mut c_void
    }
);

winapi_api!(
    "kernel32.dll",
    "VirtualFree",
    pub unsafe extern "win64" fn virtual_free(
        address: *mut c_void,
        _size: usize,
        free_type: u32,
    ) -> i32 {
        if free_type & memory::MEM_RELEASE != 0 {
            if memory::free(address as *mut u8) {
                1
            } else {
                0
            }
        } else {
            1
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "VirtualProtect",
    pub unsafe extern "win64" fn virtual_protect(
        address: *mut c_void,
        size: usize,
        new_protect: u32,
        old_protect: *mut u32,
    ) -> i32 {
        if !old_protect.is_null() {
            *old_protect = memory::PAGE_READWRITE;
        }
        if memory::protect(address as *mut u8, size, new_protect) {
            1
        } else {
            0
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "VirtualQuery",
    pub unsafe extern "win64" fn virtual_query(
        address: *const c_void,
        buffer: *mut u8,
        length: usize,
    ) -> usize {
        if buffer.is_null() || length < 48 {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        ptr::write_bytes(buffer, 0, 48);
        let (size, protect) = memory::block_info(address as usize).unwrap_or((0, 0));
        *(buffer as *mut usize) = address as usize;
        *(buffer.add(8) as *mut usize) = address as usize;
        *(buffer.add(16) as *mut u32) = protect;
        *(buffer.add(24) as *mut usize) = size;
        *(buffer.add(32) as *mut u32) = if size != 0 { memory::MEM_COMMIT } else { 0 };
        *(buffer.add(36) as *mut u32) = protect;
        *(buffer.add(40) as *mut u32) = if size != 0 { 0x20000 } else { 0 };
        48
    }
);

winapi_api!(
    "kernel32.dll",
    "GetProcessHeap",
    pub unsafe extern "win64" fn get_process_heap() -> usize {
        PROCESS_HEAP
    }
);

winapi_api!(
    "kernel32.dll",
    "HeapCreate",
    pub unsafe extern "win64" fn heap_create(
        _flags: u32,
        _initial: usize,
        _maximum: usize,
    ) -> usize {
        0x00B0_0001
    }
);

winapi_api!(
    "kernel32.dll",
    "HeapDestroy",
    pub unsafe extern "win64" fn heap_destroy(_heap: usize) -> i32 {
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "HeapAlloc",
    pub unsafe extern "win64" fn heap_alloc(_heap: usize, flags: u32, bytes: usize) -> *mut c_void {
        memory::alloc(
            bytes,
            16,
            flags & memory::HEAP_ZERO_MEMORY != 0,
            memory::PAGE_READWRITE,
        ) as *mut c_void
    }
);

winapi_api!(
    "kernel32.dll",
    "HeapReAlloc",
    pub unsafe extern "win64" fn heap_realloc(
        _heap: usize,
        _flags: u32,
        memory_pointer: *mut c_void,
        bytes: usize,
    ) -> *mut c_void {
        memory::realloc(memory_pointer as *mut u8, bytes) as *mut c_void
    }
);

winapi_api!(
    "kernel32.dll",
    "HeapFree",
    pub unsafe extern "win64" fn heap_free(
        _heap: usize,
        _flags: u32,
        memory_pointer: *mut c_void,
    ) -> i32 {
        if memory_pointer.is_null() || memory::free(memory_pointer as *mut u8) {
            1
        } else {
            0
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "HeapSize",
    pub unsafe extern "win64" fn heap_size(
        _heap: usize,
        _flags: u32,
        memory_pointer: *const c_void,
    ) -> usize {
        memory::block_size(memory_pointer as *mut u8).unwrap_or(usize::MAX)
    }
);

winapi_api!(
    "kernel32.dll",
    "HeapValidate",
    pub unsafe extern "win64" fn heap_validate(
        _heap: usize,
        _flags: u32,
        _memory: *const c_void,
    ) -> i32 {
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "GlobalAlloc",
    pub unsafe extern "win64" fn global_alloc(flags: u32, bytes: usize) -> usize {
        let fixed = flags & 0x0002 == 0 || flags & GMEM_FIXED != 0;
        let zero = flags & GMEM_ZEROINIT != 0;
        let pointer = memory::alloc(bytes, 16, zero, memory::PAGE_READWRITE);
        if !fixed && !pointer.is_null() {
            0
        } else {
            pointer as usize
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GlobalFree",
    pub unsafe extern "win64" fn global_free(handle: usize) -> usize {
        if handle != 0 && memory::free(handle as *mut u8) {
            0
        } else {
            handle
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "GlobalReAlloc",
    pub unsafe extern "win64" fn global_realloc(handle: usize, bytes: usize, _flags: u32) -> usize {
        memory::realloc(handle as *mut u8, bytes) as usize
    }
);

winapi_api!(
    "kernel32.dll",
    "GlobalSize",
    pub unsafe extern "win64" fn global_size(handle: usize) -> usize {
        memory::block_size(handle as *mut u8).unwrap_or(0)
    }
);

winapi_api!(
    "kernel32.dll",
    "LocalAlloc",
    pub unsafe extern "win64" fn local_alloc(flags: u32, bytes: usize) -> usize {
        let zero = flags & LMEM_ZEROINIT != 0;
        let _fixed = flags & LMEM_FIXED != 0;
        memory::alloc(bytes, 16, zero, memory::PAGE_READWRITE) as usize
    }
);

winapi_api!(
    "kernel32.dll",
    "LocalFree",
    pub unsafe extern "win64" fn local_free(handle: usize) -> usize {
        if handle != 0 && memory::free(handle as *mut u8) {
            0
        } else {
            handle
        }
    }
);

winapi_api!(
    "kernel32.dll",
    "LocalReAlloc",
    pub unsafe extern "win64" fn local_realloc(handle: usize, bytes: usize, _flags: u32) -> usize {
        memory::realloc(handle as *mut u8, bytes) as usize
    }
);

winapi_api!(
    "kernel32.dll",
    "LocalSize",
    pub unsafe extern "win64" fn local_size(handle: usize) -> usize {
        memory::block_size(handle as *mut u8).unwrap_or(0)
    }
);

winapi_api!(
    "kernel32.dll",
    "IsDebuggerPresent",
    pub unsafe extern "win64" fn is_debugger_present() -> i32 {
        0
    }
);

winapi_api!(
    "kernel32.dll",
    "OutputDebugStringA",
    pub unsafe extern "win64" fn output_debug_string_a(text: *const u8) {
        log::debug!("[n2np-debug] {}", utf::read_ansi(text));
    }
);

winapi_api!(
    "kernel32.dll",
    "OutputDebugStringW",
    pub unsafe extern "win64" fn output_debug_string_w(text: *const u16) {
        log::debug!("[n2np-debug] {}", utf::read_wide(text));
    }
);

winapi_api!(
    "kernel32.dll",
    "GetACP",
    pub unsafe extern "win64" fn get_acp() -> u32 {
        65001
    }
);
winapi_api!(
    "kernel32.dll",
    "GetOEMCP",
    pub unsafe extern "win64" fn get_oemcp() -> u32 {
        65001
    }
);
winapi_api!(
    "kernel32.dll",
    "GetConsoleCP",
    pub unsafe extern "win64" fn get_console_cp() -> u32 {
        65001
    }
);
winapi_api!(
    "kernel32.dll",
    "GetConsoleOutputCP",
    pub unsafe extern "win64" fn get_console_output_cp() -> u32 {
        65001
    }
);
winapi_api!(
    "kernel32.dll",
    "SetConsoleCP",
    pub unsafe extern "win64" fn set_console_cp(_code_page: u32) -> i32 {
        1
    }
);
winapi_api!(
    "kernel32.dll",
    "SetConsoleOutputCP",
    pub unsafe extern "win64" fn set_console_output_cp(_code_page: u32) -> i32 {
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "GetConsoleMode",
    pub unsafe extern "win64" fn get_console_mode(_handle: usize, mode: *mut u32) -> i32 {
        if mode.is_null() {
            error::set_last_error(ERROR_INVALID_PARAMETER);
            return 0;
        }
        *mode = 3;
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "SetConsoleMode",
    pub unsafe extern "win64" fn set_console_mode(_handle: usize, _mode: u32) -> i32 {
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "SetConsoleTitleA",
    pub unsafe extern "win64" fn set_console_title_a(_title: *const u8) -> i32 {
        1
    }
);
winapi_api!(
    "kernel32.dll",
    "SetConsoleTitleW",
    pub unsafe extern "win64" fn set_console_title_w(_title: *const u16) -> i32 {
        1
    }
);
winapi_api!(
    "kernel32.dll",
    "AllocConsole",
    pub unsafe extern "win64" fn alloc_console() -> i32 {
        1
    }
);
winapi_api!(
    "kernel32.dll",
    "FreeConsole",
    pub unsafe extern "win64" fn free_console() -> i32 {
        1
    }
);

winapi_api!(
    "kernel32.dll",
    "SetErrorMode",
    pub unsafe extern "win64" fn set_error_mode(_mode: u32) -> u32 {
        0
    }
);

winapi_api!(
    "kernel32.dll",
    "GetSystemInfo",
    pub unsafe extern "win64" fn get_system_info(info: *mut u8) {
        write_system_info(info)
    }
);

winapi_api!(
    "kernel32.dll",
    "GetNativeSystemInfo",
    pub unsafe extern "win64" fn get_native_system_info(info: *mut u8) {
        write_system_info(info)
    }
);

unsafe fn write_system_info(info: *mut u8) {
    if info.is_null() {
        return;
    }
    ptr::write_bytes(info, 0, 48);
    let cpus = std::thread::available_parallelism()
        .map(|value| value.get() as u32)
        .unwrap_or(1)
        .min(64);
    *(info as *mut u16) = 9;
    *(info.add(4) as *mut u32) = 4096;
    *(info.add(8) as *mut usize) = 0x1_0000;
    *(info.add(16) as *mut usize) = 0x0000_7FFF_FFFE_FFFF;
    *(info.add(24) as *mut usize) = ((1u64 << cpus) - 1) as usize;
    *(info.add(32) as *mut u32) = cpus;
    *(info.add(36) as *mut u32) = 8664;
    *(info.add(40) as *mut u32) = 65536;
    *(info.add(44) as *mut u16) = 6;
}

winapi_api!(
    "kernel32.dll",
    "MultiByteToWideChar",
    pub unsafe extern "win64" fn multi_byte_to_wide_char(
        _code_page: u32,
        _flags: u32,
        input: *const u8,
        input_length: i32,
        output: *mut u16,
        output_length: i32,
    ) -> i32 {
        let text = if input_length < 0 {
            utf::read_ansi(input)
        } else {
            utf::read_ansi_slice(input, input_length as usize)
        };
        let units: Vec<u16> = text.encode_utf16().collect();
        let needed = units.len() + if input_length < 0 { 1 } else { 0 };
        if output_length == 0 || output.is_null() {
            return needed as i32;
        }
        if (output_length as usize) < needed {
            error::set_last_error(ERROR_INSUFFICIENT_BUFFER);
            return 0;
        }
        ptr::copy_nonoverlapping(units.as_ptr(), output, units.len());
        if input_length < 0 {
            *output.add(units.len()) = 0;
        }
        needed as i32
    }
);

winapi_api!(
    "kernel32.dll",
    "WideCharToMultiByte",
    pub unsafe extern "win64" fn wide_char_to_multi_byte(
        _code_page: u32,
        _flags: u32,
        input: *const u16,
        input_length: i32,
        output: *mut u8,
        output_length: i32,
        _default_char: *const u8,
        _used_default: *mut i32,
    ) -> i32 {
        let text = if input_length < 0 {
            utf::read_wide(input)
        } else {
            utf::read_wide_slice(input, input_length as usize)
        };
        let bytes = text.as_bytes();
        let needed = bytes.len() + if input_length < 0 { 1 } else { 0 };
        if output_length == 0 || output.is_null() {
            return needed as i32;
        }
        if (output_length as usize) < needed {
            error::set_last_error(ERROR_INSUFFICIENT_BUFFER);
            return 0;
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
        if input_length < 0 {
            *output.add(bytes.len()) = 0;
        }
        needed as i32
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrlenA",
    pub unsafe extern "win64" fn lstrlen_a(text: *const u8) -> i32 {
        utf::read_ansi(text).len() as i32
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrlenW",
    pub unsafe extern "win64" fn lstrlen_w(text: *const u16) -> i32 {
        utf::read_wide(text).encode_utf16().count() as i32
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrcpyA",
    pub unsafe extern "win64" fn lstrcpy_a(destination: *mut u8, source: *const u8) -> *mut u8 {
        let text = utf::read_ansi(source);
        utf::write_ansi(destination, &text, usize::MAX);
        destination
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrcpyW",
    pub unsafe extern "win64" fn lstrcpy_w(destination: *mut u16, source: *const u16) -> *mut u16 {
        let text = utf::read_wide(source);
        utf::write_wide(destination, &text, usize::MAX);
        destination
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrcatA",
    pub unsafe extern "win64" fn lstrcat_a(destination: *mut u8, source: *const u8) -> *mut u8 {
        let mut text = utf::read_ansi(destination);
        text.push_str(&utf::read_ansi(source));
        utf::write_ansi(destination, &text, usize::MAX);
        destination
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrcatW",
    pub unsafe extern "win64" fn lstrcat_w(destination: *mut u16, source: *const u16) -> *mut u16 {
        let mut text = utf::read_wide(destination);
        text.push_str(&utf::read_wide(source));
        utf::write_wide(destination, &text, usize::MAX);
        destination
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrcmpA",
    pub unsafe extern "win64" fn lstrcmp_a(left: *const u8, right: *const u8) -> i32 {
        utf::read_ansi(left).cmp(&utf::read_ansi(right)) as i32
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrcmpW",
    pub unsafe extern "win64" fn lstrcmp_w(left: *const u16, right: *const u16) -> i32 {
        utf::read_wide(left).cmp(&utf::read_wide(right)) as i32
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrcmpiA",
    pub unsafe extern "win64" fn lstrcmpi_a(left: *const u8, right: *const u8) -> i32 {
        utf::read_ansi(left)
            .to_lowercase()
            .cmp(&utf::read_ansi(right).to_lowercase()) as i32
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrcmpiW",
    pub unsafe extern "win64" fn lstrcmpi_w(left: *const u16, right: *const u16) -> i32 {
        utf::read_wide(left)
            .to_lowercase()
            .cmp(&utf::read_wide(right).to_lowercase()) as i32
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrcpynA",
    pub unsafe extern "win64" fn lstrcpyn_a(
        destination: *mut u8,
        source: *const u8,
        count: i32,
    ) -> *mut u8 {
        if count <= 0 || destination.is_null() {
            return destination;
        }
        let text = utf::read_ansi(source);
        utf::write_ansi(destination, &text, count as usize);
        destination
    }
);

winapi_api!(
    "kernel32.dll",
    "lstrcpynW",
    pub unsafe extern "win64" fn lstrcpyn_w(
        destination: *mut u16,
        source: *const u16,
        count: i32,
    ) -> *mut u16 {
        if count <= 0 || destination.is_null() {
            return destination;
        }
        let text = utf::read_wide(source);
        utf::write_wide(destination, &text, count as usize);
        destination
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_section_is_recursive_and_exclusive() {
        let state = Arc::new(CriticalSectionState::new());
        assert_eq!(state.acquire(1), 1);
        assert_eq!(state.acquire(1), 2);
        assert_eq!(state.try_acquire(2), None);
        assert_eq!(state.release(1), Some(1));
        assert_eq!(state.release(1), Some(0));
        assert_eq!(state.try_acquire(2), Some(1));
        assert_eq!(state.release(2), Some(0));
    }

    #[test]
    fn critical_section_wakes_blocked_thread() {
        let state = Arc::new(CriticalSectionState::new());
        assert_eq!(state.acquire(1), 1);
        let waiter = state.clone();
        let join = std::thread::spawn(move || waiter.acquire(2));
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(state.release(1), Some(0));
        assert_eq!(join.join().unwrap(), 1);
        assert_eq!(state.release(2), Some(0));
    }

    #[test]
    fn tls_round_trips_and_validates_indexes() {
        let index = unsafe { tls_alloc() };
        assert!(index < 1088);
        unsafe {
            assert_eq!(tls_set_value(index, 0x1234), 1);
            assert_eq!(tls_get_value(index), 0x1234);
            assert_eq!(tls_free(index), 1);
            assert_eq!(tls_get_value(index), 0);
            assert_eq!(tls_get_value(u32::MAX), 0);
            assert_eq!(tls_set_value(u32::MAX, 1), 0);
        }
    }
}

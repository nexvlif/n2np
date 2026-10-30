use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

pub type Handle = usize;

pub const INVALID_HANDLE_VALUE: Handle = usize::MAX;
pub const STD_INPUT_HANDLE: u32 = 0xFFFF_FFF6;
pub const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;
pub const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    Signaled,
    Timeout,
    Invalid,
}

pub struct ThreadState {
    done: Mutex<bool>,
    code: Mutex<u32>,
    ready: Condvar,
}

impl ThreadState {
    pub fn new() -> Self {
        Self {
            done: Mutex::new(false),
            code: Mutex::new(259),
            ready: Condvar::new(),
        }
    }

    pub fn finish(&self, code: u32) {
        *self.code.lock().unwrap() = code;
        *self.done.lock().unwrap() = true;
        self.ready.notify_all();
    }

    pub fn exit_code(&self) -> u32 {
        *self.code.lock().unwrap()
    }

    pub fn is_done(&self) -> bool {
        *self.done.lock().unwrap()
    }

    pub fn wait(&self, timeout: Option<Duration>) -> bool {
        let mut done = self.done.lock().unwrap();
        if *done {
            return true;
        }
        match timeout {
            None => {
                while !*done {
                    done = self.ready.wait(done).unwrap();
                }
                true
            }
            Some(limit) => {
                let started = std::time::Instant::now();
                loop {
                    if *done {
                        return true;
                    }
                    let elapsed = started.elapsed();
                    if elapsed >= limit {
                        return *done;
                    }
                    let (guard, _) = self.ready.wait_timeout(done, limit - elapsed).unwrap();
                    done = guard;
                }
            }
        }
    }
}

impl Default for ThreadState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct EventState {
    signal: Mutex<bool>,
    manual_reset: bool,
    ready: Condvar,
}

impl EventState {
    pub fn new(manual_reset: bool, initial: bool) -> Self {
        Self {
            signal: Mutex::new(initial),
            manual_reset,
            ready: Condvar::new(),
        }
    }

    pub fn set(&self) {
        *self.signal.lock().unwrap() = true;
        self.ready.notify_all();
    }

    pub fn reset(&self) {
        *self.signal.lock().unwrap() = false;
    }

    pub fn is_signaled(&self) -> bool {
        *self.signal.lock().unwrap()
    }

    fn consume_if_auto(&self) {
        if !self.manual_reset {
            *self.signal.lock().unwrap() = false;
        }
    }

    pub fn wait(&self, timeout: Option<Duration>) -> bool {
        {
            let mut signal = self.signal.lock().unwrap();
            if !*signal {
                match timeout {
                    None => {
                        while !*signal {
                            signal = self.ready.wait(signal).unwrap();
                        }
                    }
                    Some(limit) => {
                        let started = std::time::Instant::now();
                        while !*signal {
                            let elapsed = started.elapsed();
                            if elapsed >= limit {
                                break;
                            }
                            let (guard, _) =
                                self.ready.wait_timeout(signal, limit - elapsed).unwrap();
                            signal = guard;
                        }
                    }
                }
                if !*signal {
                    return false;
                }
            }
            if !self.manual_reset {
                *signal = false;
            }
        }
        true
    }
}

pub struct MutexState {
    owner: Mutex<Option<u32>>,
}

impl MutexState {
    pub fn new() -> Self {
        Self {
            owner: Mutex::new(None),
        }
    }

    pub fn acquire(&self, thread_id: u32) {
        *self.owner.lock().unwrap() = Some(thread_id);
    }

    pub fn release(&self) {
        *self.owner.lock().unwrap() = None;
    }
}

impl Default for MutexState {
    fn default() -> Self {
        Self::new()
    }
}

pub enum HandleKind {
    File { file: File, path: PathBuf },
    Directory { path: PathBuf },
    Thread(Arc<ThreadState>),
    Mutex(Arc<MutexState>),
    Event(Arc<EventState>),
}

impl HandleKind {
    pub fn as_file_mut(&mut self) -> Option<&mut File> {
        match self {
            HandleKind::File { file, .. } => Some(file),
            _ => None,
        }
    }

    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            HandleKind::File { path, .. } | HandleKind::Directory { path } => Some(path),
            _ => None,
        }
    }
}

static TABLE: OnceLock<Mutex<HashMap<Handle, HandleKind>>> = OnceLock::new();
static NEXT_HANDLE: AtomicUsize = AtomicUsize::new(0x1000);
static STD_HANDLES: OnceLock<Mutex<[Handle; 3]>> = OnceLock::new();

fn table() -> &'static Mutex<HashMap<Handle, HandleKind>> {
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn std_handles() -> &'static Mutex<[Handle; 3]> {
    STD_HANDLES.get_or_init(|| {
        Mutex::new([
            STD_INPUT_HANDLE as usize,
            STD_OUTPUT_HANDLE as usize,
            STD_ERROR_HANDLE as usize,
        ])
    })
}

pub fn std_index(handle: Handle) -> Option<usize> {
    let low = handle as u32;
    match low {
        STD_INPUT_HANDLE => Some(0),
        STD_OUTPUT_HANDLE => Some(1),
        STD_ERROR_HANDLE => Some(2),
        _ => None,
    }
}

pub fn std_handle(kind: u32) -> Handle {
    let index = match kind as u32 {
        STD_INPUT_HANDLE => 0,
        STD_OUTPUT_HANDLE => 1,
        STD_ERROR_HANDLE => 2,
        _ => return INVALID_HANDLE_VALUE,
    };
    std_handles().lock().unwrap()[index]
}

pub fn set_std_handle(kind: u32, handle: Handle) -> bool {
    let index = match kind as u32 {
        STD_INPUT_HANDLE => 0,
        STD_OUTPUT_HANDLE => 1,
        STD_ERROR_HANDLE => 2,
        _ => return false,
    };
    std_handles().lock().unwrap()[index] = handle;
    true
}

pub fn insert(kind: HandleKind) -> Handle {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    table().lock().unwrap().insert(handle, kind);
    handle
}

pub fn remove(handle: Handle) -> Option<HandleKind> {
    table().lock().unwrap().remove(&handle)
}

pub fn contains(handle: Handle) -> bool {
    table().lock().unwrap().contains_key(&handle)
}

pub fn with<R>(handle: Handle, f: impl FnOnce(&HandleKind) -> R) -> Option<R> {
    let table = table().lock().unwrap();
    table.get(&handle).map(f)
}

pub fn with_mut<R>(handle: Handle, f: impl FnOnce(&mut HandleKind) -> R) -> Option<R> {
    let mut table = table().lock().unwrap();
    table.get_mut(&handle).map(f)
}

pub fn wait(handle: Handle, timeout: Option<Duration>) -> WaitOutcome {
    if let Some(index) = std_index(handle) {
        let _ = index;
        return WaitOutcome::Invalid;
    }
    match with(handle, |kind| match kind {
        HandleKind::Thread(state) => {
            if state.wait(timeout) {
                WaitOutcome::Signaled
            } else {
                WaitOutcome::Timeout
            }
        }
        HandleKind::Event(state) => {
            if state.wait(timeout) {
                WaitOutcome::Signaled
            } else {
                WaitOutcome::Timeout
            }
        }
        HandleKind::Mutex(_) => WaitOutcome::Signaled,
        _ => WaitOutcome::Invalid,
    }) {
        Some(outcome) => outcome,
        None => WaitOutcome::Invalid,
    }
}

pub fn is_signaled(handle: Handle) -> bool {
    with(handle, |kind| match kind {
        HandleKind::Thread(state) => state.is_done(),
        HandleKind::Event(state) => state.is_signaled(),
        HandleKind::Mutex(_) => true,
        _ => false,
    })
    .unwrap_or(false)
}

pub fn consume_signal(handle: Handle) {
    let _ = with(handle, |kind| {
        if let HandleKind::Event(state) = kind {
            if state.is_signaled() {
                state.consume_if_auto();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_signals_and_times_out() {
        let event = Arc::new(EventState::new(false, false));
        let handle = insert(HandleKind::Event(event.clone()));
        assert_eq!(
            wait(handle, Some(Duration::from_millis(1))),
            WaitOutcome::Timeout
        );
        event.set();
        assert_eq!(
            wait(handle, Some(Duration::from_millis(100))),
            WaitOutcome::Signaled
        );
        assert!(!is_signaled(handle));
        remove(handle);
    }

    #[test]
    fn thread_state_reports_exit_code() {
        let thread = Arc::new(ThreadState::new());
        let handle = insert(HandleKind::Thread(thread.clone()));
        thread.finish(7);
        assert!(is_signaled(handle));
        assert_eq!(thread.exit_code(), 7);
        remove(handle);
    }

    #[test]
    fn standard_handles_round_trip() {
        assert_eq!(std_handle(STD_OUTPUT_HANDLE), STD_OUTPUT_HANDLE as usize);
        assert!(set_std_handle(STD_OUTPUT_HANDLE, 0x1234));
        assert_eq!(std_handle(STD_OUTPUT_HANDLE), 0x1234);
        assert!(set_std_handle(
            STD_OUTPUT_HANDLE,
            STD_OUTPUT_HANDLE as usize
        ));
    }
}

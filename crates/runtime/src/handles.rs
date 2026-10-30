use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

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
    suspended: Mutex<bool>,
    start_ready: Condvar,
}

impl ThreadState {
    pub fn new() -> Self {
        Self::with_suspended(false)
    }

    pub fn with_suspended(suspended: bool) -> Self {
        Self {
            done: Mutex::new(false),
            code: Mutex::new(259),
            ready: Condvar::new(),
            suspended: Mutex::new(suspended),
            start_ready: Condvar::new(),
        }
    }

    /// Block a newly created thread until `ResumeThread` clears the suspended
    /// state. This only affects the initial suspension; arbitrary running PE
    /// code cannot be safely preempted from Rust.
    pub fn wait_until_started(&self) {
        let mut suspended = self.suspended.lock().unwrap();
        while *suspended {
            suspended = self.start_ready.wait(suspended).unwrap();
        }
    }

    pub fn resume(&self) -> u32 {
        let mut suspended = self.suspended.lock().unwrap();
        let previous = u32::from(*suspended);
        *suspended = false;
        self.start_ready.notify_all();
        previous
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

struct MutexInner {
    owner: Option<u32>,
    recursion: u32,
}

/// Win32 mutex with recursive ownership and blocking waits.
pub struct MutexState {
    inner: Mutex<MutexInner>,
    ready: Condvar,
}

impl MutexState {
    pub fn new(initial_owner: Option<u32>) -> Self {
        Self {
            inner: Mutex::new(MutexInner {
                owner: initial_owner,
                recursion: u32::from(initial_owner.is_some()),
            }),
            ready: Condvar::new(),
        }
    }

    pub fn acquire(&self, thread_id: u32, timeout: Option<Duration>) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.owner == Some(thread_id) {
            inner.recursion = inner.recursion.saturating_add(1);
            return true;
        }
        if inner.owner.is_none() {
            inner.owner = Some(thread_id);
            inner.recursion = 1;
            return true;
        }

        let started = Instant::now();
        loop {
            match timeout {
                Some(limit) => {
                    let elapsed = started.elapsed();
                    if elapsed >= limit {
                        return false;
                    }
                    let (guard, result) = self.ready.wait_timeout(inner, limit - elapsed).unwrap();
                    inner = guard;
                    if result.timed_out() && inner.owner.is_some() {
                        return false;
                    }
                }
                None => {
                    inner = self.ready.wait(inner).unwrap();
                }
            }
            if inner.owner.is_none() {
                inner.owner = Some(thread_id);
                inner.recursion = 1;
                return true;
            }
        }
    }

    pub fn release(&self, thread_id: u32) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.owner != Some(thread_id) {
            return false;
        }
        inner.recursion = inner.recursion.saturating_sub(1);
        if inner.recursion == 0 {
            inner.owner = None;
            self.ready.notify_one();
        }
        true
    }

    pub fn is_available(&self) -> bool {
        self.inner.lock().unwrap().owner.is_none()
    }
}

impl Default for MutexState {
    fn default() -> Self {
        Self::new(None)
    }
}

struct SemaphoreInner {
    count: u32,
    maximum: u32,
}

/// Win32 semaphore with a maximum count and blocking waits.
pub struct SemaphoreState {
    inner: Mutex<SemaphoreInner>,
    ready: Condvar,
}

impl SemaphoreState {
    pub fn new(initial: u32, maximum: u32) -> Self {
        let maximum = maximum.max(1);
        Self {
            inner: Mutex::new(SemaphoreInner {
                count: initial.min(maximum),
                maximum,
            }),
            ready: Condvar::new(),
        }
    }

    pub fn acquire(&self, timeout: Option<Duration>) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.count > 0 {
            inner.count -= 1;
            return true;
        }

        let started = Instant::now();
        loop {
            match timeout {
                Some(limit) => {
                    let elapsed = started.elapsed();
                    if elapsed >= limit {
                        return false;
                    }
                    let (guard, result) = self.ready.wait_timeout(inner, limit - elapsed).unwrap();
                    inner = guard;
                    if result.timed_out() && inner.count == 0 {
                        return false;
                    }
                }
                None => {
                    inner = self.ready.wait(inner).unwrap();
                }
            }
            if inner.count > 0 {
                inner.count -= 1;
                return true;
            }
        }
    }

    /// Returns the previous count, or `None` when the maximum would be exceeded.
    pub fn release(&self, count: u32) -> Option<u32> {
        let mut inner = self.inner.lock().unwrap();
        let previous = inner.count;
        let next = previous.checked_add(count)?;
        if next > inner.maximum {
            return None;
        }
        inner.count = next;
        self.ready.notify_all();
        Some(previous)
    }

    pub fn is_signaled(&self) -> bool {
        self.inner.lock().unwrap().count > 0
    }
}

pub enum HandleKind {
    File { file: File, path: PathBuf },
    Directory { path: PathBuf },
    Thread(Arc<ThreadState>),
    Mutex(Arc<MutexState>),
    Event(Arc<EventState>),
    Semaphore(Arc<SemaphoreState>),
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

/// A cloned handle payload that can be waited on without holding the global
/// handle-table lock. Blocking while holding that lock would prevent another
/// thread from releasing the object and deadlock the process.
enum Waitable {
    Thread(Arc<ThreadState>),
    Event(Arc<EventState>),
    Mutex(Arc<MutexState>),
    Semaphore(Arc<SemaphoreState>),
}

fn waitable(handle: Handle) -> Option<Waitable> {
    let table = table().lock().unwrap();
    match table.get(&handle)? {
        HandleKind::Thread(state) => Some(Waitable::Thread(state.clone())),
        HandleKind::Event(state) => Some(Waitable::Event(state.clone())),
        HandleKind::Mutex(state) => Some(Waitable::Mutex(state.clone())),
        HandleKind::Semaphore(state) => Some(Waitable::Semaphore(state.clone())),
        _ => None,
    }
}

pub fn wait(handle: Handle, timeout: Option<Duration>, thread_id: u32) -> WaitOutcome {
    if std_index(handle).is_some() {
        return WaitOutcome::Invalid;
    }
    match waitable(handle) {
        Some(Waitable::Thread(state)) => {
            if state.wait(timeout) {
                WaitOutcome::Signaled
            } else {
                WaitOutcome::Timeout
            }
        }
        Some(Waitable::Event(state)) => {
            if state.wait(timeout) {
                WaitOutcome::Signaled
            } else {
                WaitOutcome::Timeout
            }
        }
        Some(Waitable::Mutex(state)) => {
            if state.acquire(thread_id, timeout) {
                WaitOutcome::Signaled
            } else {
                WaitOutcome::Timeout
            }
        }
        Some(Waitable::Semaphore(state)) => {
            if state.acquire(timeout) {
                WaitOutcome::Signaled
            } else {
                WaitOutcome::Timeout
            }
        }
        None => WaitOutcome::Invalid,
    }
}

pub fn is_signaled(handle: Handle) -> bool {
    match waitable(handle) {
        Some(Waitable::Thread(state)) => state.is_done(),
        Some(Waitable::Event(state)) => state.is_signaled(),
        Some(Waitable::Mutex(state)) => state.is_available(),
        Some(Waitable::Semaphore(state)) => state.is_signaled(),
        None => false,
    }
}

pub fn consume_signal(handle: Handle, thread_id: u32) {
    match waitable(handle) {
        Some(Waitable::Event(state)) => {
            if state.is_signaled() {
                state.consume_if_auto();
            }
        }
        Some(Waitable::Mutex(state)) => {
            state.acquire(thread_id, Some(Duration::ZERO));
        }
        Some(Waitable::Semaphore(state)) => {
            state.acquire(Some(Duration::ZERO));
        }
        Some(Waitable::Thread(_)) | None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_signals_and_times_out() {
        let event = Arc::new(EventState::new(false, false));
        let handle = insert(HandleKind::Event(event.clone()));
        assert_eq!(
            wait(handle, Some(Duration::from_millis(1)), 1),
            WaitOutcome::Timeout
        );
        event.set();
        assert_eq!(
            wait(handle, Some(Duration::from_millis(100)), 1),
            WaitOutcome::Signaled
        );
        assert!(!is_signaled(handle));
        remove(handle);
    }

    #[test]
    fn mutex_blocks_until_released_and_is_recursive() {
        let mutex = Arc::new(MutexState::new(None));
        let handle = insert(HandleKind::Mutex(mutex.clone()));
        assert_eq!(
            wait(handle, Some(Duration::from_millis(10)), 1),
            WaitOutcome::Signaled
        );
        // Recursive acquisition by the same thread succeeds immediately.
        assert_eq!(
            wait(handle, Some(Duration::from_millis(10)), 1),
            WaitOutcome::Signaled
        );
        // A different thread cannot take it.
        assert_eq!(
            wait(handle, Some(Duration::from_millis(5)), 2),
            WaitOutcome::Timeout
        );
        assert!(mutex.release(1));
        assert!(mutex.release(1));
        assert!(!mutex.release(1));
        assert_eq!(
            wait(handle, Some(Duration::from_millis(10)), 2),
            WaitOutcome::Signaled
        );
        assert!(mutex.release(2));
        remove(handle);
    }

    #[test]
    fn mutex_wait_wakes_after_release_from_another_thread() {
        let mutex = Arc::new(MutexState::new(Some(1)));
        let waiter = mutex.clone();
        let handle = std::thread::spawn(move || waiter.acquire(2, Some(Duration::from_secs(2))));
        std::thread::sleep(Duration::from_millis(10));
        assert!(mutex.release(1));
        assert_eq!(handle.join().unwrap(), true);
        assert!(mutex.release(2));
    }

    #[test]
    fn blocking_wait_does_not_hold_the_handle_table_lock() {
        let mutex = Arc::new(MutexState::new(Some(1)));
        let handle = insert(HandleKind::Mutex(mutex.clone()));
        let waiter = std::thread::spawn(move || wait(handle, Some(Duration::from_secs(2)), 2));
        std::thread::sleep(Duration::from_millis(10));
        // Releasing through the table must not block behind the waiter.
        assert!(with(handle, |kind| match kind {
            HandleKind::Mutex(state) => state.release(1),
            _ => false,
        })
        .unwrap());
        assert_eq!(waiter.join().unwrap(), WaitOutcome::Signaled);
        assert!(mutex.release(2));
        remove(handle);
    }

    #[test]
    fn semaphore_counts_and_limits() {
        let semaphore = Arc::new(SemaphoreState::new(1, 2));
        let handle = insert(HandleKind::Semaphore(semaphore.clone()));
        assert_eq!(
            wait(handle, Some(Duration::from_millis(10)), 1),
            WaitOutcome::Signaled
        );
        assert_eq!(
            wait(handle, Some(Duration::from_millis(5)), 1),
            WaitOutcome::Timeout
        );
        assert_eq!(semaphore.release(1), Some(0));
        assert_eq!(semaphore.release(2), None);
        assert_eq!(
            wait(handle, Some(Duration::from_millis(10)), 1),
            WaitOutcome::Signaled
        );
        remove(handle);
    }

    #[test]
    fn suspended_thread_waits_for_resume() {
        let thread = Arc::new(ThreadState::with_suspended(true));
        let state = thread.clone();
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = started.clone();
        let join = std::thread::spawn(move || {
            state.wait_until_started();
            flag.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(20));
        assert!(!started.load(Ordering::SeqCst));
        assert_eq!(thread.resume(), 1);
        join.join().unwrap();
        assert!(started.load(Ordering::SeqCst));
        assert_eq!(thread.resume(), 0);
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

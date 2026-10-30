//! Minimal Win32 window manager backed by Wayland.
//!
//! The backend implements just enough of the Win32 windowing model for small
//! profile-guided applications: window classes, top-level xdg-toplevel windows,
//! a message queue, and basic text/rectangle painting through shared memory.

mod input;
mod message_box;
mod protocol;
mod render;

use std::collections::{HashMap, VecDeque};
use std::os::fd::{AsFd, AsRawFd};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_compositor, wl_keyboard, wl_pointer, wl_seat, wl_shm, wl_surface,
};
use wayland_client::{Connection, EventQueue, QueueHandle};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

use render::Canvas;

pub use message_box::message_box;

pub const WM_QUIT: u32 = 0x0012;

const WS_VISIBLE: u32 = 0x1000_0000;
const CW_USEDEFAULT: i32 = i32::MIN;

const WM_SIZE: u32 = 0x0005;
const WM_SETFOCUS: u32 = 0x0007;
const WM_KILLFOCUS: u32 = 0x0008;
const WM_PAINT: u32 = 0x000F;
const WM_CLOSE: u32 = 0x0010;
const WM_TIMER: u32 = 0x0113;
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;

#[allow(dead_code)]
const COLOR_WINDOW: u32 = 5;
#[allow(dead_code)]
const WHITE: u32 = 0x00FF_FFFF;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Msg {
    pub hwnd: usize,
    pub message: u32,
    pub _pad: u32,
    pub wparam: usize,
    pub lparam: isize,
    pub time: u32,
    pub x: i32,
    pub y: i32,
    pub private: u32,
}

struct Window {
    hwnd: usize,
    class: String,
    proc_: usize,
    title: String,
    style: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    visible: bool,
    user_data: isize,
    surface: wl_surface::WlSurface,
    xdg_surface: xdg_surface::XdgSurface,
    toplevel: xdg_toplevel::XdgToplevel,
    canvas: Option<Canvas>,
    configured: bool,
    pending_size: Option<(i32, i32)>,
}

#[derive(Clone)]
struct ClassInfo {
    name: String,
    atom: u16,
    proc_: usize,
    background: usize,
    cursor: usize,
    icon: usize,
}

#[derive(Clone, Copy)]
struct DeviceContext {
    hwnd: usize,
    text_color: u32,
    background_color: u32,
    background_mode: i32,
}

#[derive(Clone, Copy)]
struct Timer {
    deadline: Instant,
    interval: Duration,
    callback: usize,
}

struct Manager {
    conn: Connection,
    queue: Option<EventQueue<Manager>>,
    qh: QueueHandle<Manager>,
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    classes: HashMap<String, ClassInfo>,
    classes_by_atom: HashMap<u16, String>,
    windows: HashMap<usize, Window>,
    order: Vec<usize>,
    dcs: HashMap<usize, DeviceContext>,
    messages: VecDeque<Msg>,
    timers: HashMap<(usize, usize), Timer>,
    pending_callbacks: Vec<(usize, usize, usize)>,
    next_hwnd: usize,
    next_atom: u16,
    next_dc: usize,
    next_timer_id: usize,
    start: Instant,
    autoclose: Option<Duration>,
    autoclose_fired: bool,
    keyboard_focus: Option<usize>,
    pointer_focus: Option<usize>,
    pointer_pos: (f64, f64),
}

static MANAGER: OnceLock<Mutex<Option<Manager>>> = OnceLock::new();

fn manager_cell() -> &'static Mutex<Option<Manager>> {
    MANAGER.get_or_init(|| Mutex::new(None))
}

fn with_manager<R>(f: impl FnOnce(&mut Manager) -> R) -> Option<R> {
    let mut guard = manager_cell().lock().unwrap();
    if guard.is_none() {
        match Manager::connect() {
            Ok(manager) => *guard = Some(manager),
            Err(err) => {
                log::error!("[n2np-gui] failed to connect to Wayland: {err}");
                return None;
            }
        }
    }
    guard.as_mut().map(f)
}

impl Manager {
    fn connect() -> Result<Self, Box<dyn std::error::Error>> {
        let conn = Connection::connect_to_env()?;
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());

        let autoclose = std::env::var("N2NP_GUI_AUTOCLOSE_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_millis);

        let mut manager = Self {
            conn,
            queue: None,
            qh,
            compositor: None,
            shm: None,
            wm_base: None,
            seat: None,
            keyboard: None,
            pointer: None,
            classes: HashMap::new(),
            classes_by_atom: HashMap::new(),
            windows: HashMap::new(),
            order: Vec::new(),
            dcs: HashMap::new(),
            messages: VecDeque::new(),
            timers: HashMap::new(),
            pending_callbacks: Vec::new(),
            next_hwnd: 0x0001_0000,
            next_atom: 0xC000,
            next_dc: 0x0001_0000,
            next_timer_id: 1,
            start: Instant::now(),
            autoclose,
            autoclose_fired: false,
            keyboard_focus: None,
            pointer_focus: None,
            pointer_pos: (0.0, 0.0),
        };

        queue.roundtrip(&mut manager)?;

        if manager.compositor.is_none() || manager.shm.is_none() || manager.wm_base.is_none() {
            return Err("compositor is missing wl_compositor, wl_shm or xdg_wm_base".into());
        }

        manager.queue = Some(queue);
        Ok(manager)
    }

    fn now_ms(&self) -> u32 {
        self.start.elapsed().as_millis() as u32
    }

    fn queue(&mut self, hwnd: usize, message: u32, wparam: usize, lparam: isize) {
        let time = self.now_ms();
        self.messages.push_back(Msg {
            hwnd,
            message,
            _pad: 0,
            wparam,
            lparam,
            time,
            x: 0,
            y: 0,
            private: 0,
        });
    }

    fn queue_mouse(&mut self, hwnd: usize, message: u32) {
        let x = self.pointer_pos.0 as i32;
        let y = self.pointer_pos.1 as i32;
        let lparam = ((y as isize) << 16) | (x as isize & 0xFFFF);
        self.queue(hwnd, message, 0, lparam);
    }

    fn window_for_surface(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.windows
            .values()
            .find(|window| &window.surface == surface)
            .map(|window| window.hwnd)
    }

    fn pump(&mut self, timeout: Option<Duration>) {
        let Some(mut queue) = self.queue.take() else {
            return;
        };

        let _ = self.conn.flush();

        // Wait for Wayland data without blocking past the next timer deadline.
        let mut readable = true;
        if let Some(timeout) = timeout {
            let mut fd = libc::pollfd {
                fd: self.conn.as_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
            let result = unsafe { libc::poll(&mut fd, 1, millis) };
            readable = result > 0 && (fd.revents & libc::POLLIN) != 0;
        }
        if readable {
            if let Some(guard) = self.conn.prepare_read() {
                if let Err(err) = guard.read() {
                    log::trace!("[n2np-gui] Wayland read: {err}");
                }
            }
        }

        loop {
            match queue.dispatch_pending(self) {
                Ok(0) => break,
                Ok(_) => {}
                Err(err) => {
                    log::warn!("[n2np-gui] Wayland dispatch error: {err}");
                    break;
                }
            }
        }

        self.queue = Some(queue);
        self.check_autoclose();
        self.fire_timers();
    }

    fn set_timer(&mut self, hwnd: usize, id: usize, elapse: u32, callback: usize) -> usize {
        let id = if id == 0 {
            let id = self.next_timer_id;
            self.next_timer_id += 1;
            id
        } else {
            id
        };
        let interval = Duration::from_millis(u64::from(elapse.max(1)));
        self.timers.insert(
            (hwnd, id),
            Timer {
                deadline: Instant::now() + interval,
                interval,
                callback,
            },
        );
        id
    }

    fn kill_timer(&mut self, hwnd: usize, id: usize) -> bool {
        self.timers.remove(&(hwnd, id)).is_some()
    }

    fn fire_timers(&mut self) {
        let now = Instant::now();
        let mut expired = Vec::new();
        for (&(hwnd, id), timer) in self.timers.iter_mut() {
            if now < timer.deadline {
                continue;
            }
            if timer.callback != 0 {
                self.pending_callbacks.push((timer.callback, hwnd, id));
            } else {
                expired.push((hwnd, id));
            }
            while timer.deadline <= now {
                timer.deadline += timer.interval;
            }
        }
        for (hwnd, id) in expired {
            self.queue(hwnd, WM_TIMER, id, 0);
        }
    }

    fn take_callbacks(&mut self) -> Vec<(usize, usize, usize)> {
        std::mem::take(&mut self.pending_callbacks)
    }

    fn next_timeout(&self) -> Duration {
        let now = Instant::now();
        let mut timeout = self
            .timers
            .values()
            .map(|timer| timer.deadline.saturating_duration_since(now))
            .min();
        if let Some(limit) = self.autoclose {
            if !self.autoclose_fired {
                let remaining = limit.saturating_sub(self.start.elapsed());
                timeout = Some(timeout.map_or(remaining, |value| value.min(remaining)));
            }
        }
        // Cross-thread PostMessage calls cannot wake the Wayland fd, so keep a
        // short poll interval even when no timer is pending.
        const MAX_POLL: Duration = Duration::from_millis(16);
        timeout.map_or(MAX_POLL, |value| value.min(MAX_POLL))
    }

    fn check_autoclose(&mut self) {
        if self.autoclose_fired {
            return;
        }
        let Some(limit) = self.autoclose else {
            return;
        };
        if self.start.elapsed() < limit {
            return;
        }
        self.autoclose_fired = true;
        log::info!("[n2np-gui] autoclose after {} ms", limit.as_millis());
        let windows: Vec<usize> = self
            .order
            .iter()
            .copied()
            .filter(|hwnd| self.windows.contains_key(hwnd))
            .collect();
        if windows.is_empty() {
            self.queue(0, WM_QUIT, 0, 0);
        } else {
            for hwnd in windows {
                self.queue(hwnd, WM_CLOSE, 0, 0);
            }
        }
    }

    fn pop_message(&mut self, hwnd: usize, min: u32, max: u32, remove: bool) -> Option<Msg> {
        let position = self.messages.iter().position(|message| {
            if hwnd != 0 && message.hwnd != hwnd && message.message != WM_QUIT {
                return false;
            }
            if message.message == WM_QUIT {
                return true;
            }
            if min != 0 && message.message < min {
                return false;
            }
            if max != 0 && message.message > max {
                return false;
            }
            true
        })?;
        if remove {
            self.messages.remove(position)
        } else {
            self.messages.get(position).cloned()
        }
    }

    fn ensure_canvas(&mut self, hwnd: usize) -> bool {
        let Some((width, height)) = self
            .windows
            .get(&hwnd)
            .map(|window| (window.width.max(1), window.height.max(1)))
        else {
            return false;
        };
        let needs_new = match self
            .windows
            .get(&hwnd)
            .and_then(|window| window.canvas.as_ref())
        {
            Some(canvas) => canvas.width() != width || canvas.height() != height,
            None => true,
        };
        if !needs_new {
            return true;
        }
        let Some(shm) = self.shm.clone() else {
            return false;
        };
        match Canvas::new(width, height, &shm, &self.qh) {
            Ok(canvas) => {
                if let Some(window) = self.windows.get_mut(&hwnd) {
                    window.canvas = Some(canvas);
                }
                true
            }
            Err(err) => {
                log::error!("[n2np-gui] failed to create shm canvas: {err}");
                false
            }
        }
    }

    fn present(&mut self, hwnd: usize) {
        let Some(window) = self.windows.get(&hwnd) else {
            return;
        };
        if !window.visible || !window.configured {
            return;
        }
        let surface = window.surface.clone();
        let conn = self.conn.clone();
        let Some(canvas) = self
            .windows
            .get_mut(&hwnd)
            .and_then(|window| window.canvas.as_mut())
        else {
            return;
        };
        canvas.present(&surface, &conn);
    }

    fn configure_window(&mut self, hwnd: usize, serial: u32) {
        let Some((was_configured, pending)) = self.windows.get_mut(&hwnd).map(|window| {
            window.xdg_surface.ack_configure(serial);
            (window.configured, window.pending_size.take())
        }) else {
            return;
        };

        let mut resized = false;
        if let Some((width, height)) = pending {
            if width > 0 && height > 0 {
                if let Some(window) = self.windows.get_mut(&hwnd) {
                    window.width = width;
                    window.height = height;
                    resized = true;
                }
            }
        }
        if let Some(window) = self.windows.get_mut(&hwnd) {
            window.configured = true;
        }

        self.ensure_canvas(hwnd);
        if resized || !was_configured {
            let (width, height) = self
                .windows
                .get(&hwnd)
                .map(|window| (window.width, window.height))
                .unwrap_or((0, 0));
            let lparam = ((height as isize) << 16) | (width as isize & 0xFFFF);
            self.queue(hwnd, WM_SIZE, 0, lparam);
        }
        self.queue(hwnd, WM_PAINT, 0, 0);
        self.present(hwnd);
    }

    fn remove_window(&mut self, hwnd: usize) -> Option<Window> {
        let window = self.windows.remove(&hwnd)?;
        self.order.retain(|value| *value != hwnd);
        self.dcs.retain(|_, dc| dc.hwnd != hwnd);
        if self.keyboard_focus == Some(hwnd) {
            self.keyboard_focus = None;
        }
        if self.pointer_focus == Some(hwnd) {
            self.pointer_focus = None;
        }
        window.toplevel.destroy();
        window.xdg_surface.destroy();
        window.surface.destroy();
        let _ = self.conn.flush();
        Some(window)
    }

    fn allocate_dc(&mut self, hwnd: usize) -> usize {
        let hdc = self.next_dc;
        self.next_dc += 0x10;
        self.dcs.insert(
            hdc,
            DeviceContext {
                hwnd,
                text_color: 0x0000_0000,
                background_color: 0x00FF_FFFF,
                background_mode: 2,
            },
        );
        hdc
    }

    fn create_window_internal(
        &mut self,
        class_name: &str,
        title: &str,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        user_data: isize,
    ) -> Option<(usize, usize)> {
        let Some(compositor) = self.compositor.clone() else {
            return None;
        };
        let Some(wm_base) = self.wm_base.clone() else {
            return None;
        };
        let class = self.classes.get(class_name).cloned().unwrap_or(ClassInfo {
            name: class_name.to_string(),
            atom: 0,
            proc_: 0,
            background: 0,
            cursor: 0,
            icon: 0,
        });

        let width = if width <= 0 || width == CW_USEDEFAULT {
            640
        } else {
            width
        };
        let height = if height <= 0 || height == CW_USEDEFAULT {
            480
        } else {
            height
        };
        let hwnd = self.next_hwnd;
        self.next_hwnd += 0x10;

        let surface = compositor.create_surface(&self.qh, ());
        let xdg_surface = wm_base.get_xdg_surface(&surface, &self.qh, hwnd);
        let toplevel = xdg_surface.get_toplevel(&self.qh, hwnd);
        toplevel.set_title(title.to_string());
        toplevel.set_app_id("n2np".to_string());
        toplevel.set_min_size(1, 1);
        surface.commit();

        let visible = style & WS_VISIBLE != 0;
        self.windows.insert(
            hwnd,
            Window {
                hwnd,
                class: class.name,
                proc_: class.proc_,
                title: title.to_string(),
                style,
                x,
                y,
                width,
                height,
                visible,
                user_data,
                surface,
                xdg_surface,
                toplevel,
                canvas: None,
                configured: false,
                pending_size: None,
            },
        );
        self.order.push(hwnd);
        let _ = self.conn.flush();
        log::info!(
            "[n2np-gui] created Wayland window hwnd=0x{hwnd:x} class={} title={:?} {}x{}",
            class_name,
            title,
            width,
            height
        );
        Some((hwnd, class.proc_))
    }

    fn show_window_internal(&mut self, hwnd: usize, command: i32) -> Option<bool> {
        let previous = self.windows.get(&hwnd)?.visible;
        if command == 0 {
            if let Some(window) = self.windows.get_mut(&hwnd) {
                window.visible = false;
                window.surface.attach(None, 0, 0);
                window.surface.commit();
            }
            let _ = self.conn.flush();
        } else {
            if let Some(window) = self.windows.get_mut(&hwnd) {
                window.visible = true;
            }
            self.ensure_canvas(hwnd);
            self.present(hwnd);
        }
        Some(previous)
    }

    fn resize_window(&mut self, hwnd: usize, width: i32, height: i32) -> bool {
        let Some((old_width, old_height)) = self
            .windows
            .get(&hwnd)
            .map(|window| (window.width, window.height))
        else {
            return false;
        };
        if let Some(window) = self.windows.get_mut(&hwnd) {
            window.width = width.max(1);
            window.height = height.max(1);
        }
        if width.max(1) != old_width || height.max(1) != old_height {
            self.ensure_canvas(hwnd);
            let (width, height) = self
                .windows
                .get(&hwnd)
                .map(|window| (window.width, window.height))
                .unwrap_or((0, 0));
            let lparam = ((height as isize) << 16) | (width as isize & 0xFFFF);
            self.queue(hwnd, WM_SIZE, 0, lparam);
            self.present(hwnd);
        }
        true
    }

    fn text_out_internal(&mut self, hdc: usize, x: i32, y: i32, text: &str) -> bool {
        let Some(dc) = self.dcs.get(&hdc).copied() else {
            return false;
        };
        if !self.ensure_canvas(dc.hwnd) {
            return false;
        }
        let foreground = render::colorref_to_argb(dc.text_color);
        let background = render::colorref_to_argb(dc.background_color);
        let opaque = dc.background_mode == 2;
        {
            let Some(canvas) = self
                .windows
                .get_mut(&dc.hwnd)
                .and_then(|window| window.canvas.as_mut())
            else {
                return false;
            };
            canvas.text(x, y, text, foreground, background, opaque);
        }
        self.present(dc.hwnd);
        true
    }

    fn fill_rect_internal(
        &mut self,
        hdc: usize,
        color: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> bool {
        let Some(dc) = self.dcs.get(&hdc).copied() else {
            return false;
        };
        if !self.ensure_canvas(dc.hwnd) {
            return false;
        }
        {
            let Some(canvas) = self
                .windows
                .get_mut(&dc.hwnd)
                .and_then(|window| window.canvas.as_mut())
            else {
                return false;
            };
            canvas.fill_rect(render::colorref_to_argb(color), x, y, width, height);
        }
        self.present(dc.hwnd);
        true
    }
}

pub fn register_class(
    name: &str,
    proc_: usize,
    background: usize,
    cursor: usize,
    icon: usize,
) -> Option<u16> {
    with_manager(|manager| {
        if let Some(class) = manager.classes.get(name) {
            return class.atom;
        }
        let atom = manager.next_atom;
        manager.next_atom = manager.next_atom.wrapping_add(1);
        manager.classes.insert(
            name.to_string(),
            ClassInfo {
                name: name.to_string(),
                atom,
                proc_,
                background,
                cursor,
                icon,
            },
        );
        manager.classes_by_atom.insert(atom, name.to_string());
        atom
    })
}

pub fn unregister_class(name: &str) -> bool {
    with_manager(|manager| {
        let Some(class) = manager.classes.remove(name) else {
            return false;
        };
        manager.classes_by_atom.remove(&class.atom);
        true
    })
    .unwrap_or(false)
}

pub fn class_name_for_atom(atom: u16) -> Option<String> {
    with_manager(|manager| manager.classes_by_atom.get(&atom).cloned()).flatten()
}

#[allow(dead_code)]
pub fn class_proc(name: &str) -> Option<usize> {
    with_manager(|manager| manager.classes.get(name).map(|class| class.proc_)).flatten()
}

pub fn create_window(
    class_name: &str,
    title: &str,
    style: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    user_data: isize,
) -> Option<(usize, usize)> {
    with_manager(|manager| {
        manager.create_window_internal(class_name, title, style, x, y, width, height, user_data)
    })
    .flatten()
}

pub fn destroy_window(hwnd: usize) -> bool {
    with_manager(|manager| manager.remove_window(hwnd).is_some()).unwrap_or(false)
}

pub fn window_proc(hwnd: usize) -> Option<usize> {
    with_manager(|manager| manager.windows.get(&hwnd).map(|window| window.proc_)).flatten()
}

#[allow(dead_code)]
pub fn set_window_proc(hwnd: usize, proc_: usize) -> Option<usize> {
    with_manager(|manager| {
        manager
            .windows
            .get_mut(&hwnd)
            .map(|window| std::mem::replace(&mut window.proc_, proc_))
    })
    .flatten()
}

pub fn show_window(hwnd: usize, command: i32) -> Option<bool> {
    with_manager(|manager| manager.show_window_internal(hwnd, command)).flatten()
}

pub fn is_window(hwnd: usize) -> bool {
    with_manager(|manager| manager.windows.contains_key(&hwnd)).unwrap_or(false)
}

pub fn is_window_visible(hwnd: usize) -> bool {
    with_manager(|manager| {
        manager
            .windows
            .get(&hwnd)
            .map(|window| window.visible)
            .unwrap_or(false)
    })
    .unwrap_or(false)
}

pub fn invalidate(hwnd: usize) -> bool {
    with_manager(|manager| {
        if !manager.windows.contains_key(&hwnd) {
            return false;
        }
        manager.queue(hwnd, WM_PAINT, 0, 0);
        true
    })
    .unwrap_or(false)
}

pub fn post_message(hwnd: usize, message: u32, wparam: usize, lparam: isize) -> bool {
    with_manager(|manager| {
        if hwnd != 0 && !manager.windows.contains_key(&hwnd) {
            return false;
        }
        manager.queue(hwnd, message, wparam, lparam);
        true
    })
    .unwrap_or(false)
}

pub fn post_quit(code: i32) {
    with_manager(|manager| manager.queue(0, WM_QUIT, code as usize, 0));
}

pub fn set_timer(hwnd: usize, id: usize, elapse: u32, callback: usize) -> usize {
    with_manager(|manager| manager.set_timer(hwnd, id, elapse, callback)).unwrap_or(0)
}

pub fn kill_timer(hwnd: usize, id: usize) -> bool {
    with_manager(|manager| manager.kill_timer(hwnd, id)).unwrap_or(false)
}

fn flush_timer_callbacks() {
    loop {
        let Some(callbacks) = with_manager(|manager| manager.take_callbacks()) else {
            return;
        };
        if callbacks.is_empty() {
            return;
        }
        for (callback, hwnd, id) in callbacks {
            call_window_proc(callback, hwnd, WM_TIMER, id, 0);
        }
    }
}

pub fn wait_message(hwnd: usize, min: u32, max: u32) -> Option<Msg> {
    loop {
        let timeout = with_manager(|manager| manager.next_timeout())?;
        let message = with_manager(|manager| {
            manager.pump(Some(timeout));
            manager.pop_message(hwnd, min, max, true)
        })?;
        flush_timer_callbacks();
        if let Some(message) = message {
            return Some(message);
        }
    }
}

pub fn peek_message(hwnd: usize, min: u32, max: u32, remove: bool) -> Option<Msg> {
    let message = with_manager(|manager| {
        manager.pump(Some(Duration::ZERO));
        manager.pop_message(hwnd, min, max, remove)
    })
    .flatten();
    flush_timer_callbacks();
    message
}

pub fn begin_paint(hwnd: usize) -> Option<usize> {
    with_manager(|manager| {
        if !manager.windows.contains_key(&hwnd) {
            return None;
        }
        manager.ensure_canvas(hwnd);
        Some(manager.allocate_dc(hwnd))
    })
    .flatten()
}

pub fn end_paint(hdc: usize) {
    with_manager(|manager| {
        if let Some(dc) = manager.dcs.remove(&hdc) {
            manager.present(dc.hwnd);
        }
    });
}

pub fn get_dc(hwnd: usize) -> Option<usize> {
    with_manager(|manager| {
        if !manager.windows.contains_key(&hwnd) {
            return None;
        }
        manager.ensure_canvas(hwnd);
        Some(manager.allocate_dc(hwnd))
    })
    .flatten()
}

pub fn release_dc(hdc: usize) -> bool {
    with_manager(|manager| {
        let Some(dc) = manager.dcs.remove(&hdc) else {
            return false;
        };
        manager.present(dc.hwnd);
        true
    })
    .unwrap_or(false)
}

pub fn client_size(hwnd: usize) -> Option<(i32, i32)> {
    with_manager(|manager| {
        manager
            .windows
            .get(&hwnd)
            .map(|window| (window.width, window.height))
    })
    .flatten()
}

pub fn text_out(hdc: usize, x: i32, y: i32, text: &str) -> bool {
    with_manager(|manager| manager.text_out_internal(hdc, x, y, text)).unwrap_or(false)
}

pub fn fill_rect(hdc: usize, color: u32, x: i32, y: i32, width: i32, height: i32) -> bool {
    with_manager(|manager| manager.fill_rect_internal(hdc, color, x, y, width, height))
        .unwrap_or(false)
}

pub fn set_text_color(hdc: usize, color: u32) -> Option<u32> {
    with_manager(|manager| {
        manager
            .dcs
            .get_mut(&hdc)
            .map(|dc| std::mem::replace(&mut dc.text_color, color))
    })
    .flatten()
}

pub fn get_text_color(hdc: usize) -> Option<u32> {
    with_manager(|manager| manager.dcs.get(&hdc).map(|dc| dc.text_color)).flatten()
}

pub fn set_background_color(hdc: usize, color: u32) -> Option<u32> {
    with_manager(|manager| {
        manager
            .dcs
            .get_mut(&hdc)
            .map(|dc| std::mem::replace(&mut dc.background_color, color))
    })
    .flatten()
}

pub fn get_background_color(hdc: usize) -> Option<u32> {
    with_manager(|manager| manager.dcs.get(&hdc).map(|dc| dc.background_color)).flatten()
}

pub fn set_background_mode(hdc: usize, mode: i32) -> Option<i32> {
    with_manager(|manager| {
        manager
            .dcs
            .get_mut(&hdc)
            .map(|dc| std::mem::replace(&mut dc.background_mode, mode))
    })
    .flatten()
}

pub fn get_background_mode(hdc: usize) -> Option<i32> {
    with_manager(|manager| manager.dcs.get(&hdc).map(|dc| dc.background_mode)).flatten()
}

pub fn window_text(hwnd: usize) -> Option<String> {
    with_manager(|manager| {
        manager
            .windows
            .get(&hwnd)
            .map(|window| window.title.clone())
    })
    .flatten()
}

pub fn set_window_text(hwnd: usize, text: &str) -> bool {
    with_manager(|manager| {
        let Some(window) = manager.windows.get_mut(&hwnd) else {
            return false;
        };
        window.title = text.to_string();
        window.toplevel.set_title(text.to_string());
        let _ = manager.conn.flush();
        true
    })
    .unwrap_or(false)
}

pub fn get_window_long(hwnd: usize, index: i32) -> isize {
    with_manager(|manager| {
        let Some(window) = manager.windows.get(&hwnd) else {
            return 0;
        };
        match index {
            -4 => window.proc_ as isize,
            -21 => window.user_data,
            -16 => window.style as isize,
            _ => 0,
        }
    })
    .unwrap_or(0)
}

pub fn set_window_long(hwnd: usize, index: i32, value: isize) -> isize {
    with_manager(|manager| {
        let Some(window) = manager.windows.get_mut(&hwnd) else {
            return 0;
        };
        match index {
            -4 => std::mem::replace(&mut window.proc_, value as usize) as isize,
            -21 => std::mem::replace(&mut window.user_data, value),
            -16 => std::mem::replace(&mut window.style, value as u32) as isize,
            _ => 0,
        }
    })
    .unwrap_or(0)
}

pub fn move_window(hwnd: usize, x: i32, y: i32, width: i32, height: i32) -> bool {
    with_manager(|manager| {
        {
            let Some(window) = manager.windows.get_mut(&hwnd) else {
                return false;
            };
            // Wayland compositors own window placement, so x/y are best effort.
            window.x = x;
            window.y = y;
        }
        manager.resize_window(hwnd, width, height)
    })
    .unwrap_or(false)
}

pub fn set_window_pos(hwnd: usize, x: i32, y: i32, width: i32, height: i32, flags: u32) -> bool {
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    with_manager(|manager| {
        let Some(window) = manager.windows.get(&hwnd) else {
            return false;
        };
        let new_x = if flags & SWP_NOMOVE != 0 { window.x } else { x };
        let new_y = if flags & SWP_NOMOVE != 0 { window.y } else { y };
        let new_width = if flags & SWP_NOSIZE != 0 {
            window.width
        } else {
            width
        };
        let new_height = if flags & SWP_NOSIZE != 0 {
            window.height
        } else {
            height
        };
        if let Some(window) = manager.windows.get_mut(&hwnd) {
            window.x = new_x;
            window.y = new_y;
        }
        manager.resize_window(hwnd, new_width, new_height)
    })
    .unwrap_or(false)
}

#[allow(dead_code)]
pub fn set_title_utf8(hwnd: usize, text: &str) -> bool {
    set_window_text(hwnd, text)
}

#[allow(dead_code)]
pub fn colorref_to_pixel(color: u32) -> u32 {
    render::colorref_to_argb(color)
}

#[allow(dead_code)]
pub fn system_color(index: u32) -> u32 {
    match index {
        COLOR_WINDOW => WHITE,
        _ => WHITE,
    }
}

#[allow(dead_code)]
pub fn default_background() -> u32 {
    WHITE
}

pub fn call_window_proc(
    proc_: usize,
    hwnd: usize,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if proc_ == 0 {
        return 0;
    }
    let function: unsafe extern "win64" fn(usize, u32, usize, isize) -> isize =
        unsafe { std::mem::transmute(proc_) };
    unsafe { function(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_colorref_to_argb_pixel() {
        assert_eq!(colorref_to_pixel(0x0000_00FF), 0xFFFF_0000);
        assert_eq!(colorref_to_pixel(0x00FF_0000), 0xFF00_00FF);
        assert_eq!(colorref_to_pixel(0x0012_3456), 0xFF56_3412);
    }

    #[test]
    fn message_layout_matches_win64() {
        assert_eq!(std::mem::size_of::<Msg>(), 48);
        assert_eq!(std::mem::offset_of!(Msg, message), 8);
        assert_eq!(std::mem::offset_of!(Msg, wparam), 16);
        assert_eq!(std::mem::offset_of!(Msg, lparam), 24);
    }
}

//! Wayland protocol dispatch implementations for the GUI manager.

use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

use super::input::keycode_to_virtual_key;
use super::{
    Manager, WM_CLOSE, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_SETFOCUS,
};

const BTN_LEFT: u32 = 0x110;

impl Dispatch<wl_registry::WlRegistry, ()> for Manager {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "wl_compositor" if state.compositor.is_none() => {
                let compositor = registry.bind(name, version.min(4), qh, ());
                state.compositor = Some(compositor);
            }
            "wl_shm" if state.shm.is_none() => {
                let shm = registry.bind(name, version.min(1), qh, ());
                state.shm = Some(shm);
            }
            "wl_seat" if state.seat.is_none() => {
                let seat = registry.bind(name, version.min(5), qh, ());
                state.seat = Some(seat);
            }
            "xdg_wm_base" if state.wm_base.is_none() => {
                let wm_base = registry.bind(name, version.min(1), qh, ());
                state.wm_base = Some(wm_base);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for Manager {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_seat::Event::Capabilities {
                capabilities: WEnum::Value(capabilities),
            } => {
                if capabilities.contains(wl_seat::Capability::Keyboard) && state.keyboard.is_none()
                {
                    state.keyboard = Some(seat.get_keyboard(qh, ()));
                }
                if capabilities.contains(wl_seat::Capability::Pointer) && state.pointer.is_none() {
                    state.pointer = Some(seat.get_pointer(qh, ()));
                }
            }
            wl_seat::Event::Name { name } => {
                log::debug!("[n2np-gui] seat name: {name}");
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Manager {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                let Some(hwnd) = state.keyboard_focus else {
                    return;
                };
                let virtual_key = keycode_to_virtual_key(key);
                if virtual_key == 0 {
                    return;
                }
                match key_state {
                    WEnum::Value(wl_keyboard::KeyState::Pressed) => {
                        state.queue(hwnd, WM_KEYDOWN, virtual_key, 0);
                    }
                    WEnum::Value(wl_keyboard::KeyState::Released) => {
                        state.queue(hwnd, WM_KEYUP, virtual_key, 0);
                    }
                    _ => {}
                }
            }
            wl_keyboard::Event::Enter { surface, .. } => {
                let hwnd = state.window_for_surface(&surface);
                state.keyboard_focus = hwnd;
                if let Some(hwnd) = hwnd {
                    state.queue(hwnd, WM_SETFOCUS, 0, 0);
                }
            }
            wl_keyboard::Event::Leave { .. } => {
                if let Some(hwnd) = state.keyboard_focus.take() {
                    state.queue(hwnd, WM_KILLFOCUS, 0, 0);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for Manager {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } => {
                let hwnd = state.window_for_surface(&surface);
                state.pointer_focus = hwnd;
                state.pointer_pos = (surface_x, surface_y);
                if let Some(hwnd) = hwnd {
                    state.queue_mouse(hwnd, WM_MOUSEMOVE);
                }
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                state.pointer_pos = (surface_x, surface_y);
                if let Some(hwnd) = state.pointer_focus {
                    state.queue_mouse(hwnd, WM_MOUSEMOVE);
                }
            }
            wl_pointer::Event::Button {
                button,
                state: button_state,
                ..
            } if button == BTN_LEFT => {
                let Some(hwnd) = state.pointer_focus else {
                    return;
                };
                match button_state {
                    WEnum::Value(wl_pointer::ButtonState::Pressed) => {
                        state.queue_mouse(hwnd, WM_LBUTTONDOWN);
                    }
                    WEnum::Value(wl_pointer::ButtonState::Released) => {
                        state.queue_mouse(hwnd, WM_LBUTTONUP);
                    }
                    _ => {}
                }
            }
            wl_pointer::Event::Leave { .. } => {
                state.pointer_focus = None;
            }
            _ => {}
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Manager {
    fn event(
        _: &mut Self,
        wm_base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, usize> for Manager {
    fn event(
        state: &mut Self,
        _: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        hwnd: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            state.configure_window(*hwnd, serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, usize> for Manager {
    fn event(
        state: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        hwnd: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } => {
                if let Some(window) = state.windows.get_mut(hwnd) {
                    if width > 0 && height > 0 {
                        window.pending_size = Some((width, height));
                    }
                }
            }
            xdg_toplevel::Event::Close => {
                state.queue(*hwnd, WM_CLOSE, 0, 0);
            }
            _ => {}
        }
    }
}

delegate_noop!(Manager: ignore wl_compositor::WlCompositor);
delegate_noop!(Manager: ignore wl_surface::WlSurface);
delegate_noop!(Manager: ignore wl_shm::WlShm);
delegate_noop!(Manager: ignore wl_shm_pool::WlShmPool);
delegate_noop!(Manager: ignore wl_buffer::WlBuffer);

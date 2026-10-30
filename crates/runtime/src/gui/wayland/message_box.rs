//! Modal MessageBox rendering on top of the Wayland canvas backend.

use std::time::{Duration, Instant};

use super::render::{argb, truncate_to_width, wrap_text, FONT_HEIGHT, FONT_WIDTH};
use super::{
    with_manager, Manager, CW_USEDEFAULT, WM_CLOSE, WM_KEYDOWN, WM_LBUTTONDOWN, WS_VISIBLE,
};

const IDOK: i32 = 1;
const IDCANCEL: i32 = 2;

const WIDTH: i32 = 640;
const HEIGHT: i32 = 320;
const HEADER_HEIGHT: i32 = 52;
const BUTTON_WIDTH: i32 = 104;
const BUTTON_HEIGHT: i32 = 34;

pub fn message_box(text: &str, caption: &str, utype: u32) -> i32 {
    with_manager(|manager| manager.run_message_box(text, caption, utype)).unwrap_or(1)
}

impl Manager {
    fn run_message_box(&mut self, text: &str, caption: &str, _utype: u32) -> i32 {
        let title = if caption.trim().is_empty() {
            "n2np"
        } else {
            caption
        };
        log::info!(
            "[n2np-gui] opening MessageBox over Wayland WAYLAND_DISPLAY={:?}",
            std::env::var("WAYLAND_DISPLAY").ok()
        );

        let Some((hwnd, _)) = self.create_window_internal(
            "__n2np_message_box",
            title,
            WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WIDTH,
            HEIGHT,
            0,
        ) else {
            return 1;
        };

        self.ensure_canvas(hwnd);
        self.draw_message_box(hwnd, text, title);
        self.present(hwnd);

        // The message box owns its own autoclose timer; suspend the global one
        // so `pump` does not queue an extra WM_CLOSE for this window.
        let autoclose = self.autoclose.take();
        let started = Instant::now();
        let result = loop {
            if let Some(limit) = autoclose {
                if started.elapsed() >= limit {
                    log::info!(
                        "[n2np-gui] MessageBox autoclose after {} ms",
                        limit.as_millis()
                    );
                    break IDOK;
                }
            }

            self.pump();

            let mut outcome = None;
            while let Some(message) = self.pop_message(hwnd, 0, 0, true) {
                match message.message {
                    super::WM_PAINT | super::WM_SIZE => {
                        self.draw_message_box(hwnd, text, title);
                        self.present(hwnd);
                    }
                    WM_LBUTTONDOWN | WM_KEYDOWN => {
                        outcome = Some(IDOK);
                        break;
                    }
                    WM_CLOSE => {
                        outcome = Some(IDCANCEL);
                        break;
                    }
                    _ => {}
                }
            }
            if let Some(value) = outcome {
                break value;
            }
            std::thread::sleep(Duration::from_millis(10));
        };

        self.remove_window(hwnd);
        self.autoclose = autoclose;
        result
    }

    fn draw_message_box(&mut self, hwnd: usize, text: &str, caption: &str) {
        let Some((width, height)) = self
            .windows
            .get(&hwnd)
            .map(|window| (window.width.max(1), window.height.max(1)))
        else {
            return;
        };
        let Some(canvas) = self
            .windows
            .get_mut(&hwnd)
            .and_then(|window| window.canvas.as_mut())
        else {
            return;
        };

        canvas.clear(argb(0xF3, 0xF3, 0xF3));
        canvas.fill_rect(argb(0x2F, 0x6F, 0xED), 0, 0, width, HEADER_HEIGHT);

        let text_width = (width - 48).max(FONT_WIDTH);
        let title_chars = (text_width / FONT_WIDTH) as usize;
        canvas.text(
            24,
            18,
            &truncate_to_width(caption, title_chars),
            argb(0xFF, 0xFF, 0xFF),
            0,
            false,
        );

        let available_lines = ((height - HEADER_HEIGHT - 96) / (FONT_HEIGHT + 4)).max(1) as usize;
        let lines = wrap_text(text, title_chars, available_lines);
        let mut line_y = HEADER_HEIGHT + 24;
        for line in &lines {
            canvas.text(24, line_y, line, argb(0x1A, 0x1A, 0x1A), 0, false);
            line_y += FONT_HEIGHT + 4;
        }

        let button_x = (width - BUTTON_WIDTH) / 2;
        let button_y = height - 52;
        canvas.fill_rect(
            argb(0x2F, 0x6F, 0xED),
            button_x,
            button_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
        );
        let label = "OK";
        let label_x = button_x + (BUTTON_WIDTH - 2 * FONT_WIDTH) / 2;
        let label_y = button_y + (BUTTON_HEIGHT - FONT_HEIGHT) / 2;
        canvas.text(label_x, label_y, label, argb(0xFF, 0xFF, 0xFF), 0, false);
    }
}

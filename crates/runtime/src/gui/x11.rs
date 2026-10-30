use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeGCAux, ConnectionExt as XprotoConnectionExt, CreateGCAux, CreateWindowAux,
    EventMask, PropMode, Rectangle, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as WrapperConnectionExt;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
    }
}

const WINDOW_WIDTH: u16 = 560;
const WINDOW_HEIGHT: u16 = 260;
const FONT_LINE_HEIGHT: i16 = 16;
const TEXT_MARGIN: i16 = 24;
const HEADER_HEIGHT: i16 = 44;
const BUTTON_WIDTH: u16 = 96;
const BUTTON_HEIGHT: u16 = 30;
const IDOK: i32 = 1;
const IDCANCEL: i32 = 2;

pub fn message_box(
    text: &str,
    caption: &str,
    _utype: u32,
) -> Result<i32, Box<dyn std::error::Error>> {
    log::info!(
        "[n2np-gui] opening MessageBox over X11 DISPLAY={:?} WAYLAND_DISPLAY={:?}",
        std::env::var("DISPLAY").ok(),
        std::env::var("WAYLAND_DISPLAY").ok()
    );

    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let atoms = Atoms::new(&conn)?.reply()?;

    let x = ((screen.width_in_pixels as i32 - WINDOW_WIDTH as i32) / 2) as i16;
    let y = ((screen.height_in_pixels as i32 - WINDOW_HEIGHT as i32) / 2) as i16;

    let win = conn.generate_id()?;
    let gc = conn.generate_id()?;
    let font = conn.generate_id()?;
    let title = if caption.trim().is_empty() {
        "n2np"
    } else {
        caption
    };

    let aux = CreateWindowAux::new()
        .background_pixel(0x00f3_f3f3)
        .event_mask(
            EventMask::EXPOSURE
                | EventMask::KEY_PRESS
                | EventMask::BUTTON_PRESS
                | EventMask::STRUCTURE_NOTIFY,
        );
    conn.create_window(
        screen.root_depth,
        win,
        screen.root,
        x,
        y,
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &aux,
    )?;

    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        title.as_bytes(),
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        b"n2np\0n2np\0",
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atoms.WM_PROTOCOLS,
        AtomEnum::ATOM,
        &[atoms.WM_DELETE_WINDOW],
    )?;

    conn.create_gc(gc, win, &CreateGCAux::new())?;
    conn.open_font(font, b"fixed")?.check()?;
    conn.change_gc(
        gc,
        &ChangeGCAux::new()
            .font(font)
            .foreground(0x001a_1a1a)
            .background(0x00f3_f3f3),
    )?;

    conn.map_window(win)?;
    conn.flush()?;

    log::info!(
        "[n2np-gui] window mapped id=0x{:x} at {}x{} screen={}x{}",
        win,
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        screen.width_in_pixels,
        screen.height_in_pixels
    );

    let autoclose = std::env::var("N2NP_GUI_AUTOCLOSE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    let started = Instant::now();

    loop {
        if let Some(ms) = autoclose {
            if started.elapsed() >= Duration::from_millis(ms) {
                log::info!("[n2np-gui] autoclose after {} ms", ms);
                return Ok(IDOK);
            }
        }

        let event = match autoclose {
            Some(_) => match conn.poll_for_event()? {
                Some(event) => event,
                None => {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
            },
            None => conn.wait_for_event()?,
        };

        match event {
            Event::Expose(event) if event.window == win && event.count == 0 => {
                draw(&conn, win, gc, text, caption)?;
            }
            Event::ButtonPress(event) if event.event == win => {
                return Ok(IDOK);
            }
            Event::KeyPress(event) if event.event == win => {
                return Ok(IDOK);
            }
            Event::ClientMessage(event) if event.window == win => {
                let data = event.data.as_data32();
                if event.format == 32 && data[0] == atoms.WM_DELETE_WINDOW {
                    return Ok(IDCANCEL);
                }
            }
            Event::DestroyNotify(event) if event.window == win => {
                return Ok(IDCANCEL);
            }
            _ => {}
        }
    }
}

fn draw<C: Connection>(
    conn: &C,
    win: x11rb::protocol::xproto::Window,
    gc: x11rb::protocol::xproto::Gcontext,
    text: &str,
    caption: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.change_gc(gc, &ChangeGCAux::new().foreground(0x00f3_f3f3))?;
    conn.poly_fill_rectangle(
        win,
        gc,
        &[Rectangle {
            x: 0,
            y: 0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }],
    )?;

    conn.change_gc(gc, &ChangeGCAux::new().foreground(0x002f_6f_ed))?;
    conn.poly_fill_rectangle(
        win,
        gc,
        &[Rectangle {
            x: 0,
            y: 0,
            width: WINDOW_WIDTH,
            height: HEADER_HEIGHT as u16,
        }],
    )?;
    conn.change_gc(gc, &ChangeGCAux::new().foreground(0x00ff_ff_ff))?;
    conn.image_text8(
        win,
        gc,
        TEXT_MARGIN,
        27,
        truncate_to_width(
            if caption.trim().is_empty() {
                "n2np"
            } else {
                caption
            },
            80,
        )
        .as_bytes(),
    )?;

    let lines = wrap_text(text, 82, 10);
    conn.change_gc(gc, &ChangeGCAux::new().foreground(0x001a_1a1a))?;
    let mut line_y = HEADER_HEIGHT + 26;
    for line in &lines {
        conn.image_text8(win, gc, TEXT_MARGIN, line_y, line.as_bytes())?;
        line_y += FONT_LINE_HEIGHT;
    }

    let button_x = ((WINDOW_WIDTH as i16 - BUTTON_WIDTH as i16) / 2) as i16;
    let button_y = WINDOW_HEIGHT as i16 - 48;
    conn.change_gc(gc, &ChangeGCAux::new().foreground(0x002f_6f_ed))?;
    conn.poly_fill_rectangle(
        win,
        gc,
        &[Rectangle {
            x: button_x,
            y: button_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
        }],
    )?;
    conn.change_gc(gc, &ChangeGCAux::new().foreground(0x00ff_ff_ff))?;
    conn.image_text8(win, gc, button_x + 40, button_y + 19, b"OK")?;

    conn.flush()?;
    Ok(())
}

fn truncate_to_width(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

fn wrap_text(text: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    let max_chars = max_chars.max(8);
    let mut lines = Vec::new();
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    for raw_line in normalized.split('\n') {
        if raw_line.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in raw_line.split_whitespace() {
            let word_len = word.chars().count();
            let line_len = line.chars().count();
            if line.is_empty() {
                if word_len > max_chars {
                    let mut current = String::new();
                    for ch in word.chars() {
                        if current.chars().count() >= max_chars {
                            lines.push(std::mem::take(&mut current));
                        }
                        current.push(ch);
                    }
                    line = current;
                } else {
                    line.push_str(word);
                }
            } else if line_len + 1 + word_len <= max_chars {
                line.push(' ');
                line.push_str(word);
            } else {
                lines.push(std::mem::take(&mut line));
                line.push_str(word);
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines.saturating_sub(1));
        lines.push("...".to_string());
    }
    lines
}

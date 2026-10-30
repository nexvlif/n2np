//! CPU-side drawing for the Wayland backend.
//!
//! Wayland has no server-side text or rectangle drawing, so every window owns a
//! small ARGB8888 canvas. Painting calls mutate the canvas and `present` copies
//! it into a shared-memory buffer before committing the surface.

use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::os::unix::io::AsFd;

use font8x8::{UnicodeFonts, BASIC_FONTS};
use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{Connection, QueueHandle};

use super::Manager;

/// Bitmap font scale. `font8x8` glyphs are 8x8 pixels; two gives readable
/// 16x16 cells for normal windows.
pub const FONT_SCALE: i32 = 2;
pub const FONT_WIDTH: i32 = 8 * FONT_SCALE;
pub const FONT_HEIGHT: i32 = 8 * FONT_SCALE;

/// Convert a Win32 `COLORREF` (`0x00BBGGRR`) into Wayland ARGB8888.
pub fn colorref_to_argb(color: u32) -> u32 {
    0xFF00_0000
        | ((color & 0x0000_00FF) << 16)
        | (color & 0x0000_FF00)
        | ((color & 0x00FF_0000) >> 16)
}

pub fn argb(red: u8, green: u8, blue: u8) -> u32 {
    0xFF00_0000 | (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
}

/// A double-buffered shared-memory surface canvas.
pub struct Canvas {
    width: i32,
    height: i32,
    pixels: Vec<u32>,
    file: File,
    #[allow(dead_code)]
    pool: wl_shm_pool::WlShmPool,
    buffers: [wl_buffer::WlBuffer; 2],
    index: usize,
}

impl Canvas {
    pub fn new(
        width: i32,
        height: i32,
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<Manager>,
    ) -> io::Result<Self> {
        let width = width.max(1);
        let height = height.max(1);
        let stride = width * 4;
        let size = stride * height;
        let file = tempfile::tempfile()?;
        file.set_len(size as u64 * 2)?;
        let pool = shm.create_pool(file.as_fd(), size * 2, qh, ());
        let first = pool.create_buffer(0, width, height, stride, wl_shm::Format::Argb8888, qh, ());
        let second = pool.create_buffer(
            size,
            width,
            height,
            stride,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );
        Ok(Self {
            width,
            height,
            pixels: vec![0xFFFF_FFFF; (width * height) as usize],
            file,
            pool,
            buffers: [first, second],
            index: 0,
        })
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    pub fn fill_rect(&mut self, color: u32, x: i32, y: i32, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        let left = x.max(0);
        let top = y.max(0);
        let right = x.saturating_add(width).min(self.width);
        let bottom = y.saturating_add(height).min(self.height);
        if left >= right || top >= bottom {
            return;
        }
        for row in top..bottom {
            let start = (row * self.width + left) as usize;
            let end = (row * self.width + right) as usize;
            self.pixels[start..end].fill(color);
        }
    }

    /// Draw text with the built-in 8x8 font. `x`/`y` are the top-left corner of
    /// the first glyph cell, matching GDI's `TextOut` origin.
    pub fn text(
        &mut self,
        x: i32,
        y: i32,
        text: &str,
        foreground: u32,
        background: u32,
        opaque: bool,
    ) {
        let mut cursor = x;
        for character in text.chars() {
            if cursor >= self.width {
                break;
            }
            let glyph = BASIC_FONTS
                .get(character)
                .or_else(|| BASIC_FONTS.get('?'))
                .unwrap_or([0; 8]);
            for (row, bits) in glyph.iter().enumerate() {
                for column in 0..8 {
                    let on = bits & (1 << column) != 0;
                    if !on && !opaque {
                        continue;
                    }
                    let pixel_x = cursor + column * FONT_SCALE;
                    let pixel_y = y + row as i32 * FONT_SCALE;
                    for dy in 0..FONT_SCALE {
                        for dx in 0..FONT_SCALE {
                            let px = pixel_x + dx;
                            let py = pixel_y + dy;
                            if px < 0 || py < 0 || px >= self.width || py >= self.height {
                                continue;
                            }
                            let color = if on { foreground } else { background };
                            self.pixels[(py * self.width + px) as usize] = color;
                        }
                    }
                }
            }
            cursor += FONT_WIDTH;
        }
    }

    /// Copy the canvas into the next shm buffer and commit the surface.
    pub fn present(&mut self, surface: &wl_surface::WlSurface, conn: &Connection) -> bool {
        let stride = self.width * 4;
        let size = stride * self.height;
        let offset = size as u64 * self.index as u64;
        let mut bytes = Vec::with_capacity(self.pixels.len() * 4);
        for pixel in &self.pixels {
            bytes.extend_from_slice(&pixel.to_le_bytes());
        }
        if self.file.write_all_at(&bytes, offset).is_err() {
            return false;
        }
        let buffer = &self.buffers[self.index];
        surface.attach(Some(buffer), 0, 0);
        surface.damage(0, 0, self.width, self.height);
        surface.commit();
        self.index ^= 1;
        let _ = conn.flush();
        true
    }
}

pub fn truncate_to_width(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

pub fn wrap_text(text: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_colorref_to_argb() {
        assert_eq!(colorref_to_argb(0x0000_00FF), 0xFFFF_0000);
        assert_eq!(colorref_to_argb(0x00FF_0000), 0xFF00_00FF);
        assert_eq!(colorref_to_argb(0x0012_3456), 0xFF56_3412);
    }

    #[test]
    fn wraps_long_text() {
        let lines = wrap_text("the quick brown fox jumps", 10, 5);
        assert!(lines.iter().all(|line| line.chars().count() <= 10));
        assert!(lines.len() > 1);
    }

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(truncate_to_width("abcdef", 4), "a...");
    }
}

use crate::gui;
use crate::utf;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const WHITE_BRUSH: usize = 0;
const LTGRAY_BRUSH: usize = 1;
const GRAY_BRUSH: usize = 2;
const DKGRAY_BRUSH: usize = 3;
const BLACK_BRUSH: usize = 4;
const NULL_BRUSH: usize = 5;
const HOLLOW_BRUSH: usize = 5;
const DC_BRUSH: usize = 18;

const BRUSH_BASE: usize = 0x0000_0001_0000_0000;

static BRUSHES: OnceLock<Mutex<HashMap<usize, u32>>> = OnceLock::new();
static SELECTED_BRUSHES: OnceLock<Mutex<HashMap<usize, usize>>> = OnceLock::new();

fn brushes() -> &'static Mutex<HashMap<usize, u32>> {
    BRUSHES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn selected_brushes() -> &'static Mutex<HashMap<usize, usize>> {
    SELECTED_BRUSHES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn stock_brush_color(brush: usize) -> Option<u32> {
    match brush {
        WHITE_BRUSH | DC_BRUSH => Some(0x00ff_ffff),
        LTGRAY_BRUSH => Some(0x00c0_c0c0),
        GRAY_BRUSH => Some(0x0080_8080),
        DKGRAY_BRUSH => Some(0x0040_4040),
        BLACK_BRUSH => Some(0x0000_0000),
        NULL_BRUSH | HOLLOW_BRUSH => None,
        _ => None,
    }
}

fn brush_color(brush: usize) -> Option<u32> {
    if let Some(color) = brushes().lock().unwrap().get(&brush).copied() {
        return Some(color);
    }
    stock_brush_color(brush)
}

winapi_api!(
    "gdi32.dll",
    "TextOutA",
    pub unsafe extern "win64" fn text_out_a(
        hdc: usize,
        x: i32,
        y: i32,
        text: *const u8,
        count: i32,
    ) -> i32 {
        let text = if count < 0 {
            utf::read_ansi(text)
        } else {
            utf::read_ansi_slice(text, count as usize)
        };
        i32::from(gui::text_out(hdc, x, y, &text))
    }
);

winapi_api!(
    "gdi32.dll",
    "TextOutW",
    pub unsafe extern "win64" fn text_out_w(
        hdc: usize,
        x: i32,
        y: i32,
        text: *const u16,
        count: i32,
    ) -> i32 {
        let text = if count < 0 {
            utf::read_wide(text)
        } else {
            utf::read_wide_slice(text, count as usize)
        };
        i32::from(gui::text_out(hdc, x, y, &text))
    }
);

winapi_api!(
    "gdi32.dll",
    "SetTextColor",
    pub unsafe extern "win64" fn set_text_color(hdc: usize, color: u32) -> u32 {
        gui::set_text_color(hdc, color).unwrap_or(0)
    }
);

winapi_api!(
    "gdi32.dll",
    "GetTextColor",
    pub unsafe extern "win64" fn get_text_color(hdc: usize) -> u32 {
        gui::get_text_color(hdc).unwrap_or(0)
    }
);

winapi_api!(
    "gdi32.dll",
    "SetBkColor",
    pub unsafe extern "win64" fn set_bk_color(hdc: usize, color: u32) -> u32 {
        gui::set_background_color(hdc, color).unwrap_or(0)
    }
);

winapi_api!(
    "gdi32.dll",
    "GetBkColor",
    pub unsafe extern "win64" fn get_bk_color(hdc: usize) -> u32 {
        gui::get_background_color(hdc).unwrap_or(0x00ff_ffff)
    }
);

winapi_api!(
    "gdi32.dll",
    "SetBkMode",
    pub unsafe extern "win64" fn set_bk_mode(hdc: usize, mode: i32) -> i32 {
        gui::set_background_mode(hdc, mode).unwrap_or(0)
    }
);

winapi_api!(
    "gdi32.dll",
    "GetBkMode",
    pub unsafe extern "win64" fn get_bk_mode(hdc: usize) -> i32 {
        gui::get_background_mode(hdc).unwrap_or(2)
    }
);

winapi_api!(
    "gdi32.dll",
    "CreateSolidBrush",
    pub unsafe extern "win64" fn create_solid_brush(color: u32) -> usize {
        let mut brushes = brushes().lock().unwrap();
        let handle = BRUSH_BASE + brushes.len() + 1;
        brushes.insert(handle, color);
        handle
    }
);

winapi_api!(
    "gdi32.dll",
    "DeleteObject",
    pub unsafe extern "win64" fn delete_object(object: usize) -> i32 {
        if brushes().lock().unwrap().remove(&object).is_some() {
            return 1;
        }
        i32::from(stock_brush_color(object).is_some() || object == NULL_BRUSH)
    }
);

winapi_api!(
    "gdi32.dll",
    "SelectObject",
    pub unsafe extern "win64" fn select_object(hdc: usize, object: usize) -> usize {
        let mut selected = selected_brushes().lock().unwrap();
        selected.insert(hdc, object).unwrap_or(0)
    }
);

winapi_api!(
    "gdi32.dll",
    "GetStockObject",
    pub unsafe extern "win64" fn get_stock_object(index: i32) -> usize {
        match index {
            0 => WHITE_BRUSH,
            1 => LTGRAY_BRUSH,
            2 => GRAY_BRUSH,
            3 => DKGRAY_BRUSH,
            4 => BLACK_BRUSH,
            5 => NULL_BRUSH,
            18 => DC_BRUSH,
            _ => 0,
        }
    }
);

winapi_api!(
    "gdi32.dll",
    "FillRect",
    pub unsafe extern "win64" fn fill_rect(hdc: usize, rect: *const i32, brush: usize) -> i32 {
        if rect.is_null() {
            return 0;
        }
        let left = *rect;
        let top = *rect.add(1);
        let right = *rect.add(2);
        let bottom = *rect.add(3);
        let Some(color) = brush_color(brush) else {
            return 1;
        };
        i32::from(gui::fill_rect(
            hdc,
            color,
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        ))
    }
);

winapi_api!(
    "gdi32.dll",
    "Rectangle",
    pub unsafe extern "win64" fn rectangle(
        hdc: usize,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    ) -> i32 {
        let brush = *selected_brushes()
            .lock()
            .unwrap()
            .get(&hdc)
            .unwrap_or(&WHITE_BRUSH);
        let Some(color) = brush_color(brush) else {
            return 1;
        };
        i32::from(gui::fill_rect(
            hdc,
            color,
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        ))
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_brushes_map_to_colors() {
        assert_eq!(stock_brush_color(WHITE_BRUSH), Some(0x00ff_ffff));
        assert_eq!(stock_brush_color(BLACK_BRUSH), Some(0x0000_0000));
        assert_eq!(stock_brush_color(NULL_BRUSH), None);
    }
}

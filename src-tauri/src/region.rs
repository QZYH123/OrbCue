//! Circular ball region. CreateEllipticRgn uses these bounds.

use tauri::{AppHandle, Manager, WebviewWindow};
#[cfg(not(windows))]
use tauri::{PhysicalPosition, Position};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircleRegion {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub fn physical_circle_region(physical_width: i32, physical_height: i32) -> Option<CircleRegion> {
    if physical_width <= 0 || physical_height <= 0 {
        return None;
    }
    let side = physical_width.min(physical_height);
    Some(CircleRegion {
        left: 0,
        top: 0,
        right: side,
        bottom: side,
    })
}

/// tao keeps these on undecorated windows so DWM can hide the title bar.
const WS_CAPTION: u32 = 0x00C0_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const WS_MINIMIZEBOX: u32 = 0x0002_0000;
const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
const CAPTION_CHROME_STYLE: u32 = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;

pub fn style_without_caption_chrome(style: u32) -> u32 {
    style & !CAPTION_CHROME_STYLE
}

const WM_NCPAINT: u32 = 0x0085;
const WM_NCACTIVATE: u32 = 0x0086;
const WM_NCUAHDRAWCAPTION: u32 = 0x00AE;
const WM_NCUAHDRAWFRAME: u32 = 0x00AF;

pub fn suppresses_non_client_paint(msg: u32) -> bool {
    matches!(msg, WM_NCPAINT | WM_NCUAHDRAWCAPTION | WM_NCUAHDRAWFRAME)
}

pub fn rewrites_ncactivate(msg: u32) -> bool {
    msg == WM_NCACTIVATE
}

/// `WM_NCACTIVATE` with this lParam skips the default caption redraw.
pub fn ncactivate_skip_redraw_lparam() -> isize {
    -1
}

pub fn apply_ball_region_for(app: &AppHandle) {
    if let Some(ball) = app.get_webview_window("ball") {
        apply_ball_region(&ball);
    }
}

pub fn apply_ball_region(window: &WebviewWindow) {
    #[cfg(windows)]
    win32::apply(window);
    #[cfg(not(windows))]
    {
        let _ = window;
    }
}

pub fn cursor_over_window(window: &WebviewWindow, pad: i32) -> bool {
    #[cfg(windows)]
    {
        win32::cursor_over_window(window, pad)
    }
    #[cfg(not(windows))]
    {
        let _ = (window, pad);
        false
    }
}

pub fn set_physical_position(window: &WebviewWindow, x: i32, y: i32) -> Result<(), String> {
    #[cfg(windows)]
    {
        win32::set_pos(window, x, y)
    }
    #[cfg(not(windows))]
    {
        window
            .set_position(Position::Physical(PhysicalPosition::new(x, y)))
            .map_err(|error| error.to_string())
    }
}

#[cfg(windows)]
mod win32 {
    use super::{physical_circle_region, CircleRegion};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use tauri::WebviewWindow;

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateEllipticRgn(x1: i32, y1: i32, x2: i32, y2: i32) -> isize;
        fn DeleteObject(object: isize) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn SetWindowRgn(hwnd: isize, hrgn: isize, redraw: i32) -> i32;
        fn SetWindowPos(
            hwnd: isize,
            insert_after: isize,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
        fn GetCursorPos(point: *mut Point) -> i32;
        fn GetWindowLongW(hwnd: isize, index: i32) -> i32;
        fn SetWindowLongW(hwnd: isize, index: i32, value: i32) -> i32;
    }

    #[link(name = "comctl32")]
    extern "system" {
        fn SetWindowSubclass(
            hwnd: isize,
            pfn: Option<unsafe extern "system" fn(isize, u32, usize, isize, usize, usize) -> isize>,
            id: usize,
            data: usize,
        ) -> i32;
        fn DefSubclassProc(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    }

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: isize,
            attribute: u32,
            value: *const core::ffi::c_void,
            size: u32,
        ) -> i32;
        fn DwmExtendFrameIntoClientArea(hwnd: isize, margins: *const Margins) -> i32;
    }

    #[repr(C)]
    struct Margins {
        cx_left_width: i32,
        cx_right_width: i32,
        cy_top_height: i32,
        cy_bottom_height: i32,
    }

    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_DONOTROUND: u32 = 1;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;

    unsafe fn set_dwm_u32(hwnd: isize, attribute: u32, value: u32) {
        let _ = DwmSetWindowAttribute(hwnd, attribute, (&value as *const u32).cast(), 4);
    }

    unsafe fn suppress_dwm_frame(hwnd: isize) {
        set_dwm_u32(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND);
        set_dwm_u32(hwnd, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE);
    }

    unsafe fn extend_frame_into_client(hwnd: isize) {
        let margins = Margins {
            cx_left_width: -1,
            cx_right_width: -1,
            cy_top_height: -1,
            cy_bottom_height: -1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
    }

    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOREDRAW: u32 = 0x0008;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;
    const SWP_NOSENDCHANGING: u32 = 0x0400;
    const GWL_STYLE: i32 = -16;

    pub fn cursor_over_window(window: &WebviewWindow, pad: i32) -> bool {
        let Ok(pos) = window.outer_position() else {
            return false;
        };
        let Ok(size) = window.outer_size() else {
            return false;
        };
        let mut point = Point { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut point) } == 0 {
            return false;
        }
        point.x >= pos.x - pad
            && point.x < pos.x + size.width as i32 + pad
            && point.y >= pos.y - pad
            && point.y < pos.y + size.height as i32 + pad
    }

    pub fn set_pos(window: &WebviewWindow, x: i32, y: i32) -> Result<(), String> {
        let hwnd = hwnd_of(window).ok_or_else(|| "no hwnd".to_owned())?;
        let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING;
        let ok = unsafe { SetWindowPos(hwnd, 0, x, y, 0, 0, flags) };
        if ok == 0 {
            return Err("SetWindowPos failed".to_owned());
        }
        Ok(())
    }

    pub fn apply(window: &WebviewWindow) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        let Ok(size) = window.outer_size() else {
            return;
        };
        let Some(region) = physical_circle_region(size.width as i32, size.height as i32) else {
            return;
        };
        unsafe {
            strip_caption_chrome(hwnd);
            install_nc_guard(hwnd);
            extend_frame_into_client(hwnd);
            suppress_dwm_frame(hwnd);
            set_elliptic_region(hwnd, region);
        }
    }

    const BALL_SUBCLASS_ID: usize = 0x4F52_4243;

    unsafe fn install_nc_guard(hwnd: isize) {
        let _ = SetWindowSubclass(hwnd, Some(ball_nc_guard), BALL_SUBCLASS_ID, 0);
    }

    unsafe extern "system" fn ball_nc_guard(
        hwnd: isize,
        msg: u32,
        wparam: usize,
        lparam: isize,
        _id: usize,
        _data: usize,
    ) -> isize {
        if super::suppresses_non_client_paint(msg) {
            return 0;
        }
        if super::rewrites_ncactivate(msg) {
            return DefSubclassProc(hwnd, msg, wparam, super::ncactivate_skip_redraw_lparam());
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    unsafe fn strip_caption_chrome(hwnd: isize) {
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        if style == 0 {
            return;
        }
        let next = super::style_without_caption_chrome(style);
        if next == style {
            return;
        }
        SetWindowLongW(hwnd, GWL_STYLE, next as i32);
        // Recalc NC metrics without painting: FRAMECHANGED alone flashes a white caption bar.
        let flags = SWP_NOMOVE
            | SWP_NOSIZE
            | SWP_NOZORDER
            | SWP_NOACTIVATE
            | SWP_FRAMECHANGED
            | SWP_NOREDRAW;
        let _ = SetWindowPos(hwnd, 0, 0, 0, 0, 0, flags);
    }

    fn hwnd_of(window: &WebviewWindow) -> Option<isize> {
        let handle = window.window_handle().ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(win) => Some(win.hwnd.get()),
            _ => None,
        }
    }

    unsafe fn set_elliptic_region(hwnd: isize, region: CircleRegion) {
        let hrgn =
            unsafe { CreateEllipticRgn(region.left, region.top, region.right, region.bottom) };
        if hrgn == 0 {
            return;
        }
        // SetWindowRgn takes ownership of hrgn. Do not hide/show the window.
        if unsafe { SetWindowRgn(hwnd, hrgn, 1) } == 0 {
            unsafe {
                DeleteObject(hrgn);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_without_caption_chrome_strips_tao_undecorated_defaults() {
        const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
        const WS_CLIPCHILDREN: u32 = 0x0200_0000;
        const WS_VISIBLE: u32 = 0x1000_0000;
        let tao_undecorated = WS_CAPTION
            | WS_SYSMENU
            | WS_MINIMIZEBOX
            | WS_CLIPSIBLINGS
            | WS_CLIPCHILDREN
            | WS_VISIBLE;
        let stripped = style_without_caption_chrome(tao_undecorated);
        assert_eq!(stripped & CAPTION_CHROME_STYLE, 0);
        assert_eq!(stripped, WS_CLIPSIBLINGS | WS_CLIPCHILDREN | WS_VISIBLE);
    }

    #[test]
    fn style_without_caption_chrome_is_idempotent() {
        const WS_VISIBLE: u32 = 0x1000_0000;
        let stripped = style_without_caption_chrome(WS_VISIBLE | CAPTION_CHROME_STYLE);
        assert_eq!(stripped, WS_VISIBLE);
        assert_eq!(style_without_caption_chrome(stripped), stripped);
    }

    #[test]
    fn left_click_nc_paint_messages_are_suppressed() {
        assert!(suppresses_non_client_paint(WM_NCPAINT));
        assert!(suppresses_non_client_paint(WM_NCUAHDRAWCAPTION));
        assert!(suppresses_non_client_paint(WM_NCUAHDRAWFRAME));
        assert!(!suppresses_non_client_paint(WM_NCACTIVATE));
        assert!(rewrites_ncactivate(WM_NCACTIVATE));
        assert!(!rewrites_ncactivate(WM_NCPAINT));
        assert!(!suppresses_non_client_paint(0x000F));
    }

    #[test]
    fn ncactivate_skips_default_caption_redraw() {
        assert_eq!(ncactivate_skip_redraw_lparam(), -1);
    }
}

//! Circular ball region. Bounds match `frontend/src/circleRegion.ts`.

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
    }

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: isize,
            attribute: u32,
            value: *const core::ffi::c_void,
            size: u32,
        ) -> i32;
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

    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_NOSENDCHANGING: u32 = 0x0400;

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
            suppress_dwm_frame(hwnd);
            set_elliptic_region(hwnd, region);
        }
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

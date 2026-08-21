//! Circular ball region. Bounds match `frontend/src/circleRegion.ts`.

use tauri::{AppHandle, Manager, WebviewWindow};

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

#[cfg(windows)]
mod win32 {
    use super::{physical_circle_region, CircleRegion};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use tauri::WebviewWindow;

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateEllipticRgn(x1: i32, y1: i32, x2: i32, y2: i32) -> isize;
        fn DeleteObject(object: isize) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn SetWindowRgn(hwnd: isize, hrgn: isize, redraw: i32) -> i32;
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
        let hrgn = unsafe { CreateEllipticRgn(region.left, region.top, region.right, region.bottom) };
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

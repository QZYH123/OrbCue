//! Presenter-side jump-back execution. Window matching never scans processes.

#[cfg(windows)]
use agent_activity_dock_core::select_unique_window_title;
use agent_activity_dock_core::{focus_decision, FocusDecision, FocusRequest, SessionSnapshot};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusResult {
    pub focused: bool,
    pub reason: Option<String>,
}

pub fn focus_session(
    sessions: &[SessionSnapshot],
    source: &str,
    session_id: &str,
    open_deep_link: impl FnOnce(&str) -> Result<(), String>,
) -> FocusResult {
    let Some(session) = sessions
        .iter()
        .find(|session| session.source == source && session.session_id == session_id)
    else {
        return FocusResult {
            focused: false,
            reason: Some("会话已不存在".to_owned()),
        };
    };
    match focus_decision(&FocusRequest {
        deep_link: session.deep_link.clone(),
        window_title: session.window_title.clone(),
        project_path: session.project_path.clone(),
    }) {
        FocusDecision::OpenDeepLink(url) => match open_deep_link(&url) {
            Ok(()) => FocusResult {
                focused: true,
                reason: None,
            },
            Err(reason) => FocusResult {
                focused: false,
                reason: Some(reason),
            },
        },
        FocusDecision::MatchWindow { hint } => focus_window_by_hint(&hint),
        FocusDecision::Unavailable { reason } => FocusResult {
            focused: false,
            reason: Some(reason),
        },
    }
}

fn focus_window_by_hint(hint: &str) -> FocusResult {
    #[cfg(not(windows))]
    {
        let _ = hint;
        FocusResult {
            focused: false,
            reason: Some("当前平台不能聚焦源终端".to_owned()),
        }
    }
    #[cfg(windows)]
    {
        let windows = win32::visible_top_level_windows();
        let titles: Vec<String> = windows.iter().map(|window| window.title.clone()).collect();
        match select_unique_window_title(&titles, hint) {
            Ok(title) => match windows.iter().find(|window| window.title == title) {
                Some(window) if win32::bring_to_foreground(window.hwnd) => FocusResult {
                    focused: true,
                    reason: None,
                },
                Some(_) => FocusResult {
                    focused: false,
                    reason: Some("无法把窗口提到前台".to_owned()),
                },
                None => FocusResult {
                    focused: false,
                    reason: Some("没有找到匹配的窗口".to_owned()),
                },
            },
            Err(reason) => FocusResult {
                focused: false,
                reason: Some(reason),
            },
        }
    }
}

#[cfg(windows)]
mod win32 {
    pub struct VisibleWindow {
        pub hwnd: isize,
        pub title: String,
    }

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(cb: unsafe extern "system" fn(isize, isize) -> i32, lparam: isize) -> i32;
        fn IsWindowVisible(hwnd: isize) -> i32;
        fn GetWindowTextLengthW(hwnd: isize) -> i32;
        fn GetWindowTextW(hwnd: isize, lp: *mut u16, n: i32) -> i32;
        fn SetForegroundWindow(hwnd: isize) -> i32;
        fn IsIconic(hwnd: isize) -> i32;
        fn ShowWindow(hwnd: isize, ncmd: i32) -> i32;
    }

    const SW_RESTORE: i32 = 9;

    unsafe extern "system" fn enum_proc(hwnd: isize, lparam: isize) -> i32 {
        let windows = unsafe { &mut *(lparam as *mut Vec<VisibleWindow>) };
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len <= 0 {
            return 1;
        }
        let mut buf = vec![0u16; (len as usize) + 1];
        let written = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        if written <= 0 {
            return 1;
        }
        let title = String::from_utf16_lossy(&buf[..written as usize]);
        if !title.is_empty() {
            windows.push(VisibleWindow { hwnd, title });
        }
        1
    }

    pub fn visible_top_level_windows() -> Vec<VisibleWindow> {
        let mut windows = Vec::new();
        unsafe {
            EnumWindows(enum_proc, &mut windows as *mut Vec<VisibleWindow> as isize);
        }
        windows
    }

    pub fn bring_to_foreground(hwnd: isize) -> bool {
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd) != 0
        }
    }
}

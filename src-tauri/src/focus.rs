//! Presenter-side jump-back execution.
//!
//! Primary path: remember the foreground terminal HWND when a snapshot says
//! to capture (new session or transition into working). Used only for the
//! user's explicit「回去」click — never to infer Dock session state.
//! Title matching among terminal windows is the fallback.

#[cfg(windows)]
use agent_activity_dock_core::select_window_by_hints;
use agent_activity_dock_core::{
    captured_keys_to_drop, focus_decision, sessions_to_capture, CaptureSession, FocusDecision,
    FocusRequest, SessionKey, SessionSnapshot,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusResult {
    pub focused: bool,
    pub reason: Option<String>,
}

static CAPTURED_HWNDS: Mutex<Option<HashMap<(String, String), isize>>> = Mutex::new(None);

fn snapshot_views(sessions: &[SessionSnapshot]) -> Vec<CaptureSession> {
    sessions
        .iter()
        .map(|session| CaptureSession {
            source: session.source.clone(),
            session_id: session.session_id.clone(),
            state: session.state,
            parent_session_id: None,
        })
        .collect()
}

fn log_capture(line: &str) {
    eprintln!("{line}");
    let path = std::env::temp_dir().join("agent-activity-dock-jump-capture.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{line}");
    }
}

pub fn apply_snapshot_captures(previous: &[SessionSnapshot], current: &[SessionSnapshot]) {
    let previous_views = snapshot_views(previous);
    let current_views = snapshot_views(current);
    let wanted = sessions_to_capture(&previous_views, &current_views);
    #[cfg(windows)]
    {
        if let Some(hwnd) = win32::foreground_terminal_hwnd() {
            if let Ok(mut guard) = CAPTURED_HWNDS.lock() {
                let table = guard.get_or_insert_with(HashMap::new);
                for key in &wanted {
                    table.insert((key.source.clone(), key.session_id.clone()), hwnd);
                    log_capture(&format!(
                        "jump-capture source={} session={} hwnd={}",
                        key.source, key.session_id, hwnd
                    ));
                }
            }
        } else {
            for key in &wanted {
                log_capture(&format!(
                    "jump-capture source={} session={} skipped",
                    key.source, key.session_id
                ));
            }
        }
    }
    #[cfg(not(windows))]
    {
        for key in &wanted {
            log_capture(&format!(
                "jump-capture source={} session={} skipped",
                key.source, key.session_id
            ));
        }
    }

    let live: Vec<SessionKey> = current_views.iter().map(CaptureSession::key).collect();
    if let Ok(mut guard) = CAPTURED_HWNDS.lock() {
        let Some(table) = guard.as_mut() else {
            return;
        };
        let stored: Vec<SessionKey> = table
            .keys()
            .map(|(source, session_id)| SessionKey::new(source.clone(), session_id.clone()))
            .collect();
        for key in captured_keys_to_drop(&stored, &live) {
            table.remove(&(key.source.clone(), key.session_id.clone()));
        }
    }
}

fn take_stale_or_hwnd(source: &str, session_id: &str) -> Option<isize> {
    let guard = CAPTURED_HWNDS.lock().ok()?;
    guard
        .as_ref()?
        .get(&(source.to_owned(), session_id.to_owned()))
        .copied()
}

fn forget_hwnd(source: &str, session_id: &str) {
    if let Ok(mut guard) = CAPTURED_HWNDS.lock() {
        if let Some(table) = guard.as_mut() {
            table.remove(&(source.to_owned(), session_id.to_owned()));
        }
    }
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
        forget_hwnd(source, session_id);
        return FocusResult {
            focused: false,
            reason: Some("会话已不存在".to_owned()),
        };
    };
    #[cfg(windows)]
    if let Some(hwnd) = take_stale_or_hwnd(source, session_id) {
        if win32::is_window(hwnd) {
            if win32::bring_to_foreground(hwnd) {
                return FocusResult {
                    focused: true,
                    reason: None,
                };
            }
            return FocusResult {
                focused: false,
                reason: Some("无法把窗口提到前台".to_owned()),
            };
        }
        forget_hwnd(source, session_id);
    }
    #[cfg(not(windows))]
    {
        let _ = take_stale_or_hwnd(source, session_id);
    }
    match focus_decision(&FocusRequest {
        deep_link: session.deep_link.clone(),
        window_title: session.window_title.clone(),
        project_path: session.project_path.clone(),
        source: Some(session.source.clone()),
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
        FocusDecision::MatchWindow { hints } => focus_window_by_hints(&hints),
        FocusDecision::Unavailable { reason } => FocusResult {
            focused: false,
            reason: Some(reason),
        },
    }
}

fn focus_window_by_hints(hints: &[String]) -> FocusResult {
    #[cfg(not(windows))]
    {
        let _ = hints;
        FocusResult {
            focused: false,
            reason: Some("当前平台不能聚焦源终端".to_owned()),
        }
    }
    #[cfg(windows)]
    {
        let windows = win32::visible_terminal_windows();
        let titles: Vec<String> = windows.iter().map(|window| window.title.clone()).collect();
        match select_window_by_hints(&titles, hints) {
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
                    reason: Some(format!(
                        "没有找到匹配的终端窗口（线索：{}）",
                        hints
                            .iter()
                            .map(|hint| format!("「{hint}」"))
                            .collect::<Vec<_>>()
                            .join("、")
                    )),
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
    use agent_activity_dock_core::is_terminal_window_candidate;
    use std::os::windows::ffi::OsStringExt;

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
        fn GetClassNameW(hwnd: isize, lp: *mut u16, n: i32) -> i32;
        fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
        fn SetForegroundWindow(hwnd: isize) -> i32;
        fn IsIconic(hwnd: isize) -> i32;
        fn ShowWindow(hwnd: isize, ncmd: i32) -> i32;
        fn GetForegroundWindow() -> isize;
        fn IsWindow(hwnd: isize) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn QueryFullProcessImageNameW(
            process: isize,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }

    const SW_RESTORE: i32 = 9;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    fn utf16_to_string(buf: &[u16], written: i32) -> String {
        if written <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..written as usize])
    }

    fn class_name(hwnd: isize) -> String {
        let mut buf = [0u16; 256];
        let written = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        utf16_to_string(&buf, written)
    }

    fn process_image(hwnd: isize) -> String {
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
        }
        if pid == 0 {
            return String::new();
        }
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle == 0 {
            return String::new();
        }
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let written = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) };
        unsafe {
            CloseHandle(handle);
        }
        if written == 0 || size == 0 {
            return String::new();
        }
        std::ffi::OsString::from_wide(&buf[..size as usize])
            .to_string_lossy()
            .into_owned()
    }

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
        let title = utf16_to_string(&buf, written);
        if title.is_empty() {
            return 1;
        }
        // Jump-back only: restrict to terminal hosts. Not used for state.
        if !is_terminal_hwnd(hwnd) {
            return 1;
        }
        windows.push(VisibleWindow { hwnd, title });
        1
    }

    fn is_terminal_hwnd(hwnd: isize) -> bool {
        is_terminal_window_candidate(&class_name(hwnd), &process_image(hwnd))
    }

    pub fn foreground_terminal_hwnd() -> Option<isize> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd == 0 || !is_terminal_hwnd(hwnd) {
            None
        } else {
            Some(hwnd)
        }
    }

    pub fn is_window(hwnd: isize) -> bool {
        unsafe { IsWindow(hwnd) != 0 }
    }

    pub fn visible_terminal_windows() -> Vec<VisibleWindow> {
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

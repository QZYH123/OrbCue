//! Presenter-side jump-back execution.
//!
//! Ladder: deep_link → dock: marker (precise tab) → captured HWND (window-level)
//! → honest failure. Used only for the user's explicit「回去」click.

#[cfg(windows)]
use agent_activity_dock_core::{captured_hwnd_usable, select_unique_window_title};
use agent_activity_dock_core::{
    captured_keys_to_drop, dock_terminal_marker, focus_decision, sessions_to_capture,
    CaptureSession, FocusDecision, FocusRequest, SessionKey, SessionSnapshot, JUMP_WINDOW_MISSING,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusResult {
    pub focused: bool,
    #[serde(default)]
    pub precise: bool,
    pub reason: Option<String>,
}

impl FocusResult {
    fn success(precise: bool) -> Self {
        Self {
            focused: true,
            precise,
            reason: None,
        }
    }

    fn failure(reason: impl Into<String>) -> Self {
        Self {
            focused: false,
            precise: false,
            reason: Some(reason.into()),
        }
    }
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

fn take_hwnd(source: &str, session_id: &str) -> Option<isize> {
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
        return FocusResult::failure("会话已不存在");
    };
    match focus_decision(&FocusRequest {
        deep_link: session.deep_link.clone(),
        terminal_id: session.terminal_id.clone(),
    }) {
        FocusDecision::OpenDeepLink(url) => match open_deep_link(&url) {
            Ok(()) => FocusResult::success(true),
            Err(reason) => FocusResult::failure(reason),
        },
        FocusDecision::FocusDockMarker { marker } => focus_dock_marker(&marker),
        FocusDecision::UseCapturedWindow => focus_captured_window(source, session_id),
    }
}

fn focus_captured_window(source: &str, session_id: &str) -> FocusResult {
    #[cfg(windows)]
    {
        if let Some(hwnd) = take_hwnd(source, session_id) {
            let alive = win32::is_window(hwnd);
            let terminal = win32::is_terminal_hwnd(hwnd);
            if captured_hwnd_usable(alive, terminal) {
                if win32::bring_to_foreground(hwnd) {
                    return FocusResult::success(false);
                }
                return FocusResult::failure("无法把窗口提到前台");
            }
            forget_hwnd(source, session_id);
            log_capture(&format!(
                "jump-capture dropped source={source} session={session_id} alive={alive} terminal={terminal}"
            ));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (source, session_id);
        forget_hwnd(source, session_id);
    }
    FocusResult::failure(JUMP_WINDOW_MISSING)
}

fn focus_dock_marker(marker: &str) -> FocusResult {
    if dock_terminal_marker(marker).is_none() {
        return FocusResult::failure(JUMP_WINDOW_MISSING);
    }
    #[cfg(windows)]
    {
        if focus_window_title_containing(marker) {
            return FocusResult::success(true);
        }
        match win32::focus_tab_named(marker) {
            Ok(()) => FocusResult::success(true),
            Err(reason) => {
                log_capture(&format!("jump-marker {marker} missed: {reason}"));
                FocusResult::failure(JUMP_WINDOW_MISSING)
            }
        }
    }
    #[cfg(not(windows))]
    {
        FocusResult::failure("当前平台不能聚焦源终端")
    }
}

#[cfg(windows)]
fn focus_window_title_containing(marker: &str) -> bool {
    let windows = win32::visible_terminal_windows();
    let titles: Vec<String> = windows.iter().map(|window| window.title.clone()).collect();
    match select_unique_window_title(&titles, marker) {
        Ok(title) => windows
            .iter()
            .find(|window| window.title == title)
            .is_some_and(|window| win32::bring_to_foreground(window.hwnd)),
        Err(_) => false,
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
        if !is_terminal_hwnd(hwnd) {
            return 1;
        }
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        let title = if len <= 0 {
            String::new()
        } else {
            let mut buf = vec![0u16; (len as usize) + 1];
            let written = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
            utf16_to_string(&buf, written)
        };
        windows.push(VisibleWindow { hwnd, title });
        1
    }

    pub fn is_terminal_hwnd(hwnd: isize) -> bool {
        hwnd != 0 && is_terminal_window_candidate(&class_name(hwnd), &process_image(hwnd))
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
        hwnd != 0 && unsafe { IsWindow(hwnd) != 0 }
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

    pub fn focus_tab_named(marker: &str) -> Result<(), String> {
        uia::select_tab_containing(marker, &visible_terminal_windows())
    }

    mod uia {
        use super::{bring_to_foreground, VisibleWindow};
        use std::sync::Once;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::System::Variant::VARIANT;
        use windows::Win32::UI::Accessibility::{
            CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationElementArray,
            IUIAutomationSelectionItemPattern, TreeScope_Descendants, UIA_ControlTypePropertyId,
            UIA_SelectionItemPatternId, UIA_TabItemControlTypeId, UIA_WindowControlTypeId,
        };

        static COM: Once = Once::new();

        pub fn select_tab_containing(
            marker: &str,
            windows: &[VisibleWindow],
        ) -> Result<(), String> {
            COM.call_once(|| unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            });
            let automation: IUIAutomation = unsafe {
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                    .map_err(|error| error.to_string())?
            };
            let needle = marker.to_lowercase();
            let root = unsafe {
                automation
                    .GetRootElement()
                    .map_err(|error| error.to_string())?
            };
            if focus_named_control(
                &automation,
                &root,
                UIA_WindowControlTypeId.0,
                &needle,
                false,
            )? {
                return Ok(());
            }
            if focus_named_control(
                &automation,
                &root,
                UIA_TabItemControlTypeId.0,
                &needle,
                true,
            )? {
                return Ok(());
            }
            for window in windows {
                let Some(element) = (unsafe {
                    automation
                        .ElementFromHandle(HWND(window.hwnd as *mut core::ffi::c_void))
                        .ok()
                }) else {
                    continue;
                };
                if name_contains(&element, &needle) && bring_to_foreground(window.hwnd) {
                    return Ok(());
                }
                if focus_named_control(
                    &automation,
                    &element,
                    UIA_TabItemControlTypeId.0,
                    &needle,
                    true,
                )? {
                    let _ = bring_to_foreground(window.hwnd);
                    return Ok(());
                }
            }
            Err("no matching tab".to_owned())
        }

        fn focus_named_control(
            automation: &IUIAutomation,
            root: &IUIAutomationElement,
            control_type: i32,
            needle: &str,
            select_tab: bool,
        ) -> Result<bool, String> {
            let condition = unsafe {
                automation
                    .CreatePropertyCondition(
                        UIA_ControlTypePropertyId,
                        &VARIANT::from(control_type),
                    )
                    .map_err(|error| error.to_string())?
            };
            let found = unsafe {
                root.FindAll(TreeScope_Descendants, &condition)
                    .map_err(|error| error.to_string())?
            };
            for element in automation_elements(&found)? {
                if !name_contains(&element, needle) {
                    continue;
                }
                if select_tab {
                    if let Ok(pattern) = unsafe {
                        element.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                            UIA_SelectionItemPatternId,
                        )
                    } {
                        let _ = unsafe { pattern.Select() };
                    }
                }
                if let Some(hwnd) = containing_window(automation, &element) {
                    if bring_to_foreground(hwnd) {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }

        fn automation_elements(
            array: &IUIAutomationElementArray,
        ) -> Result<Vec<IUIAutomationElement>, String> {
            let count = unsafe { array.Length().map_err(|error| error.to_string())? };
            let mut elements = Vec::new();
            for index in 0..count {
                elements
                    .push(unsafe { array.GetElement(index).map_err(|error| error.to_string())? });
            }
            Ok(elements)
        }

        fn name_contains(element: &IUIAutomationElement, needle: &str) -> bool {
            unsafe { element.CurrentName().ok() }
                .map(|value| value.to_string().to_lowercase().contains(needle))
                .unwrap_or(false)
        }

        fn containing_window(
            automation: &IUIAutomation,
            element: &IUIAutomationElement,
        ) -> Option<isize> {
            if let Some(hwnd) = native_window(element) {
                return Some(hwnd);
            }
            let walker = unsafe { automation.RawViewWalker().ok()? };
            let mut current = element.clone();
            for _ in 0..24 {
                let parent = unsafe { walker.GetParentElement(&current).ok()? };
                if let Some(hwnd) = native_window(&parent) {
                    return Some(hwnd);
                }
                current = parent;
            }
            None
        }

        fn native_window(element: &IUIAutomationElement) -> Option<isize> {
            let handle = unsafe { element.CurrentNativeWindowHandle().ok()? };
            if handle.0.is_null() {
                None
            } else {
                Some(handle.0 as isize)
            }
        }
    }
}

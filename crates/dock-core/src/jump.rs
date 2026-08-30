//! Presenter jump-back decisions. These are not lifecycle rules.

pub const DOCK_TERMINAL_PREFIX: &str = "orb:";
pub const DOCK_MARKER_HEX_LEN: usize = 6;
pub const JUMP_WINDOW_MISSING: &str = "找不到该会话的窗口。用 orb run 启动可获得精确跳回";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusRequest {
    pub deep_link: Option<String>,
    pub terminal_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusDecision {
    OpenDeepLink(String),
    FocusDockMarker { marker: String },
    UseCapturedWindow,
}

impl FocusDecision {
    pub fn is_precise(&self) -> bool {
        !matches!(self, Self::UseCapturedWindow)
    }
}

/// Ordered attempts for one jump-back click. `orb:` is precise; captured HWND
/// is only a fallback after that channel misses.
pub fn focus_attempts(request: &FocusRequest) -> Vec<FocusDecision> {
    if let Some(url) = nonempty(request.deep_link.as_deref()) {
        return vec![FocusDecision::OpenDeepLink(url.to_owned())];
    }
    if let Some(marker) = request
        .terminal_id
        .as_deref()
        .and_then(dock_terminal_marker)
    {
        return vec![
            FocusDecision::FocusDockMarker {
                marker: marker.to_owned(),
            },
            FocusDecision::UseCapturedWindow,
        ];
    }
    vec![FocusDecision::UseCapturedWindow]
}

/// `orb:` + 6 hex digits. Used as both `terminal_id` and the WT tab title marker.
pub fn dock_terminal_marker(terminal_id: &str) -> Option<&str> {
    let trimmed = terminal_id.trim();
    let rest = trimmed.strip_prefix(DOCK_TERMINAL_PREFIX)?;
    (rest.len() == DOCK_MARKER_HEX_LEN && rest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(trimmed)
}

pub fn format_dock_marker(suffix: u32) -> String {
    format!("{DOCK_TERMINAL_PREFIX}{:06x}", suffix & 0x00FF_FFFF)
}

/// Captured HWND is only usable when the window still exists *and* is still a
/// terminal host. Either failure drops the record and degrades.
pub fn captured_hwnd_usable(window_alive: bool, is_terminal: bool) -> bool {
    window_alive && is_terminal
}

pub fn select_unique_window_title<'a, T: AsRef<str>>(
    titles: &'a [T],
    hint: &str,
) -> Result<&'a str, String> {
    let matches = titles_matching(titles, hint);
    match matches.len() {
        1 => Ok(matches[0]),
        0 => Err(format!("没有找到匹配的终端窗口（线索：「{hint}」）")),
        _ => Err(format!("终端窗口匹配不唯一（线索：「{hint}」）")),
    }
}

fn titles_matching<'a, T: AsRef<str>>(titles: &'a [T], hint: &str) -> Vec<&'a str> {
    let needle = hint.to_lowercase();
    titles
        .iter()
        .map(AsRef::as_ref)
        .filter(|title| title.to_lowercase().contains(&needle))
        .collect()
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.and_then(|text| (!text.is_empty()).then_some(text))
}

pub fn project_path_hint(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    match trimmed.rsplit(['/', '\\']).next() {
        Some(segment) if !segment.is_empty() => segment.to_owned(),
        _ => path.to_owned(),
    }
}

/// Window class names that identify a terminal host. Presenter supplies the
/// Win32 class; this crate never calls Win32.
pub const TERMINAL_WINDOW_CLASSES: &[&str] =
    &["CASCADIA_HOSTING_WINDOW_CLASS", "ConsoleWindowClass"];

/// Process image file names treated as terminals. Used only when the user
/// clicks jump-back, never to infer Dock state.
pub const TERMINAL_PROCESS_NAMES: &[&str] = &[
    "windowsterminal.exe",
    "conhost.exe",
    "openconsole.exe",
    "alacritty.exe",
    "wezterm-gui.exe",
    "wezterm.exe",
    "mintty.exe",
    "tabby.exe",
];

pub fn process_image_file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Pure predicate: a jump-back candidate is a terminal window.
/// Filtering happens in the presenter after it reads class/image from Win32.
pub fn is_terminal_window_candidate(class_name: &str, process_image: &str) -> bool {
    if TERMINAL_WINDOW_CLASSES
        .iter()
        .any(|class| class_name.eq_ignore_ascii_case(class))
    {
        return true;
    }
    let file = process_image_file_name(process_image);
    TERMINAL_PROCESS_NAMES
        .iter()
        .any(|name| file.eq_ignore_ascii_case(name))
}

pub fn session_terminal_title(source: &str, project_path: Option<&str>) -> String {
    match nonempty(project_path) {
        Some(path) => format!("{} · {source}", project_path_hint(path)),
        None => source.to_owned(),
    }
}

pub fn dock_tab_title(agent: &str, project_path: Option<&str>, marker: &str) -> String {
    format!("{} · {marker}", session_terminal_title(agent, project_path))
}

#[cfg(test)]
mod tests {
    use super::{
        captured_hwnd_usable, dock_tab_title, dock_terminal_marker, focus_attempts,
        format_dock_marker, is_terminal_window_candidate, project_path_hint,
        select_unique_window_title, session_terminal_title, FocusDecision, FocusRequest,
        JUMP_WINDOW_MISSING,
    };

    fn first_decision(request: &FocusRequest) -> FocusDecision {
        focus_attempts(request)
            .into_iter()
            .next()
            .unwrap_or(FocusDecision::UseCapturedWindow)
    }

    fn request(deep_link: Option<&str>, terminal_id: Option<&str>) -> FocusRequest {
        FocusRequest {
            deep_link: deep_link.map(str::to_owned),
            terminal_id: terminal_id.map(str::to_owned),
        }
    }

    #[test]
    fn deep_link_wins_and_skips_window_matching() {
        assert_eq!(
            first_decision(&request(
                Some("https://example.invalid/session"),
                Some("orb:ab12cd"),
            )),
            FocusDecision::OpenDeepLink("https://example.invalid/session".to_owned())
        );
        assert!(
            first_decision(&request(Some("https://example.invalid/session"), None)).is_precise()
        );
    }

    #[test]
    fn dock_marker_is_the_precise_channel() {
        assert_eq!(
            first_decision(&request(None, Some("orb:ab12cd"))),
            FocusDecision::FocusDockMarker {
                marker: "orb:ab12cd".to_owned(),
            }
        );
        assert!(first_decision(&request(None, Some("orb:AB12CD"))).is_precise());
        assert_eq!(dock_terminal_marker("orb:ab12cd"), Some("orb:ab12cd"));
        assert_eq!(dock_terminal_marker(" orb:00ffaa "), Some("orb:00ffaa"));
        assert_eq!(dock_terminal_marker("orb:abc"), None);
        assert_eq!(dock_terminal_marker("pts/3"), None);
        assert_eq!(format_dock_marker(0x00ab_12cd), "orb:ab12cd");
    }

    #[test]
    fn project_path_and_source_are_not_used_as_hints() {
        assert_eq!(
            first_decision(&request(None, None)),
            FocusDecision::UseCapturedWindow
        );
        assert!(!first_decision(&request(None, None)).is_precise());
        assert_eq!(
            first_decision(&request(Some(""), Some(""))),
            FocusDecision::UseCapturedWindow
        );
        assert_eq!(
            first_decision(&request(None, Some("pts/5"))),
            FocusDecision::UseCapturedWindow
        );
        assert!(JUMP_WINDOW_MISSING.contains("orb run"));
    }

    #[test]
    fn dock_marker_falls_back_to_captured_window() {
        assert_eq!(
            focus_attempts(&request(None, Some("orb:ab12cd"))),
            [
                FocusDecision::FocusDockMarker {
                    marker: "orb:ab12cd".to_owned(),
                },
                FocusDecision::UseCapturedWindow,
            ]
        );
        assert_eq!(
            focus_attempts(&request(
                Some("https://example.invalid/s"),
                Some("orb:ab12cd")
            )),
            [FocusDecision::OpenDeepLink(
                "https://example.invalid/s".to_owned()
            )]
        );
        assert_eq!(
            focus_attempts(&request(None, None)),
            [FocusDecision::UseCapturedWindow]
        );
    }

    #[test]
    fn captured_hwnd_dead_or_non_terminal_is_not_usable() {
        assert!(!captured_hwnd_usable(false, true));
        assert!(!captured_hwnd_usable(true, false));
        assert!(!captured_hwnd_usable(false, false));
        assert!(captured_hwnd_usable(true, true));
    }

    #[test]
    fn unique_title_match_returns_that_title() {
        let titles = [
            "Visual Studio Code",
            "agent-activity-dock · grok · orb:ab12cd",
        ];
        assert_eq!(
            select_unique_window_title(&titles, "orb:ab12cd").unwrap(),
            "agent-activity-dock · grok · orb:ab12cd"
        );
    }

    #[test]
    fn title_match_is_case_insensitive() {
        let titles = ["Windows Terminal - Dock"];
        assert_eq!(
            select_unique_window_title(&titles, "dock").unwrap(),
            "Windows Terminal - Dock"
        );
    }

    #[test]
    fn zero_matches_do_not_guess() {
        let titles = ["Visual Studio Code", "Firefox"];
        assert_eq!(
            select_unique_window_title(&titles, "agent-activity-dock").unwrap_err(),
            "没有找到匹配的终端窗口（线索：「agent-activity-dock」）"
        );
    }

    #[test]
    fn multiple_matches_do_not_pick_the_first() {
        let titles = ["Windows Terminal - dock", "Windows Terminal - dock-core"];
        assert_eq!(
            select_unique_window_title(&titles, "dock").unwrap_err(),
            "终端窗口匹配不唯一（线索：「dock」）"
        );
    }

    #[test]
    fn terminal_filter_drops_browser() {
        let windows = [
            (
                "Grok — Chat",
                "Chrome_WidgetWin_1",
                r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
            ),
            (
                "Grok Bot",
                "CASCADIA_HOSTING_WINDOW_CLASS",
                r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.0\WindowsTerminal.exe",
            ),
            ("QQ", "TXGuiFoundation", r"C:\Program Files\Tencent\QQ.exe"),
        ];
        let titles: Vec<&str> = windows
            .iter()
            .filter(|(_, class, image)| is_terminal_window_candidate(class, image))
            .map(|(title, _, _)| *title)
            .collect();
        assert_eq!(titles, ["Grok Bot"]);
        assert!(!is_terminal_window_candidate(
            "Chrome_WidgetWin_1",
            "msedge.exe"
        ));
        assert!(is_terminal_window_candidate(
            "ConsoleWindowClass",
            "conhost.exe"
        ));
        assert!(is_terminal_window_candidate("", "alacritty.exe"));
    }

    #[test]
    fn terminal_title_hint_matches_project_path_hint() {
        let path = "/home/qingz/projects/agent-activity-dock/";
        assert_eq!(project_path_hint(path), "agent-activity-dock");
        assert_eq!(
            session_terminal_title("grok", Some(path)),
            "agent-activity-dock · grok"
        );
        assert_eq!(
            dock_tab_title("grok", Some(path), "orb:ab12cd"),
            "agent-activity-dock · grok · orb:ab12cd"
        );
        assert_eq!(
            session_terminal_title("claude", Some(r"C:\Users\qingz\work\repo\")),
            format!(
                "{} · claude",
                project_path_hint(r"C:\Users\qingz\work\repo\\")
            )
        );
        assert_eq!(session_terminal_title("grok", None), "grok");
        assert_eq!(session_terminal_title("grok", Some("")), "grok");
        assert_eq!(
            dock_tab_title("claude", None, "orb:00ffaa"),
            "claude · orb:00ffaa"
        );
    }
}

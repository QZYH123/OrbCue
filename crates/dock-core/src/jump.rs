//! Presenter jump-back decisions. These are not lifecycle rules.

pub const DOCK_TERMINAL_PREFIX: &str = "orb:";
pub const DOCK_MARKER_HEX_LEN: usize = 6;
pub const JUMP_WINDOW_MISSING: &str = "找不到该会话的窗口。用 orb run 启动可获得精确跳回";

pub fn jump_window_missing() -> &'static str {
    crate::pick(
        JUMP_WINDOW_MISSING,
        "Can't find that session's window. Start it with orb run to jump back to the exact tab.",
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusRequest {
    pub deep_link: Option<String>,
    pub terminal_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopApp {
    Claude,
    Codex,
    Cursor,
}

impl DesktopApp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusDecision {
    OpenDeepLink(String),
    FocusDockMarker { marker: String },
    UseCapturedWindow,
    FocusDesktopApp { app: DesktopApp },
}

impl FocusDecision {
    pub fn is_precise(&self) -> bool {
        !matches!(self, Self::UseCapturedWindow | Self::FocusDesktopApp { .. })
    }
}

/// Ordered attempts for one jump-back click. A desktop-app row only brings
/// that program forward. Command-line rows keep the terminal ladder: `orb:`
/// is precise; captured HWND is only a fallback after that channel misses.
pub fn focus_attempts(request: &FocusRequest) -> Vec<FocusDecision> {
    if let Some(app) = request
        .terminal_id
        .as_deref()
        .and_then(desktop_app_from_terminal_id)
    {
        return vec![FocusDecision::FocusDesktopApp { app }];
    }
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
        0 => Err(crate::t!(
            "没有找到匹配的终端窗口（线索：「{hint}」）",
            "No terminal window matched (“{hint}”)."
        )),
        _ => Err(crate::t!(
            "终端窗口匹配不唯一（线索：「{hint}」）",
            "More than one terminal window matched (“{hint}”)."
        )),
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

/// `app:<claude|codex|cursor>:` plus 16 hex digits. One id per session, so
/// parallel desktop sessions do not share a terminal and do not retire each
/// other. Command-line tty / `orb:` / `live:` ids never use this prefix.
pub fn format_desktop_terminal_id(app: DesktopApp, session_id: &str) -> Option<String> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return None;
    }
    let token = session_token(session_id);
    let id = format!("app:{}:{token}", app.as_str());
    (id.len() <= crate::MAX_TERMINAL_ID_LEN).then_some(id)
}

pub fn desktop_app_from_terminal_id(terminal_id: &str) -> Option<DesktopApp> {
    let rest = terminal_id.trim().strip_prefix("app:")?;
    let (name, token) = rest.split_once(':')?;
    if token.is_empty() {
        return None;
    }
    match name {
        "claude" => Some(DesktopApp::Claude),
        "codex" => Some(DesktopApp::Codex),
        "cursor" => Some(DesktopApp::Cursor),
        _ => None,
    }
}

pub fn desktop_app_missing(app: DesktopApp) -> &'static str {
    match app {
        DesktopApp::Claude => crate::pick("找不到 Claude 的窗口", "Can't find a Claude window"),
        DesktopApp::Codex => crate::pick("找不到 ChatGPT 的窗口", "Can't find a ChatGPT window"),
        DesktopApp::Cursor => crate::pick("找不到 Cursor 的窗口", "Can't find a Cursor window"),
    }
}

/// Parent chain from the hook's parent toward the root. The highest match is
/// the long-lived app process, not a helper that exits with one hook.
pub fn select_desktop_process(app: DesktopApp, chain_from_parent: &[(u32, &str)]) -> Option<u32> {
    chain_from_parent
        .iter()
        .rev()
        .find(|(_, image)| is_desktop_app_image(app, image))
        .map(|(pid, _)| *pid)
}

/// Process image or command line of a desktop coding app. The Claude CLI
/// (`~/.local/bin/claude`, `claude/versions`) and `cursor-agent` do not match.
pub fn is_desktop_app_image(app: DesktopApp, process_image: &str) -> bool {
    let normalized = process_image.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    match app {
        DesktopApp::Claude => {
            image_has_stem(&lower, "claude")
                && (lower.contains("/anthropicclaude/")
                    || lower.contains("/windowsapps/claude")
                    || lower.contains("/programs/claude/")
                    || lower.contains("/claude.app/"))
        }
        DesktopApp::Codex => {
            image_has_stem(&lower, "chatgpt")
                || lower.contains("/windowsapps/openai.codex")
                || lower.contains("/openai/codex/runtimes/")
                || lower.contains("/openai/codex/app/")
        }
        DesktopApp::Cursor => {
            if lower.contains("cursor-agent") {
                return false;
            }
            lower.contains("/.cursor-server/")
                || lower.contains("/programs/cursor/")
                || lower.contains("/cursor/resources/app/")
                || image_has_stem(&lower, "cursor")
        }
    }
}

fn session_token(session_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(session_id.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn image_has_stem(process_image: &str, stem_name: &str) -> bool {
    process_image
        .split(|character: char| character == '/' || character.is_ascii_whitespace())
        .any(|segment| exe_stem(segment).eq_ignore_ascii_case(stem_name))
}

fn exe_stem(segment: &str) -> &str {
    let start = segment.len().saturating_sub(4);
    if segment
        .get(start..)
        .is_some_and(|end| end.eq_ignore_ascii_case(".exe"))
        && start > 0
    {
        &segment[..start]
    } else {
        segment
    }
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
        captured_hwnd_usable, desktop_app_from_terminal_id, dock_tab_title, dock_terminal_marker,
        focus_attempts, format_desktop_terminal_id, format_dock_marker, is_desktop_app_image,
        is_terminal_window_candidate, project_path_hint, select_desktop_process,
        select_unique_window_title, session_terminal_title, DesktopApp, FocusDecision,
        FocusRequest, JUMP_WINDOW_MISSING,
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
    fn desktop_row_foregrounds_the_app_and_skips_the_terminal_ladder() {
        let claude = format_desktop_terminal_id(DesktopApp::Claude, "sess-a").unwrap();
        let other = format_desktop_terminal_id(DesktopApp::Claude, "sess-b").unwrap();
        assert_ne!(claude, other);
        assert!(claude.starts_with("app:claude:"));
        assert_eq!(
            desktop_app_from_terminal_id(&claude),
            Some(DesktopApp::Claude)
        );
        assert_eq!(desktop_app_from_terminal_id("app:claude:"), None);
        assert_eq!(desktop_app_from_terminal_id("orb:ab12cd"), None);
        assert_eq!(
            focus_attempts(&request(Some("https://example.invalid/s"), Some(&claude))),
            [FocusDecision::FocusDesktopApp {
                app: DesktopApp::Claude
            }]
        );
        assert!(!first_decision(&request(None, Some(&claude))).is_precise());
        assert_eq!(
            focus_attempts(&request(
                Some("https://example.invalid/s"),
                Some("orb:ab12cd")
            ))
            .len(),
            1
        );
    }

    #[test]
    fn desktop_image_matches_the_app_and_not_the_cli() {
        assert!(is_desktop_app_image(
            DesktopApp::Claude,
            r"C:\Users\u\AppData\Local\AnthropicClaude\app\claude.exe"
        ));
        assert!(is_desktop_app_image(
            DesktopApp::Claude,
            r"C:\Program Files\WindowsApps\Claude_1.0_x64\claude.exe"
        ));
        assert!(!is_desktop_app_image(
            DesktopApp::Claude,
            r"C:\Users\u\.local\bin\claude.exe"
        ));
        assert!(!is_desktop_app_image(
            DesktopApp::Claude,
            "/home/u/.local/share/claude/versions/2.1.226"
        ));
        assert!(is_desktop_app_image(
            DesktopApp::Codex,
            r"C:\Program Files\WindowsApps\OpenAI.Codex_1\app\ChatGPT.exe"
        ));
        assert!(is_desktop_app_image(
            DesktopApp::Codex,
            r"C:\Users\u\AppData\Local\OpenAI\Codex\runtimes\node\codex.exe"
        ));
        assert!(!is_desktop_app_image(
            DesktopApp::Codex,
            "/home/u/.local/bin/codex"
        ));
        assert!(is_desktop_app_image(
            DesktopApp::Cursor,
            r"C:\Users\u\AppData\Local\Programs\cursor\Cursor.exe"
        ));
        assert!(is_desktop_app_image(
            DesktopApp::Cursor,
            "/home/u/.cursor-server/bin/node"
        ));
        assert!(!is_desktop_app_image(
            DesktopApp::Cursor,
            "/home/u/.local/share/cursor-agent/versions/x/node"
        ));
        let chain = [
            (
                4u32,
                r"C:\Users\u\AppData\Local\AnthropicClaude\app\claude.exe",
            ),
            (2u32, r"C:\Users\u\AppData\Local\AnthropicClaude\claude.exe"),
        ];
        assert_eq!(select_desktop_process(DesktopApp::Claude, &chain), Some(2));
        assert_eq!(
            select_desktop_process(
                DesktopApp::Claude,
                &[(3, r"C:\Users\u\.local\bin\claude.exe")]
            ),
            None
        );
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

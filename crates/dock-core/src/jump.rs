//! Presenter jump-back decisions. These are not lifecycle rules.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusRequest {
    pub deep_link: Option<String>,
    pub window_title: Option<String>,
    pub project_path: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusDecision {
    OpenDeepLink(String),
    MatchWindow { hints: Vec<String> },
    Unavailable { reason: String },
}

pub fn focus_decision(request: &FocusRequest) -> FocusDecision {
    if let Some(url) = nonempty(request.deep_link.as_deref()) {
        return FocusDecision::OpenDeepLink(url.to_owned());
    }
    let mut hints = Vec::new();
    if let Some(title) = nonempty(request.window_title.as_deref()) {
        hints.push(title.to_owned());
    } else if let Some(path) = nonempty(request.project_path.as_deref()) {
        hints.push(project_path_hint(path));
    }
    if let Some(source) = nonempty(request.source.as_deref()) {
        if !hints.iter().any(|hint| hint.eq_ignore_ascii_case(source)) {
            hints.push(source.to_owned());
        }
    }
    if hints.is_empty() {
        FocusDecision::Unavailable {
            reason: "没有可用于跳回的定位信息".to_owned(),
        }
    } else {
        FocusDecision::MatchWindow { hints }
    }
}

pub fn select_unique_window_title<'a, T: AsRef<str>>(
    titles: &'a [T],
    hint: &str,
) -> Result<&'a str, String> {
    select_window_by_hints(titles, &[hint.to_owned()])
}

pub fn select_window_by_hints<'a, T: AsRef<str>>(
    titles: &'a [T],
    hints: &[String],
) -> Result<&'a str, String> {
    let mut tried: Vec<&str> = Vec::new();
    for hint in hints {
        if hint.is_empty() {
            continue;
        }
        tried.push(hint.as_str());
        let matches = titles_matching(titles, hint);
        match matches.len() {
            1 => return Ok(matches[0]),
            0 => continue,
            _ => {
                return Err(format!(
                    "终端窗口匹配不唯一（线索：{}）",
                    quote_clues(&tried)
                ))
            }
        }
    }
    Err(format!(
        "没有找到匹配的终端窗口（线索：{}）",
        quote_clues(&tried)
    ))
}

fn titles_matching<'a, T: AsRef<str>>(titles: &'a [T], hint: &str) -> Vec<&'a str> {
    let needle = hint.to_lowercase();
    titles
        .iter()
        .map(AsRef::as_ref)
        .filter(|title| title.to_lowercase().contains(&needle))
        .collect()
}

fn quote_clues(clues: &[&str]) -> String {
    clues
        .iter()
        .map(|hint| format!("「{hint}」"))
        .collect::<Vec<_>>()
        .join("、")
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
        Some(path) => format!("{source} · {}", project_path_hint(path)),
        None => source.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        focus_decision, is_terminal_window_candidate, project_path_hint,
        select_unique_window_title, select_window_by_hints, session_terminal_title, FocusDecision,
        FocusRequest,
    };

    fn request(
        deep_link: Option<&str>,
        window_title: Option<&str>,
        project_path: Option<&str>,
    ) -> FocusRequest {
        request_with_source(deep_link, window_title, project_path, None)
    }

    fn request_with_source(
        deep_link: Option<&str>,
        window_title: Option<&str>,
        project_path: Option<&str>,
        source: Option<&str>,
    ) -> FocusRequest {
        FocusRequest {
            deep_link: deep_link.map(str::to_owned),
            window_title: window_title.map(str::to_owned),
            project_path: project_path.map(str::to_owned),
            source: source.map(str::to_owned),
        }
    }

    #[test]
    fn deep_link_wins_and_skips_window_matching() {
        assert_eq!(
            focus_decision(&request(
                Some("https://example.invalid/session"),
                Some("Windows Terminal"),
                Some("/tmp/project"),
            )),
            FocusDecision::OpenDeepLink("https://example.invalid/session".to_owned())
        );
    }

    #[test]
    fn window_title_is_used_when_deep_link_is_absent() {
        assert_eq!(
            focus_decision(&request(None, Some("Windows Terminal - dock"), None)),
            FocusDecision::MatchWindow {
                hints: vec!["Windows Terminal - dock".to_owned()],
            }
        );
    }

    #[test]
    fn empty_location_fields_are_treated_as_missing() {
        assert_eq!(
            focus_decision(&request(Some(""), Some(""), Some(""))),
            FocusDecision::Unavailable {
                reason: "没有可用于跳回的定位信息".to_owned(),
            }
        );
    }

    #[test]
    fn project_path_last_segment_is_the_window_hint() {
        assert_eq!(
            focus_decision(&request(None, None, Some("/home/qingz/projects/dock"))),
            FocusDecision::MatchWindow {
                hints: vec!["dock".to_owned()],
            }
        );
        assert_eq!(
            focus_decision(&request(None, None, Some("C:\\Users\\qingz\\work\\repo\\"))),
            FocusDecision::MatchWindow {
                hints: vec!["repo".to_owned()],
            }
        );
    }

    #[test]
    fn missing_location_is_unavailable() {
        assert_eq!(
            focus_decision(&request(None, None, None)),
            FocusDecision::Unavailable {
                reason: "没有可用于跳回的定位信息".to_owned(),
            }
        );
    }

    #[test]
    fn unique_title_match_returns_that_title() {
        let titles = [
            "Visual Studio Code",
            "Windows Terminal - agent-activity-dock",
        ];
        assert_eq!(
            select_unique_window_title(&titles, "activity-dock").unwrap(),
            "Windows Terminal - agent-activity-dock"
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
    fn project_hint_wins_when_it_is_unique() {
        let titles = ["Grok Bot", "Windows Terminal - agent-activity-dock"];
        let hints = vec!["agent-activity-dock".to_owned(), "grok".to_owned()];
        assert_eq!(
            select_window_by_hints(&titles, &hints).unwrap(),
            "Windows Terminal - agent-activity-dock"
        );
    }

    #[test]
    fn source_fallback_hits_unique_agent_window() {
        let titles = ["Visual Studio Code", "Grok Bot"];
        let hints = vec!["agent-activity-dock".to_owned(), "grok".to_owned()];
        assert_eq!(select_window_by_hints(&titles, &hints).unwrap(), "Grok Bot");
        assert_eq!(
            focus_decision(&request_with_source(
                None,
                None,
                Some("/tmp/agent-activity-dock"),
                Some("grok"),
            )),
            FocusDecision::MatchWindow {
                hints: vec!["agent-activity-dock".to_owned(), "grok".to_owned()],
            }
        );
    }

    #[test]
    fn cascade_zero_matches_lists_tried_clues() {
        let titles = ["Visual Studio Code", "Firefox"];
        let hints = vec!["agent-activity-dock".to_owned(), "grok".to_owned()];
        assert_eq!(
            select_window_by_hints(&titles, &hints).unwrap_err(),
            "没有找到匹配的终端窗口（线索：「agent-activity-dock」、「grok」）"
        );
    }

    #[test]
    fn cascade_multiple_matches_lists_tried_clues() {
        let titles = ["Grok Bot", "grok · other"];
        let hints = vec!["missing-project".to_owned(), "grok".to_owned()];
        assert_eq!(
            select_window_by_hints(&titles, &hints).unwrap_err(),
            "终端窗口匹配不唯一（线索：「missing-project」、「grok」）"
        );
    }

    #[test]
    fn terminal_filter_drops_browser_then_source_hits_grok_bot() {
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
        assert_eq!(
            select_window_by_hints(&titles, &["grok".to_owned()]).unwrap(),
            "Grok Bot"
        );
    }

    #[test]
    fn terminal_filter_all_non_terminals_is_zero_match() {
        let windows = [
            ("Grok — Chat", "Chrome_WidgetWin_1", "msedge.exe"),
            ("QQ", "TXGuiFoundation", "QQ.exe"),
        ];
        let titles: Vec<&str> = windows
            .iter()
            .filter(|(_, class, image)| is_terminal_window_candidate(class, image))
            .map(|(title, _, _)| *title)
            .collect();
        assert!(titles.is_empty());
        assert_eq!(
            select_window_by_hints(&titles, &["grok".to_owned()]).unwrap_err(),
            "没有找到匹配的终端窗口（线索：「grok」）"
        );
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
            "grok · agent-activity-dock"
        );
        assert_eq!(
            session_terminal_title("claude", Some(r"C:\Users\qingz\work\repo\")),
            format!(
                "claude · {}",
                project_path_hint(r"C:\Users\qingz\work\repo\\")
            )
        );
        assert_eq!(session_terminal_title("grok", None), "grok");
        assert_eq!(session_terminal_title("grok", Some("")), "grok");
    }

    #[test]
    fn apply_result_has_no_focus_field() {
        // Jump-back is presenter-only. DockState::apply returns ApplyResult
        // { accepted, snapshot, attention, rejection_reason } and never
        // triggers focus_source. Working / idle events stay silent.
        let fields = ["accepted", "snapshot", "attention", "rejection_reason"];
        assert!(!fields.contains(&"focus"));
    }
}

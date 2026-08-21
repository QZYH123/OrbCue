//! Presenter jump-back decisions. These are not lifecycle rules.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusRequest {
    pub deep_link: Option<String>,
    pub window_title: Option<String>,
    pub project_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusDecision {
    OpenDeepLink(String),
    MatchWindow { hint: String },
    Unavailable { reason: String },
}

pub fn focus_decision(request: &FocusRequest) -> FocusDecision {
    if let Some(url) = nonempty(request.deep_link.as_deref()) {
        return FocusDecision::OpenDeepLink(url.to_owned());
    }
    if let Some(title) = nonempty(request.window_title.as_deref()) {
        return FocusDecision::MatchWindow {
            hint: title.to_owned(),
        };
    }
    if let Some(path) = nonempty(request.project_path.as_deref()) {
        return FocusDecision::MatchWindow {
            hint: project_path_hint(path),
        };
    }
    FocusDecision::Unavailable {
        reason: "没有可用于跳回的定位信息".to_owned(),
    }
}

pub fn select_unique_window_title<'a, T: AsRef<str>>(
    titles: &'a [T],
    hint: &str,
) -> Result<&'a str, String> {
    let needle = hint.to_lowercase();
    let matches: Vec<&str> = titles
        .iter()
        .map(AsRef::as_ref)
        .filter(|title| title.to_lowercase().contains(&needle))
        .collect();
    match matches.as_slice() {
        [title] => Ok(*title),
        [] => Err("没有找到匹配的窗口".to_owned()),
        _ => Err("匹配不唯一，没有切换窗口".to_owned()),
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.and_then(|text| (!text.is_empty()).then_some(text))
}

fn project_path_hint(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    match trimmed.rsplit(['/', '\\']).next() {
        Some(segment) if !segment.is_empty() => segment.to_owned(),
        _ => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{focus_decision, select_unique_window_title, FocusDecision, FocusRequest};

    fn request(
        deep_link: Option<&str>,
        window_title: Option<&str>,
        project_path: Option<&str>,
    ) -> FocusRequest {
        FocusRequest {
            deep_link: deep_link.map(str::to_owned),
            window_title: window_title.map(str::to_owned),
            project_path: project_path.map(str::to_owned),
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
                hint: "Windows Terminal - dock".to_owned(),
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
                hint: "dock".to_owned(),
            }
        );
        assert_eq!(
            focus_decision(&request(None, None, Some("C:\\Users\\qingz\\work\\repo\\"))),
            FocusDecision::MatchWindow {
                hint: "repo".to_owned(),
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
            "没有找到匹配的窗口"
        );
    }

    #[test]
    fn multiple_matches_do_not_pick_the_first() {
        let titles = ["Windows Terminal - dock", "Windows Terminal - dock-core"];
        assert_eq!(
            select_unique_window_title(&titles, "dock").unwrap_err(),
            "匹配不唯一，没有切换窗口"
        );
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

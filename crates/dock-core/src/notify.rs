//! Presenter toast decisions. These are not lifecycle rules.

use crate::Attention;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToastSpec {
    pub source: String,
    pub session_id: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightTarget {
    pub source: String,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastDispatch {
    Silent,
    Sent(ToastSpec),
    Failed { spec: ToastSpec, error: String },
}

pub trait NotificationSink {
    fn show(&self, toast: &ToastSpec) -> Result<(), String>;
}

pub fn toast_for_attention(attention: &Attention) -> Option<ToastSpec> {
    let title = match attention.reason.as_str() {
        "input" => "等待输入",
        "permission" => "等待授权",
        "failed" => "任务失败",
        _ => return None,
    };
    Some(ToastSpec {
        source: attention.source.clone(),
        session_id: attention.session_id.clone(),
        title: title.to_owned(),
        body: attention.source.clone(),
    })
}

pub fn dispatch_attention_toast(
    sink: &dyn NotificationSink,
    attention: Option<&Attention>,
    enabled: bool,
) -> ToastDispatch {
    if !enabled {
        return ToastDispatch::Silent;
    }
    let Some(attention) = attention else {
        return ToastDispatch::Silent;
    };
    let Some(spec) = toast_for_attention(attention) else {
        return ToastDispatch::Silent;
    };
    match sink.show(&spec) {
        Ok(()) => ToastDispatch::Sent(spec),
        Err(error) => ToastDispatch::Failed { spec, error },
    }
}

pub fn highlight_target(source: Option<&str>, session_id: Option<&str>) -> Option<HighlightTarget> {
    match (source, session_id) {
        (Some(source), Some(session_id)) if !source.is_empty() && !session_id.is_empty() => {
            Some(HighlightTarget {
                source: source.to_owned(),
                session_id: session_id.to_owned(),
            })
        }
        _ => None,
    }
}

pub fn highlight_key(source: &str, session_id: &str) -> String {
    format!("{source}\0{session_id}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionJump {
    pub source: String,
    pub session_id: String,
    pub deep_link: Option<String>,
    pub terminal_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionClickFollowup {
    Stay,
    OpenPanel,
}

/// Toast click looks up the live session so jump-back can use `orb:` / deep_link.
/// A vanished session only opens the panel.
pub fn attention_jump(
    sessions: &[AttentionJump],
    source: &str,
    session_id: &str,
) -> Option<AttentionJump> {
    sessions
        .iter()
        .find(|session| session.source == source && session.session_id == session_id)
        .cloned()
}

pub fn attention_click_followup(focused: bool) -> AttentionClickFollowup {
    if focused {
        AttentionClickFollowup::Stay
    } else {
        AttentionClickFollowup::OpenPanel
    }
}

#[cfg(test)]
mod tests {
    use super::{
        attention_click_followup, attention_jump, dispatch_attention_toast, highlight_target,
        toast_for_attention, AttentionClickFollowup, AttentionJump, NotificationSink,
        ToastDispatch, ToastSpec,
    };
    use crate::{Attention, DockEvent, DockState, EventKind, Severity};
    use std::sync::Mutex;

    struct RecordingSink {
        shown: Mutex<Vec<ToastSpec>>,
        error: Option<&'static str>,
    }

    impl RecordingSink {
        fn ok() -> Self {
            Self {
                shown: Mutex::new(Vec::new()),
                error: None,
            }
        }

        fn failing() -> Self {
            Self {
                shown: Mutex::new(Vec::new()),
                error: Some("toast failed"),
            }
        }

        fn shown(&self) -> Vec<ToastSpec> {
            self.shown.lock().expect("sink lock").clone()
        }
    }

    impl NotificationSink for RecordingSink {
        fn show(&self, toast: &ToastSpec) -> Result<(), String> {
            if let Some(error) = self.error {
                return Err(error.to_owned());
            }
            self.shown.lock().expect("sink lock").push(toast.clone());
            Ok(())
        }
    }

    fn attention(reason: &str) -> Attention {
        Attention {
            source: "claude".to_owned(),
            session_id: "s1".to_owned(),
            reason: reason.to_owned(),
            severity: Severity::Attention,
        }
    }

    fn event(id: &str, kind: EventKind) -> DockEvent {
        DockEvent::new(id, kind, "claude", "s1")
    }

    #[test]
    fn toast_only_for_input_permission_and_failed() {
        let input = toast_for_attention(&attention("input")).unwrap();
        assert_eq!(input.title, "等待输入");
        assert_eq!(input.body, "claude");
        assert_eq!(input.session_id, "s1");

        let permission = toast_for_attention(&attention("permission")).unwrap();
        assert_eq!(permission.title, "等待授权");

        let failed = toast_for_attention(&attention("failed")).unwrap();
        assert_eq!(failed.title, "任务失败");

        assert!(toast_for_attention(&attention("completed")).is_none());
        assert!(toast_for_attention(&attention("cancelled")).is_none());
        assert!(toast_for_attention(&attention("other")).is_none());
    }

    #[test]
    fn waiting_notifies_once_because_repeat_apply_has_no_attention() {
        let mut state = DockState::new();
        state.apply(event("e1", EventKind::Started));
        let first = state.apply(event("e2", EventKind::WaitingInput));
        let sink = RecordingSink::ok();
        assert!(matches!(
            dispatch_attention_toast(&sink, first.attention.as_ref(), true),
            ToastDispatch::Sent(_)
        ));

        let repeat = state.apply(event("e3", EventKind::WaitingInput));
        assert!(repeat.attention.is_none());
        assert!(matches!(
            dispatch_attention_toast(&sink, repeat.attention.as_ref(), true),
            ToastDispatch::Silent
        ));
        assert_eq!(sink.shown().len(), 1);
    }

    #[test]
    fn completed_attention_stays_silent() {
        let mut state = DockState::new();
        state.apply(event("e1", EventKind::Started));
        let completed = state.apply(event("e2", EventKind::Completed));
        assert_eq!(
            completed
                .attention
                .as_ref()
                .map(|item| item.reason.as_str()),
            Some("completed")
        );
        let sink = RecordingSink::ok();
        assert_eq!(
            dispatch_attention_toast(&sink, completed.attention.as_ref(), true),
            ToastDispatch::Silent
        );
        assert!(sink.shown().is_empty());
    }

    #[test]
    fn disabled_switch_does_not_call_sink() {
        let sink = RecordingSink::ok();
        assert_eq!(
            dispatch_attention_toast(&sink, Some(&attention("failed")), false),
            ToastDispatch::Silent
        );
        assert!(sink.shown().is_empty());
    }

    #[test]
    fn sink_failure_does_not_change_dock_state() {
        let mut state = DockState::new();
        state.apply(event("e1", EventKind::Started));
        let failed = state.apply(event("e2", EventKind::Failed));
        let before = state.snapshot();
        let sink = RecordingSink::failing();
        assert!(matches!(
            dispatch_attention_toast(&sink, failed.attention.as_ref(), true),
            ToastDispatch::Failed { .. }
        ));
        assert_eq!(state.snapshot(), before);
        assert_eq!(state.snapshot().pending_mark, "!");
        assert_eq!(state.snapshot().sessions[0].session_id, "s1");
    }

    #[test]
    fn highlight_payload_requires_both_ids() {
        assert!(highlight_target(Some("claude"), Some("s1")).is_some());
        assert!(highlight_target(Some(""), Some("s1")).is_none());
        assert!(highlight_target(Some("claude"), Some("")).is_none());
        assert!(highlight_target(None, Some("s1")).is_none());
    }

    fn jump(source: &str, session_id: &str, terminal_id: Option<&str>) -> AttentionJump {
        AttentionJump {
            source: source.to_owned(),
            session_id: session_id.to_owned(),
            deep_link: None,
            terminal_id: terminal_id.map(str::to_owned),
        }
    }

    #[test]
    fn toast_click_jumps_the_matching_session() {
        let sessions = [
            jump("codex", "other", None),
            jump("claude", "s1", Some("orb:ab12cd")),
        ];
        assert_eq!(
            attention_jump(&sessions, "claude", "s1"),
            Some(jump("claude", "s1", Some("orb:ab12cd")))
        );
    }

    #[test]
    fn toast_click_opens_panel_when_the_session_is_gone() {
        let sessions = [jump("claude", "s1", Some("orb:ab12cd"))];
        assert_eq!(attention_jump(&sessions, "claude", "missing"), None);
    }

    #[test]
    fn successful_jump_leaves_the_panel_closed() {
        assert_eq!(attention_click_followup(true), AttentionClickFollowup::Stay);
        assert_eq!(
            attention_click_followup(false),
            AttentionClickFollowup::OpenPanel
        );
    }
}

//! Presenter cue decisions. These are not lifecycle rules.
//! Phone notify is the same cue as a system toast, sent only when a topic URL is set.

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

fn attention_toast(
    attention: &Attention,
    notify_completion: bool,
    project_path: Option<&str>,
) -> Option<ToastSpec> {
    let title = match attention.reason.as_str() {
        "input" => crate::pick("等待输入", "Waiting for input"),
        "permission" => crate::pick("等待授权", "Waiting for approval"),
        "failed" => crate::pick("任务失败", "Task failed"),
        "completed" if notify_completion => crate::pick("任务完成", "Task finished"),
        _ => return None,
    };
    Some(ToastSpec {
        source: attention.source.clone(),
        session_id: attention.session_id.clone(),
        title: title.to_owned(),
        body: crate::session_terminal_title(&attention.source, project_path),
    })
}

/// Folder name for the cue. The full path stays off the notification.
pub fn cue_project_path<'a>(
    sessions: &'a [crate::SessionSnapshot],
    source: &str,
    session_id: &str,
) -> Option<&'a str> {
    sessions.iter().find_map(|session| {
        if session.source != source || session.session_id != session_id {
            return None;
        }
        session
            .project_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
    })
}

pub fn dispatch_attention_toast(
    sink: &dyn NotificationSink,
    attention: Option<&Attention>,
    enabled: bool,
    notify_completion: bool,
    project_path: Option<&str>,
) -> ToastDispatch {
    if !enabled {
        return ToastDispatch::Silent;
    }
    let Some(attention) = attention else {
        return ToastDispatch::Silent;
    };
    let Some(spec) = attention_toast(attention, notify_completion, project_path) else {
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
///
/// Match is conversation-level (`source` + `session_id`). If two processes
/// resumed the same chat, the first list row wins; toast extras do not carry
/// a terminal instance.
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

const MAX_PHONE_URL_LEN: usize = 2048;

/// Where to POST one ntfy message. `path` starts with `/` and is the topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneNotifyRequest {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneNotifyUrlError {
    Invalid,
    MissingTopic,
    Credentials,
}

impl PhoneNotifyUrlError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Invalid => crate::pick(
                "手机提醒地址需要是 http 或 https 链接",
                "The phone URL has to be an http or https link",
            ),
            Self::MissingTopic => crate::pick(
                "地址要带话题名，例如 https://ntfy.sh/你的话题",
                "The URL needs a topic, for example https://ntfy.sh/your-topic",
            ),
            Self::Credentials => crate::pick(
                "先不要在地址里写账号或密码",
                "Don't put a username or password in the URL",
            ),
        }
    }
}

/// Empty means off. A topic URL is `https://host/话题名`.
pub fn normalize_phone_notify_url(raw: &str) -> Result<Option<String>, PhoneNotifyUrlError> {
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    if parse_phone_target(trimmed)?.is_none() {
        return Ok(None);
    }
    Ok(Some(trimmed.to_owned()))
}

pub fn phone_notify_request(
    raw_url: &str,
    title: &str,
    message: &str,
) -> Result<Option<PhoneNotifyRequest>, PhoneNotifyUrlError> {
    let Some(target) = parse_phone_target(raw_url.trim().trim_start_matches('\u{feff}'))? else {
        return Ok(None);
    };
    Ok(Some(PhoneNotifyRequest {
        https: target.https,
        host: target.host,
        port: target.port,
        path: target.path,
        body: phone_notify_json(title, message),
    }))
}

/// Same cues as the system toast, including completion when that switch is on.
/// An empty URL produces nothing.
pub fn phone_notify_for_attention(
    raw_url: &str,
    attention: &Attention,
    notify_completion: bool,
    project_path: Option<&str>,
) -> Result<Option<PhoneNotifyRequest>, PhoneNotifyUrlError> {
    let Some(spec) = attention_toast(attention, notify_completion, project_path) else {
        return Ok(None);
    };
    phone_notify_request(raw_url, &spec.title, &spec.body)
}

struct PhoneTarget {
    https: bool,
    host: String,
    port: u16,
    path: String,
}

fn parse_phone_target(raw: &str) -> Result<Option<PhoneTarget>, PhoneNotifyUrlError> {
    if raw.is_empty() {
        return Ok(None);
    }
    if raw.len() > MAX_PHONE_URL_LEN
        || raw.chars().any(|ch| ch.is_control() || ch.is_whitespace())
        || raw.contains('\\')
        || raw.contains('#')
    {
        return Err(PhoneNotifyUrlError::Invalid);
    }
    if raw.contains('@') {
        return Err(PhoneNotifyUrlError::Credentials);
    }
    let (https, rest) = if starts_with_ignore_ascii_case(raw, "https://") {
        (true, &raw["https://".len()..])
    } else if starts_with_ignore_ascii_case(raw, "http://") {
        (false, &raw["http://".len()..])
    } else {
        return Err(PhoneNotifyUrlError::Invalid);
    };
    let Some((hostport, path)) = rest.split_once('/') else {
        return Err(PhoneNotifyUrlError::MissingTopic);
    };
    let (path_only, query) = match path.split_once('?') {
        Some((path_only, query)) => (path_only, Some(query)),
        None => (path, None),
    };
    let path_only = path_only.trim_end_matches('/');
    if path_only.is_empty() || path_only.split('/').any(|segment| segment.is_empty()) {
        return Err(PhoneNotifyUrlError::MissingTopic);
    }
    let (host, port) = split_host_port(hostport, https)?;
    let path = match query {
        Some(query) if !query.is_empty() => format!("/{path_only}?{query}"),
        _ => format!("/{path_only}"),
    };
    Ok(Some(PhoneTarget {
        https,
        host,
        port,
        path,
    }))
}

fn split_host_port(hostport: &str, https: bool) -> Result<(String, u16), PhoneNotifyUrlError> {
    if hostport.is_empty() {
        return Err(PhoneNotifyUrlError::Invalid);
    }
    let (host, port) = if let Some(rest) = hostport.strip_prefix('[') {
        let Some((host, rest)) = rest.split_once(']') else {
            return Err(PhoneNotifyUrlError::Invalid);
        };
        if host.is_empty() || !rest.is_empty() && !rest.starts_with(':') {
            return Err(PhoneNotifyUrlError::Invalid);
        }
        let port = if let Some(port) = rest.strip_prefix(':') {
            parse_port(port)?
        } else {
            default_port(https)
        };
        (host.to_owned(), port)
    } else if let Some((host, port)) = hostport.rsplit_once(':') {
        if host.is_empty() || host.contains(':') {
            return Err(PhoneNotifyUrlError::Invalid);
        }
        (host.to_owned(), parse_port(port)?)
    } else {
        (hostport.to_owned(), default_port(https))
    };
    if !host_ok(&host) {
        return Err(PhoneNotifyUrlError::Invalid);
    }
    Ok((host, port))
}

fn host_ok(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('.')
        && !host.contains("..")
        && !host.contains('/')
        && !host.contains('?')
}

fn default_port(https: bool) -> u16 {
    if https {
        443
    } else {
        80
    }
}

fn parse_port(raw: &str) -> Result<u16, PhoneNotifyUrlError> {
    if raw.is_empty() || raw.len() > 5 || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(PhoneNotifyUrlError::Invalid);
    }
    let port: u16 = raw.parse().map_err(|_| PhoneNotifyUrlError::Invalid)?;
    if port == 0 {
        return Err(PhoneNotifyUrlError::Invalid);
    }
    Ok(port)
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value.len() >= prefix.len()
        && value.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn phone_notify_json(title: &str, message: &str) -> String {
    format!(
        "{{\"title\":{},\"message\":{}}}",
        json_string(title),
        json_string(message)
    )
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{
        attention_click_followup, attention_jump, attention_toast, cue_project_path,
        dispatch_attention_toast, highlight_target, normalize_phone_notify_url,
        phone_notify_for_attention, phone_notify_request, AttentionClickFollowup, AttentionJump,
        NotificationSink, PhoneNotifyUrlError, ToastDispatch, ToastSpec,
    };
    use crate::{
        Attention, DockEvent, DockState, EventKind, SessionSnapshot, SessionState, Severity,
    };
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
        let input = attention_toast(&attention("input"), false, None).unwrap();
        assert_eq!(input.title, "等待输入");
        assert_eq!(input.body, "claude");
        assert_eq!(input.session_id, "s1");

        let permission = attention_toast(&attention("permission"), false, None).unwrap();
        assert_eq!(permission.title, "等待授权");

        let failed = attention_toast(&attention("failed"), false, None).unwrap();
        assert_eq!(failed.title, "任务失败");

        assert!(attention_toast(&attention("completed"), false, None).is_none());
        assert!(attention_toast(&attention("completed"), true, None).is_some());
        assert!(attention_toast(&attention("cancelled"), true, None).is_none());
        assert!(attention_toast(&attention("other"), true, None).is_none());
    }

    #[test]
    fn english_cues_use_english_titles() {
        let _guard = crate::locale::force_lang(crate::locale::Lang::En);
        assert_eq!(
            attention_toast(&attention("input"), false, None)
                .unwrap()
                .title,
            "Waiting for input"
        );
        assert_eq!(
            attention_toast(&attention("completed"), true, None)
                .unwrap()
                .title,
            "Task finished"
        );
    }

    #[test]
    fn waiting_notifies_once_because_repeat_apply_has_no_attention() {
        let mut state = DockState::new();
        state.apply(event("e1", EventKind::Started));
        let first = state.apply(event("e2", EventKind::WaitingInput));
        let sink = RecordingSink::ok();
        assert!(matches!(
            dispatch_attention_toast(&sink, first.attention.as_ref(), true, false, None),
            ToastDispatch::Sent(_)
        ));

        let repeat = state.apply(event("e3", EventKind::WaitingInput));
        assert!(repeat.attention.is_none());
        assert!(matches!(
            dispatch_attention_toast(&sink, repeat.attention.as_ref(), true, false, None),
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
            dispatch_attention_toast(&sink, completed.attention.as_ref(), true, false, None),
            ToastDispatch::Silent
        );
        assert!(sink.shown().is_empty());
    }

    #[test]
    fn completion_toast_is_on_unless_its_switch_is_off() {
        let done = attention("completed");
        let sink = RecordingSink::ok();
        assert_eq!(
            dispatch_attention_toast(&sink, Some(&done), true, false, None),
            ToastDispatch::Silent
        );

        let sent = dispatch_attention_toast(&sink, Some(&done), true, true, None);
        let ToastDispatch::Sent(spec) = sent else {
            panic!("expected a completion toast");
        };
        assert_eq!(spec.title, "任务完成");
        assert_eq!(spec.body, "claude");
        assert!(!spec.body.contains("s1"));

        let blocked = RecordingSink::ok();
        assert_eq!(
            dispatch_attention_toast(&blocked, Some(&done), false, true, None),
            ToastDispatch::Silent
        );
        assert_eq!(
            dispatch_attention_toast(&blocked, Some(&attention("cancelled")), true, true, None),
            ToastDispatch::Silent
        );
        assert!(blocked.shown().is_empty());
    }

    #[test]
    fn disabled_switch_does_not_call_sink() {
        let sink = RecordingSink::ok();
        assert_eq!(
            dispatch_attention_toast(&sink, Some(&attention("failed")), false, true, None),
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
            dispatch_attention_toast(&sink, failed.attention.as_ref(), true, false, None),
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
    fn toast_click_picks_the_first_row_of_a_resumed_conversation() {
        let sessions = [
            jump("grok", "resume-id", Some("term-a")),
            jump("grok", "resume-id", Some("term-b")),
        ];
        assert_eq!(
            attention_jump(&sessions, "grok", "resume-id"),
            Some(jump("grok", "resume-id", Some("term-a")))
        );
    }

    #[test]
    fn successful_jump_leaves_the_panel_closed() {
        assert_eq!(attention_click_followup(true), AttentionClickFollowup::Stay);
        assert_eq!(
            attention_click_followup(false),
            AttentionClickFollowup::OpenPanel
        );
    }

    #[test]
    fn empty_phone_url_is_off() {
        assert_eq!(normalize_phone_notify_url("  ").unwrap(), None);
        assert_eq!(normalize_phone_notify_url("\u{feff}\n").unwrap(), None);
    }

    #[test]
    fn phone_url_accepts_topic_host_port_and_ipv6() {
        let https = normalize_phone_notify_url(" https://ntfy.sh/secret-topic/ ").unwrap();
        assert_eq!(https.as_deref(), Some("https://ntfy.sh/secret-topic/"));
        let request = phone_notify_request(https.as_deref().unwrap(), "等待输入", "claude")
            .unwrap()
            .unwrap();
        assert!(request.https);
        assert_eq!(request.host, "ntfy.sh");
        assert_eq!(request.port, 443);
        assert_eq!(request.path, "/secret-topic");
        assert_eq!(request.body, r#"{"title":"等待输入","message":"claude"}"#);

        let local = phone_notify_request("http://10.0.0.8:2586/desk", "t", "m")
            .unwrap()
            .unwrap();
        assert!(!local.https);
        assert_eq!(local.host, "10.0.0.8");
        assert_eq!(local.port, 2586);
        assert_eq!(local.path, "/desk");

        let ipv6 = phone_notify_request("http://[::1]:8443/topic/?x=1", "t", "m")
            .unwrap()
            .unwrap();
        assert_eq!(ipv6.host, "::1");
        assert_eq!(ipv6.port, 8443);
        assert_eq!(ipv6.path, "/topic?x=1");

        let nested = phone_notify_request("HTTPS://ntfy.example/ntfy/Topic", "t", "m")
            .unwrap()
            .unwrap();
        assert!(nested.https);
        assert_eq!(nested.port, 443);
        assert_eq!(nested.path, "/ntfy/Topic");
    }

    #[test]
    fn phone_url_rejects_missing_topic_credentials_and_other_schemes() {
        assert_eq!(
            normalize_phone_notify_url("https://ntfy.sh").unwrap_err(),
            PhoneNotifyUrlError::MissingTopic
        );
        assert_eq!(
            normalize_phone_notify_url("https://ntfy.sh/").unwrap_err(),
            PhoneNotifyUrlError::MissingTopic
        );
        assert_eq!(
            normalize_phone_notify_url("https://user:pass@ntfy.sh/topic").unwrap_err(),
            PhoneNotifyUrlError::Credentials
        );
        assert_eq!(
            normalize_phone_notify_url("ftp://ntfy.sh/topic").unwrap_err(),
            PhoneNotifyUrlError::Invalid
        );
        assert_eq!(
            normalize_phone_notify_url("http:///topic").unwrap_err(),
            PhoneNotifyUrlError::Invalid
        );
        assert!(
            normalize_phone_notify_url(&format!("https://ntfy.sh/{}", "a".repeat(2048))).is_err()
        );
    }

    #[test]
    fn phone_body_is_title_and_tool_name_only() {
        let input = phone_notify_for_attention(
            "https://ntfy.sh/secret-topic",
            &attention("input"),
            false,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(input.body, r#"{"title":"等待输入","message":"claude"}"#);
        assert!(!input.body.contains("s1"));
        assert!(!input.body.contains("secret-topic"));

        let permission = phone_notify_for_attention(
            "https://ntfy.sh/secret-topic",
            &attention("permission"),
            false,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            permission.body,
            r#"{"title":"等待授权","message":"claude"}"#
        );

        let failed = phone_notify_for_attention(
            "https://ntfy.sh/secret-topic",
            &attention("failed"),
            true,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(failed.body, r#"{"title":"任务失败","message":"claude"}"#);

        assert!(phone_notify_for_attention(
            "https://ntfy.sh/secret-topic",
            &attention("completed"),
            false,
            None
        )
        .unwrap()
        .is_none());
        let completed = phone_notify_for_attention(
            "https://ntfy.sh/secret-topic",
            &attention("completed"),
            true,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(completed.body, r#"{"title":"任务完成","message":"claude"}"#);
        assert!(!completed.body.contains("s1"));
        assert!(
            phone_notify_for_attention("   ", &attention("input"), true, None)
                .unwrap()
                .is_none()
        );
        assert!(
            phone_notify_for_attention("notaurl", &attention("completed"), false, None)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            phone_notify_for_attention("notaurl", &attention("completed"), true, None).unwrap_err(),
            PhoneNotifyUrlError::Invalid
        );
        assert_eq!(
            phone_notify_for_attention("notaurl", &attention("failed"), false, None).unwrap_err(),
            PhoneNotifyUrlError::Invalid
        );
    }

    #[test]
    fn cue_shows_the_project_folder_and_not_the_full_path() {
        let path = r"C:\Users\qingz\projects\OrbCue\";
        let spec = attention_toast(&attention("input"), false, Some(path)).unwrap();
        assert_eq!(spec.body, "OrbCue · claude");
        assert!(!spec.body.contains("Users"));
        assert!(!spec.body.contains('\\'));

        let quoted = phone_notify_for_attention(
            "https://ntfy.sh/secret-topic",
            &attention("failed"),
            false,
            Some("/tmp/my\"app"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            quoted.body,
            r#"{"title":"任务失败","message":"my\"app · claude"}"#
        );
        assert!(!quoted.body.contains("/tmp"));

        let sessions = [
            snapshot_session("other", "s1", Some("/elsewhere/Nope")),
            snapshot_session("claude", "s1", Some("")),
            snapshot_session("claude", "s1", Some("/home/qingz/projects/OrbCue")),
        ];
        assert_eq!(
            cue_project_path(&sessions, "claude", "s1"),
            Some("/home/qingz/projects/OrbCue")
        );
        assert_eq!(cue_project_path(&sessions, "claude", "missing"), None);
    }

    fn snapshot_session(source: &str, session_id: &str, path: Option<&str>) -> SessionSnapshot {
        SessionSnapshot {
            source: source.to_owned(),
            session_id: session_id.to_owned(),
            state: SessionState::NeedsAttention,
            mark: "?".to_owned(),
            attention_reason: Some("input".to_owned()),
            deep_link: None,
            project_path: path.map(str::to_owned),
            terminal_id: None,
            acknowledged: false,
            occurred_at: "2026-09-27T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn phone_json_escapes_quotes_and_slashes() {
        let request = phone_notify_request("https://ntfy.sh/topic", "a\"b", "c\\d\n")
            .unwrap()
            .unwrap();
        assert_eq!(request.body, r#"{"title":"a\"b","message":"c\\d\n"}"#);
    }
}

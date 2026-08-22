//! Public domain seam for Agent Activity Dock.
//!
//! The rest of the application talks to this crate through events and
//! snapshots. Rendering, transport and Agent-specific adapters stay outside
//! this boundary.

mod capture;
mod jump;
mod notify;

pub use capture::{captured_keys_to_drop, sessions_to_capture, CaptureSession, SessionKey};
pub use jump::{
    captured_hwnd_usable, dock_tab_title, dock_terminal_marker, focus_decision, format_dock_marker,
    is_terminal_window_candidate, process_image_file_name, project_path_hint,
    select_unique_window_title, select_window_by_hints, session_terminal_title, FocusDecision,
    FocusRequest, DOCK_MARKER_HEX_LEN, DOCK_TERMINAL_PREFIX, JUMP_WINDOW_MISSING,
    TERMINAL_PROCESS_NAMES, TERMINAL_WINDOW_CLASSES,
};
pub use notify::{
    dispatch_attention_toast, highlight_key, highlight_target, toast_for_attention,
    HighlightTarget, NotificationSink, ToastDispatch, ToastSpec,
};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet, VecDeque};
use time::{Duration, OffsetDateTime};

pub const EVENT_VERSION: u16 = 1;
pub const MAX_EVENT_ID_LEN: usize = 128;
pub const MAX_TERMINAL_ID_LEN: usize = 128;
pub const MAX_SOURCE_LEN: usize = 64;
pub const MAX_SESSION_ID_LEN: usize = 256;
pub const MAX_SUMMARY_LEN: usize = 512;
pub const MAX_DEEP_LINK_LEN: usize = 2_048;
pub const MAX_METADATA_ITEMS: usize = 32;
pub const MAX_METADATA_VALUE_LEN: usize = 256;
pub const MAX_EVENT_AGE: Duration = Duration::hours(24);
pub const MAX_FUTURE_SKEW: Duration = Duration::minutes(5);
const SEEN_EVENT_LIMIT: usize = 8_192;
const MAX_SESSIONS: usize = 256;
const MAX_AUDIT_ENTRIES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    #[serde(rename = "session.started")]
    Started,
    #[serde(rename = "session.idle")]
    Idle,
    #[serde(rename = "session.working")]
    Working,
    #[serde(rename = "session.waiting_input")]
    WaitingInput,
    #[serde(rename = "session.permission_requested")]
    PermissionRequested,
    #[serde(rename = "session.completed")]
    Completed,
    #[serde(rename = "session.failed")]
    Failed,
    #[serde(rename = "session.cancelled")]
    Cancelled,
    #[serde(rename = "session.closed")]
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Attention,
    Error,
}

impl Default for Severity {
    fn default() -> Self {
        Self::Info
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Working,
    NeedsAttention,
    Completed,
    Failed,
    Cancelled,
}

impl SessionState {
    pub fn is_open(self) -> bool {
        true
    }

    pub fn mark(self) -> &'static str {
        match self {
            Self::Working => "",
            Self::Idle => "o",
            Self::Completed => "*",
            Self::Cancelled => "x",
            Self::NeedsAttention => "?",
            Self::Failed => "!",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockEvent {
    pub version: u16,
    #[serde(rename = "type")]
    pub kind: EventKind,
    pub event_id: String,
    pub source: String,
    pub session_id: String,
    pub occurred_at: String,
    #[serde(default)]
    pub severity: Severity,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub deep_link: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub window_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
    #[serde(default)]
    pub requires_user_action: Option<bool>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl DockEvent {
    pub fn new(event_id: &str, kind: EventKind, source: &str, session_id: &str) -> Self {
        Self {
            version: EVENT_VERSION,
            kind,
            event_id: event_id.to_owned(),
            source: source.to_owned(),
            session_id: session_id.to_owned(),
            occurred_at: OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned()),
            severity: Severity::Info,
            summary: None,
            deep_link: None,
            cwd: None,
            workspace_root: None,
            window_title: None,
            parent_session_id: None,
            terminal_id: None,
            requires_user_action: None,
            metadata: BTreeMap::new(),
        }
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    pub fn with_occurred_at(mut self, occurred_at: impl Into<String>) -> Self {
        self.occurred_at = occurred_at.into();
        self
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    pub fn requiring_user_action(mut self, required: bool) -> Self {
        self.requires_user_action = Some(required);
        self
    }

    pub fn with_parent_session_id(mut self, parent_session_id: impl Into<String>) -> Self {
        let parent = parent_session_id.into();
        self.parent_session_id = (!parent.is_empty()).then_some(parent);
        self
    }

    pub fn with_terminal_id(mut self, terminal_id: impl Into<String>) -> Self {
        let terminal_id = terminal_id.into();
        self.terminal_id = (!terminal_id.is_empty()).then_some(terminal_id);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub source: String,
    pub session_id: String,
    pub state: SessionState,
    pub mark: String,
    pub attention_reason: Option<String>,
    pub summary: Option<String>,
    pub deep_link: Option<String>,
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub window_title: Option<String>,
    #[serde(default)]
    pub terminal_id: Option<String>,
    pub requires_user_action: bool,
    pub acknowledged: bool,
    pub occurred_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockSnapshot {
    pub working_count: usize,
    pub tracked_count: usize,
    pub pending_count: usize,
    pub pending_mark: String,
    pub sessions: Vec<SessionSnapshot>,
    pub audit: Vec<AuditEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub source: String,
    pub session_id: String,
    pub state: SessionState,
    pub attention_reason: Option<String>,
    pub occurred_at: String,
}

impl DockSnapshot {
    pub fn count_label(&self) -> String {
        format!("{}/{}", self.working_count, self.tracked_count)
    }

    pub fn is_working(&self) -> bool {
        self.working_count > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attention {
    pub source: String,
    pub session_id: String,
    pub reason: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyResult {
    pub accepted: bool,
    pub snapshot: DockSnapshot,
    pub attention: Option<Attention>,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedState {
    pub version: u16,
    pub sessions: Vec<PersistedSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedSession {
    pub source: String,
    pub session_id: String,
    pub state: SessionState,
    pub attention_reason: Option<String>,
    pub requires_user_action: bool,
    pub acknowledged: bool,
    pub occurred_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
}

#[derive(Debug, Clone)]
struct SessionRecord {
    source: String,
    session_id: String,
    state: SessionState,
    attention_reason: Option<String>,
    summary: Option<String>,
    deep_link: Option<String>,
    project_path: Option<String>,
    window_title: Option<String>,
    requires_user_action: bool,
    acknowledged: bool,
    occurred_at: String,
    terminal_id: Option<String>,
}

/// Deterministic in-memory session registry.
///
/// Persistence, transport and presentation intentionally do not appear here;
/// they can be replaced without changing lifecycle semantics.
#[derive(Debug, Default)]
pub struct DockState {
    sessions: BTreeMap<String, SessionRecord>,
    seen_event_ids: HashSet<String>,
    seen_order: VecDeque<String>,
    audit: VecDeque<AuditEntry>,
}

impl DockState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_persisted(persisted: PersistedState) -> Self {
        let mut state = Self::new();
        if persisted.version != EVENT_VERSION {
            return state;
        }
        for item in persisted.sessions {
            if !valid_len(&item.source, MAX_SOURCE_LEN)
                || !valid_len(&item.session_id, MAX_SESSION_ID_LEN)
            {
                continue;
            }
            let terminal_id = normalize_optional(&item.terminal_id)
                .filter(|value| valid_len(value, MAX_TERMINAL_ID_LEN))
                .map(str::to_owned);
            let key = session_key(&item.source, &item.session_id);
            state.sessions.insert(
                key,
                SessionRecord {
                    source: item.source,
                    session_id: item.session_id,
                    state: item.state,
                    attention_reason: item.attention_reason,
                    summary: None,
                    deep_link: None,
                    project_path: None,
                    window_title: None,
                    requires_user_action: item.requires_user_action,
                    acknowledged: item.acknowledged,
                    occurred_at: item.occurred_at,
                    terminal_id,
                },
            );
        }
        state
    }

    pub fn persisted(&self) -> PersistedState {
        const MAX_PERSISTED_SESSIONS: usize = 100;
        let mut sessions: Vec<_> = self
            .sessions
            .values()
            .map(|record| PersistedSession {
                source: record.source.clone(),
                session_id: record.session_id.clone(),
                state: record.state,
                attention_reason: record.attention_reason.clone(),
                requires_user_action: record.requires_user_action,
                acknowledged: record.acknowledged,
                occurred_at: record.occurred_at.clone(),
                terminal_id: record.terminal_id.clone(),
            })
            .collect();
        sessions.sort_by(|left, right| left.occurred_at.cmp(&right.occurred_at));
        if sessions.len() > MAX_PERSISTED_SESSIONS {
            sessions.drain(..sessions.len() - MAX_PERSISTED_SESSIONS);
        }
        PersistedState {
            version: EVENT_VERSION,
            sessions,
        }
    }

    pub fn snapshot(&self) -> DockSnapshot {
        let mut sessions: Vec<_> = self
            .sessions
            .values()
            .map(SessionRecord::snapshot)
            .collect();
        sessions.sort_by(|left, right| {
            session_priority(left)
                .cmp(&session_priority(right))
                .then_with(|| left.occurred_at.cmp(&right.occurred_at))
                .then_with(|| left.source.cmp(&right.source))
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        let working_count = sessions
            .iter()
            .filter(|session| session.state == SessionState::Working)
            .count();
        let tracked_count = sessions
            .iter()
            .filter(|session| session.state.is_open())
            .count();
        let pending_count = tracked_count.saturating_sub(working_count);
        let pending_mark = if sessions
            .iter()
            .any(|session| session.state == SessionState::Failed)
        {
            "!".to_owned()
        } else if sessions
            .iter()
            .any(|session| session.state == SessionState::NeedsAttention)
        {
            "?".to_owned()
        } else if sessions
            .iter()
            .any(|session| session.state == SessionState::Completed)
        {
            "*".to_owned()
        } else if sessions
            .iter()
            .any(|session| session.state == SessionState::Cancelled)
        {
            "x".to_owned()
        } else if sessions
            .iter()
            .any(|session| session.state == SessionState::Idle)
        {
            "o".to_owned()
        } else {
            String::new()
        };
        DockSnapshot {
            working_count,
            tracked_count,
            pending_count,
            pending_mark,
            sessions,
            audit: self.audit.iter().cloned().collect(),
        }
    }

    pub fn apply(&mut self, event: DockEvent) -> ApplyResult {
        self.apply_at(event, OffsetDateTime::now_utc())
    }

    /// Apply an event against an explicit clock. Keeping the clock at this
    /// boundary makes stale-event policy deterministic in tests and adapters.
    pub fn apply_at(&mut self, event: DockEvent, now: OffsetDateTime) -> ApplyResult {
        if let Some(reason) = validate_event(&event) {
            return self.rejected(reason);
        }
        if let Some(reason) = stale_reason(&event.occurred_at, now) {
            return self.rejected(reason);
        }
        if self.seen_event_ids.contains(&event.event_id) {
            return self.accepted(None);
        }

        if let Some(parent_id) = normalize_optional(&event.parent_session_id).map(str::to_owned) {
            return self.apply_child_event(event, parent_id);
        }

        let key = session_key(&event.source, &event.session_id);
        let kind = event.kind;
        if matches!(
            kind,
            EventKind::Started | EventKind::Working | EventKind::Idle
        ) && !self.sessions.contains_key(&key)
        {
            if let Some(terminal_id) = normalize_optional(&event.terminal_id).map(str::to_owned) {
                self.retire_other_terminal_sessions(&terminal_id, &key);
            }
        }
        let previous_state = self.sessions.get(&key).map(|record| record.state);
        let result = match kind {
            EventKind::Idle => self.apply_idle(&key, event),
            EventKind::Started | EventKind::Working => self.apply_working(&key, event),
            EventKind::WaitingInput => {
                self.apply_attention(&key, event, "input", Severity::Attention)
            }
            EventKind::PermissionRequested => {
                self.apply_attention(&key, event, "permission", Severity::Attention)
            }
            EventKind::Completed => self.apply_terminal(
                &key,
                event,
                SessionState::Completed,
                "completed",
                Severity::Info,
            ),
            EventKind::Failed => {
                self.apply_terminal(&key, event, SessionState::Failed, "failed", Severity::Error)
            }
            EventKind::Cancelled => self.apply_terminal(
                &key,
                event,
                SessionState::Cancelled,
                "cancelled",
                Severity::Info,
            ),
            EventKind::Closed => self.apply_closed(&key, event),
        };

        if result.reason.is_none() {
            self.remember_event(&result.event_id);
            if kind != EventKind::Closed {
                let current_state = self.sessions.get(&key).map(|record| record.state);
                if previous_state != current_state {
                    if let Some(record) = self.sessions.get(&key).cloned() {
                        self.remember_audit(&record);
                    }
                }
            }
            self.accepted(result.attention)
        } else {
            self.rejected(
                result
                    .reason
                    .unwrap_or_else(|| "invalid_transition".to_owned()),
            )
        }
    }

    pub fn acknowledge(&mut self, source: &str, session_id: &str) -> DockSnapshot {
        if source == "*" && session_id == "*" {
            for record in self.sessions.values_mut() {
                record.acknowledged = true;
            }
        } else if let Some(record) = self.sessions.get_mut(&session_key(source, session_id)) {
            record.acknowledged = true;
        }
        self.snapshot()
    }

    pub fn reset(&mut self, source: &str, session_id: &str) -> DockSnapshot {
        if source == "*" && session_id == "*" {
            self.clear();
            return self.snapshot();
        }
        self.sessions.retain(|_, record| {
            !(source_matches(source, &record.source)
                && session_matches(session_id, &record.session_id))
        });
        self.snapshot()
    }

    pub fn clear(&mut self) {
        self.sessions.clear();
        self.seen_event_ids.clear();
        self.seen_order.clear();
    }

    fn apply_idle(&mut self, key: &str, event: DockEvent) -> TransitionResult {
        if let Some(record) = self.sessions.get_mut(key) {
            update_record(record, &event);
            record.state = SessionState::Idle;
            record.attention_reason = None;
            record.acknowledged = true;
            record.requires_user_action = false;
            return TransitionResult::accepted(event.event_id, None);
        }
        self.sessions.insert(
            key.to_owned(),
            SessionRecord::new(&event, SessionState::Idle, None),
        );
        TransitionResult::accepted(event.event_id, None)
    }

    fn apply_working(&mut self, key: &str, event: DockEvent) -> TransitionResult {
        if let Some(record) = self.sessions.get_mut(key) {
            update_record(record, &event);
            record.state = SessionState::Working;
            record.attention_reason = None;
            record.acknowledged = true;
            return TransitionResult::accepted(event.event_id, None);
        }
        self.sessions.insert(
            key.to_owned(),
            SessionRecord::new(&event, SessionState::Working, None),
        );
        TransitionResult::accepted(event.event_id, None)
    }

    fn apply_attention(
        &mut self,
        key: &str,
        event: DockEvent,
        reason: &str,
        severity: Severity,
    ) -> TransitionResult {
        if let Some(record) = self.sessions.get_mut(key) {
            let already_pending = record.state == SessionState::NeedsAttention
                && record.attention_reason.as_deref() == Some(reason)
                && !record.acknowledged;
            update_record(record, &event);
            record.state = SessionState::NeedsAttention;
            record.attention_reason = Some(reason.to_owned());
            record.acknowledged = false;
            let attention = (!already_pending).then(|| Attention {
                source: event.source.clone(),
                session_id: event.session_id.clone(),
                reason: reason.to_owned(),
                severity,
            });
            return TransitionResult::accepted(event.event_id, attention);
        }
        TransitionResult::accepted(event.event_id, None)
    }

    fn apply_terminal(
        &mut self,
        key: &str,
        event: DockEvent,
        state: SessionState,
        reason: &str,
        severity: Severity,
    ) -> TransitionResult {
        if let Some(record) = self.sessions.get_mut(key) {
            if record.state == state {
                update_record(record, &event);
                return TransitionResult::accepted(event.event_id, None);
            }
            update_record(record, &event);
            record.state = state;
            record.attention_reason = (state != SessionState::Cancelled).then(|| reason.to_owned());
            record.acknowledged = state == SessionState::Cancelled;
            let attention = (state != SessionState::Cancelled).then(|| Attention {
                source: event.source.clone(),
                session_id: event.session_id.clone(),
                reason: reason.to_owned(),
                severity,
            });
            return TransitionResult::accepted(event.event_id, attention);
        }
        TransitionResult::accepted(event.event_id, None)
    }

    fn apply_child_event(&mut self, event: DockEvent, parent_id: String) -> ApplyResult {
        let parent_key = session_key(&event.source, &parent_id);
        let foldable = matches!(
            event.kind,
            EventKind::WaitingInput | EventKind::PermissionRequested | EventKind::Failed
        );
        if !foldable || !self.sessions.contains_key(&parent_key) {
            self.remember_event(&event.event_id);
            return self.accepted(None);
        }

        let previous_state = self.sessions.get(&parent_key).map(|record| record.state);
        let kind = event.kind;
        let mut folded = event;
        folded.session_id = parent_id;
        let result = match kind {
            EventKind::WaitingInput => {
                self.apply_attention(&parent_key, folded, "input", Severity::Attention)
            }
            EventKind::PermissionRequested => {
                self.apply_attention(&parent_key, folded, "permission", Severity::Attention)
            }
            EventKind::Failed => self.apply_terminal(
                &parent_key,
                folded,
                SessionState::Failed,
                "failed",
                Severity::Error,
            ),
            _ => unreachable!("non-foldable child events return earlier"),
        };

        if result.reason.is_none() {
            self.remember_event(&result.event_id);
            let current_state = self.sessions.get(&parent_key).map(|record| record.state);
            if previous_state != current_state {
                if let Some(record) = self.sessions.get(&parent_key).cloned() {
                    self.remember_audit(&record);
                }
            }
            self.accepted(result.attention)
        } else {
            self.rejected(
                result
                    .reason
                    .unwrap_or_else(|| "invalid_transition".to_owned()),
            )
        }
    }

    fn retire_other_terminal_sessions(&mut self, terminal_id: &str, keep_key: &str) {
        let stale: Vec<String> = self
            .sessions
            .iter()
            .filter(|(key, record)| {
                key.as_str() != keep_key && record.terminal_id.as_deref() == Some(terminal_id)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in stale {
            if let Some(record) = self.sessions.remove(&key) {
                self.remember_audit(&record);
            }
        }
    }

    fn apply_closed(&mut self, key: &str, event: DockEvent) -> TransitionResult {
        if let Some(record) = self.sessions.remove(key) {
            self.remember_audit(&record);
        }
        TransitionResult::accepted(event.event_id, None)
    }

    fn accepted(&self, attention: Option<Attention>) -> ApplyResult {
        ApplyResult {
            accepted: true,
            snapshot: self.snapshot(),
            attention,
            rejection_reason: None,
        }
    }

    fn rejected(&self, reason: String) -> ApplyResult {
        ApplyResult {
            accepted: false,
            snapshot: self.snapshot(),
            attention: None,
            rejection_reason: Some(reason),
        }
    }

    fn remember_event(&mut self, event_id: &str) {
        self.seen_event_ids.insert(event_id.to_owned());
        self.seen_order.push_back(event_id.to_owned());
        if self.seen_order.len() > SEEN_EVENT_LIMIT {
            if let Some(oldest) = self.seen_order.pop_front() {
                self.seen_event_ids.remove(&oldest);
            }
        }
    }

    #[allow(dead_code)]
    fn prune_sessions(&mut self) {
        while self.sessions.len() > MAX_SESSIONS {
            let candidate = self
                .sessions
                .iter()
                .filter(|(_, record)| {
                    record.acknowledged
                        && matches!(
                            record.state,
                            SessionState::Completed
                                | SessionState::Failed
                                | SessionState::Cancelled
                        )
                })
                .min_by_key(|(_, record)| record.occurred_at.clone())
                .map(|(key, _)| key.clone())
                .or_else(|| {
                    self.sessions
                        .iter()
                        .filter(|(_, record)| {
                            matches!(
                                record.state,
                                SessionState::Completed
                                    | SessionState::Failed
                                    | SessionState::Cancelled
                            )
                        })
                        .min_by_key(|(_, record)| record.occurred_at.clone())
                        .map(|(key, _)| key.clone())
                });
            let Some(candidate) = candidate else {
                break;
            };
            self.sessions.remove(&candidate);
        }
    }

    fn remember_audit(&mut self, record: &SessionRecord) {
        self.audit.push_back(AuditEntry {
            source: record.source.clone(),
            session_id: record.session_id.clone(),
            state: record.state,
            attention_reason: record.attention_reason.clone(),
            occurred_at: record.occurred_at.clone(),
        });
        while self.audit.len() > MAX_AUDIT_ENTRIES {
            self.audit.pop_front();
        }
    }
}

impl SessionRecord {
    fn new(event: &DockEvent, state: SessionState, reason: Option<&str>) -> Self {
        Self {
            source: event.source.clone(),
            session_id: event.session_id.clone(),
            state,
            attention_reason: reason.map(str::to_owned),
            summary: event.summary.clone(),
            deep_link: event.deep_link.clone(),
            project_path: resolve_project_path(event),
            window_title: nonempty_path(event.window_title.as_deref()),
            requires_user_action: event.requires_user_action.unwrap_or(false),
            acknowledged: reason.is_none(),
            occurred_at: event.occurred_at.clone(),
            terminal_id: normalize_optional(&event.terminal_id).map(str::to_owned),
        }
    }

    fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            source: self.source.clone(),
            session_id: self.session_id.clone(),
            state: self.state,
            mark: self.state.mark().to_owned(),
            attention_reason: self.attention_reason.clone(),
            summary: self.summary.clone(),
            deep_link: self.deep_link.clone(),
            project_path: self.project_path.clone(),
            window_title: self.window_title.clone(),
            terminal_id: self.terminal_id.clone(),
            requires_user_action: self.requires_user_action,
            acknowledged: self.acknowledged,
            occurred_at: self.occurred_at.clone(),
        }
    }
}

fn update_record(record: &mut SessionRecord, event: &DockEvent) {
    record.summary = event.summary.clone().or_else(|| record.summary.clone());
    record.deep_link = event.deep_link.clone().or_else(|| record.deep_link.clone());
    if let Some(path) = resolve_project_path(event) {
        record.project_path = Some(path);
    }
    if let Some(title) = nonempty_path(event.window_title.as_deref()) {
        record.window_title = Some(title);
    }
    if let Some(required) = event.requires_user_action {
        record.requires_user_action = required;
    }
    if let Some(terminal_id) = normalize_optional(&event.terminal_id) {
        record.terminal_id = Some(terminal_id.to_owned());
    }
    record.occurred_at = event.occurred_at.clone();
}

fn resolve_project_path(event: &DockEvent) -> Option<String> {
    nonempty_path(event.workspace_root.as_deref())
        .or_else(|| nonempty_path(event.cwd.as_deref()))
        .or_else(|| nonempty_path(event.metadata.get("workspaceRoot").map(String::as_str)))
        .or_else(|| nonempty_path(event.metadata.get("workspace_root").map(String::as_str)))
        .or_else(|| nonempty_path(event.metadata.get("cwd").map(String::as_str)))
}

fn nonempty_path(value: Option<&str>) -> Option<String> {
    value.and_then(|path| (!path.is_empty()).then(|| path.to_owned()))
}

fn normalize_optional(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn validate_event(event: &DockEvent) -> Option<String> {
    if event.version != EVENT_VERSION {
        return Some("unsupported_version".to_owned());
    }
    if !valid_len(&event.event_id, MAX_EVENT_ID_LEN)
        || !valid_len(&event.source, MAX_SOURCE_LEN)
        || !valid_len(&event.session_id, MAX_SESSION_ID_LEN)
        || event.occurred_at.is_empty()
    {
        return Some("invalid_event".to_owned());
    }
    if let Some(parent) = normalize_optional(&event.parent_session_id) {
        if !valid_len(parent, MAX_SESSION_ID_LEN) {
            return Some("invalid_event".to_owned());
        }
    }
    if let Some(terminal_id) = normalize_optional(&event.terminal_id) {
        if !valid_len(terminal_id, MAX_TERMINAL_ID_LEN) {
            return Some("invalid_event".to_owned());
        }
    }
    if event
        .summary
        .as_ref()
        .is_some_and(|value| value.len() > MAX_SUMMARY_LEN)
        || event
            .deep_link
            .as_ref()
            .is_some_and(|value| value.len() > MAX_DEEP_LINK_LEN)
        || event
            .cwd
            .as_ref()
            .is_some_and(|value| value.len() > MAX_METADATA_VALUE_LEN)
        || event
            .workspace_root
            .as_ref()
            .is_some_and(|value| value.len() > MAX_METADATA_VALUE_LEN)
        || event
            .window_title
            .as_ref()
            .is_some_and(|value| value.len() > MAX_METADATA_VALUE_LEN)
        || event.metadata.len() > MAX_METADATA_ITEMS
        || event.metadata.iter().any(|(key, value)| {
            !valid_len(key, MAX_METADATA_VALUE_LEN) || !valid_len(value, MAX_METADATA_VALUE_LEN)
        })
    {
        return Some("payload_too_large".to_owned());
    }
    if OffsetDateTime::parse(
        &event.occurred_at,
        &time::format_description::well_known::Rfc3339,
    )
    .is_err()
    {
        return Some("invalid_timestamp".to_owned());
    }
    None
}

fn stale_reason(timestamp: &str, now: OffsetDateTime) -> Option<String> {
    let occurred_at =
        OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339).ok()?;
    let age = now - occurred_at;
    if age > MAX_EVENT_AGE || age < -MAX_FUTURE_SKEW {
        Some("stale_event".to_owned())
    } else {
        None
    }
}

fn valid_len(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn session_key(source: &str, session_id: &str) -> String {
    format!("{source}\0{session_id}")
}

fn source_matches(pattern: &str, source: &str) -> bool {
    pattern == "*" || pattern == source
}

fn session_matches(pattern: &str, session_id: &str) -> bool {
    pattern == "*" || pattern == session_id
}

fn session_priority(session: &SessionSnapshot) -> u8 {
    if !session.acknowledged && session.attention_reason.is_some() {
        return 0;
    }
    match session.state {
        SessionState::Working => 1,
        SessionState::NeedsAttention => 2,
        SessionState::Failed => 3,
        SessionState::Completed => 4,
        SessionState::Cancelled => 5,
        SessionState::Idle => 6,
    }
}

struct TransitionResult {
    event_id: String,
    attention: Option<Attention>,
    reason: Option<String>,
}

impl TransitionResult {
    fn accepted(event_id: String, attention: Option<Attention>) -> Self {
        Self {
            event_id,
            attention,
            reason: None,
        }
    }
}

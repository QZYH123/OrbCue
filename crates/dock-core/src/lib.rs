//! Public domain seam for OrbCue.
//!
//! The rest of the application talks to this crate through events and
//! snapshots. Rendering, transport and Agent-specific adapters stay outside
//! this boundary.

mod capture;
mod jump;
mod notify;

pub use capture::{captured_keys_to_drop, sessions_to_capture, CaptureSession, SessionKey};
pub use jump::{
    captured_hwnd_usable, dock_tab_title, dock_terminal_marker, focus_attempts, format_dock_marker,
    is_terminal_window_candidate, process_image_file_name, project_path_hint,
    select_unique_window_title, session_terminal_title, FocusDecision, FocusRequest,
    DOCK_MARKER_HEX_LEN, DOCK_TERMINAL_PREFIX, JUMP_WINDOW_MISSING, TERMINAL_PROCESS_NAMES,
    TERMINAL_WINDOW_CLASSES,
};
pub use notify::{
    attention_click_followup, attention_jump, dispatch_attention_toast, highlight_target,
    AttentionClickFollowup, AttentionJump, HighlightTarget, NotificationSink, ToastDispatch,
    ToastSpec,
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
    Closed,
}

impl SessionState {
    pub fn is_audit_event(self) -> bool {
        !matches!(self, Self::Working | Self::Idle)
    }

    pub fn mark(self) -> &'static str {
        match self {
            Self::Working => "",
            Self::Idle => "o",
            Self::Completed => "*",
            Self::Cancelled => "x",
            Self::NeedsAttention => "?",
            Self::Failed => "!",
            Self::Closed => "",
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_starttime: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_wsl_distro: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLiveness {
    pub os: String,
    pub pid: u32,
    pub starttime: u64,
    pub distro: Option<String>,
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
    liveness: Option<AgentLiveness>,
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
            let project_path = normalize_optional(&item.project_path)
                .filter(|value| valid_len(value, MAX_METADATA_VALUE_LEN))
                .map(str::to_owned);
            let liveness = complete_liveness(
                item.agent_os.clone(),
                item.agent_pid,
                item.agent_starttime,
                item.agent_wsl_distro.clone(),
            );
            let key = persist_instance_key(
                &state,
                &item.source,
                &item.session_id,
                terminal_id.as_deref(),
            );
            state.sessions.insert(
                key,
                SessionRecord {
                    source: item.source,
                    session_id: item.session_id,
                    state: item.state,
                    attention_reason: item.attention_reason,
                    summary: None,
                    deep_link: None,
                    project_path,
                    window_title: None,
                    requires_user_action: item.requires_user_action,
                    acknowledged: item.acknowledged,
                    occurred_at: item.occurred_at,
                    terminal_id,
                    liveness,
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
                project_path: record.project_path.clone(),
                agent_os: record.liveness.as_ref().map(|item| item.os.clone()),
                agent_pid: record.liveness.as_ref().map(|item| item.pid),
                agent_starttime: record.liveness.as_ref().map(|item| item.starttime),
                agent_wsl_distro: record
                    .liveness
                    .as_ref()
                    .and_then(|item| item.distro.clone()),
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

    pub fn liveness_targets(&self) -> Vec<(String, String, AgentLiveness)> {
        self.sessions
            .values()
            .filter_map(|record| {
                record
                    .liveness
                    .clone()
                    .map(|liveness| (record.source.clone(), record.session_id.clone(), liveness))
            })
            .collect()
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
        let tracked_count = sessions.len();
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

        let event_id = event.event_id.clone();
        let kind = event.kind;
        let creating = matches!(
            kind,
            EventKind::Started | EventKind::Working | EventKind::Idle
        );
        let key = self.resolve_session_key(&event, creating);
        if creating && !self.sessions.contains_key(&key) {
            if let Some(terminal_id) = normalize_optional(&event.terminal_id).map(str::to_owned) {
                if self.ignore_nested_start_on_running_terminal(&terminal_id, &event) {
                    self.remember_event(&event.event_id);
                    return self.accepted(None);
                }
                self.retire_other_terminal_sessions(&terminal_id, &key);
            }
        }
        let previous_state = self.sessions.get(&key).map(|record| record.state);
        let attention = match kind {
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
        self.finish_transition(&event_id, kind, &key, previous_state, attention)
    }

    pub fn acknowledge(
        &mut self,
        source: &str,
        session_id: &str,
        terminal_id: Option<&str>,
    ) -> DockSnapshot {
        if source == "*" && session_id == "*" {
            for record in self.sessions.values_mut() {
                record.acknowledged = true;
            }
        } else {
            for record in self.sessions.values_mut() {
                if instance_matches(record, source, session_id, terminal_id) {
                    record.acknowledged = true;
                }
            }
        }
        self.snapshot()
    }

    pub fn reset(
        &mut self,
        source: &str,
        session_id: &str,
        terminal_id: Option<&str>,
    ) -> DockSnapshot {
        if source == "*" && session_id == "*" {
            self.clear();
            return self.snapshot();
        }
        self.sessions
            .retain(|_, record| !instance_matches(record, source, session_id, terminal_id));
        self.snapshot()
    }

    pub fn clear(&mut self) {
        self.sessions.clear();
        self.seen_event_ids.clear();
        self.seen_order.clear();
    }

    fn apply_idle(&mut self, key: &str, event: DockEvent) -> Option<Attention> {
        self.apply_open(key, event, SessionState::Idle, true)
    }

    fn apply_working(&mut self, key: &str, event: DockEvent) -> Option<Attention> {
        self.apply_open(key, event, SessionState::Working, false)
    }

    fn apply_open(
        &mut self,
        key: &str,
        event: DockEvent,
        state: SessionState,
        clear_user_action: bool,
    ) -> Option<Attention> {
        if let Some(record) = self.sessions.get_mut(key) {
            update_record(record, &event);
            record.state = state;
            record.attention_reason = None;
            record.acknowledged = true;
            if clear_user_action {
                record.requires_user_action = false;
            }
            return None;
        }
        self.sessions
            .insert(key.to_owned(), SessionRecord::new(&event, state, None));
        None
    }

    fn apply_attention(
        &mut self,
        key: &str,
        event: DockEvent,
        reason: &str,
        severity: Severity,
    ) -> Option<Attention> {
        let record = self.sessions.get_mut(key)?;
        let already_pending = record.state == SessionState::NeedsAttention
            && record.attention_reason.as_deref() == Some(reason)
            && !record.acknowledged;
        update_record(record, &event);
        record.state = SessionState::NeedsAttention;
        record.attention_reason = Some(reason.to_owned());
        record.acknowledged = false;
        (!already_pending).then(|| Attention {
            source: event.source.clone(),
            session_id: event.session_id.clone(),
            reason: reason.to_owned(),
            severity,
        })
    }

    fn apply_terminal(
        &mut self,
        key: &str,
        event: DockEvent,
        state: SessionState,
        reason: &str,
        severity: Severity,
    ) -> Option<Attention> {
        let record = self.sessions.get_mut(key)?;
        if record.state == state {
            update_record(record, &event);
            return None;
        }
        update_record(record, &event);
        record.state = state;
        record.attention_reason = (state != SessionState::Cancelled).then(|| reason.to_owned());
        record.acknowledged = state == SessionState::Cancelled;
        (state != SessionState::Cancelled).then(|| Attention {
            source: event.source.clone(),
            session_id: event.session_id.clone(),
            reason: reason.to_owned(),
            severity,
        })
    }

    fn apply_child_event(&mut self, event: DockEvent, parent_id: String) -> ApplyResult {
        let foldable = matches!(
            event.kind,
            EventKind::WaitingInput | EventKind::PermissionRequested | EventKind::Failed
        );
        if !foldable {
            self.remember_event(&event.event_id);
            return self.accepted(None);
        }
        let mut parent_probe = event.clone();
        parent_probe.session_id = parent_id.clone();
        let parent_key = self.resolve_session_key(&parent_probe, false);
        if !self.sessions.contains_key(&parent_key) {
            self.remember_event(&event.event_id);
            return self.accepted(None);
        }

        let previous_state = self.sessions.get(&parent_key).map(|record| record.state);
        let event_id = event.event_id.clone();
        let kind = event.kind;
        let mut folded = event;
        folded.session_id = parent_id;
        let attention = match kind {
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
        self.finish_transition(&event_id, kind, &parent_key, previous_state, attention)
    }

    fn resolve_session_key(&self, event: &DockEvent, for_create: bool) -> String {
        let base = session_key(&event.source, &event.session_id);
        if let Some(live) = liveness_from_event(event) {
            if let Some(key) = self.sessions.iter().find_map(|(key, record)| {
                (record.source == event.source
                    && record.session_id == event.session_id
                    && record
                        .liveness
                        .as_ref()
                        .is_some_and(|existing| same_liveness(existing, &live)))
                .then(|| key.clone())
            }) {
                return key;
            }
        }
        if let Some(terminal_id) = normalize_optional(&event.terminal_id) {
            if let Some((key, record)) = self.sessions.iter().find(|(_, record)| {
                record.source == event.source
                    && record.session_id == event.session_id
                    && record.terminal_id.as_deref() == Some(terminal_id)
            }) {
                if for_create && should_fork_resume(record, event) {
                    return fork_resume_key(event, &base);
                }
                return key.clone();
            }
        }
        let matches: Vec<(&String, &SessionRecord)> = self
            .sessions
            .iter()
            .filter(|(_, record)| {
                record.source == event.source && record.session_id == event.session_id
            })
            .collect();
        if matches.len() == 1 {
            if for_create && should_fork_resume(matches[0].1, event) {
                return fork_resume_key(event, &base);
            }
            if !for_create && !event_fits_instance(matches[0].1, event) {
                return String::new();
            }
            return matches[0].0.clone();
        }
        if for_create && !matches.is_empty() {
            return fork_resume_key(event, &base);
        }
        if !for_create {
            return String::new();
        }
        base
    }

    fn ignore_nested_start_on_running_terminal(
        &self,
        terminal_id: &str,
        incoming: &DockEvent,
    ) -> bool {
        if resolve_project_path(incoming).is_some() {
            return false;
        }
        self.sessions.values().any(|record| {
            record.terminal_id.as_deref() == Some(terminal_id)
                && record.project_path.is_some()
                && matches!(
                    record.state,
                    SessionState::Working | SessionState::NeedsAttention
                )
        })
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
                self.remember_closed_audit(record, None);
            }
        }
    }

    fn apply_closed(&mut self, key: &str, event: DockEvent) -> Option<Attention> {
        if let Some(record) = self.sessions.remove(key) {
            self.remember_closed_audit(record, Some(event.occurred_at.clone()));
        }
        None
    }

    fn finish_transition(
        &mut self,
        event_id: &str,
        kind: EventKind,
        key: &str,
        previous_state: Option<SessionState>,
        attention: Option<Attention>,
    ) -> ApplyResult {
        self.remember_event(event_id);
        if kind != EventKind::Closed {
            self.audit_if_changed(key, previous_state);
        }
        self.accepted(attention)
    }

    fn audit_if_changed(&mut self, key: &str, previous_state: Option<SessionState>) {
        let current_state = self.sessions.get(key).map(|record| record.state);
        if previous_state != current_state {
            if let Some(record) = self.sessions.get(key).cloned() {
                self.remember_audit(&record);
            }
        }
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

    fn remember_closed_audit(&mut self, mut record: SessionRecord, occurred_at: Option<String>) {
        record.state = SessionState::Closed;
        record.attention_reason = None;
        if let Some(occurred_at) = occurred_at {
            record.occurred_at = occurred_at;
        }
        self.remember_audit(&record);
    }

    fn remember_audit(&mut self, record: &SessionRecord) {
        if !record.state.is_audit_event() {
            return;
        }
        self.audit.push_back(AuditEntry {
            source: record.source.clone(),
            session_id: record.session_id.clone(),
            state: record.state,
            attention_reason: record.attention_reason.clone(),
            occurred_at: record.occurred_at.clone(),
            project_path: record.project_path.clone(),
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
            liveness: liveness_from_event(event),
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
    record.liveness = merge_liveness(record.liveness.take(), liveness_from_event(event));
    record.occurred_at = event.occurred_at.clone();
}

fn merge_liveness(
    existing: Option<AgentLiveness>,
    incoming: Option<AgentLiveness>,
) -> Option<AgentLiveness> {
    match (existing, incoming) {
        (None, incoming) => incoming,
        (existing, None) => existing,
        (Some(old), Some(new)) if old.pid == new.pid && old.starttime == new.starttime => {
            Some(AgentLiveness {
                os: new.os,
                pid: old.pid,
                starttime: old.starttime,
                distro: new.distro.or(old.distro),
            })
        }
        (Some(old), Some(_)) => Some(old),
    }
}

fn liveness_from_event(event: &DockEvent) -> Option<AgentLiveness> {
    if normalize_optional(&event.parent_session_id).is_some() {
        return None;
    }
    complete_liveness(
        event.metadata.get("agent_os").cloned(),
        event
            .metadata
            .get("agent_pid")
            .and_then(|value| value.parse().ok()),
        event
            .metadata
            .get("agent_starttime")
            .and_then(|value| value.parse().ok()),
        event.metadata.get("agent_wsl_distro").cloned(),
    )
}

fn complete_liveness(
    os: Option<String>,
    pid: Option<u32>,
    starttime: Option<u64>,
    distro: Option<String>,
) -> Option<AgentLiveness> {
    let os = normalize_optional(&os)?.to_owned();
    if os != "linux" && os != "windows" {
        return None;
    }
    Some(AgentLiveness {
        os,
        pid: pid?,
        starttime: starttime?,
        distro: normalize_optional(&distro).map(str::to_owned),
    })
}

/// SHA-256 prefix so a 256-byte session_id still fits `MAX_EVENT_ID_LEN`.
pub fn liveness_closed_event_id(
    source: &str,
    session_id: &str,
    pid: u32,
    starttime: u64,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    hasher.update([0x1f]);
    hasher.update(session_id.as_bytes());
    hasher.update([0x1f]);
    hasher.update(pid.to_string().as_bytes());
    hasher.update([0x1f]);
    hasher.update(starttime.to_string().as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("orb-liveness-{hex}")
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

fn persist_instance_key(
    state: &DockState,
    source: &str,
    session_id: &str,
    terminal_id: Option<&str>,
) -> String {
    let base = session_key(source, session_id);
    match terminal_id {
        Some(terminal_id) if state.sessions.contains_key(&base) => {
            format!("{base}\0{terminal_id}")
        }
        _ => base,
    }
}

fn fork_resume_key(event: &DockEvent, base: &str) -> String {
    if let Some(terminal_id) = normalize_optional(&event.terminal_id) {
        return format!("{base}\0{terminal_id}");
    }
    if let Some(live) = liveness_from_event(event) {
        return format!("{base}\0{}:{}", live.pid, live.starttime);
    }
    base.to_owned()
}

fn should_fork_resume(existing: &SessionRecord, incoming: &DockEvent) -> bool {
    let Some(existing_live) = existing.liveness.as_ref() else {
        return false;
    };
    let Some(incoming_live) = liveness_from_event(incoming) else {
        return false;
    };
    !same_liveness(existing_live, &incoming_live)
}

fn event_fits_instance(record: &SessionRecord, event: &DockEvent) -> bool {
    if let Some(live) = liveness_from_event(event) {
        if record
            .liveness
            .as_ref()
            .is_some_and(|existing| !same_liveness(existing, &live))
        {
            return false;
        }
    }
    if let Some(terminal_id) = normalize_optional(&event.terminal_id) {
        if record
            .terminal_id
            .as_deref()
            .is_some_and(|existing| existing != terminal_id)
        {
            return false;
        }
    }
    true
}

fn same_liveness(existing: &AgentLiveness, incoming: &AgentLiveness) -> bool {
    existing.os == incoming.os
        && existing.pid == incoming.pid
        && existing.starttime == incoming.starttime
}

fn source_matches(pattern: &str, source: &str) -> bool {
    pattern == "*" || pattern == source
}

fn session_matches(pattern: &str, session_id: &str) -> bool {
    pattern == "*" || pattern == session_id
}

fn instance_matches(
    record: &SessionRecord,
    source: &str,
    session_id: &str,
    terminal_id: Option<&str>,
) -> bool {
    if !source_matches(source, &record.source) || !session_matches(session_id, &record.session_id) {
        return false;
    }
    match terminal_id
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "*")
    {
        Some(terminal_id) => record.terminal_id.as_deref() == Some(terminal_id),
        None => true,
    }
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
        SessionState::Closed => 7,
    }
}

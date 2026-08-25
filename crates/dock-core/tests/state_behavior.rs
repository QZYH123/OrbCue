use agent_activity_dock_core::{DockEvent, DockState, EventKind, SessionState};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

fn event(id: &str, kind: EventKind, session: &str) -> DockEvent {
    DockEvent::new(id, kind, "claude", session)
}

fn attach_liveness(event: &mut DockEvent, pid: u32, starttime: u64) {
    event
        .metadata
        .insert("agent_os".to_owned(), "linux".to_owned());
    event
        .metadata
        .insert("agent_pid".to_owned(), pid.to_string());
    event
        .metadata
        .insert("agent_starttime".to_owned(), starttime.to_string());
}

#[test]
fn a_session_lifecycle_updates_the_aggregate_and_notifies_once() {
    let mut state = DockState::new();

    let started = state.apply(event("e1", EventKind::Started, "s1"));
    assert!(started.accepted);
    assert_eq!(started.snapshot.working_count, 1);
    assert_eq!(started.snapshot.tracked_count, 1);
    assert_eq!(started.snapshot.sessions[0].state, SessionState::Working);
    assert!(started.attention.is_none());

    let completed = state.apply(event("e2", EventKind::Completed, "s1"));
    assert!(completed.accepted);
    assert_eq!(completed.snapshot.working_count, 0);
    assert_eq!(completed.snapshot.tracked_count, 1);
    assert_eq!(completed.snapshot.pending_count, 1);
    assert_eq!(completed.snapshot.pending_mark, "*");
    assert_eq!(completed.snapshot.sessions[0].mark, "*");
    assert_eq!(
        completed.snapshot.sessions[0].state,
        SessionState::Completed
    );
    assert_eq!(completed.snapshot.audit.len(), 1);
    assert_eq!(completed.snapshot.audit[0].state, SessionState::Completed);
    assert_eq!(completed.attention.as_ref().unwrap().reason, "completed");

    let duplicate = state.apply(event("e3", EventKind::Completed, "s1"));
    assert!(duplicate.accepted);
    assert!(duplicate.attention.is_none());
    assert_eq!(duplicate.snapshot.pending_count, 1);

    let resumed = state.apply(event("e4", EventKind::Working, "s1"));
    assert!(resumed.accepted);
    assert_eq!(resumed.snapshot.working_count, 1);
    assert_eq!(resumed.snapshot.tracked_count, 1);
    assert_eq!(resumed.snapshot.pending_count, 0);

    let closed = state.apply(event("e5", EventKind::Closed, "s1"));
    assert!(closed.accepted);
    assert_eq!(closed.snapshot.working_count, 0);
    assert_eq!(closed.snapshot.tracked_count, 0);
    assert!(closed.snapshot.sessions.is_empty());
    assert_eq!(
        closed.snapshot.audit.last().map(|entry| entry.state),
        Some(SessionState::Closed)
    );
    assert_eq!(
        closed
            .snapshot
            .audit
            .last()
            .map(|entry| entry.session_id.as_str()),
        Some("s1")
    );
}

#[test]
fn idle_sessions_are_tracked_without_counting_as_working() {
    let mut state = DockState::new();
    let idle = state.apply(event("e1", EventKind::Idle, "s1"));
    assert!(idle.accepted);
    assert_eq!(idle.snapshot.working_count, 0);
    assert_eq!(idle.snapshot.tracked_count, 1);
    assert_eq!(idle.snapshot.pending_count, 1);
    assert_eq!(idle.snapshot.pending_mark, "o");
    assert_eq!(idle.snapshot.sessions[0].state, SessionState::Idle);
    assert_eq!(idle.snapshot.sessions[0].mark, "o");
    assert!(idle.attention.is_none());

    let working = state.apply(event("e2", EventKind::Working, "s1"));
    assert!(working.accepted);
    assert_eq!(working.snapshot.working_count, 1);
    assert_eq!(working.snapshot.tracked_count, 1);
    assert_eq!(working.snapshot.sessions[0].state, SessionState::Working);
}

#[test]
fn multiple_sessions_are_counted_without_duplicate_starts() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "s1"));
    state.apply(event("e2", EventKind::Started, "s2"));
    let duplicate_start = state.apply(event("e3", EventKind::Started, "s1"));

    assert_eq!(duplicate_start.snapshot.working_count, 2);
    assert_eq!(duplicate_start.snapshot.tracked_count, 2);
}

#[test]
fn waiting_for_permission_is_distinct_and_acknowledgeable() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "s1"));

    let waiting = state.apply(
        event("e2", EventKind::PermissionRequested, "s1")
            .requiring_user_action(true)
            .with_summary("Permission required"),
    );
    assert_eq!(waiting.snapshot.working_count, 0);
    assert_eq!(waiting.snapshot.tracked_count, 1);
    assert_eq!(waiting.snapshot.pending_count, 1);
    assert_eq!(waiting.snapshot.pending_mark, "?");
    assert_eq!(waiting.snapshot.sessions[0].mark, "?");
    assert_eq!(
        waiting.snapshot.sessions[0].state,
        SessionState::NeedsAttention
    );
    assert_eq!(
        waiting.snapshot.sessions[0].attention_reason.as_deref(),
        Some("permission")
    );
    assert!(waiting.snapshot.sessions[0].requires_user_action);
    assert_eq!(waiting.attention.as_ref().unwrap().reason, "permission");

    let repeated = state.apply(event("e3", EventKind::PermissionRequested, "s1"));
    assert!(repeated.attention.is_none());
    assert_eq!(repeated.snapshot.pending_count, 1);

    let acknowledged = state.acknowledge("claude", "s1", None);
    assert_eq!(acknowledged.pending_count, 1);
    assert_eq!(acknowledged.pending_mark, "?");
    assert!(acknowledged.sessions[0].requires_user_action);
}

#[test]
fn restart_state_excludes_ephemeral_content_and_does_not_replay_attention() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started.workspace_root = Some("/home/qingz/projects/agent-activity-dock".to_owned());
    started.cwd = Some("/home/qingz/projects/agent-activity-dock".to_owned());
    started.window_title = Some("Secret Title".to_owned());
    state.apply(started);
    state.apply(event("e2", EventKind::Failed, "s1").with_summary("private failure details"));

    let json = serde_json::to_string(&state.persisted()).unwrap();
    assert!(!json.contains("private failure details"));
    assert!(!json.contains("summary"));
    assert!(!json.contains("transcript"));
    assert!(!json.contains("window_title"));
    assert!(!json.contains("Secret Title"));
    assert!(json.contains("/home/qingz/projects/agent-activity-dock"));

    let restored = DockState::from_persisted(serde_json::from_str(&json).unwrap());
    assert_eq!(restored.snapshot().pending_count, 1);
    assert_eq!(restored.snapshot().pending_mark, "!");
    assert_eq!(restored.snapshot().sessions[0].state, SessionState::Failed);
    assert_eq!(restored.snapshot().sessions[0].mark, "!");
    assert!(restored.snapshot().sessions[0].summary.is_none());
    assert_eq!(
        restored.snapshot().sessions[0].project_path.as_deref(),
        Some("/home/qingz/projects/agent-activity-dock")
    );
    assert!(restored.snapshot().sessions[0].window_title.is_none());
}

#[test]
fn old_state_files_without_project_path_still_load() {
    let restored = DockState::from_persisted(serde_json::from_str(
        r#"{"version":1,"sessions":[{"source":"grok","session_id":"s1","state":"idle","attention_reason":null,"requires_user_action":false,"acknowledged":true,"occurred_at":"2026-08-23T00:00:00Z","terminal_id":"dock:ab12cd"}]}"#,
    ).unwrap());
    assert_eq!(
        restored.snapshot().sessions[0].terminal_id.as_deref(),
        Some("dock:ab12cd")
    );
    assert_eq!(restored.snapshot().sessions[0].project_path, None);
}

#[test]
fn stale_and_future_events_are_rejected_without_touching_state() {
    let now = OffsetDateTime::parse("2026-08-16T00:00:00Z", &Rfc3339).unwrap();
    let old = DockEvent::new("old", EventKind::Started, "claude", "s1")
        .with_occurred_at("2026-08-14T23:59:59Z");
    let future = DockEvent::new("future", EventKind::Started, "claude", "s2")
        .with_occurred_at("2026-08-16T00:10:00Z");

    let mut state = DockState::new();
    assert_eq!(
        state.apply_at(old, now).rejection_reason.as_deref(),
        Some("stale_event")
    );
    assert_eq!(
        state.apply_at(future, now).rejection_reason.as_deref(),
        Some("stale_event")
    );
    assert_eq!(state.snapshot().tracked_count, 0);
}

#[test]
fn malformed_timestamp_and_oversized_metadata_are_rejected() {
    let malformed = DockEvent::new("bad-time", EventKind::Started, "claude", "s1")
        .with_occurred_at("not-a-timestamp");
    let mut oversized = DockEvent::new("bad-meta", EventKind::Started, "claude", "s2");
    oversized
        .metadata
        .insert("x".repeat(257), "value".to_owned());
    let mut state = DockState::new();

    assert_eq!(
        state.apply(malformed).rejection_reason.as_deref(),
        Some("invalid_timestamp")
    );
    assert_eq!(
        state.apply(oversized).rejection_reason.as_deref(),
        Some("payload_too_large")
    );
    assert_eq!(state.snapshot().tracked_count, 0);
}

#[test]
fn project_path_comes_from_workspace_root() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started.workspace_root = Some("/tmp/workspace".to_owned());
    started.cwd = Some("/tmp/cwd-should-lose".to_owned());
    let result = state.apply(started);
    assert!(result.accepted);
    assert_eq!(
        result.snapshot.sessions[0].project_path.as_deref(),
        Some("/tmp/workspace")
    );
}

#[test]
fn project_path_falls_back_to_cwd_when_workspace_root_is_absent() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started.cwd = Some("/tmp/cwd-only".to_owned());
    let result = state.apply(started);
    assert!(result.accepted);
    assert_eq!(
        result.snapshot.sessions[0].project_path.as_deref(),
        Some("/tmp/cwd-only")
    );
}

#[test]
fn project_path_falls_back_to_metadata_workspace_root() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started
        .metadata
        .insert("workspaceRoot".to_owned(), "/tmp/from-meta".to_owned());
    let result = state.apply(started);
    assert!(result.accepted);
    assert_eq!(
        result.snapshot.sessions[0].project_path.as_deref(),
        Some("/tmp/from-meta")
    );
}

#[test]
fn sessions_without_a_path_keep_project_path_none_and_still_apply() {
    let mut state = DockState::new();
    let result = state.apply(event("e1", EventKind::Started, "s1"));
    assert!(result.accepted);
    assert_eq!(result.snapshot.sessions[0].project_path, None);
}

#[test]
fn different_project_paths_do_not_change_working_or_tracked_counts() {
    let mut state = DockState::new();
    let mut first = event("e1", EventKind::Started, "s1");
    first.workspace_root = Some("/proj/alpha".to_owned());
    let mut second = event("e2", EventKind::Started, "s2");
    second.cwd = Some("/proj/beta".to_owned());
    state.apply(first);
    let result = state.apply(second);
    assert_eq!(result.snapshot.working_count, 2);
    assert_eq!(result.snapshot.tracked_count, 2);
    assert_eq!(result.snapshot.pending_count, 0);
    assert_eq!(result.snapshot.count_label(), "2/2");
    assert_eq!(result.snapshot.pending_mark, "");

    let completed = state.apply(event("e3", EventKind::Completed, "s1"));
    assert_eq!(completed.snapshot.working_count, 1);
    assert_eq!(completed.snapshot.tracked_count, 2);
    assert_eq!(completed.snapshot.pending_count, 1);
}

#[test]
fn later_events_keep_project_path_unless_a_new_value_arrives() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started.cwd = Some("/tmp/first".to_owned());
    state.apply(started);

    let kept = state.apply(event("e2", EventKind::Working, "s1"));
    assert_eq!(
        kept.snapshot.sessions[0].project_path.as_deref(),
        Some("/tmp/first")
    );

    let mut moved = event("e3", EventKind::Idle, "s1");
    moved.workspace_root = Some("/tmp/second".to_owned());
    let moved = state.apply(moved);
    assert_eq!(
        moved.snapshot.sessions[0].project_path.as_deref(),
        Some("/tmp/second")
    );
}

#[test]
fn empty_path_fields_are_treated_as_missing() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started.workspace_root = Some(String::new());
    started.cwd = Some("/tmp/real".to_owned());
    let result = state.apply(started);
    assert!(result.accepted);
    assert_eq!(
        result.snapshot.sessions[0].project_path.as_deref(),
        Some("/tmp/real")
    );
}

#[test]
fn oversized_cwd_is_rejected_as_payload_too_large() {
    let mut oversized = event("e1", EventKind::Started, "s1");
    oversized.cwd = Some("x".repeat(257));
    let mut state = DockState::new();
    assert_eq!(
        state.apply(oversized).rejection_reason.as_deref(),
        Some("payload_too_large")
    );
    assert_eq!(state.snapshot().tracked_count, 0);
}

#[test]
fn window_title_appears_on_the_session_snapshot() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started.window_title = Some("Windows Terminal - dock".to_owned());
    let result = state.apply(started);
    assert!(result.accepted);
    assert_eq!(
        result.snapshot.sessions[0].window_title.as_deref(),
        Some("Windows Terminal - dock")
    );
}

#[test]
fn sessions_without_location_fields_keep_them_empty() {
    let mut state = DockState::new();
    let result = state.apply(event("e1", EventKind::Started, "s1"));
    assert!(result.accepted);
    assert_eq!(result.snapshot.sessions[0].deep_link, None);
    assert_eq!(result.snapshot.sessions[0].window_title, None);
    assert_eq!(result.snapshot.sessions[0].project_path, None);
}

#[test]
fn empty_window_title_is_treated_as_missing() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started.window_title = Some(String::new());
    let result = state.apply(started);
    assert!(result.accepted);
    assert_eq!(result.snapshot.sessions[0].window_title, None);
}

#[test]
fn oversized_window_title_is_rejected_as_payload_too_large() {
    let mut oversized = event("e1", EventKind::Started, "s1");
    oversized.window_title = Some("x".repeat(257));
    let mut state = DockState::new();
    assert_eq!(
        state.apply(oversized).rejection_reason.as_deref(),
        Some("payload_too_large")
    );
    assert_eq!(state.snapshot().tracked_count, 0);
}

#[test]
fn later_events_keep_window_title_unless_a_new_value_arrives() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started.window_title = Some("first-title".to_owned());
    state.apply(started);
    let kept = state.apply(event("e2", EventKind::Working, "s1"));
    assert_eq!(
        kept.snapshot.sessions[0].window_title.as_deref(),
        Some("first-title")
    );
    let mut renamed = event("e3", EventKind::Idle, "s1");
    renamed.window_title = Some("second-title".to_owned());
    let renamed = state.apply(renamed);
    assert_eq!(
        renamed.snapshot.sessions[0].window_title.as_deref(),
        Some("second-title")
    );
}

#[test]
fn unknown_session_attention_and_terminal_do_not_create_records() {
    let mut state = DockState::new();
    for (id, kind) in [
        ("e1", EventKind::WaitingInput),
        ("e2", EventKind::PermissionRequested),
        ("e3", EventKind::Completed),
        ("e4", EventKind::Failed),
        ("e5", EventKind::Cancelled),
    ] {
        let result = state.apply(event(id, kind, "unknown"));
        assert!(result.accepted, "{kind:?} should be accepted");
        assert!(
            result.attention.is_none(),
            "{kind:?} should have no attention"
        );
        assert_eq!(result.snapshot.tracked_count, 0);
        assert!(result.snapshot.sessions.is_empty());
    }
}

#[test]
fn reset_then_late_completed_does_not_resurrect() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "a"));
    state.reset("claude", "a", None);
    let late = state.apply(event("e2", EventKind::Completed, "a"));
    assert!(late.accepted);
    assert!(late.attention.is_none());
    assert_eq!(late.snapshot.tracked_count, 0);
    assert!(late
        .snapshot
        .sessions
        .iter()
        .all(|session| session.session_id != "a"));
}

#[test]
fn parent_waiting_folds_into_parent_attention() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "parent"));
    let result =
        state.apply(event("e2", EventKind::WaitingInput, "child").with_parent_session_id("parent"));
    assert!(result.accepted);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.sessions.len(), 1);
    assert_eq!(result.snapshot.sessions[0].session_id, "parent");
    assert_eq!(result.snapshot.sessions[0].mark, "?");
    assert_eq!(result.snapshot.pending_mark, "?");
    assert_eq!(
        result
            .attention
            .as_ref()
            .map(|item| item.session_id.as_str()),
        Some("parent")
    );
    assert_eq!(
        result.attention.as_ref().map(|item| item.reason.as_str()),
        Some("input")
    );
}

#[test]
fn missing_parent_child_event_has_no_side_effects() {
    let mut state = DockState::new();
    let result = state.apply(
        event("e1", EventKind::WaitingInput, "child").with_parent_session_id("missing-parent"),
    );
    assert!(result.accepted);
    assert!(result.attention.is_none());
    assert_eq!(result.snapshot.tracked_count, 0);
    assert!(result.snapshot.sessions.is_empty());
}

#[test]
fn child_sessions_are_not_counted() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "parent"));
    let result =
        state.apply(event("e2", EventKind::Started, "child").with_parent_session_id("parent"));
    assert!(result.accepted);
    assert_eq!(result.snapshot.working_count, 1);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.sessions.len(), 1);
    assert_eq!(result.snapshot.sessions[0].session_id, "parent");
}

#[test]
fn empty_parent_session_id_is_treated_as_missing() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started.parent_session_id = Some(String::new());
    let result = state.apply(started);
    assert!(result.accepted);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.sessions[0].session_id, "s1");
}

#[test]
fn parent_failed_folds_into_parent_failed_mark() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "parent"));
    let result =
        state.apply(event("e2", EventKind::Failed, "child").with_parent_session_id("parent"));
    assert!(result.accepted);
    assert_eq!(result.snapshot.sessions[0].state, SessionState::Failed);
    assert_eq!(result.snapshot.pending_mark, "!");
    assert_eq!(result.attention.as_ref().unwrap().reason, "failed");
}

#[test]
fn parent_waiting_reuses_attention_dedup() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "parent"));
    let first =
        state.apply(event("e2", EventKind::WaitingInput, "child").with_parent_session_id("parent"));
    assert!(first.attention.is_some());
    let repeat = state.apply(
        event("e3", EventKind::WaitingInput, "other-child").with_parent_session_id("parent"),
    );
    assert!(repeat.attention.is_none());
    assert_eq!(repeat.snapshot.tracked_count, 1);
}

#[test]
fn oversized_parent_session_id_is_rejected() {
    let mut oversized = event("e1", EventKind::Started, "s1");
    oversized.parent_session_id = Some("x".repeat(257));
    let mut state = DockState::new();
    assert_eq!(
        state.apply(oversized).rejection_reason.as_deref(),
        Some("invalid_event")
    );
    assert_eq!(state.snapshot().tracked_count, 0);
}

#[test]
fn same_terminal_id_started_replaces_the_previous_session() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "old").with_terminal_id("pts-1"));
    let replaced = state.apply(event("e2", EventKind::Idle, "fresh").with_terminal_id("pts-1"));
    assert!(replaced.accepted);
    assert_eq!(replaced.snapshot.tracked_count, 1);
    assert_eq!(replaced.snapshot.sessions.len(), 1);
    assert_eq!(replaced.snapshot.sessions[0].session_id, "fresh");
    assert_eq!(replaced.snapshot.sessions[0].mark, "o");
}

#[test]
fn same_terminal_id_replaces_across_sources() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "claude-s").with_terminal_id("term"));
    let grok = DockEvent::new("e2", EventKind::Idle, "grok", "grok-s").with_terminal_id("term");
    let replaced = state.apply(grok);
    assert_eq!(replaced.snapshot.sessions.len(), 1);
    assert_eq!(replaced.snapshot.sessions[0].source, "grok");
    assert_eq!(replaced.snapshot.sessions[0].session_id, "grok-s");
}

#[test]
fn nested_pathless_start_does_not_steal_a_working_terminal_session() {
    let mut state = DockState::new();
    let mut grok = DockEvent::new("e1", EventKind::Working, "grok", "grok-s");
    grok.cwd = Some("/tmp/project".to_owned());
    state.apply(grok.with_terminal_id("term"));

    let nested =
        DockEvent::new("e2", EventKind::Started, "codex", "codex-version").with_terminal_id("term");
    let result = state.apply(nested);
    assert!(result.accepted);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.working_count, 1);
    assert_eq!(result.snapshot.sessions[0].source, "grok");
    assert_eq!(result.snapshot.sessions[0].session_id, "grok-s");
    assert_eq!(result.snapshot.sessions[0].state, SessionState::Working);

    let done = DockEvent::new("e3", EventKind::Completed, "codex", "codex-version")
        .with_terminal_id("term");
    let result = state.apply(done);
    assert!(result.accepted);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.sessions[0].source, "grok");
    assert_eq!(result.snapshot.sessions[0].state, SessionState::Working);
}

#[test]
fn nested_pathless_start_does_not_steal_a_waiting_terminal_session() {
    let mut state = DockState::new();
    let mut grok = DockEvent::new("e1", EventKind::Working, "grok", "grok-s");
    grok.cwd = Some("/tmp/project".to_owned());
    state.apply(grok.with_terminal_id("term"));
    state.apply(DockEvent::new(
        "e2",
        EventKind::WaitingInput,
        "grok",
        "grok-s",
    ));
    assert_eq!(
        state.snapshot().sessions[0].state,
        SessionState::NeedsAttention
    );

    let nested =
        DockEvent::new("e3", EventKind::Started, "codex", "codex-version").with_terminal_id("term");
    let result = state.apply(nested);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.sessions[0].source, "grok");
    assert_eq!(
        result.snapshot.sessions[0].state,
        SessionState::NeedsAttention
    );
}

#[test]
fn start_with_a_project_path_still_replaces_a_working_terminal_session() {
    let mut state = DockState::new();
    let mut grok = DockEvent::new("e1", EventKind::Working, "grok", "grok-s");
    grok.cwd = Some("/tmp/project".to_owned());
    state.apply(grok.with_terminal_id("term"));

    let mut codex = DockEvent::new("e2", EventKind::Idle, "codex", "codex-s");
    codex.cwd = Some("/tmp/other".to_owned());
    let result = state.apply(codex.with_terminal_id("term"));
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.sessions[0].source, "codex");
    assert_eq!(result.snapshot.sessions[0].session_id, "codex-s");
}

#[test]
fn grok_lifecycle_with_terminal_and_liveness_fills_audit() {
    let mut state = DockState::new();
    let mut start = DockEvent::new("e1", EventKind::Idle, "grok", "sid");
    attach_liveness(&mut start, 11, 100);
    state.apply(start.with_terminal_id("term"));

    let mut working = DockEvent::new("e2", EventKind::Working, "grok", "sid");
    attach_liveness(&mut working, 11, 100);
    state.apply(working.with_terminal_id("term"));
    assert!(state.snapshot().audit.is_empty());

    let completed =
        DockEvent::new("e3", EventKind::Completed, "grok", "sid").with_terminal_id("term");
    let result = state.apply(completed);
    assert_eq!(result.snapshot.audit.len(), 1);
    assert_eq!(result.snapshot.audit[0].state, SessionState::Completed);

    let closed = DockEvent::new("e4", EventKind::Closed, "grok", "sid").with_terminal_id("term");
    let result = state.apply(closed);
    assert_eq!(result.snapshot.tracked_count, 0);
    assert_eq!(result.snapshot.audit.len(), 2);
    assert_eq!(result.snapshot.audit[1].state, SessionState::Closed);
}

#[test]
fn two_resumes_of_the_same_session_are_two_main_sessions() {
    let mut state = DockState::new();
    let mut first = DockEvent::new("e1", EventKind::Working, "grok", "resume-id");
    first.cwd = Some("/tmp/project".to_owned());
    attach_liveness(&mut first, 11, 100);
    state.apply(first.with_terminal_id("term-a"));

    let mut second = DockEvent::new("e2", EventKind::Idle, "grok", "resume-id");
    second.cwd = Some("/tmp/project".to_owned());
    attach_liveness(&mut second, 22, 200);
    let stacked = state.apply(second.with_terminal_id("term-b"));
    assert_eq!(stacked.snapshot.tracked_count, 2);
    assert_eq!(stacked.snapshot.working_count, 1);
    assert!(stacked.snapshot.sessions.iter().any(|session| {
        session.session_id == "resume-id"
            && session.terminal_id.as_deref() == Some("term-a")
            && session.state == SessionState::Working
    }));
    assert!(stacked.snapshot.sessions.iter().any(|session| {
        session.session_id == "resume-id"
            && session.terminal_id.as_deref() == Some("term-b")
            && session.state == SessionState::Idle
    }));
}

#[test]
fn closing_one_resume_leaves_the_other_live_session() {
    let mut state = DockState::new();
    let mut first = DockEvent::new("e1", EventKind::Working, "grok", "resume-id");
    attach_liveness(&mut first, 11, 100);
    state.apply(first.with_terminal_id("term-a"));

    let mut second = DockEvent::new("e2", EventKind::Idle, "grok", "resume-id");
    attach_liveness(&mut second, 22, 200);
    state.apply(second.with_terminal_id("term-b"));
    assert_eq!(state.snapshot().tracked_count, 2);

    let closed =
        DockEvent::new("e3", EventKind::Closed, "grok", "resume-id").with_terminal_id("term-b");
    let result = state.apply(closed);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.working_count, 1);
    assert_eq!(result.snapshot.sessions[0].session_id, "resume-id");
    assert_eq!(
        result.snapshot.sessions[0].terminal_id.as_deref(),
        Some("term-a")
    );
    assert_eq!(result.snapshot.sessions[0].state, SessionState::Working);
}

#[test]
fn persisted_two_resumes_round_trip() {
    let mut state = DockState::new();
    let mut first = DockEvent::new("e1", EventKind::Working, "grok", "resume-id");
    attach_liveness(&mut first, 11, 100);
    state.apply(first.with_terminal_id("term-a"));
    let mut second = DockEvent::new("e2", EventKind::Idle, "grok", "resume-id");
    attach_liveness(&mut second, 22, 200);
    state.apply(second.with_terminal_id("term-b"));

    let mut restored = DockState::from_persisted(state.persisted());
    assert_eq!(restored.snapshot().tracked_count, 2);

    let closed =
        DockEvent::new("e3", EventKind::Closed, "grok", "resume-id").with_terminal_id("term-b");
    let result = restored.apply(closed);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(
        result.snapshot.sessions[0].terminal_id.as_deref(),
        Some("term-a")
    );
}

#[test]
fn reset_one_resume_leaves_the_other() {
    let mut state = DockState::new();
    let mut first = DockEvent::new("e1", EventKind::Working, "grok", "resume-id");
    attach_liveness(&mut first, 11, 100);
    state.apply(first.with_terminal_id("term-a"));
    let mut second = DockEvent::new("e2", EventKind::Idle, "grok", "resume-id");
    attach_liveness(&mut second, 22, 200);
    state.apply(second.with_terminal_id("term-b"));
    assert_eq!(state.snapshot().tracked_count, 2);

    state.reset("grok", "resume-id", Some("term-b"));
    let snapshot = state.snapshot();
    assert_eq!(snapshot.tracked_count, 1);
    assert_eq!(snapshot.sessions[0].terminal_id.as_deref(), Some("term-a"));
}

#[test]
fn closed_without_instance_identity_does_not_drop_both_resumes() {
    let mut state = DockState::new();
    let mut first = DockEvent::new("e1", EventKind::Working, "grok", "resume-id");
    attach_liveness(&mut first, 11, 100);
    state.apply(first.with_terminal_id("term-a"));
    let mut second = DockEvent::new("e2", EventKind::Idle, "grok", "resume-id");
    attach_liveness(&mut second, 22, 200);
    state.apply(second.with_terminal_id("term-b"));

    let result = state.apply(DockEvent::new("e3", EventKind::Closed, "grok", "resume-id"));
    assert_eq!(result.snapshot.tracked_count, 2);
}

#[test]
fn liveness_closed_of_one_resume_does_not_drop_the_other() {
    let mut state = DockState::new();
    let mut first = DockEvent::new("e1", EventKind::Working, "grok", "resume-id");
    attach_liveness(&mut first, 11, 100);
    state.apply(first.with_terminal_id("term-a"));

    let mut second = DockEvent::new("e2", EventKind::Working, "grok", "resume-id");
    attach_liveness(&mut second, 22, 200);
    state.apply(second.with_terminal_id("term-b"));

    let mut closed = DockEvent::new("e3", EventKind::Closed, "grok", "resume-id");
    attach_liveness(&mut closed, 22, 200);
    let result = state.apply(closed);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(
        result.snapshot.sessions[0].terminal_id.as_deref(),
        Some("term-a")
    );
}

#[test]
fn quit_hooks_of_one_resume_do_not_close_the_other() {
    let mut state = DockState::new();
    let mut first = DockEvent::new("e1", EventKind::Working, "grok", "resume-id");
    attach_liveness(&mut first, 11, 100);
    state.apply(first.with_terminal_id("term-a"));

    let mut second = DockEvent::new("e2", EventKind::Idle, "grok", "resume-id");
    attach_liveness(&mut second, 22, 200);
    state.apply(second.with_terminal_id("term-b"));
    assert_eq!(state.snapshot().tracked_count, 2);

    let mut stop = DockEvent::new("e3", EventKind::Closed, "grok", "resume-id");
    attach_liveness(&mut stop, 22, 200);
    state.apply(stop.with_terminal_id("term-b"));
    assert_eq!(state.snapshot().tracked_count, 1);

    let mut session_end = DockEvent::new("e4", EventKind::Closed, "grok", "resume-id");
    attach_liveness(&mut session_end, 22, 200);
    let result = state.apply(session_end.with_terminal_id("term-b"));
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.working_count, 1);
    assert_eq!(
        result.snapshot.sessions[0].terminal_id.as_deref(),
        Some("term-a")
    );
    assert_eq!(result.snapshot.sessions[0].state, SessionState::Working);
}

#[test]
fn completed_session_is_still_replaced_by_a_pathless_start() {
    let mut state = DockState::new();
    let mut grok = DockEvent::new("e1", EventKind::Working, "grok", "grok-s");
    grok.cwd = Some("/tmp/project".to_owned());
    state.apply(grok.with_terminal_id("term"));
    state.apply(DockEvent::new("e2", EventKind::Completed, "grok", "grok-s"));

    let nested =
        DockEvent::new("e3", EventKind::Started, "codex", "codex-s").with_terminal_id("term");
    let result = state.apply(nested);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.sessions[0].source, "codex");
}

#[test]
fn idle_session_with_a_path_is_still_replaced_by_a_pathless_start() {
    let mut state = DockState::new();
    let mut grok = DockEvent::new("e1", EventKind::Idle, "grok", "grok-s");
    grok.cwd = Some("/tmp/project".to_owned());
    state.apply(grok.with_terminal_id("term"));

    let nested =
        DockEvent::new("e2", EventKind::Started, "codex", "codex-s").with_terminal_id("term");
    let result = state.apply(nested);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.sessions[0].source, "codex");
}

#[test]
fn events_without_terminal_id_do_not_replace() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "one"));
    let second = state.apply(event("e2", EventKind::Started, "two"));
    assert_eq!(second.snapshot.tracked_count, 2);
}

#[test]
fn existing_session_idle_does_not_close_other_sessions() {
    let mut state = DockState::new();
    let mut keep = event("e1", EventKind::Started, "keep");
    keep.cwd = Some("/tmp/same-project".to_owned());
    state.apply(keep);
    let mut other = DockEvent::new("e2", EventKind::Started, "grok", "other");
    other.cwd = Some("/tmp/same-project".to_owned());
    state.apply(other.with_terminal_id("term"));
    assert_eq!(state.snapshot().tracked_count, 2);

    let idle = state.apply(event("e3", EventKind::Idle, "keep").with_terminal_id("term"));
    assert_eq!(
        idle.snapshot.tracked_count, 2,
        "idle of an existing session must not evict others"
    );

    let closed = state.apply(event("e4", EventKind::Closed, "keep"));
    assert_eq!(closed.snapshot.tracked_count, 1);
    assert_eq!(closed.snapshot.sessions[0].session_id, "other");
}

#[test]
fn reset_one_session_leaves_others_in_the_same_project() {
    let mut state = DockState::new();
    let mut first = event("e1", EventKind::Started, "s1");
    first.cwd = Some("/tmp/same-project".to_owned());
    state.apply(first);
    let mut second = event("e2", EventKind::Started, "s2");
    second.cwd = Some("/tmp/same-project".to_owned());
    state.apply(second);
    state.reset("claude", "s1", None);
    let snapshot = state.snapshot();
    assert_eq!(snapshot.tracked_count, 1);
    assert_eq!(snapshot.sessions[0].session_id, "s2");
}

#[test]
fn parent_events_do_not_replace_the_terminal_session() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "parent").with_terminal_id("term"));
    let child = event("e2", EventKind::Started, "child")
        .with_parent_session_id("parent")
        .with_terminal_id("term");
    let result = state.apply(child);
    assert_eq!(result.snapshot.tracked_count, 1);
    assert_eq!(result.snapshot.sessions[0].session_id, "parent");
}

#[test]
fn terminal_replacement_is_recorded_in_audit() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "old").with_terminal_id("term"));
    let replaced = state.apply(event("e2", EventKind::Started, "fresh").with_terminal_id("term"));
    assert!(replaced
        .snapshot
        .audit
        .iter()
        .any(|entry| { entry.session_id == "old" && entry.state == SessionState::Closed }));
    assert!(!replaced
        .snapshot
        .audit
        .iter()
        .any(|entry| entry.session_id == "fresh"));
}

#[test]
fn snapshot_exposes_terminal_id() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "keep").with_terminal_id("dock:ab12cd"));
    assert_eq!(
        state.snapshot().sessions[0].terminal_id.as_deref(),
        Some("dock:ab12cd")
    );
}

#[test]
fn persisted_state_round_trips_terminal_id() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, "keep").with_terminal_id("term-keep"));
    let persisted = state.persisted();
    assert_eq!(
        persisted.sessions[0].terminal_id.as_deref(),
        Some("term-keep")
    );
    let json = serde_json::to_string(&persisted).unwrap();
    assert!(json.contains("term-keep"));

    let mut restored = DockState::from_persisted(serde_json::from_str(&json).unwrap());
    let replaced =
        restored.apply(event("e2", EventKind::Idle, "after-restart").with_terminal_id("term-keep"));
    assert_eq!(replaced.snapshot.sessions.len(), 1);
    assert_eq!(replaced.snapshot.sessions[0].session_id, "after-restart");
}

#[test]
fn missing_terminal_id_in_old_state_json_defaults_to_none() {
    let json = r#"{"version":1,"sessions":[{"source":"claude","session_id":"legacy","state":"working","attention_reason":null,"requires_user_action":false,"acknowledged":true,"occurred_at":"2026-08-16T00:00:00Z"}]}"#;
    let restored = DockState::from_persisted(serde_json::from_str(json).unwrap());
    assert_eq!(restored.snapshot().sessions[0].session_id, "legacy");
    let persisted = restored.persisted();
    assert_eq!(persisted.sessions[0].terminal_id, None);
    assert!(!serde_json::to_string(&persisted)
        .unwrap()
        .contains("terminal_id"));
}

#[test]
fn working_and_idle_do_not_fill_audit() {
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Idle, "s1"));
    state.apply(event("e2", EventKind::Working, "s1"));
    assert!(state.snapshot().audit.is_empty());
    state.apply(event("e3", EventKind::WaitingInput, "s1"));
    assert_eq!(state.snapshot().audit.len(), 1);
    assert_eq!(
        state.snapshot().audit[0].state,
        SessionState::NeedsAttention
    );
}

#[test]
fn audit_stream_is_bounded_and_contains_no_event_content() {
    let mut state = DockState::new();
    for index in 0..140 {
        let session_id = format!("s-{index}");
        let event_id = format!("e-{index}");
        state.apply(
            DockEvent::new(&event_id, EventKind::Started, "claude", &session_id)
                .with_summary("private content must stay out of audit"),
        );
        state.apply(DockEvent::new(
            &format!("{event_id}-done"),
            EventKind::Completed,
            "claude",
            &session_id,
        ));
    }

    let snapshot = state.snapshot();
    assert_eq!(snapshot.audit.len(), 128);
    assert_eq!(snapshot.audit.first().unwrap().session_id, "s-12");
    assert_eq!(snapshot.audit.last().unwrap().session_id, "s-139");
}

#[test]
fn liveness_merges_as_a_complete_tuple_and_stays_off_the_snapshot() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started
        .metadata
        .insert("agent_os".to_owned(), "linux".to_owned());
    started
        .metadata
        .insert("agent_pid".to_owned(), "42".to_owned());
    started
        .metadata
        .insert("agent_starttime".to_owned(), "99".to_owned());
    state.apply(started);
    assert!(state
        .snapshot()
        .sessions
        .iter()
        .all(|session| serde_json::to_value(session)
            .unwrap()
            .get("agent_pid")
            .is_none()));
    let targets = state.liveness_targets();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].2.pid, 42);
    assert_eq!(targets[0].2.distro, None);

    let mut later = event("e2", EventKind::Working, "s1");
    later
        .metadata
        .insert("agent_os".to_owned(), "linux".to_owned());
    later
        .metadata
        .insert("agent_pid".to_owned(), "42".to_owned());
    later
        .metadata
        .insert("agent_starttime".to_owned(), "99".to_owned());
    later
        .metadata
        .insert("agent_wsl_distro".to_owned(), "Ubuntu-24.04".to_owned());
    state.apply(later);
    assert_eq!(
        state.liveness_targets()[0].2.distro.as_deref(),
        Some("Ubuntu-24.04")
    );

    let mut hijack = event("e2b", EventKind::Completed, "s1");
    hijack
        .metadata
        .insert("agent_os".to_owned(), "linux".to_owned());
    hijack
        .metadata
        .insert("agent_pid".to_owned(), "99".to_owned());
    hijack
        .metadata
        .insert("agent_starttime".to_owned(), "1".to_owned());
    state.apply(hijack);
    assert_eq!(state.liveness_targets()[0].2.pid, 42);

    let mut incomplete = event("e3", EventKind::Idle, "s1");
    incomplete
        .metadata
        .insert("agent_os".to_owned(), "linux".to_owned());
    state.apply(incomplete);
    assert_eq!(
        state.liveness_targets()[0].2.distro.as_deref(),
        Some("Ubuntu-24.04")
    );
}

#[test]
fn old_state_files_without_liveness_still_load() {
    let restored = DockState::from_persisted(serde_json::from_str(
        r#"{"version":1,"sessions":[{"source":"grok","session_id":"s1","state":"idle","attention_reason":null,"requires_user_action":false,"acknowledged":true,"occurred_at":"2026-08-23T00:00:00Z"}]}"#,
    ).unwrap());
    assert!(restored.liveness_targets().is_empty());
}

#[test]
fn hashed_liveness_event_id_fits_a_256_byte_session_id() {
    use agent_activity_dock_core::{liveness_closed_event_id, MAX_EVENT_ID_LEN};
    let session_id = "s".repeat(256);
    let event_id = liveness_closed_event_id("grok", &session_id, 7, 11);
    assert!(event_id.len() <= MAX_EVENT_ID_LEN);
    assert!(event_id.starts_with("dock-liveness-"));
    let mut state = DockState::new();
    state.apply(event("e1", EventKind::Started, &session_id));
    let closed = state.apply(DockEvent::new(
        &event_id,
        EventKind::Closed,
        "claude",
        &session_id,
    ));
    assert!(closed.accepted);
    assert_eq!(closed.snapshot.tracked_count, 0);
}

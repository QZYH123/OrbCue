use agent_activity_dock_core::{DockEvent, DockState, EventKind, SessionState};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

fn event(id: &str, kind: EventKind, session: &str) -> DockEvent {
    DockEvent::new(id, kind, "claude", session)
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
    assert_eq!(completed.snapshot.audit.len(), 2);
    assert_eq!(completed.snapshot.audit[0].state, SessionState::Working);
    assert_eq!(completed.snapshot.audit[1].state, SessionState::Completed);
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

    let acknowledged = state.acknowledge("claude", "s1");
    assert_eq!(acknowledged.pending_count, 1);
    assert_eq!(acknowledged.pending_mark, "?");
    assert!(acknowledged.sessions[0].requires_user_action);
}

#[test]
fn restart_state_excludes_ephemeral_content_and_does_not_replay_attention() {
    let mut state = DockState::new();
    let mut started = event("e1", EventKind::Started, "s1");
    started.workspace_root = Some("/secret/project".to_owned());
    started.cwd = Some("/secret/project".to_owned());
    started.window_title = Some("Secret Title".to_owned());
    state.apply(started);
    state.apply(event("e2", EventKind::Failed, "s1").with_summary("private failure details"));

    let json = serde_json::to_string(&state.persisted()).unwrap();
    assert!(!json.contains("private failure details"));
    assert!(!json.contains("summary"));
    assert!(!json.contains("transcript"));
    assert!(!json.contains("project_path"));
    assert!(!json.contains("window_title"));
    assert!(!json.contains("Secret Title"));
    assert!(!json.contains("/secret/project"));

    let restored = DockState::from_persisted(serde_json::from_str(&json).unwrap());
    assert_eq!(restored.snapshot().pending_count, 1);
    assert_eq!(restored.snapshot().pending_mark, "!");
    assert_eq!(restored.snapshot().sessions[0].state, SessionState::Failed);
    assert_eq!(restored.snapshot().sessions[0].mark, "!");
    assert!(restored.snapshot().sessions[0].summary.is_none());
    assert!(restored.snapshot().sessions[0].project_path.is_none());
    assert!(restored.snapshot().sessions[0].window_title.is_none());
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
fn audit_stream_is_bounded_and_contains_no_event_content() {
    let mut state = DockState::new();
    for index in 0..140 {
        let session_id = format!("s-{index}");
        let event_id = format!("e-{index}");
        state.apply(
            DockEvent::new(&event_id, EventKind::Started, "claude", &session_id)
                .with_summary("private content must stay out of audit"),
        );
    }

    let snapshot = state.snapshot();
    assert_eq!(snapshot.audit.len(), 128);
    assert_eq!(snapshot.audit.first().unwrap().session_id, "s-12");
    assert_eq!(snapshot.audit.last().unwrap().session_id, "s-139");
}

use agent_activity_dock_core::{AuditEntry, DockEvent, EventKind, SessionState};
use agent_activity_dock_ipc::{encode_request, parse_request, IpcRequest, SnapshotView};

#[test]
fn event_json_round_trips_without_exposing_core_types_to_callers() {
    let event = DockEvent::new("e-1", EventKind::Started, "claude", "session-1");
    let request = parse_request(serde_json::to_vec(&event).unwrap().as_slice()).unwrap();
    match request {
        IpcRequest::Event(received) => {
            assert_eq!(received.event_id, "e-1");
            assert_eq!(received.source, "claude");
        }
        other => panic!("expected event, got {other:?}"),
    }
}

#[test]
fn queries_are_explicit_and_oversized_frames_are_rejected() {
    let encoded = encode_request(&IpcRequest::Snapshot).unwrap();
    assert_eq!(encoded, b"{\"query\":\"snapshot\"}\n");
    let request = parse_request(br#"{"query":"snapshot"}"#).unwrap();
    assert!(matches!(request, IpcRequest::Snapshot));

    let request = parse_request(br#"{"query":"reset","source":"claude","task_id":"s1"}"#).unwrap();
    assert!(matches!(
        request,
        IpcRequest::Reset { source, session_id, terminal_id: None }
            if source == "claude" && session_id == "s1"
    ));

    let error = parse_request(&vec![b'x'; 16 * 1024 + 1]).unwrap_err();
    assert_eq!(error.to_string(), "message_too_large");
}

#[test]
fn snapshot_view_contains_stable_display_fields() {
    let view = SnapshotView::from(&agent_activity_dock_core::DockSnapshot {
        working_count: 2,
        tracked_count: 3,
        pending_count: 1,
        pending_mark: "?".to_owned(),
        sessions: vec![],
        audit: vec![AuditEntry {
            source: "claude".to_owned(),
            session_id: "s1".to_owned(),
            state: SessionState::Completed,
            attention_reason: Some("completed".to_owned()),
            occurred_at: "2026-08-16T00:00:00Z".to_owned(),
            project_path: Some("/tmp/demo".to_owned()),
        }],
    });
    assert_eq!(view.count_label, "2/3");
    assert_eq!(view.border_state, "working");
    assert_eq!(view.audit.len(), 1);
    assert_eq!(view.audit[0].session_id, "s1");
}

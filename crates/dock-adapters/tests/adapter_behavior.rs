use agent_activity_dock_adapters::{claude_hook, codex_notification, dsh_projection, grok_hook};
use agent_activity_dock_core::{DockState, EventKind, SessionState};

#[test]
fn claude_adapter_uses_only_hook_metadata() {
    let payload = serde_json::json!({
        "hook_event_name": "PermissionRequest",
        "session_id": "claude-1",
        "transcript_path": "/private/should-not-be-opened",
        "tool_input": {"command": "secret"}
    });
    let event = claude_hook(&payload).unwrap();
    assert_eq!(event.kind, EventKind::PermissionRequested);
    assert_eq!(event.session_id, "claude-1");
    assert_eq!(event.summary, None);
    assert_eq!(event.metadata.len(), 0);
}

#[test]
fn projection_adapters_reject_unknown_payloads_without_throwing() {
    assert!(dsh_projection(&serde_json::json!({"event":"unknown"})).is_none());
    assert!(codex_notification(&serde_json::json!({"type":"unknown"})).is_none());
}

#[test]
fn grok_adapter_keeps_one_record_per_session() {
    let opened = grok_hook(&serde_json::json!({
        "hookEventName": "session_start",
        "sessionId": "grok-session"
    }))
    .unwrap();
    assert_eq!(opened.kind, EventKind::Idle);
    assert_eq!(opened.session_id, "grok-session");
    assert_eq!(opened.source, "grok");

    let started = grok_hook(&serde_json::json!({
        "hookEventName": "user_prompt_submit",
        "sessionId": "grok-session",
        "promptId": "turn-1"
    }))
    .unwrap();
    assert_eq!(started.kind, EventKind::Working);
    assert_eq!(started.session_id, "grok-session");

    let permission = grok_hook(&serde_json::json!({
        "hookEventName": "notification",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "notificationType": "permission_prompt"
    }))
    .unwrap();
    assert_eq!(permission.kind, EventKind::PermissionRequested);
    assert_eq!(permission.session_id, "grok-session");

    let idle = grok_hook(&serde_json::json!({
        "hookEventName": "stop",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "reason": "end_turn"
    }))
    .unwrap();
    assert_eq!(idle.kind, EventKind::Completed);
    assert_eq!(idle.session_id, "grok-session");

    let ended = grok_hook(&serde_json::json!({
        "hookEventName": "session_end",
        "sessionId": "grok-session"
    }))
    .unwrap();
    assert_eq!(ended.kind, EventKind::Closed);

    assert!(grok_hook(&serde_json::json!({
        "hookEventName": "user_prompt_submit",
        "sessionId": "grok-session",
        "promptId": "turn-2",
        "subagentType": "explore"
    }))
    .is_none());
}

#[test]
fn grok_session_can_work_again_after_a_turn_ends() {
    let mut state = DockState::new();
    let first = grok_hook(&serde_json::json!({
        "hookEventName": "user_prompt_submit",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "event_id": "g-1"
    }))
    .unwrap();
    assert!(state.apply(first).accepted);

    let idle = grok_hook(&serde_json::json!({
        "hookEventName": "stop",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "reason": "end_turn",
        "event_id": "g-2"
    }))
    .unwrap();
    let idle = state.apply(idle);
    assert!(idle.accepted);
    assert_eq!(idle.snapshot.working_count, 0);
    assert_eq!(idle.snapshot.tracked_count, 1);
    assert_eq!(idle.snapshot.pending_count, 1);
    assert_eq!(idle.snapshot.pending_mark, "*");
    assert_eq!(idle.snapshot.sessions[0].state, SessionState::Completed);

    let second = grok_hook(&serde_json::json!({
        "hookEventName": "user_prompt_submit",
        "sessionId": "grok-session",
        "promptId": "turn-2",
        "event_id": "g-3"
    }))
    .unwrap();
    let second = state.apply(second);
    assert!(second.accepted);
    assert_eq!(second.snapshot.working_count, 1);
    assert_eq!(second.snapshot.tracked_count, 1);
    assert_eq!(second.snapshot.sessions[0].session_id, "grok-session");
    assert_eq!(second.snapshot.sessions[0].state, SessionState::Working);
}

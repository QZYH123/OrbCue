use orbcue_adapters::{
    claude_hook, codex_hook, codex_notification, cursor_hook, grok_hook, opencode_hook, pi_hook,
};
use orbcue_core::{ApplyResult, DockEvent, DockState, EventKind, SessionState};

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
    assert_eq!(event.metadata.len(), 0);
}

#[test]
fn claude_adapter_follows_turn_lifecycle() {
    let opened = claude_hook(&serde_json::json!({
        "hook_event_name": "SessionStart",
        "session_id": "claude-session"
    }))
    .unwrap();
    assert_eq!(opened.kind, EventKind::Idle);

    let prompt = claude_hook(&serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "claude-session"
    }))
    .unwrap();
    assert_eq!(prompt.kind, EventKind::Working);

    let after_tool = claude_hook(&serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": "claude-session",
        "tool_name": "Read"
    }))
    .unwrap();
    assert_eq!(after_tool.kind, EventKind::Working);

    let asking = claude_hook(&serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": "claude-session",
        "tool_name": "AskUserQuestion"
    }))
    .unwrap();
    assert_eq!(asking.kind, EventKind::WaitingInput);

    let answered = claude_hook(&serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": "claude-session",
        "tool_name": "AskUserQuestion"
    }))
    .unwrap();
    assert_eq!(answered.kind, EventKind::Working);

    let stop = claude_hook(&serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "claude-session"
    }))
    .unwrap();
    assert_eq!(stop.kind, EventKind::Completed);

    let nested = claude_hook(&serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "claude-session",
        "background_tasks": [{"id": "s1", "type": "subagent", "status": "running"}]
    }))
    .unwrap();
    assert_eq!(nested.kind, EventKind::Working);

    let running_shell = claude_hook(&serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "claude-session",
        "background_tasks": [{"id": "t1", "type": "shell", "status": "running"}]
    }))
    .unwrap();
    assert_eq!(running_shell.kind, EventKind::Completed);

    assert!(claude_hook(&serde_json::json!({
        "hook_event_name": "Notification",
        "session_id": "claude-session",
        "notification_type": "task_complete"
    }))
    .is_none());
    assert!(claude_hook(&serde_json::json!({
        "hook_event_name": "Notification",
        "session_id": "claude-session",
        "notification_type": "agent_completed"
    }))
    .is_none());
    assert!(claude_hook(&serde_json::json!({
        "hook_event_name": "TaskCompleted",
        "session_id": "claude-session"
    }))
    .is_none());

    let idle_prompt = claude_hook(&serde_json::json!({
        "hook_event_name": "Notification",
        "session_id": "claude-session",
        "notification_type": "idle_prompt"
    }))
    .unwrap();
    assert_eq!(idle_prompt.kind, EventKind::Completed);

    let denied = claude_hook(&serde_json::json!({
        "hook_event_name": "PermissionDenied",
        "session_id": "claude-session",
        "tool_name": "Bash"
    }))
    .unwrap();
    assert_eq!(denied.kind, EventKind::Working);

    let ended = claude_hook(&serde_json::json!({
        "hook_event_name": "SessionEnd",
        "session_id": "claude-session"
    }))
    .unwrap();
    assert_eq!(ended.kind, EventKind::Closed);
}

#[test]
fn claude_named_subagent_hooks_are_dropped_without_parent() {
    assert!(claude_hook(&serde_json::json!({
        "hook_event_name": "SubagentStop",
        "session_id": "child-1"
    }))
    .is_none());
    assert!(claude_hook(&serde_json::json!({
        "hook_event_name": "SubagentStart",
        "session_id": "child-1"
    }))
    .is_none());
}

#[test]
fn claude_subagent_clues_fill_parent_when_present() {
    let event = claude_hook(&serde_json::json!({
        "hook_event_name": "PermissionRequest",
        "session_id": "child-1",
        "parent_session_id": "parent-1"
    }))
    .unwrap();
    assert_eq!(event.kind, EventKind::PermissionRequested);
    assert_eq!(event.parent_session_id.as_deref(), Some("parent-1"));
}

#[test]
fn claude_unknown_subagent_shape_stays_a_main_session() {
    let event = claude_hook(&serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "main-1",
        "subagentType": "explore"
    }))
    .unwrap();
    assert_eq!(event.kind, EventKind::Working);
    assert_eq!(event.session_id, "main-1");
    assert_eq!(event.parent_session_id, None);
}

#[test]
fn codex_notification_copies_parent_session_id() {
    let codex = codex_notification(&serde_json::json!({
        "type": "failed",
        "session_id": "child",
        "parentSessionId": "parent"
    }))
    .unwrap();
    assert_eq!(codex.kind, EventKind::Failed);
    assert_eq!(codex.parent_session_id.as_deref(), Some("parent"));
}

#[test]
fn projection_adapters_reject_unknown_payloads_without_throwing() {
    assert!(codex_notification(&serde_json::json!({"type":"unknown"})).is_none());
}

fn apply_permission_then(event: DockEvent) -> SessionState {
    let mut state = DockState::new();
    let started = DockEvent::new(
        "start-1",
        EventKind::Working,
        &event.source,
        &event.session_id,
    );
    assert!(state.apply(started).accepted);
    let waiting = state.apply(DockEvent::new(
        "perm-1",
        EventKind::PermissionRequested,
        &event.source,
        &event.session_id,
    ));
    assert_eq!(
        waiting.snapshot.sessions[0].state,
        SessionState::NeedsAttention
    );
    state.apply(event).snapshot.sessions[0].state
}

#[test]
fn claude_allowing_a_permission_prompt_returns_to_working() {
    let allowed = claude_hook(&serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": "claude-session",
        "tool_name": "Bash",
        "event_id": "c-allow-1"
    }))
    .unwrap();
    assert_eq!(allowed.kind, EventKind::Working);
    assert_eq!(apply_permission_then(allowed), SessionState::Working);
}

#[test]
fn claude_auto_mode_permission_denied_returns_to_working() {
    let denied = claude_hook(&serde_json::json!({
        "hook_event_name": "PermissionDenied",
        "session_id": "claude-session",
        "tool_name": "Bash",
        "event_id": "c-deny-1"
    }))
    .unwrap();
    assert_eq!(denied.kind, EventKind::Working);
    assert_eq!(apply_permission_then(denied), SessionState::Working);
}

#[test]
fn codex_allowing_a_permission_prompt_returns_to_working() {
    let prompt = codex_hook(&serde_json::json!({
        "hook_event_name": "PermissionRequest",
        "session_id": "codex-session",
        "tool_name": "Bash"
    }))
    .unwrap();
    assert_eq!(prompt.kind, EventKind::PermissionRequested);

    let allowed = codex_hook(&serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": "codex-session",
        "tool_name": "Bash",
        "event_id": "x-allow-1"
    }))
    .unwrap();
    assert_eq!(allowed.kind, EventKind::Working);
    assert_eq!(apply_permission_then(allowed), SessionState::Working);
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

    assert!(grok_hook(&serde_json::json!({
        "hookEventName": "pre_tool_use",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "toolName": "read_file"
    }))
    .is_none());

    let denied = grok_hook(&serde_json::json!({
        "hookEventName": "permission_denied",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "toolName": "run_terminal_command"
    }))
    .unwrap();
    assert_eq!(denied.kind, EventKind::Working);

    let asking = grok_hook(&serde_json::json!({
        "hookEventName": "pre_tool_use",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "toolName": "ask_user_question"
    }))
    .unwrap();
    assert_eq!(asking.kind, EventKind::WaitingInput);
    assert_eq!(asking.severity, orbcue_core::Severity::Attention);

    let answered = grok_hook(&serde_json::json!({
        "hookEventName": "post_tool_use",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "toolName": "ask_user_question"
    }))
    .unwrap();
    assert_eq!(answered.kind, EventKind::Working);

    let dismissed = grok_hook(&serde_json::json!({
        "hookEventName": "post_tool_use_failure",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "tool_name": "ask_user_question"
    }))
    .unwrap();
    assert_eq!(dismissed.kind, EventKind::Working);

    let idle = grok_hook(&serde_json::json!({
        "hookEventName": "stop",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "reason": "end_turn"
    }))
    .unwrap();
    assert_eq!(idle.kind, EventKind::Completed);
    assert_eq!(idle.session_id, "grok-session");

    let hanging_service = grok_hook(&serde_json::json!({
        "hookEventName": "stop",
        "sessionId": "grok-session",
        "reason": "end_turn",
        "backgroundTasks": [{"id": "m1", "type": "monitor", "status": "running"}]
    }))
    .unwrap();
    assert_eq!(hanging_service.kind, EventKind::Completed);

    let running_shell = grok_hook(&serde_json::json!({
        "hookEventName": "stop",
        "sessionId": "grok-session",
        "reason": "end_turn",
        "backgroundTasks": [{"id": "t1", "type": "shell", "status": "running"}]
    }))
    .unwrap();
    assert_eq!(running_shell.kind, EventKind::Completed);

    assert!(grok_hook(&serde_json::json!({
        "hookEventName": "notification",
        "sessionId": "grok-session",
        "notificationType": "task_complete"
    }))
    .is_none());

    let wake_prompt = grok_hook(&serde_json::json!({
        "hookEventName": "user_prompt_submit",
        "sessionId": "grok-session",
        "promptId": "task-completed-t1"
    }))
    .unwrap();
    assert_eq!(wake_prompt.kind, EventKind::Working);

    let settled = grok_hook(&serde_json::json!({
        "hookEventName": "notification",
        "sessionId": "grok-session",
        "notificationType": "idle_prompt"
    }))
    .unwrap();
    assert_eq!(settled.kind, EventKind::Completed);

    let finished_shell = grok_hook(&serde_json::json!({
        "hookEventName": "stop",
        "sessionId": "grok-session",
        "reason": "end_turn",
        "backgroundTasks": [{"id": "t1", "type": "shell", "status": "completed"}]
    }))
    .unwrap();
    assert_eq!(finished_shell.kind, EventKind::Completed);

    let nested = grok_hook(&serde_json::json!({
        "hookEventName": "stop",
        "sessionId": "grok-session",
        "reason": "end_turn",
        "backgroundTasks": [{"id": "s1", "type": "subagent", "status": "running"}]
    }))
    .unwrap();
    assert_eq!(nested.kind, EventKind::Working);

    let finished_subagent = grok_hook(&serde_json::json!({
        "hookEventName": "stop",
        "sessionId": "grok-session",
        "reason": "end_turn",
        "backgroundTasks": [{"id": "s1", "type": "subagent", "status": "completed"}]
    }))
    .unwrap();
    assert_eq!(finished_subagent.kind, EventKind::Completed);

    let failed_tool = grok_hook(&serde_json::json!({
        "hookEventName": "PostToolUseFailure",
        "sessionId": "grok-session"
    }))
    .unwrap();
    assert_eq!(failed_tool.kind, EventKind::Working);

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
fn grok_adapter_copies_explicit_workspace_fields() {
    let event = grok_hook(&serde_json::json!({
        "hookEventName": "session_start",
        "sessionId": "grok-session",
        "cwd": "/tmp/cwd",
        "workspaceRoot": "/tmp/workspace",
        "transcript_path": "/private/should-not-be-opened"
    }))
    .unwrap();
    assert_eq!(event.cwd.as_deref(), Some("/tmp/cwd"));
    assert_eq!(event.workspace_root.as_deref(), Some("/tmp/workspace"));

    let snake = grok_hook(&serde_json::json!({
        "hookEventName": "session_start",
        "sessionId": "grok-session",
        "workspace_root": "/tmp/snake"
    }))
    .unwrap();
    assert_eq!(snake.workspace_root.as_deref(), Some("/tmp/snake"));
}

#[test]
fn grok_denying_a_permission_prompt_returns_to_working() {
    let mut state = DockState::new();
    let started = grok_hook(&serde_json::json!({
        "hookEventName": "user_prompt_submit",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "event_id": "g-perm-1"
    }))
    .unwrap();
    assert!(state.apply(started).accepted);

    let permission = grok_hook(&serde_json::json!({
        "hookEventName": "notification",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "notificationType": "permission_prompt",
        "event_id": "g-perm-2"
    }))
    .unwrap();
    let waiting = state.apply(permission);
    assert_eq!(
        waiting.snapshot.sessions[0].state,
        SessionState::NeedsAttention
    );
    assert_eq!(
        waiting.snapshot.sessions[0].attention_reason.as_deref(),
        Some("permission")
    );

    let denied = grok_hook(&serde_json::json!({
        "hookEventName": "PermissionDenied",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "toolName": "run_terminal_command",
        "event_id": "g-perm-3"
    }))
    .expect("Grok fires PermissionDenied after the user picks deny");
    assert_eq!(denied.kind, EventKind::Working);

    let resumed = state.apply(denied);
    assert!(resumed.accepted);
    assert_eq!(resumed.snapshot.working_count, 1);
    assert_eq!(resumed.snapshot.pending_count, 0);
    assert_eq!(resumed.snapshot.sessions[0].state, SessionState::Working);
    assert_eq!(resumed.snapshot.sessions[0].attention_reason, None);
}

#[test]
fn grok_allowing_a_permission_prompt_returns_to_working() {
    let mut state = DockState::new();
    let started = grok_hook(&serde_json::json!({
        "hookEventName": "user_prompt_submit",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "event_id": "g-allow-1"
    }))
    .unwrap();
    assert!(state.apply(started).accepted);

    let permission = grok_hook(&serde_json::json!({
        "hookEventName": "notification",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "notificationType": "permission_prompt",
        "event_id": "g-allow-2"
    }))
    .unwrap();
    let waiting = state.apply(permission);
    assert_eq!(
        waiting.snapshot.sessions[0].state,
        SessionState::NeedsAttention
    );

    assert!(
        grok_hook(&serde_json::json!({
            "hookEventName": "pre_tool_use",
            "sessionId": "grok-session",
            "promptId": "turn-1",
            "toolName": "run_terminal_command",
            "event_id": "g-allow-pre"
        }))
        .is_none(),
        "PreToolUse fires before the prompt, so it cannot resume after allow"
    );

    let allowed = grok_hook(&serde_json::json!({
        "hookEventName": "post_tool_use",
        "sessionId": "grok-session",
        "promptId": "turn-1",
        "toolName": "run_terminal_command",
        "event_id": "g-allow-3"
    }))
    .expect("Grok fires PostToolUse after an allowed tool runs");
    assert_eq!(allowed.kind, EventKind::Working);

    let resumed = state.apply(allowed);
    assert!(resumed.accepted);
    assert_eq!(resumed.snapshot.working_count, 1);
    assert_eq!(resumed.snapshot.pending_count, 0);
    assert_eq!(resumed.snapshot.sessions[0].state, SessionState::Working);
    assert_eq!(resumed.snapshot.sessions[0].attention_reason, None);
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

#[test]
fn codex_hook_follows_claude_turn_lifecycle() {
    let opened = codex_hook(&serde_json::json!({
        "hook_event_name": "SessionStart",
        "session_id": "codex-session"
    }))
    .unwrap();
    assert_eq!(opened.kind, EventKind::Idle);
    assert_eq!(opened.source, "codex");

    let prompt = codex_hook(&serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "codex-session"
    }))
    .unwrap();
    assert_eq!(prompt.kind, EventKind::Working);

    assert!(codex_hook(&serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": "codex-session"
    }))
    .is_none());

    let stop = codex_hook(&serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "codex-session"
    }))
    .unwrap();
    assert_eq!(stop.kind, EventKind::Completed);

    let nested = codex_hook(&serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "codex-session",
        "background_tasks": [{"id": "s1", "type": "subagent", "status": "running"}]
    }))
    .unwrap();
    assert_eq!(nested.kind, EventKind::Working);

    let hanging = codex_hook(&serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "codex-session",
        "background_tasks": [{"id": "m1", "type": "monitor", "status": "running"}]
    }))
    .unwrap();
    assert_eq!(hanging.kind, EventKind::Completed);

    let running_shell = codex_hook(&serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "codex-session",
        "background_tasks": [{"id": "t1", "type": "shell", "status": "running"}]
    }))
    .unwrap();
    assert_eq!(running_shell.kind, EventKind::Completed);

    let ended = codex_hook(&serde_json::json!({
        "hook_event_name": "SessionEnd",
        "session_id": "codex-session"
    }))
    .unwrap();
    assert_eq!(ended.kind, EventKind::Closed);
}

#[test]
fn codex_hook_falls_back_to_notification_payloads() {
    let started = codex_hook(&serde_json::json!({
        "type": "started",
        "session_id": "codex-notify"
    }))
    .unwrap();
    assert_eq!(started.kind, EventKind::Working);
    assert_eq!(started.source, "codex");
}

#[test]
fn cursor_hook_follows_turn_lifecycle_with_conversation_id() {
    let opened = cursor_hook(&serde_json::json!({
        "hook_event_name": "sessionStart",
        "conversation_id": "cursor-session",
        "workspace_roots": ["/tmp/workspace"]
    }))
    .unwrap();
    assert_eq!(opened.kind, EventKind::Idle);
    assert_eq!(opened.source, "cursor");
    assert_eq!(opened.session_id, "cursor-session");
    assert_eq!(opened.workspace_root.as_deref(), Some("/tmp/workspace"));

    let prompt = cursor_hook(&serde_json::json!({
        "hook_event_name": "beforeSubmitPrompt",
        "conversation_id": "cursor-session"
    }))
    .unwrap();
    assert_eq!(prompt.kind, EventKind::Working);

    assert!(cursor_hook(&serde_json::json!({
        "hook_event_name": "beforeShellExecution",
        "conversation_id": "cursor-session"
    }))
    .is_none());

    assert!(cursor_hook(&serde_json::json!({
        "hook_event_name": "afterAgentThought",
        "conversation_id": "cursor-session"
    }))
    .is_none());

    let reply = cursor_hook(&serde_json::json!({
        "hook_event_name": "afterAgentResponse",
        "conversation_id": "cursor-session"
    }))
    .unwrap();
    assert_eq!(reply.kind, EventKind::Completed);

    let stop = cursor_hook(&serde_json::json!({
        "hook_event_name": "stop",
        "conversation_id": "cursor-session",
        "status": "completed"
    }))
    .unwrap();
    assert_eq!(stop.kind, EventKind::Completed);

    let aborted = cursor_hook(&serde_json::json!({
        "hook_event_name": "stop",
        "conversation_id": "cursor-session",
        "status": "aborted"
    }))
    .unwrap();
    assert_eq!(aborted.kind, EventKind::Cancelled);

    let failed = cursor_hook(&serde_json::json!({
        "hook_event_name": "stop",
        "conversation_id": "cursor-session",
        "status": "error"
    }))
    .unwrap();
    assert_eq!(failed.kind, EventKind::Failed);

    let ended = cursor_hook(&serde_json::json!({
        "hook_event_name": "sessionEnd",
        "conversation_id": "cursor-session"
    }))
    .unwrap();
    assert_eq!(ended.kind, EventKind::Closed);
}

#[test]
fn cursor_hook_accepts_official_cli_payload_fields() {
    let opened = cursor_hook(&serde_json::json!({
        "hook_event_name": "sessionStart",
        "session_id": "sess-official",
        "conversation_id": "conv-official",
        "workspace_roots": ["/tmp/official-project"],
        "cursor_version": "2026.08.25-3e8eec8",
        "user_email": "dev@example.com",
        "transcript_path": "/tmp/should-not-be-opened.jsonl"
    }))
    .unwrap();
    assert_eq!(opened.kind, EventKind::Idle);
    assert_eq!(opened.source, "cursor");
    assert_eq!(opened.session_id, "sess-official");
    assert_eq!(
        opened.workspace_root.as_deref(),
        Some("/tmp/official-project")
    );
    assert!(opened.metadata.is_empty());

    let working = cursor_hook(&serde_json::json!({
        "hook_event_name": "beforeSubmitPrompt",
        "conversation_id": "conv-official",
        "workspace_roots": ["/tmp/official-project"],
        "cursor_version": "2026.08.25-3e8eec8"
    }))
    .unwrap();
    assert_eq!(working.kind, EventKind::Working);
    assert_eq!(working.session_id, "conv-official");

    let failed = cursor_hook(&serde_json::json!({
        "hook_event_name": "stop",
        "conversation_id": "conv-official",
        "status": "error",
        "cursor_version": "2026.08.25-3e8eec8"
    }))
    .unwrap();
    assert_eq!(failed.kind, EventKind::Failed);
}

#[test]
fn cursor_session_end_with_official_close_fields_is_closed() {
    // Captured from `agent -p` (2026.09.26-dd393fe). reason=completed means
    // the CLI conversation exited, not a mid-turn pause. Adapter maps
    // sessionEnd → Closed; IDE vs CLI filtering is the CLI process check.
    let ended = cursor_hook(&serde_json::json!({
        "hook_event_name": "sessionEnd",
        "conversation_id": "5b9f37e5-d3ee-47e2-9c2f-651a17c9b118",
        "session_id": "5b9f37e5-d3ee-47e2-9c2f-651a17c9b118",
        "generation_id": "5b9f37e5-d3ee-47e2-9c2f-651a17c9b118",
        "reason": "completed",
        "duration_ms": 10316,
        "is_background_agent": false,
        "final_status": "completed",
        "cursor_version": "2026.09.26-dd393fe",
        "workspace_roots": ["/tmp/orbcue-cursor-probe"]
    }))
    .unwrap();
    assert_eq!(ended.kind, EventKind::Closed);
    assert_eq!(ended.source, "cursor");
    assert_eq!(ended.session_id, "5b9f37e5-d3ee-47e2-9c2f-651a17c9b118");
    assert!(ended.metadata.is_empty());

    let started = cursor_hook(&serde_json::json!({
        "hook_event_name": "sessionStart",
        "conversation_id": "5b9f37e5-d3ee-47e2-9c2f-651a17c9b118",
        "session_id": "5b9f37e5-d3ee-47e2-9c2f-651a17c9b118",
        "is_background_agent": false,
        "cursor_version": "2026.09.26-dd393fe",
        "workspace_roots": ["/tmp/orbcue-cursor-probe"]
    }))
    .unwrap();
    assert_eq!(started.kind, EventKind::Idle);
}

#[test]
fn cursor_hook_drops_claude_compat_tool_events() {
    // Captured from Cursor CLI Task child (2026.09.26-dd393fe). Claude
    // settings.json still fires this; mapping it to Working steals the row.
    let child_post = cursor_hook(&serde_json::json!({
        "hook_event_name": "postToolUse",
        "conversation_id": "2f6d3fa6-7264-45c6-9ec0-433e6bf19f5d",
        "session_id": "2f6d3fa6-7264-45c6-9ec0-433e6bf19f5d",
        "generation_id": "2f6d3fa6-7264-45c6-9ec0-433e6bf19f5d",
        "tool_name": "Shell",
        "tool_use_id": "f574fc6d-8d28-45fc-b6da-ab1864eb49fa",
        "cursor_version": "2026.09.26-dd393fe",
        "workspace_roots": ["/tmp/orbcue-cursor-probe3"]
    }));
    assert!(child_post.is_none());
    assert!(cursor_hook(&serde_json::json!({
        "hook_event_name": "PostToolUse",
        "conversation_id": "child-task",
        "tool_name": "Shell"
    }))
    .is_none());
    assert!(cursor_hook(&serde_json::json!({
        "hook_event_name": "PostToolUseFailure",
        "conversation_id": "child-task"
    }))
    .is_none());
    assert!(cursor_hook(&serde_json::json!({
        "hook_event_name": "preToolUse",
        "conversation_id": "child-task",
        "tool_name": "Shell"
    }))
    .is_none());
    assert_eq!(
        claude_hook(&serde_json::json!({
            "hook_event_name": "PostToolUse",
            "session_id": "claude-session",
            "tool_name": "Read"
        }))
        .unwrap()
        .kind,
        EventKind::Working
    );
}

fn apply_cursor(state: &mut DockState, payload: serde_json::Value) -> ApplyResult {
    state.apply(cursor_hook(&payload).unwrap())
}

#[test]
fn cursor_subagent_hooks_stay_on_the_parent_and_defer_completion() {
    let started = cursor_hook(&serde_json::json!({
        "hook_event_name": "subagentStart",
        "conversation_id": "child-1",
        "parent_conversation_id": "parent-1",
        "subagent_id": "sub-1",
        "generation_id": "gen-1",
        "task": "secret task",
        "workspace_roots": ["/tmp/proj"]
    }))
    .unwrap();
    assert_eq!(started.kind, EventKind::Working);
    assert_eq!(started.session_id, "parent-1");
    assert!(started.parent_session_id.is_none());
    assert_eq!(started.metadata.len(), 1);
    assert_eq!(
        started.metadata.get("subagent_phase").map(String::as_str),
        Some("start")
    );
    assert!(!started.event_id.contains("secret"));

    let stopped = cursor_hook(&serde_json::json!({
        "hook_event_name": "subagentStop",
        "conversation_id": "child-1",
        "parent_conversation_id": "parent-1",
        "subagent_id": "sub-1",
        "generation_id": "gen-1",
        "summary": "secret summary",
        "description": "secret description",
        "task": "secret task"
    }))
    .unwrap();
    assert_eq!(stopped.session_id, "parent-1");
    assert!(stopped.parent_session_id.is_none());
    assert_ne!(started.event_id, stopped.event_id);
    assert_eq!(
        stopped.metadata.get("subagent_phase").map(String::as_str),
        Some("stop")
    );
    assert!(stopped
        .metadata
        .values()
        .all(|value| !value.contains("secret")));

    let mut state = DockState::new();
    assert!(
        apply_cursor(
            &mut state,
            serde_json::json!({
                "hook_event_name": "sessionStart",
                "conversation_id": "parent-1",
                "event_id": "parent-1-open",
                "workspace_roots": ["/tmp/proj"]
            })
        )
        .accepted
    );
    assert!(
        apply_cursor(
            &mut state,
            serde_json::json!({
                "hook_event_name": "beforeSubmitPrompt",
                "conversation_id": "parent-1",
                "event_id": "parent-1-prompt"
            })
        )
        .accepted
    );
    assert!(state.apply(started).accepted);
    let held = apply_cursor(
        &mut state,
        serde_json::json!({
            "hook_event_name": "stop",
            "conversation_id": "parent-1",
            "status": "completed",
            "generation_id": "parent-gen",
            "event_id": "parent-1-stop"
        }),
    );
    assert!(held.attention.is_none());
    assert_eq!(held.snapshot.tracked_count, 1);
    assert_eq!(held.snapshot.sessions[0].state, SessionState::Working);
    assert_eq!(held.snapshot.sessions[0].session_id, "parent-1");

    assert!(
        apply_cursor(
            &mut state,
            serde_json::json!({
                "hook_event_name": "subagentStart",
                "parent_conversation_id": "parent-1",
                "conversation_id": "child-2",
                "subagent_id": "sub-2",
                "task": "also secret"
            })
        )
        .accepted
    );
    let first_done = state.apply(stopped);
    assert!(first_done.attention.is_none());
    assert_eq!(first_done.snapshot.sessions[0].state, SessionState::Working);

    let released = apply_cursor(
        &mut state,
        serde_json::json!({
            "hook_event_name": "subagentStop",
            "parent_conversation_id": "parent-1",
            "subagent_id": "sub-2",
            "summary": "secret done"
        }),
    );
    assert_eq!(released.snapshot.tracked_count, 1);
    assert_eq!(released.snapshot.sessions[0].state, SessionState::Completed);
    assert!(released.attention.is_some());

    let mut finished_first = DockState::new();
    assert!(
        apply_cursor(
            &mut finished_first,
            serde_json::json!({
                "hook_event_name": "beforeSubmitPrompt",
                "conversation_id": "parent-2",
                "event_id": "parent-2-prompt"
            })
        )
        .accepted
    );
    assert!(
        apply_cursor(
            &mut finished_first,
            serde_json::json!({
                "hook_event_name": "subagentStart",
                "parent_conversation_id": "parent-2",
                "subagent_id": "sub-a"
            })
        )
        .accepted
    );
    assert!(
        apply_cursor(
            &mut finished_first,
            serde_json::json!({
                "hook_event_name": "subagentStop",
                "parent_conversation_id": "parent-2",
                "subagent_id": "sub-a"
            })
        )
        .accepted
    );
    assert_eq!(
        finished_first.snapshot().sessions[0].state,
        SessionState::Working
    );
    let done = apply_cursor(
        &mut finished_first,
        serde_json::json!({
            "hook_event_name": "stop",
            "conversation_id": "parent-2",
            "status": "completed",
            "event_id": "parent-2-stop"
        }),
    );
    assert_eq!(done.snapshot.sessions[0].state, SessionState::Completed);
    assert!(done.attention.is_some());

    let mut continued = DockState::new();
    assert!(
        apply_cursor(
            &mut continued,
            serde_json::json!({
                "hook_event_name": "beforeSubmitPrompt",
                "conversation_id": "parent-3",
                "event_id": "parent-3-prompt"
            })
        )
        .accepted
    );
    assert!(
        apply_cursor(
            &mut continued,
            serde_json::json!({
                "hook_event_name": "subagentStart",
                "parent_conversation_id": "parent-3",
                "subagent_id": "sub-b"
            })
        )
        .accepted
    );
    let held_again = apply_cursor(
        &mut continued,
        serde_json::json!({
            "hook_event_name": "stop",
            "conversation_id": "parent-3",
            "status": "completed",
            "event_id": "parent-3-stop"
        }),
    );
    assert!(held_again.attention.is_none());
    assert_eq!(held_again.snapshot.sessions[0].state, SessionState::Working);
    assert!(
        apply_cursor(
            &mut continued,
            serde_json::json!({
                "hook_event_name": "beforeSubmitPrompt",
                "conversation_id": "parent-3",
                "event_id": "parent-3-prompt-2"
            })
        )
        .accepted
    );
    let still_working = apply_cursor(
        &mut continued,
        serde_json::json!({
            "hook_event_name": "subagentStop",
            "parent_conversation_id": "parent-3",
            "subagent_id": "sub-b"
        }),
    );
    assert!(still_working.attention.is_none());
    assert_eq!(
        still_working.snapshot.sessions[0].state,
        SessionState::Working
    );

    let mut failed = DockState::new();
    assert!(
        apply_cursor(
            &mut failed,
            serde_json::json!({
                "hook_event_name": "beforeSubmitPrompt",
                "conversation_id": "parent-4",
                "event_id": "parent-4-prompt"
            })
        )
        .accepted
    );
    assert!(
        apply_cursor(
            &mut failed,
            serde_json::json!({
                "hook_event_name": "subagentStart",
                "parent_conversation_id": "parent-4",
                "subagent_id": "sub-c"
            })
        )
        .accepted
    );
    let errored = apply_cursor(
        &mut failed,
        serde_json::json!({
            "hook_event_name": "stop",
            "conversation_id": "parent-4",
            "status": "error",
            "event_id": "parent-4-stop"
        }),
    );
    assert_eq!(errored.snapshot.sessions[0].state, SessionState::Failed);
    assert!(errored.attention.is_some());
    let after_error = apply_cursor(
        &mut failed,
        serde_json::json!({
            "hook_event_name": "subagentStop",
            "parent_conversation_id": "parent-4",
            "subagent_id": "sub-c"
        }),
    );
    assert_eq!(after_error.snapshot.sessions[0].state, SessionState::Failed);
    assert!(after_error.attention.is_none());

    let mut closed = DockState::new();
    assert!(
        apply_cursor(
            &mut closed,
            serde_json::json!({
                "hook_event_name": "sessionStart",
                "conversation_id": "parent-5",
                "event_id": "parent-5-open"
            })
        )
        .accepted
    );
    assert!(
        apply_cursor(
            &mut closed,
            serde_json::json!({
                "hook_event_name": "subagentStart",
                "parent_conversation_id": "parent-5",
                "subagent_id": "sub-d"
            })
        )
        .accepted
    );
    assert!(
        apply_cursor(
            &mut closed,
            serde_json::json!({
                "hook_event_name": "sessionEnd",
                "conversation_id": "parent-5",
                "reason": "completed",
                "final_status": "completed",
                "event_id": "parent-5-end"
            })
        )
        .accepted
    );
    assert_eq!(closed.snapshot().tracked_count, 0);
    assert!(
        apply_cursor(
            &mut closed,
            serde_json::json!({
                "hook_event_name": "subagentStop",
                "parent_conversation_id": "parent-5",
                "subagent_id": "sub-d"
            })
        )
        .accepted
    );
    assert_eq!(closed.snapshot().tracked_count, 0);

    let mut absent = DockState::new();
    assert!(
        apply_cursor(
            &mut absent,
            serde_json::json!({
                "hook_event_name": "subagentStart",
                "parent_conversation_id": "missing-parent",
                "subagent_id": "sub-e",
                "task": "secret task"
            })
        )
        .accepted
    );
    assert_eq!(absent.snapshot().tracked_count, 0);
}

#[test]
fn opencode_adapter_follows_the_plugin_lifecycle() {
    let mut state = DockState::new();
    let opened = opencode_hook(&serde_json::json!({
        "type": "session.created",
        "sessionID": "ses_parent",
        "cwd": "/tmp/orbcue-opencode",
        "event_id": "oc-1",
        "title": "do not keep",
        "questions": ["secret question"]
    }))
    .unwrap();
    assert_eq!(opened.kind, EventKind::Idle);
    assert_eq!(opened.source, "opencode");
    assert_eq!(opened.cwd.as_deref(), Some("/tmp/orbcue-opencode"));
    assert!(opened.metadata.is_empty());
    assert!(opened.terminal_id.as_deref().unwrap().starts_with("oc:"));
    assert!(state.apply(opened).accepted);

    let other = opencode_hook(&serde_json::json!({
        "type": "session.created",
        "sessionID": "ses_other",
        "event_id": "oc-other"
    }))
    .unwrap();
    let parent_terminal = state.snapshot().sessions[0].terminal_id.clone();
    assert_ne!(
        other.terminal_id, parent_terminal,
        "parallel OpenCode sessions must not share a terminal id"
    );
    assert!(state.apply(other).accepted);
    assert_eq!(state.snapshot().tracked_count, 2);

    let busy = opencode_hook(&serde_json::json!({
        "type": "session.status",
        "sessionID": "ses_parent",
        "status": "busy",
        "event_id": "oc-2"
    }))
    .unwrap();
    assert_eq!(busy.kind, EventKind::Working);
    assert!(state.apply(busy).accepted);

    let permission = opencode_hook(&serde_json::json!({
        "type": "permission.asked",
        "sessionID": "ses_parent",
        "event_id": "oc-3",
        "patterns": ["rm -rf /"]
    }))
    .unwrap();
    assert_eq!(permission.kind, EventKind::PermissionRequested);
    assert!(permission.metadata.is_empty());
    let waiting = state.apply(permission);
    let parent = waiting
        .snapshot
        .sessions
        .iter()
        .find(|session| session.session_id == "ses_parent")
        .unwrap();
    assert_eq!(parent.attention_reason.as_deref(), Some("permission"));

    let replied = opencode_hook(&serde_json::json!({
        "type": "permission.replied",
        "sessionID": "ses_parent",
        "event_id": "oc-4"
    }))
    .unwrap();
    assert_eq!(replied.kind, EventKind::Working);
    let resumed = state.apply(replied);
    let parent = resumed
        .snapshot
        .sessions
        .iter()
        .find(|session| session.session_id == "ses_parent")
        .unwrap();
    assert_eq!(parent.state, SessionState::Working);

    let question = opencode_hook(&serde_json::json!({
        "type": "question.asked",
        "sessionID": "ses_parent",
        "event_id": "oc-5"
    }))
    .unwrap();
    assert_eq!(question.kind, EventKind::WaitingInput);
    let asked = state.apply(question);
    let parent = asked
        .snapshot
        .sessions
        .iter()
        .find(|session| session.session_id == "ses_parent")
        .unwrap();
    assert_eq!(parent.attention_reason.as_deref(), Some("input"));
    let answered = opencode_hook(&serde_json::json!({
        "type": "question.replied",
        "sessionID": "ses_parent",
        "event_id": "oc-6"
    }))
    .unwrap();
    let answered = state.apply(answered);
    let parent = answered
        .snapshot
        .sessions
        .iter()
        .find(|session| session.session_id == "ses_parent")
        .unwrap();
    assert_eq!(parent.state, SessionState::Working);

    let idle = opencode_hook(&serde_json::json!({
        "type": "session.status",
        "sessionID": "ses_parent",
        "status": "idle",
        "event_id": "oc-7"
    }))
    .unwrap();
    assert_eq!(idle.kind, EventKind::Completed);
    assert!(state.apply(idle).attention.is_some());
    let again = opencode_hook(&serde_json::json!({
        "type": "session.idle",
        "sessionID": "ses_parent",
        "event_id": "oc-8"
    }))
    .unwrap();
    assert_eq!(again.kind, EventKind::Completed);
    assert!(state.apply(again).attention.is_none());

    let aborted = opencode_hook(&serde_json::json!({
        "type": "session.error",
        "sessionID": "ses_parent",
        "errorName": "MessageAbortedError",
        "event_id": "oc-9"
    }))
    .unwrap();
    assert_eq!(aborted.kind, EventKind::Cancelled);

    let failed = opencode_hook(&serde_json::json!({
        "type": "session.error",
        "sessionID": "ses_parent",
        "errorName": "APIError",
        "event_id": "oc-10"
    }))
    .unwrap();
    assert_eq!(failed.kind, EventKind::Failed);
    let failed = state.apply(failed);
    let parent = failed
        .snapshot
        .sessions
        .iter()
        .find(|session| session.session_id == "ses_parent")
        .unwrap();
    assert_eq!(parent.state, SessionState::Failed);

    let closed = opencode_hook(&serde_json::json!({
        "type": "session.deleted",
        "sessionID": "ses_parent",
        "event_id": "oc-11"
    }))
    .unwrap();
    assert_eq!(closed.kind, EventKind::Closed);
    assert!(state.apply(closed).accepted);
    assert_eq!(state.snapshot().tracked_count, 1);
    assert_eq!(state.snapshot().sessions[0].session_id, "ses_other");

    assert!(opencode_hook(
        &serde_json::json!({"type": "session.updated", "sessionID": "ses_other"})
    )
    .is_none());
    assert!(
        opencode_hook(&serde_json::json!({"type": "session.diff", "sessionID": "ses_other"}))
            .is_none()
    );
}

#[test]
fn pi_adapter_follows_the_extension_lifecycle() {
    let mut state = DockState::new();
    let opened = pi_hook(&serde_json::json!({
        "type": "session.started",
        "sessionID": "pi-session",
        "cwd": "/tmp/orbcue-pi",
        "event_id": "pi-1",
        "title": "do not keep",
        "prompt": "secret prompt"
    }))
    .unwrap();
    assert_eq!(opened.kind, EventKind::Idle);
    assert_eq!(opened.source, "pi");
    assert_eq!(opened.cwd.as_deref(), Some("/tmp/orbcue-pi"));
    assert!(opened.terminal_id.is_none());
    assert!(opened.metadata.is_empty());
    assert!(state.apply(opened).accepted);

    let busy = pi_hook(&serde_json::json!({
        "type": "agent.started",
        "sessionID": "pi-session",
        "event_id": "pi-2"
    }))
    .unwrap();
    assert_eq!(busy.kind, EventKind::Working);
    assert!(state.apply(busy).accepted);

    let permission = pi_hook(&serde_json::json!({
        "type": "permission.asked",
        "sessionID": "pi-session",
        "event_id": "pi-3",
        "title": "Run this command?"
    }))
    .unwrap();
    assert_eq!(permission.kind, EventKind::PermissionRequested);
    assert!(permission.metadata.is_empty());
    let waiting = state.apply(permission);
    assert_eq!(
        waiting.snapshot.sessions[0].attention_reason.as_deref(),
        Some("permission")
    );

    let resumed = pi_hook(&serde_json::json!({
        "type": "agent.started",
        "sessionID": "pi-session",
        "event_id": "pi-4"
    }))
    .unwrap();
    assert_eq!(
        state.apply(resumed).snapshot.sessions[0].state,
        SessionState::Working
    );

    let question = pi_hook(&serde_json::json!({
        "type": "question.asked",
        "sessionID": "pi-session",
        "event_id": "pi-5",
        "title": "Pick one"
    }))
    .unwrap();
    assert_eq!(question.kind, EventKind::WaitingInput);
    assert!(question.metadata.is_empty());
    assert_eq!(
        state.apply(question).snapshot.sessions[0]
            .attention_reason
            .as_deref(),
        Some("input")
    );

    let idle_again = pi_hook(&serde_json::json!({
        "type": "session.started",
        "sessionID": "pi-session",
        "event_id": "pi-6"
    }))
    .unwrap();
    assert_eq!(idle_again.kind, EventKind::Idle);
    assert_eq!(
        state.apply(idle_again).snapshot.sessions[0].state,
        SessionState::Idle
    );

    let done = pi_hook(&serde_json::json!({
        "type": "agent.settled",
        "sessionID": "pi-session",
        "outcome": "completed",
        "event_id": "pi-7"
    }))
    .unwrap();
    assert_eq!(done.kind, EventKind::Completed);
    assert!(state.apply(done).attention.is_some());
    let again = pi_hook(&serde_json::json!({
        "type": "agent.settled",
        "sessionID": "pi-session",
        "event_id": "pi-8"
    }))
    .unwrap();
    assert_eq!(again.kind, EventKind::Completed);
    assert!(state.apply(again).attention.is_none());

    let aborted = pi_hook(&serde_json::json!({
        "type": "agent.settled",
        "sessionID": "pi-session",
        "outcome": "aborted",
        "event_id": "pi-9"
    }))
    .unwrap();
    assert_eq!(aborted.kind, EventKind::Cancelled);
    let failed = pi_hook(&serde_json::json!({
        "type": "agent.settled",
        "sessionID": "pi-session",
        "outcome": "error",
        "event_id": "pi-10"
    }))
    .unwrap();
    assert_eq!(failed.kind, EventKind::Failed);
    assert_eq!(
        state.apply(failed).snapshot.sessions[0].state,
        SessionState::Failed
    );

    let mut replaced = pi_hook(&serde_json::json!({
        "type": "session.started",
        "sessionID": "pi-next",
        "cwd": "/tmp/orbcue-pi",
        "event_id": "pi-11"
    }))
    .unwrap();
    replaced.terminal_id = Some("pts-pi".to_owned());
    state.apply(
        pi_hook(&serde_json::json!({
            "type": "session.started",
            "sessionID": "pi-session",
            "event_id": "pi-12"
        }))
        .unwrap()
        .with_terminal_id("pts-pi"),
    );
    let replaced = state.apply(replaced);
    assert_eq!(replaced.snapshot.tracked_count, 1);
    assert_eq!(replaced.snapshot.sessions[0].session_id, "pi-next");

    let closed = pi_hook(&serde_json::json!({
        "type": "session.closed",
        "sessionID": "pi-next",
        "event_id": "pi-13"
    }))
    .unwrap();
    assert_eq!(closed.kind, EventKind::Closed);
    assert!(state.apply(closed).accepted);
    assert_eq!(state.snapshot().tracked_count, 0);

    assert!(
        pi_hook(&serde_json::json!({"type": "session.shutdown", "sessionID": "pi-next"})).is_none()
    );
    assert!(pi_hook(&serde_json::json!({"type": "tool_call", "sessionID": "pi-next"})).is_none());
}

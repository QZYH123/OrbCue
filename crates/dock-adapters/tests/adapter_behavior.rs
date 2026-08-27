use orbcue_adapters::{
    claude_hook, codex_hook, codex_notification, cursor_hook, dsh_projection, grok_hook,
};
use orbcue_core::{DockEvent, DockState, EventKind, SessionState};

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
    assert_eq!(asking.requires_user_action, Some(true));

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
fn dsh_and_codex_copy_parent_session_id() {
    let dsh = dsh_projection(&serde_json::json!({
        "event": "session.waiting_input",
        "session_id": "child",
        "parent_session_id": "parent"
    }))
    .unwrap();
    assert_eq!(dsh.parent_session_id.as_deref(), Some("parent"));

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
    assert!(dsh_projection(&serde_json::json!({"event":"unknown"})).is_none());
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
fn dsh_permission_then_working_resumes() {
    let permission = dsh_projection(&serde_json::json!({
        "event": "session.permission_requested",
        "session_id": "dsh-session",
        "event_id": "d-perm-1"
    }))
    .expect("DSH session.permission_requested is the dock permission event");
    assert_eq!(permission.kind, EventKind::PermissionRequested);

    let working = dsh_projection(&serde_json::json!({
        "event": "session.working",
        "session_id": "dsh-session",
        "event_id": "d-work-1"
    }))
    .unwrap();
    assert_eq!(apply_permission_then(working), SessionState::Working);
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
    assert_eq!(asking.requires_user_action, Some(true));

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
    assert_eq!(event.summary, None);

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
    assert_eq!(opened.summary, None);
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

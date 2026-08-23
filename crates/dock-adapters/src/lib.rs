//! First-party payload adapters. They consume structured public payloads only.

use agent_activity_dock_core::{DockEvent, EventKind, Severity};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub fn claude_hook(payload: &Value) -> Option<DockEvent> {
    let event_name = payload
        .get("hook_event_name")
        .or_else(|| payload.get("hook_event"))
        .or_else(|| payload.get("hookEventName"))
        .and_then(Value::as_str)
        .map(normalize_hook_event)?;
    let kind = match event_name.as_str() {
        "session_start" => EventKind::Idle,
        "user_prompt_submit" | "pre_tool_use" | "post_tool_use" | "subagent_start" => {
            EventKind::Working
        }
        "permission_request" => EventKind::PermissionRequested,
        "stop" => match payload.get("reason").and_then(Value::as_str).unwrap_or("") {
            "channel_closed" | "shutdown" => EventKind::Closed,
            _ if stop_has_active_subagent(payload) => EventKind::Working,
            _ => EventKind::Completed,
        },
        "stop_failure" => EventKind::Failed,
        "session_end" => EventKind::Closed,
        "notification" => match notification_type(payload)? {
            "permission_prompt" | "permission" => EventKind::PermissionRequested,
            "agent_needs_input" => EventKind::WaitingInput,
            "idle_prompt" | "task_complete" | "agent_completed" => EventKind::Completed,
            _ => return None,
        },
        "subagent_stop" => return None,
        _ => return None,
    };
    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("sessionId"))
        .and_then(Value::as_str)?;
    if is_named_subagent_hook(&event_name) && extract_parent(payload).is_none() {
        return None;
    }
    let mut event = make_event("claude", session_id, kind, payload);
    if matches!(
        kind,
        EventKind::PermissionRequested | EventKind::WaitingInput
    ) {
        event.severity = Severity::Attention;
        event.requires_user_action = Some(true);
    }
    Some(event)
}

pub fn dsh_projection(payload: &Value) -> Option<DockEvent> {
    let kind = match payload.get("event").and_then(Value::as_str)? {
        "session.started" | "session.working" => EventKind::Working,
        "session.waiting_input" => EventKind::WaitingInput,
        "session.completed" => EventKind::Completed,
        "session.failed" => EventKind::Failed,
        "session.cancelled" => EventKind::Cancelled,
        _ => return None,
    };
    let session_id = payload.get("session_id").and_then(Value::as_str)?;
    Some(make_event("dsh", session_id, kind, payload))
}

pub fn grok_hook(payload: &Value) -> Option<DockEvent> {
    if payload
        .get("subagentType")
        .or_else(|| payload.get("subagent_type"))
        .and_then(Value::as_str)
        .is_some()
    {
        return None;
    }
    let event_name = payload
        .get("hookEventName")
        .or_else(|| payload.get("hook_event_name"))
        .or_else(|| payload.get("hook_event"))
        .and_then(Value::as_str)
        .map(normalize_hook_event)?;
    let kind = match event_name.as_str() {
        "session_start" => EventKind::Idle,
        "user_prompt_submit" | "pre_tool_use" | "post_tool_use" => EventKind::Working,
        "stop" => match payload.get("reason").and_then(Value::as_str).unwrap_or("") {
            "channel_closed" | "shutdown" => EventKind::Closed,
            "end_turn" | "" if stop_has_active_subagent(payload) => EventKind::Working,
            "end_turn" | "" => EventKind::Completed,
            _ => return None,
        },
        "stop_failure" => EventKind::Failed,
        "stop_cancelled" => EventKind::Idle,
        "session_end" => EventKind::Closed,
        "notification" => match notification_type(payload)? {
            "permission_prompt" => EventKind::PermissionRequested,
            "idle_prompt" | "task_complete" => EventKind::Completed,
            _ => return None,
        },
        _ => return None,
    };
    let session_id = payload
        .get("sessionId")
        .or_else(|| payload.get("session_id"))
        .and_then(Value::as_str)?;
    let mut event = make_event("grok", session_id, kind, payload);
    if matches!(
        kind,
        EventKind::PermissionRequested | EventKind::WaitingInput
    ) {
        event.severity = Severity::Attention;
        event.requires_user_action = Some(true);
    }
    Some(event)
}

fn stop_has_active_subagent(payload: &Value) -> bool {
    payload
        .get("backgroundTasks")
        .or_else(|| payload.get("background_tasks"))
        .and_then(Value::as_array)
        .is_some_and(|tasks| {
            tasks
                .iter()
                .any(|task| task.get("type").and_then(Value::as_str) == Some("subagent"))
        })
}

fn notification_type(payload: &Value) -> Option<&str> {
    payload
        .get("notificationType")
        .or_else(|| payload.get("notification_type"))
        .or_else(|| payload.get("type"))
        .and_then(Value::as_str)
}

pub fn codex_notification(payload: &Value) -> Option<DockEvent> {
    let kind = match payload
        .get("type")
        .or_else(|| payload.get("event"))
        .and_then(Value::as_str)?
    {
        "session.started" | "started" | "working" => EventKind::Working,
        "session.completed" | "completed" | "stopped" => EventKind::Completed,
        "session.failed" | "failed" | "error" => EventKind::Failed,
        "session.cancelled" | "cancelled" => EventKind::Cancelled,
        _ => return None,
    };
    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("id"))
        .and_then(Value::as_str)?;
    Some(make_event("codex", session_id, kind, payload))
}

fn make_event(source: &str, session_id: &str, kind: EventKind, payload: &Value) -> DockEvent {
    let event_id = payload
        .get("event_id")
        .or_else(|| payload.get("id"))
        .or_else(|| payload.get("occurred_at"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let stamp = payload
                .get("timestamp")
                .or_else(|| payload.get("promptId"))
                .or_else(|| payload.get("prompt_id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| OffsetDateTime::now_utc().unix_timestamp_nanos().to_string());
            format!("{source}-{session_id}-{}-{stamp}", kind_name(kind))
        });
    let mut event = DockEvent::new(&event_id, kind, source, session_id);
    event.occurred_at = payload
        .get("occurred_at")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| OffsetDateTime::now_utc().format(&Rfc3339).ok())
        .unwrap_or(event.occurred_at);
    if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
        event.cwd = Some(cwd.to_owned());
    }
    if let Some(workspace_root) = payload
        .get("workspaceRoot")
        .or_else(|| payload.get("workspace_root"))
        .and_then(Value::as_str)
    {
        event.workspace_root = Some(workspace_root.to_owned());
    }
    if let Some(parent) = extract_parent(payload) {
        event.parent_session_id = Some(parent);
    }
    event
}

fn extract_parent(payload: &Value) -> Option<String> {
    payload
        .get("parent_session_id")
        .or_else(|| payload.get("parentSessionId"))
        .or_else(|| payload.get("parent_agent_id"))
        .or_else(|| payload.get("parentAgentId"))
        .or_else(|| payload.get("parent_id"))
        .or_else(|| payload.get("parentId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn normalize_hook_event(value: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for character in value.chars() {
        if matches!(character, '-' | '_') {
            if !out.ends_with('_') {
                out.push('_');
            }
            prev_lower = false;
            continue;
        }
        if character.is_ascii_uppercase() && prev_lower {
            out.push('_');
        }
        out.push(character.to_ascii_lowercase());
        prev_lower = character.is_ascii_lowercase() || character.is_ascii_digit();
    }
    out
}

fn is_named_subagent_hook(event_name: &str) -> bool {
    normalize_hook_event(event_name).contains("subagent")
}

fn kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Started => "started",
        EventKind::Idle => "idle",
        EventKind::Working => "working",
        EventKind::WaitingInput => "waiting_input",
        EventKind::PermissionRequested => "permission_requested",
        EventKind::Completed => "completed",
        EventKind::Failed => "failed",
        EventKind::Cancelled => "cancelled",
        EventKind::Closed => "closed",
    }
}

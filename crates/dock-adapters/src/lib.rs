//! First-party payload adapters. They consume structured public payloads only.

use orbcue_core::{DockEvent, EventKind, Severity};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub fn claude_hook(payload: &Value) -> Option<DockEvent> {
    map_cli_hook("claude", payload)
}

pub fn codex_hook(payload: &Value) -> Option<DockEvent> {
    map_cli_hook("codex", payload).or_else(|| codex_notification(payload))
}

pub fn cursor_hook(payload: &Value) -> Option<DockEvent> {
    map_cli_hook("cursor", payload)
}

pub fn grok_hook(payload: &Value) -> Option<DockEvent> {
    if json_str(payload, &["subagentType", "subagent_type"]).is_some() {
        return None;
    }
    let event_name = json_str(payload, &["hookEventName", "hook_event_name", "hook_event"])
        .map(normalize_hook_event)?;
    let kind = match event_name.as_str() {
        "session_start" => EventKind::Idle,
        "user_prompt_submit" => EventKind::Working,
        "stop" => match payload.get("reason").and_then(Value::as_str).unwrap_or("") {
            "channel_closed" | "shutdown" => EventKind::Closed,
            "end_turn" | "" if stop_has_active_subagent(payload) => EventKind::Working,
            "end_turn" | "" => EventKind::Completed,
            _ => return None,
        },
        "stop_failure" => EventKind::Failed,
        "stop_cancelled" => EventKind::Idle,
        "session_end" => EventKind::Closed,
        "permission_denied" => EventKind::Working,
        "pre_tool_use" if is_ask_user_question(payload) => EventKind::WaitingInput,
        "post_tool_use" | "post_tool_use_failure" => EventKind::Working,
        "notification" => match notification_type(payload)? {
            "permission_prompt" => EventKind::PermissionRequested,
            "idle_prompt" => EventKind::Completed,
            _ => return None,
        },
        _ => return None,
    };
    let session_id = json_str(payload, &["sessionId", "session_id"])?;
    Some(attentive_event("grok", session_id, kind, payload))
}

fn map_cli_hook(source: &str, payload: &Value) -> Option<DockEvent> {
    let event_name = extract_hook_event(payload)?;
    let kind = lifecycle_kind(&event_name, payload)?;
    let session_id = extract_session_id(payload)?;
    if is_named_subagent_hook(&event_name) && extract_parent(payload).is_none() {
        return None;
    }
    Some(attentive_event(source, session_id, kind, payload))
}

fn extract_hook_event(payload: &Value) -> Option<String> {
    json_str(
        payload,
        &[
            "hook_event_name",
            "hook_event",
            "hookEventName",
            "hookEvent",
        ],
    )
    .map(normalize_hook_event)
}

fn extract_session_id(payload: &Value) -> Option<&str> {
    json_str(
        payload,
        &[
            "session_id",
            "sessionId",
            "conversation_id",
            "conversationId",
            "thread_id",
            "threadId",
        ],
    )
}

fn lifecycle_kind(event_name: &str, payload: &Value) -> Option<EventKind> {
    match event_name {
        "session_start" => Some(EventKind::Idle),
        "user_prompt_submit" | "before_submit_prompt" | "subagent_start" => {
            Some(EventKind::Working)
        }
        "permission_request" => Some(EventKind::PermissionRequested),
        "permission_denied" => Some(EventKind::Working),
        "pre_tool_use" if is_ask_user_question(payload) => Some(EventKind::WaitingInput),
        "post_tool_use" | "post_tool_use_failure" => Some(EventKind::Working),
        "stop" | "after_agent_response" => Some(stop_kind(payload)),
        "stop_failure" => Some(EventKind::Failed),
        "session_end" => Some(EventKind::Closed),
        "notification" => match notification_type(payload)? {
            "permission_prompt" | "permission" => Some(EventKind::PermissionRequested),
            "agent_needs_input" => Some(EventKind::WaitingInput),
            "idle_prompt" => Some(EventKind::Completed),
            _ => None,
        },
        "subagent_stop" => None,
        _ => None,
    }
}

fn stop_kind(payload: &Value) -> EventKind {
    match payload.get("status").and_then(Value::as_str) {
        Some("error") => return EventKind::Failed,
        Some("aborted") => return EventKind::Cancelled,
        _ => {}
    }
    match payload.get("reason").and_then(Value::as_str).unwrap_or("") {
        "channel_closed" | "shutdown" => EventKind::Closed,
        _ if stop_has_active_subagent(payload) => EventKind::Working,
        _ => EventKind::Completed,
    }
}

fn stop_has_active_subagent(payload: &Value) -> bool {
    payload
        .get("backgroundTasks")
        .or_else(|| payload.get("background_tasks"))
        .and_then(Value::as_array)
        .is_some_and(|tasks| tasks.iter().any(is_running_background_subagent))
}

fn is_running_background_subagent(task: &Value) -> bool {
    let Some(kind) = task.get("type").and_then(Value::as_str) else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("subagent") {
        return false;
    }
    match task.get("status").and_then(Value::as_str) {
        None => true,
        Some(status) => matches!(
            status.to_ascii_lowercase().as_str(),
            "running" | "in_progress" | "active" | "pending"
        ),
    }
}

fn notification_type(payload: &Value) -> Option<&str> {
    json_str(payload, &["notificationType", "notification_type", "type"])
}

fn tool_name(payload: &Value) -> Option<&str> {
    json_str(payload, &["toolName", "tool_name"])
}

fn is_ask_user_question(payload: &Value) -> bool {
    tool_name(payload).is_some_and(|name| {
        matches!(
            normalize_hook_event(name).as_str(),
            "ask_user_question" | "ask_question"
        )
    })
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
    let session_id = json_str(payload, &["session_id", "id"])?;
    Some(make_event("codex", session_id, kind, payload))
}

fn make_event(source: &str, session_id: &str, kind: EventKind, payload: &Value) -> DockEvent {
    let event_id = json_str(payload, &["event_id", "id", "occurred_at"])
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let stamp = json_str(
                payload,
                &[
                    "timestamp",
                    "promptId",
                    "prompt_id",
                    "generation_id",
                    "generationId",
                    "turn_id",
                    "turnId",
                ],
            )
            .map(str::to_owned)
            .unwrap_or_else(|| OffsetDateTime::now_utc().unix_timestamp_nanos().to_string());
            format!("{source}-{session_id}-{}-{stamp}", kind_name(kind))
        });
    let mut event = DockEvent::new(&event_id, kind, source, session_id);
    event.occurred_at = json_str(payload, &["occurred_at"])
        .map(str::to_owned)
        .or_else(|| OffsetDateTime::now_utc().format(&Rfc3339).ok())
        .unwrap_or(event.occurred_at);
    if let Some(cwd) = json_str(payload, &["cwd"]) {
        event.cwd = Some(cwd.to_owned());
    }
    if let Some(workspace_root) = json_str(payload, &["workspaceRoot", "workspace_root"])
        .map(str::to_owned)
        .or_else(|| first_workspace_root(payload))
    {
        event.workspace_root = Some(workspace_root);
    }
    if let Some(parent) = extract_parent(payload) {
        event.parent_session_id = Some(parent);
    }
    event
}

fn attentive_event(source: &str, session_id: &str, kind: EventKind, payload: &Value) -> DockEvent {
    let mut event = make_event(source, session_id, kind, payload);
    if matches!(
        kind,
        EventKind::PermissionRequested | EventKind::WaitingInput
    ) {
        event.severity = Severity::Attention;
        event.requires_user_action = Some(true);
    }
    event
}

fn extract_parent(payload: &Value) -> Option<String> {
    json_str(
        payload,
        &[
            "parent_session_id",
            "parentSessionId",
            "parent_agent_id",
            "parentAgentId",
            "parent_id",
            "parentId",
            "parent_conversation_id",
            "parentConversationId",
        ],
    )
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(str::to_owned)
}

fn json_str<'a>(payload: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_str))
}

fn first_workspace_root(payload: &Value) -> Option<String> {
    payload
        .get("workspace_roots")
        .or_else(|| payload.get("workspaceRoots"))
        .and_then(Value::as_array)
        .and_then(|roots| roots.first())
        .and_then(Value::as_str)
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

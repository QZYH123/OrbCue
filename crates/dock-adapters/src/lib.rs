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
    if is_cursor_unsubscribed_tool_event(payload) {
        return None;
    }
    if let Some(event_name) = extract_hook_event(payload) {
        if event_name == "subagent_start" || event_name == "subagent_stop" {
            return cursor_subagent_hook(payload, &event_name);
        }
    }
    map_cli_hook("cursor", payload)
}

/// Cursor fires `stop` when the parent generation ends, even if a background
/// subagent is still running. These two hooks count that work on the parent
/// row. Task text stays out of the event.
fn cursor_subagent_hook(payload: &Value, event_name: &str) -> Option<DockEvent> {
    let phase = if event_name == "subagent_start" {
        "start"
    } else {
        "stop"
    };
    let session_id =
        extract_parent(payload).or_else(|| extract_session_id(payload).map(str::to_owned))?;
    let mut event = attentive_event("cursor", &session_id, EventKind::Working, payload);
    event.parent_session_id = None;
    event.metadata.clear();
    event
        .metadata
        .insert("subagent_phase".to_owned(), phase.to_owned());
    let token = json_str(
        payload,
        &["subagent_id", "subagentId", "generation_id", "generationId"],
    )
    .unwrap_or("event");
    event.event_id = cursor_subagent_event_id(&session_id, phase, token);
    Some(event)
}

fn cursor_subagent_event_id(session_id: &str, phase: &str, token: &str) -> String {
    let id = format!("cursor-{session_id}-subagent-{phase}-{token}");
    if id.len() <= orbcue_core::MAX_EVENT_ID_LEN {
        return id;
    }
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in session_id.as_bytes().iter().chain(token.as_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("cursor-subagent-{phase}-{hash:016x}")
}

/// Cursor's own hooks.json does not subscribe to Pre/Post tool. Cursor CLI
/// still loads Claude Code `~/.claude/settings.json` and remaps those events
/// to source `cursor`. A Task/subagent `PostToolUse` uses the child's
/// `conversation_id` and would otherwise create/steal the terminal row, so
/// the parent's later `stop` cannot mark the visible session completed.
pub fn is_cursor_unsubscribed_tool_event(payload: &Value) -> bool {
    let Some(event_name) = extract_hook_event(payload) else {
        return false;
    };
    match event_name.as_str() {
        "post_tool_use" | "post_tool_use_failure" => true,
        "pre_tool_use" => !is_ask_user_question(payload),
        _ => false,
    }
}

/// OpenCode plugin events. The plugin forwards lifecycle fields only.
/// `session.created` is idle, `session.status` `busy`/`retry` is working, and
/// both `session.status` `idle` and `session.idle` are completed. A second
/// completed event does not cue again. `permission.replied` returns to
/// working for allow and reject. `question.asked` waits for input.
/// `MessageAbortedError` is cancelled; any other `session.error` is failed.
/// `session.deleted` closes the row. A main session gets its own `oc:` terminal
/// id so parallel conversations in one TUI do not retire each other.
pub fn opencode_hook(payload: &Value) -> Option<DockEvent> {
    let event_type = json_str(payload, &["type"])?;
    let session_id = json_str(payload, &["sessionID", "session_id", "sessionId"])?;
    let kind = match event_type {
        "session.created" => EventKind::Idle,
        "session.idle" => EventKind::Completed,
        "session.deleted" => EventKind::Closed,
        "session.status" => match json_str(payload, &["status"])? {
            "busy" | "retry" => EventKind::Working,
            "idle" => EventKind::Completed,
            _ => return None,
        },
        "session.error" => match json_str(payload, &["errorName", "error_name"]).unwrap_or("") {
            "MessageAbortedError" => EventKind::Cancelled,
            _ => EventKind::Failed,
        },
        "permission.asked" => EventKind::PermissionRequested,
        "permission.replied" => EventKind::Working,
        "question.asked" => EventKind::WaitingInput,
        "question.replied" | "question.rejected" => EventKind::Working,
        _ => return None,
    };
    let mut event = attentive_event("opencode", session_id, kind, payload);
    if event.parent_session_id.is_none() {
        event.terminal_id = Some(opencode_terminal_id(session_id));
    }
    if event.event_id.len() > orbcue_core::MAX_EVENT_ID_LEN {
        event.event_id = format!(
            "opencode-{}-{}",
            kind_name(kind),
            opencode_terminal_id(session_id)
        );
    }
    Some(event)
}

/// Pi extension events. The extension forwards lifecycle fields only.
/// `session.started` is idle and `agent.started` is working. `agent.settled`
/// uses the outcome remembered earlier: `aborted` is cancelled, `error` is
/// failed, and anything else is completed. A confirm dialog asks for
/// permission. Other prompts wait for input. `session.closed` is quit only.
/// Terminal id stays unset, so one Pi process keeps one row and a later
/// session on that terminal replaces it.
pub fn pi_hook(payload: &Value) -> Option<DockEvent> {
    let event_type = json_str(payload, &["type"])?;
    let session_id = json_str(payload, &["sessionID", "session_id", "sessionId"])?;
    let kind = match event_type {
        "session.started" => EventKind::Idle,
        "agent.started" => EventKind::Working,
        "agent.settled" => match json_str(payload, &["outcome"]).unwrap_or("completed") {
            "aborted" => EventKind::Cancelled,
            "error" => EventKind::Failed,
            _ => EventKind::Completed,
        },
        "permission.asked" => EventKind::PermissionRequested,
        "question.asked" => EventKind::WaitingInput,
        "session.closed" => EventKind::Closed,
        _ => return None,
    };
    let mut event = attentive_event("pi", session_id, kind, payload);
    if event.event_id.len() > orbcue_core::MAX_EVENT_ID_LEN {
        let tail = event.event_id.len().saturating_sub(32);
        event.event_id = format!("pi-{}-{}", kind_name(kind), &event.event_id[tail..]);
    }
    Some(event)
}

fn opencode_terminal_id(session_id: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in session_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("oc:{hash:016x}")
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
            "parentID",
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

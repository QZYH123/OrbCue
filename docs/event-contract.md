# Dock Event Contract (MVP)

This is the public integration surface.  Adapters, Skills, and MCP tools must
only use this contract; they must not import Dock internals or read Agent
content.

## Transport

- Unix domain socket, current user only, no network listener.
- Newline-delimited JSON, one request per connection.
- Maximum request line: **16 KiB**.  Larger messages get
  `message_too_large` and do not change state.
- Socket location:
  1. `AGENT_ACTIVITY_DOCK_SOCKET` env var, if set;
  2. `$XDG_RUNTIME_DIR/agent-activity-dock/agent-activity-dock.sock`;
  3. `~/.local/state/agent-activity-dock/agent-activity-dock.sock`.

The CLI resolves the same locations:

```bash
dock --socket "$AGENT_ACTIVITY_DOCK_SOCKET" start my-task --source my-agent
```

## Event request

Required string fields:

```json
{
  "task_id": "session-123",
  "source": "claude",
  "event_id": "uuid-or-monotonic-id",
  "action": "start"
}
```

Optional string fields:

```json
{
  "occurred_at": "2026-08-15T10:00:00Z",
  "terminal": "wezterm://..."
}
```

`task_id` is one independently trackable task/session, never an Agent type
name.  Reuse the same `event_id` for retries; Dock de-duplicates by event id
and by task state transition.

## Actions

| Action | Meaning | Counting / attention |
| --- | --- | --- |
| `start` | Task starts, or a waiting task resumes after user input | New task adds to `tracked_count` and `working_count`; repeated start is no-op |
| `stop` | Normal end | `working_count` decreases; one prompt; leaves `!` |
| `waiting` | Waiting for user input/approval | `working_count` decreases; one prompt; leaves `!`; later `start` resumes same task without adding one |
| `error` | Failed end | `working_count` decreases; one prompt; leaves `!` |
| `reset` | Manual recovery from lost/stale state | Removes `task_id`; `task_id="*"` removes all tracked tasks |

Terminal events for an unknown `task_id` are rejected with `unknown_task` and
do not create tracking entries.  Unknown actions are rejected with
`unknown_action`.

## Response

Accepted:

```json
{
  "ok": true,
  "accepted": true,
  "rejection_reason": null,
  "attention": {"task_id": "session-123", "reason": "stop"},
  "snapshot": {
    "working_count": 2,
    "tracked_count": 3,
    "pending_count": 1,
    "count_label": "2/3",
    "border_state": "working",
    "tasks": [
      {
        "task_id": "session-123",
        "source": "claude",
        "working": false,
        "needs_attention": true,
        "last_action": "stop",
        "terminal": null
      }
    ]
  }
}
```

Rejected events return `"accepted": false` and the unchanged snapshot.  A
repeated terminal event returns accepted with `"attention": null`.

Snapshot query:

```json
{"query": "snapshot"}
```

## Raw example

```bash
printf '%s\n' \
  '{"task_id":"job-1","source":"my-agent","event_id":"e-1","action":"start"}' \
  | socat - UNIX-CONNECT:"$AGENT_ACTIVITY_DOCK_SOCKET"
```

Or simply use `dock start|stop|waiting|error|reset` and `dock status`.

## Adapter mapping

| Source | Public surface used | Translated actions |
| --- | --- | --- |
| Claude Code | `settings.json` hooks (`SessionStart`, `PreToolUse`, `PermissionRequest`, `SessionEnd`, `StopFailure`) | start, waiting, stop, error |
| Codex | user-level wrapper around original `codex` (no stable Hook today) | start, stop, error |
| DSH | user-level wrapper; explicit projection payload `session.*` | start, stop, waiting, error |

Privacy: the Dock and its generated hooks/wrappers never open
`transcript_path`, prompts, commands, code, or terminal output.  A regression
test proves a payload carrying `transcript_path` does not leak that path into
Dock.

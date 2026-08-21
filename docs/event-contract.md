# Dock Event Contract

这是 Agent Activity Dock 的稳定集成边界。适配器只发送结构化生命周期事件，不得导入状态机内部类型，也不得读取 Agent 内容。

## Transport

当前实现使用当前用户的本地 IPC，newline-delimited JSON，每个普通请求一行、一个连接。Unix 使用 domain socket；Windows 使用 named pipe。路径/名称按以下规则决定：

1. `AGENT_ACTIVITY_DOCK_SOCKET`
2. Windows：`\\.\\pipe\\agent-activity-dock`
3. Unix：`$XDG_RUNTIME_DIR/agent-activity-dock/agent-activity-dock.sock`
4. Unix fallback：`~/.local/state/agent-activity-dock/agent-activity-dock.sock`

请求行最大 16 KiB。服务端先限制大小，再解析 JSON；错误、超大或未知请求不会改变状态。Windows named pipe 不监听 TCP/UDP 网络端口，仍只供当前用户本机服务使用。

## Event

```json
{
  "version": 1,
  "type": "session.started",
  "event_id": "claude-session-123-start-1",
  "source": "claude",
  "session_id": "session-123",
  "occurred_at": "2026-08-16T08:00:00Z",
  "severity": "info",
  "summary": "optional short in-memory label",
  "deep_link": "https://example.invalid/session/123",
  "requires_user_action": false,
  "metadata": {"workspace": "optional-bounded-value"}
}
```

必填字段是 `version`、`type`、`event_id`、`source`、`session_id` 和 RFC3339 `occurred_at`。`severity` 默认为 `info`，其余字段可省略。未知 JSON 字段会被忽略；Dock 不把原始 payload 写入状态文件。

大小限制：`event_id` 128 字节、`source` 64 字节、`session_id` 256 字节、`summary` 512 字节、`deep_link` 2048 字节、metadata 最多 32 项且 key/value 各 256 字节。

事件时间超过当前时间 24 小时，或超前超过 5 分钟，会返回 `stale_event`。这避免服务离线恢复后突然播放很久以前的提醒。`DockEvent::new` 也会生成当前 RFC3339 时间；外部集成不应发送伪造的 epoch 时间。

## Lifecycle types

| type | 含义 | 结果 |
| --- | --- | --- |
| `session.started` | 新会话开始并进入工作 | 创建/恢复工作会话，静默 |
| `session.idle` | 会话存在但未在工作 | 标记 `o`，计入打开总数 |
| `session.working` | 会话继续工作 | 更新工作状态，静默 |
| `session.waiting_input` | 等待用户文字输入 | 进入待查看，播放一次 attention 提示 |
| `session.permission_requested` | 等待授权 | 进入待查看，播放一次 attention 提示 |
| `session.completed` | 一轮对话自然结束（会话仍打开） | 标记 `*`，仍计入打开总数 |
| `session.failed` | 工作流被失败打断（会话仍打开） | 标记 `!`，仍计入打开总数 |
| `session.cancelled` | 本轮取消（会话仍打开） | 标记 `x`，仍计入打开总数 |
| `session.closed` | 会话真正关闭 | 从打开列表和总数中移除 |

打开中的会话在 `completed` / `failed` / `cancelled` 之后仍可再次进入工作。只有 `session.closed` 会从打开总数中移除该会话。事件 ID 在有界窗口内去重。

## Response and queries

普通事件返回：

```json
{
  "ok": true,
  "accepted": true,
  "rejection_reason": null,
  "attention": {
    "source": "claude",
    "session_id": "session-123",
    "reason": "completed",
    "severity": "info"
  },
  "snapshot": {
    "working_count": 0,
    "tracked_count": 1,
    "pending_count": 1,
    "pending_mark": "?",
    "count_label": "0/1",
    "border_state": "idle",
    "sessions": [],
    "audit": []
  }
}
```

`audit` 是最近的状态变更流，最多保留 128 条，只包含 `source`、`session_id`、状态、等待原因和时间；不会包含摘要或原始 payload。它只在当前运行期间保留，重启后从空流开始。面板的“审计”页按时间倒序展示它。

查询使用同一个 socket：

```json
{"query":"snapshot"}
{"query":"subscribe"}
{"query":"acknowledge","source":"claude","session_id":"session-123"}
{"query":"reset","source":"claude","session_id":"session-123"}
```

`subscribe` 连接会先收到 `type=subscribed`，之后每次状态变化收到 `type=snapshot`。`acknowledge` 只清除待查看标记；`reset` 显式移除指定会话，`source` 或 `session_id` 为 `*` 时匹配全部对应项。reset 不控制 Agent，也不会发送生命周期事件。

## CLI

集成不需要自己实现 socket 客户端：

```bash
dock start session-123 --source claude
dock permission session-123 --source claude
dock complete session-123 --source claude
dock acknowledge --source claude --session-id session-123
dock reset --source claude --session-id session-123
```

## First-party adapters

| source | 输入 | 能力 |
| --- | --- | --- |
| Claude | `settings.json` hooks：`SessionStart`、`PreToolUse`、`PermissionRequest`、`SessionEnd`、`StopFailure` | working、permission、completed、failed |
| Codex | 结构化 notification payload | working、completed、failed、cancelled |
| DSH | `session.*` projection payload | working、waiting、completed、failed、cancelled |

适配器只读取结构化 stdin。即使 payload 含有 `transcript_path`，也不会打开、保存或转发该路径。

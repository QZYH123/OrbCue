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
  "cwd": "/home/user/project",
  "workspace_root": "/home/user/project",
  "parent_session_id": "optional-parent-session",
  "terminal_id": "optional-terminal-identity",
  "requires_user_action": false,
  "metadata": {"workspace": "optional-bounded-value"}
}
```

必填字段是 `version`、`type`、`event_id`、`source`、`session_id` 和 RFC3339 `occurred_at`。`severity` 默认为 `info`，其余字段可省略。未知 JSON 字段会被忽略；Dock 不把原始 payload 写入状态文件。

可选 `cwd` / `workspace_root`，以及 metadata 同义键 `workspaceRoot`、`workspace_root`、`cwd`；空字符串视为缺失。路径只使用这些明确字段，不会读取磁盘或进程工作目录。路径按发送侧原样保存，不做 WSL ↔ Windows 翻译。

可选 `parent_session_id` 标记子代理事件，长度上限与 `session_id` 相同（256 字节）；空字符串视为缺失。带 parent 的事件永不创建独立会话，也不进入 `sessions` 计数。`waiting_input` / `permission_requested` / `failed` 在父会话（`source` + `parent_session_id`）存在时折叠为父会话的 attention / failed 标记，并复用已有 attention 去重。父会话不存在，或其他事件类型，一律 accepted 且无副作用。

可选 `terminal_id` 标识同一用户终端，长度上限 128 字节；空字符串视为缺失。`dock` CLI 在 hook 与 `start`/`complete` 等事件命令上自动附加，所有平台同一顺序：`AGENT_ACTIVITY_DOCK_TERMINAL_ID` 显式覆盖（设成空串则省略、不再探测）→ 自身 tty（Unix `ttyname(stderr/stdin)` 再 `/dev/tty`）→ **祖先进程 tty**（Linux 沿 `/proc/<pid>/stat` 的 ppid 最多向上 10 级，读 fd 0/1/2 的 readlink 或 stat 第 7 字段 `tty_nr`，再经 `ttyname` 规范成与自身 tty 相同的 `/dev/pts/N` 形式）→ `WT_SESSION`（垫底，主要给没有 `/proc` 的 Windows 原生进程）。同一终端里 wrapper 与 setsid hook 必须落到同一个设备路径才能互相顶替；`WT_SESSION` 不能当主键，否则同一 WT 标签里的 tmux pane 会误合成一条。`started` / `working` / `idle` 且无 `parent_session_id` 的事件若带 `terminal_id` T，会先移除同一 T 下的其他会话（跨 source，一个终端同一时刻只有一个前台主会话；移除记入 audit），再创建或更新目标会话。无 `terminal_id` 的事件和会话不受影响；带 parent 的事件永不触发顶替。`state.json` 会保存 `terminal_id`，缺省字段的旧文件仍可读取。

同一条 CLI 事件路径上，无 parent 的 `started` / `working` / `idle` / `completed` / `failed` / `waiting_input` / `permission_requested` 还会把标题写成 `{source} · {项目路径末段}`（无路径时只写 `{source}`）。末段算法与跳回用的 `project_path_hint` 同源。标题写入是尽力而为：部分 WSL→Windows Terminal 组合不转发 OSC 标题（实测存在标签标题恒为配置名、任何写入都不生效的环境），自己重写标题的 TUI 也会覆盖它；标题写入不是跳回前提。Unix 先向 `/dev/tty` 写 OSC `\x1b]0;…\x07`；没有控制终端但祖先 tty 存在时，以 `O_WRONLY|O_NOCTTY` 打开该 pts 写同一序列。Windows `dock.exe` 调用 `SetConsoleTitleW`。`AGENT_ACTIVITY_DOCK_NO_TITLE=1` 完全跳过；写失败静默忽略，不影响事件投递和退出码。面板「回去」首选事件到达时捕获的前台终端窗口句柄（presenter 校验为终端类窗口才记录）；句柄缺失或失效时，才在终端类窗口（Windows Terminal / conhost / alacritty 等）里按标题匹配，先项目名末段、零匹配再用 `source` 子串兜底；浏览器标签不参与。任一步多匹配或两级都零匹配则报「终端窗口匹配不唯一」或「没有找到匹配的终端窗口」并列线索。这一过滤只发生在用户点「回去」时，不做状态推断。Windows Terminal 只暴露活动标签标题，后台标签仍可能找不到。

大小限制：`event_id` / `terminal_id` 各 128 字节、`source` 64 字节、`session_id` / `parent_session_id` 各 256 字节、`summary` 512 字节、`deep_link` 2048 字节、`cwd` / `workspace_root` 各 256 字节、metadata 最多 32 项且 key/value 各 256 字节。

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

小球上的工作数 / 追踪数只统计**主会话**：`started` / `working` / `idle` 可为未知 key 创建记录；`waiting_input` / `permission_requested` / `completed` / `failed` / `cancelled` 对未知会话 accepted，但不建记录、不发 attention。因此用户 reset/clear 之后迟到的 stop / notification 不会凭空复活计数。这类孤儿事件只写 debug 日志，不进 128 条上限的 audit 流。

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
| Claude | `settings.json` hooks：`SessionStart`、`PreToolUse`、`PermissionRequest`、`SessionEnd`、`StopFailure`；`SubagentStart` / `SubagentStop` 及带 parent 线索的 payload | working、permission、completed、failed；子代理填 `parent_session_id` 或丢弃 |
| Codex | 结构化 notification payload | working、completed、failed、cancelled；payload 带 parent 线索时填 `parent_session_id` |
| DSH | `session.*` projection payload | working、waiting、completed、failed、cancelled；payload 带 parent 线索时填 `parent_session_id` |
| Grok | 结构化 hook payload | idle、working、permission、completed、failed、closed；带 `subagentType` 的 payload 仍丢弃 |

适配器只读取结构化 stdin。即使 payload 含有 `transcript_path`，也不会打开、保存或转发该路径。

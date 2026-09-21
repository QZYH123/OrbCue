# Dock Event Contract

这是 OrbCue 的稳定集成边界。适配器只发送结构化生命周期事件，不得导入状态机内部类型，也不得读取 Agent 内容。

## Transport

当前用户本机 IPC：一行一个 JSON 请求。Windows 命名管道，Unix domain socket。桌面进程对本机管道 `attach_or_listen`；WSL `orb` 把事件/`status`/`up` trampoline 到 `orb.exe`。`ORBCUE_BACKEND=wsl` 已删除。不监听 TCP/UDP。

端点：`ORBCUE_SOCKET` → Windows `\\.\pipe\orbcue` → Unix `$XDG_RUNTIME_DIR/orbcue/orbcue.sock` → `~/.local/state/orbcue/orbcue.sock`。请求最大 16 KiB；先限大小再解析。非法、超大或未知请求不改状态。路径按发送侧原样写入 `state.json`，不做盘符翻译。

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
  "metadata": {"workspace": "optional-bounded-value"}
}
```

必填字段是 `version`、`type`、`event_id`、`source`、`session_id` 和 RFC3339 `occurred_at`。`severity` 默认为 `info`，其余字段可省略。未知 JSON 字段会被忽略；Dock 不把原始 payload 写入状态文件。

可选 `cwd` / `workspace_root`，以及 metadata 同义键 `workspaceRoot`、`workspace_root`、`cwd`；空字符串视为缺失。路径只使用这些明确字段，不会读取磁盘或进程工作目录。路径按发送侧原样保存，不做 WSL ↔ Windows 翻译。

可选 `parent_session_id`（上限同 `session_id`，空串视为缺失）。带 parent 的事件永不创建独立会话、不进计数。仅 `waiting_input` / `permission_requested` / `failed` 在父会话（`source` + `parent_session_id`）存在时折叠到父会话；其它情况 accepted、无副作用。

可选 `terminal_id`（上限 128 字节，空串视为缺失）。CLI 在 hook 与 `start`/`complete` 等命令上按此顺序附加：`ORBCUE_TERMINAL_ID`（空串则省略）→ 自身 tty → Linux 祖先进程 tty（最多 10 级）→ `WT_SESSION`（仅 Windows）。无 parent 的 `started` / `working` / `idle` 若带 T，且该 `source`+`session_id` 尚不在列表里，则移除同一 T 上其它会话。例外：T 上已有带 `project_path` 且为 working / needs_attention 的会话，而新事件没有项目路径 → 不顶替、不建行。后续 idle/working 不再赶走其它会话。无显式 `terminal_id` 但有完整 hook liveness 时，写入 `live:{pid}:{starttime}`。显式 tty / WT / `orb:` 优先。带 parent 的事件不顶替。`state.json` 保存 `terminal_id` 和 `project_path`；摘要不落盘。

无 parent 的生命周期事件会尽力写终端标题 `{项目末段} · {source}`，`orb:` 标记再追加。Unix 写 OSC，Windows 调 `SetConsoleTitleW`。`ORBCUE_NO_TITLE=1` 跳过。写失败不影响事件。标题不是跳回前提。

面板「回去」只在用户点击时执行：

1. `deep_link`
2. `terminal_id` 为 `orb:` + 6 位十六进制 → 按标题/标签名聚焦（由 `orb run` 建立；造标细节见 [how-it-works.md](how-it-works.md)）
3. 新主会话或转入 working 时捕获的前台终端 HWND（仍在且仍是终端才用；不冒充标签级精确）
4. 否则报找不到窗口，并提示 `orb run`。不按项目名或 `source` 子串模糊匹配

浏览器窗口不参与。

Liveness 仅 hook 路径写入（`orb start`/`complete` 不写）：`agent_os`、`agent_pid`、`agent_starttime` 三项齐全才合并；可选 `agent_wsl_distro`。结束类事件不写。已有三元组不被后来不同的 PID 覆盖。`--detach` 必须在父进程仍挂在 agent 树上时快照 tty/活性。Linux 向上走父进程时跳过 `sh` / `dash` / `orb` 和 WSL `Relay(` / `SessionLeader` / `init-systemd`。不进 snapshot。daemon 每 15s 问「是否仍是原进程」，死亡则发 `session.closed`。不扫进程表，不因 HWND 消失删会话。

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

小球上的工作数 / 追踪数只统计**主会话**：`started` / `working` / `idle` 可为未知 key 创建记录；`waiting_input` / `permission_requested` / `completed` / `failed` / `cancelled` 对未知会话 accepted，但不建记录、不发 attention。因此用户 reset/clear 之后迟到的 stop / notification 不会凭空复活计数。这类孤儿事件只写 debug 日志，不进 128 条上限的 audit 流。同一 `source`+`session_id` 若来自**另一个还活着的进程**（hook liveness 的 os/pid/starttime 不同，典型是两个终端里 `grok --resume` 同一段对话），则再开一条主会话，不改写已有那条。关掉其中一条（含该进程的 `session.closed` 或 liveness 收割）只移除对应那条，另一条仍在列表里。同一进程后续事件仍合并到自己那条。

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
    "sessions": [],
    "audit": []
  }
}
```

`audit` 是最近的状态变更流，最多保留 128 条，包含 `source`、`session_id`、状态、等待原因、时间和可选 `project_path`；不会包含摘要或原始 payload。只记录 `needs_attention` / `completed` / `failed` / `cancelled` / `closed`，不记录 `working` / `idle`，避免回合内刷屏。`session.closed` 与同终端顶替记为 `closed`，不把消失前的状态再抄一遍。它只在当前运行期间保留，重启后从空流开始。面板的“审计”页按时间倒序展示它。

查询使用同一个 socket：

```json
{"query":"snapshot"}
{"query":"subscribe"}
{"query":"acknowledge","source":"claude","session_id":"session-123"}
{"query":"reset","source":"claude","session_id":"session-123"}
```

`subscribe` 连接会先收到 `type=subscribed`，之后每次状态变化收到 `type=snapshot`。`acknowledge` 只清除待查看标记；`reset` 显式移除指定会话，`source` 或 `session_id` 为 `*` 时匹配全部对应项。可选 `terminal_id` 只作用于该终端上的那一条（两个进程 resume 同一段对话时，面板「清除」只去掉点的那张卡）。不带 `terminal_id` 的 reset 仍按 `source`+`session_id` 去掉全部实例。reset 不控制 Agent，也不会发送生命周期事件。`session.closed` 只关闭对得上的那一条（hook liveness 或 `terminal_id`）。进程退出常连发两条 `closed`（如 Grok `Stop shutdown` 再 `SessionEnd`）；第二条对不上已关掉的那条时，不得把剩下那条当唯一匹配删掉。

## CLI

集成不需要自己实现 socket 客户端：

```bash
orb start session-123 --source claude
orb permission session-123 --source claude
orb complete session-123 --source claude
orb acknowledge --source claude --session-id session-123
orb reset --source claude --session-id session-123
```

## First-party adapters

| source | 输入 | 能力 |
| --- | --- | --- |
| Claude | 结构化 hook payload | idle、working、permission、waiting、completed、failed、closed；`UserPromptSubmit` 标工作中；`PermissionRequest` 标授权；`PermissionDenied` 仅 auto mode 分类器拒绝时回到 working，**用户在授权框点 No 不发 hook**，会停在授权直到后续 `PostToolUse` 或 `Stop`；允许命令后 `PostToolUse`/`PostToolUseFailure` 回到 working；`Stop` 在仍有 background subagent 时保持 working，否则已完成；`SessionEnd` 为关闭（不是已完成）；带 parent 线索的子代理 permission/failed 可折叠，否则丢弃。不订阅通用 PreToolUse；仅 matcher `AskUserQuestion`：选择题 → waiting。`PostToolUse`/`PostToolUseFailure` 登记 `async: true`，不挡主循环。hook 是观察者，投递失败必须 exit 0 |
| Codex | 结构化 hook payload，notification 仅作回退 | 与 Claude 同一套回合生命周期；`SessionStart`→idle，`UserPromptSubmit`→working，`PermissionRequest` 标授权；允许后 `PostToolUse` 回到 working。Codex **没有** `PermissionDenied` / `PostToolUseFailure`；用户点 deny 没有 follow-up hook，会停在授权直到后续 `PostToolUse` 或 `Stop`。`Stop` 在仍有 background subagent 时保持 working，否则已完成，`SessionEnd`→closed。无 `hook_event_name` 时仍接受旧 notification。不订阅通用 PreToolUse；仅 matcher `AskUserQuestion\|ask_user_question`。当前 Codex 运行时 PreToolUse 往往只打 Bash，选择题可能仍报不进来。打断有 `Interrupt` 事件但当前不订阅，Esc / 报错时会话可能停在 working，直到进程退出或用户 reset。`PostToolUse` 登记 `async: true`，不挡主循环。hook 是观察者，投递失败必须 exit 0 |
| Cursor | 结构化 hook payload（`conversation_id` 可当 session） | 与 Claude 同一套回合生命周期；`sessionStart`→idle，`beforeSubmitPrompt`→working，`afterAgentResponse`/`stop`→已完成（`status=error`→failed，`aborted`→cancelled），`sessionEnd`→closed。不订阅 Pre/Post tool。`beforeShellExecution` 是每条命令的拦截闸门，不是「用户正在看授权框」；订阅它会把所有 shell 标成等待授权。Cursor 的 AskQuestion 走内部 InteractionQuery，不发 hook，选择题无法标 waiting。授权框本身也没有观察事件，允许/拒绝都看不到。偶尔不发结束事件，会话停在 working 直到进程退出。Cursor CLI（`agent` / `cursor-agent`）会加载 Claude Code 的 `~/.claude/settings.json` hook；Claude 钩子若由 Cursor CLI 进程唤起，source 标为 `cursor`，避免 `or agent` 被当成 Claude。hook 是观察者，投递失败必须 exit 0 |
| Grok | 结构化 hook payload | idle、working、permission、waiting、completed、failed、closed；`UserPromptSubmit` 标工作中；`Notification permission_prompt` 标授权；`PermissionDenied`（用户点 deny / 规则拒绝）回到 working；`PostToolUse`/`PostToolUseFailure` 在允许命令后回到 working；`Stop end_turn` 仅在仍有 **status 为 running 的** background subagent 时保持 working，shell/monitor 挂起、已结束的 subagent 与空任务视为已完成；`Notification idle_prompt` 同样已完成（`task_complete` 不映射）；带 `subagentType` 的 payload 仍丢弃。不订阅通用 PreToolUse（会挡住工具、且发生在授权提示之前）；仅 matcher `ask_user_question`：弹出选择题 → waiting。Grok 没有 `async` 字段，`PostToolUse`/`PostToolUseFailure` 用 `orb hook grok --detach` 后台投递（父进程快照 tty 与活性给子进程），Stop 仍同步。hook 是观察者：投递失败必须 exit 0，不得把 Grok `Stop` 变成闸门。生成的 hook 脚本必须 `exec orb`，否则 liveness 会把短命 hook 壳当成 agent，约 15s 后误发 `session.closed` |

连接页只接上表四个工具。其他工具不要走 wrapper：用 `orb start` / `orb waiting` / `orb complete` 发事件。

适配器只读取结构化 stdin。即使 payload 含有 `transcript_path`，也不会打开、保存或转发该路径。

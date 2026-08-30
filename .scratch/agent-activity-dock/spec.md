# Agent Activity Dock

**Status:** frozen
**Labels:** historical

> **Frozen.** This is the original Unix-first spec. Do not implement from it.
> Current constraints: [`docs/agents/domain.md`](../../docs/agents/domain.md), [`docs/event-contract.md`](../../docs/event-contract.md), [`docs/adr/`](../../docs/adr/).

## Problem Statement

用户同时运行 Codex、DSH、Claude 或其他自动化 Agent 时，工作过程经常持续数分钟甚至更久。用户需要在其他窗口工作，却无法可靠知道 Agent 是仍在运行、已经完成、失败，还是正在等待权限或新的输入。

现有方案分成两类：一类是终端或桌面通知，只能在结束时发出一次提示；另一类是完整的像素办公室、桌宠或 Agent 可视化场景，功能和视觉负担都明显超过“让我知道什么时候该回来处理”的需求。用户需要一个低干扰的状态层：工作时可以手动缩成古早加速球式的小球，只有真正需要注意时才播放提示音或显示徽标。

这个问题不能靠另一个会监控屏幕、读取 transcript 或接管 Agent 的大应用解决。核心产品应只消费 Agent 主动发出的结构化生命周期事件，并且在通知失败时不影响 Agent 本身继续工作。

## Solution

提供一个本地运行的 Agent Activity Dock。Dock 接收来自不同 Agent 的结构化事件，维护每个任务的最小状态，并以展开面板或固定尺寸的收缩小球呈现：

- `working`：显示正在工作，不播放声音；
- `needs-attention`：权限请求、问题等待用户输入等情况，播放一次提示音并显示徽标；
- `completed`：任务完成，播放一次完成音并短暂显示完成态；
- `failed`：任务失败，使用不同的声音和状态标识；
- `idle`：没有待处理事件。

用户可以手动在展开面板和小球之间切换。收缩后普通进度事件不会强制展开 Dock；需要注意时只播放声音、更新徽标或让小球短暂脉冲，用户通过点击或热键显式展开。

第一版是本地、单用户、事件驱动的 Dock，不提供 Agent 控制能力。DSH、Codex、Claude、termfocus 或其他工具都通过 adapter 接入；任何单一 adapter 都不能成为核心状态模型的隐式依赖。termfocus 只是可选集成对象，不改变本产品的独立边界。

## User Stories

1. As an Agent user, I want to see whether an Agent is working, so that I know whether I need to return to it.
2. As an Agent user, I want to collapse the Dock into a small ball while automation is running, so that it occupies almost no screen space.
3. As an Agent user, I want to expand the Dock with a click or hotkey, so that I can inspect the current status on demand.
4. As an Agent user, I want my collapsed choice to remain stable during ordinary progress, so that the Dock does not interrupt my other work.
5. As an Agent user, I want a visible badge when an Agent needs my attention, so that I can notice it even when the Dock is collapsed.
6. As an Agent user, I want a short sound when a task completes, so that I can return without watching the Agent window.
7. As an Agent user, I want a different sound for failure and permission requests, so that I can distinguish urgency without opening the Dock first.
8. As an Agent user, I want to mute completion sounds and attention sounds independently, so that the Dock fits my environment.
9. As an Agent user, I want the Dock to remain silent during ordinary progress updates, so that automation does not become a stream of interruptions.
10. As an Agent user, I want the Dock to show the source Agent and session identifier, so that multiple tools do not appear to be one ambiguous task.
11. As an Agent user, I want the Dock to show the most urgent pending session first, so that attention is directed to the state that matters most.
12. As an Agent user, I want completed tasks to leave the attention queue after acknowledgement, so that old notifications do not remain active.
13. As an Agent user, I want a task that is waiting for permission to remain pending until I acknowledge it, so that I do not accidentally miss an approval decision.
14. As an Agent user, I want a task that is waiting for text input to be marked differently from a permission request, so that I understand what action is expected.
15. As an Agent user, I want a failed task to retain a short safe summary, so that I can decide whether to open the originating Agent.
16. As an Agent user, I want a completion event to produce only one sound even if it is delivered more than once, so that retries do not create notification noise.
17. As an Agent integration author, I want to emit a small documented event payload, so that I can integrate without depending on Dock internals.
18. As an Agent integration author, I want the event contract to support started, waiting, completed, failed, and cancelled states, so that common lifecycle behavior maps consistently across tools.
19. As an Agent integration author, I want an optional deep link, so that the user can open the originating session from the Dock.
20. As a DSH user, I want a DSH adapter to translate session projection changes into Dock events, so that DSH automation participates without a private UI plugin.
21. As a Codex user, I want Codex's notification payload to be accepted by an adapter, so that I can use the Dock without wrapping Codex or parsing its output.
22. As a Claude user, I want hook events such as permission requests and stops to be accepted by an adapter, so that the same attention model works across Agent CLIs.
23. As an integration author, I want a command-line event emitter in addition to library adapters, so that tools without a native SDK can integrate safely.
24. As an Agent user, I want malformed events to be rejected without changing current Dock state, so that one broken integration cannot corrupt other sessions.
25. As an Agent user, I want stale events to expire, so that an old completion does not announce itself after a long outage.
26. As an Agent user, I want the Dock to recover its minimal state after restarting, so that active notifications are not silently lost.
27. As an Agent user, I want the Dock to avoid replaying ordinary historical events after restart, so that recovery does not create a burst of old sounds.
28. As a privacy-conscious user, I want the Dock to avoid reading transcripts, prompts, commands, code, terminal output, or credentials, so that status assistance does not become content surveillance.
29. As a privacy-conscious user, I want event summaries to be bounded and ephemeral by default, so that sensitive text is not accumulated in local history.
30. As a privacy-conscious user, I want the Dock to use a current-user-only local transport, so that other local users cannot inject notifications.
31. As a privacy-conscious user, I want the Dock to make no network connection by default, so that a local status tool does not create a remote data path.
32. As an Agent user, I want notification failure to leave the Agent process untouched, so that the Dock can never break or terminate automation.
33. As an Agent user, I want the Dock to remain useful when no adapter is installed, so that setup can start with one tool and grow later.
34. As an Agent user, I want color not to be the only distinction between states, so that the status remains understandable in limited color environments.
35. As an Agent user, I want an animation-disabled mode, so that the Dock remains suitable for sensitive or low-power environments.
36. As an Agent user, I want a compact terminal-compatible presentation, so that the Dock can be used over SSH or inside a terminal workflow without requiring a full desktop shell.
37. As an Agent user, I want a desktop presentation to be optional rather than mandatory, so that the core product stays small and portable.
38. As an Agent integration author, I want versioned event fields and an unknown-field rule, so that adapters can evolve without breaking older Dock versions.
39. As an Agent user, I want an audit view of recent state changes without storing Agent content, so that I can diagnose missed notifications.
40. As an open-source maintainer, I want deterministic state and notification tests, so that new adapters do not change the meaning of existing states.

## Implementation Decisions

1. **Product boundary**
   - Agent Activity Dock is an independent local product. It is not a new mode of termfocus and does not inherit termfocus's Focus Session, Companion State, or Work Pane Non-Interference domain model.
   - Existing products such as termfocus may provide optional adapters. An adapter may submit explicit events, but it may not cause the Dock to inspect a work pane, discover processes, parse output, or control an Agent.
   - This spec does not require changing any existing product's ADR, coordinator, database, or command behavior.

2. **Core modules**
   - **Event Ingress** accepts events from the local CLI emitter and adapter processes.
   - **Event Normalizer** validates the version, required fields, timestamps, state type, severity, bounded summary, and optional deep link.
   - **Session Registry** keeps the current minimal state for each `source` plus `session_id`, and de-duplicates events by `event_id` or the documented idempotency key.
   - **State Reducer** applies valid lifecycle events and exposes a read-only snapshot.
   - **Attention Policy** maps state transitions to sound, badge, pulse, and queue behavior. It is independent from rendering.
   - **Presenter** renders expanded and collapsed modes through a small presentation interface. A terminal presenter is the first implementation; a native floating-window presenter is optional future work.
   - **Adapter SDK/CLI** translates source-specific notifications into the common event contract without importing source internals into the core.

3. **Event contract**
   - Every event has `event_id`, `type`, `source`, `session_id`, `occurred_at`, and `severity`.
   - `summary`, `deep_link`, `requires_user_action`, and `metadata` are optional. Summary length and metadata size are bounded at ingress.
   - Supported lifecycle events are `session.started`, `session.working`, `session.waiting_input`, `session.permission_requested`, `session.completed`, `session.failed`, and `session.cancelled`.
   - Unknown event types are ignored with a diagnostic counter. Unknown fields are preserved only in memory for the adapter's own debugging and are not written to the default local history.
   - Timestamps too far in the future, events outside the retention window, and events for an already expired session are rejected or marked stale without changing the active state.

4. **State model**
   - A session has one current state: `idle`, `working`, `needs-attention`, `completed`, `failed`, or `cancelled`.
   - `needs-attention` carries a reason of `input`, `permission`, or `unknown`; reason is shown as text and is never encoded by color alone.
   - A completed or failed session remains visible in recent history until acknowledged or the bounded history limit is reached. Acknowledgement clears notification urgency but does not rewrite the lifecycle outcome.
   - The registry supports multiple sessions. The attention queue orders unacknowledged events by severity, then occurrence time; ordinary working sessions do not enter the queue.
   - A session cannot move backwards from a terminal state unless a new session identifier is used. Duplicate terminal events are idempotent.

5. **Collapsed and expanded interaction**
   - Expanded mode shows the highest-priority session, its source, state, safe summary if present, and the action expected from the user.
   - Collapsed mode has a fixed small footprint and shows a state glyph plus an attention count. It is called the “ball” interaction even when a terminal presenter uses a compact cell or status segment rather than a native circle window.
   - User collapse/expand is explicit. The core never opens a window or changes terminal layout because of an ordinary event.
   - An attention event while collapsed may play the configured sound and pulse the glyph, but must not force expansion. This preserves the user's spatial choice.
   - The first presenter exposes a keyboard command and a click-equivalent where the host supports it. Mouse-specific behavior is not part of the core contract.

6. **Notification policy**
   - `session.started` and `session.working` are silent by default.
   - The first transition into `needs-attention` plays one attention sound. Repeated updates for the same reason are silent until the user acknowledges the item.
   - `session.completed` plays one completion sound; `session.failed` plays one failure sound; `session.cancelled` is visual-only unless configured otherwise.
   - Sound channels can be muted independently. A mute change takes effect immediately and does not replay suppressed sounds.
   - Sounds are supplied through an injectable asynchronous sound sink. No platform default bell, speech synthesis, or arbitrary shell command is used by the core.
   - If audio playback fails, the state transition and visual notification still complete. The error is reported once through diagnostics and does not propagate to the Agent.

7. **Local transport and security**
   - The MVP uses a current-user-only local IPC endpoint. The endpoint is not a network listener and has no unauthenticated remote mode.
   - The daemon validates peer ownership where the host supports it and rejects oversized messages before parsing them.
   - The Dock never executes event payloads as commands. Deep links are treated as data and may only be opened through an explicit user action.
   - There is no cloud account, telemetry, remote control, credential storage, or automatic upload in this spec.

8. **Persistence and privacy**
   - Runtime state and de-duplication data may be persisted locally so a daemon restart does not replay old sounds.
   - Default persistent records contain source, session identifier, lifecycle state, severity, timestamps, acknowledgement state, and event counters. They do not contain prompts, commands, code, transcript text, environment variables, or credentials.
   - Safe summaries are displayed in memory and are bounded. Persistent summary retention requires an explicit future setting and is not part of the MVP.
   - Debug diagnostics use a field allowlist and never include raw event payloads.
   - A bounded retention policy prevents the local notification history from becoming a second task database.

9. **Adapters**
   - The first-party DSH adapter consumes an explicit session projection or completion hook exposed by DSH. It does not override DSH permissions or infer state from terminal output.
   - The first-party Codex adapter consumes the documented notification payload. It does not wrap, terminate, configure, or parse the Codex process.
   - The first-party Claude adapter consumes documented hooks such as permission request and stop events. Hook execution is asynchronous and failure-tolerant.
   - A termfocus adapter may be added as an example integration, but it emits only explicit events and remains outside the Dock core.
   - Adapter packages can be versioned and released independently. A missing or incompatible adapter degrades only that source's integration.

10. **Runtime targets**
    - The core targets a local Unix-like environment first because the initial integrations run in terminal/WSL workflows.
    - The event contract and presenter interface must not expose Unix-specific types, so a Windows named-pipe transport or desktop presenter can be added later without changing session semantics.
    - The MVP does not require a native always-on-top desktop window. A terminal-compatible collapsed view is the acceptance target for the first release.

11. **Resource and failure behavior**
    - Event handling is non-blocking from the adapter's perspective. The emitter receives a quick accepted/rejected result and does not wait for sound playback or rendering.
    - The daemon uses event-driven updates and does not poll Agent processes, inspect files, or redraw unchanged frames continuously.
    - A crashed presenter must not delete registry state or affect the emitting Agent. Restarting the presenter may recover the last non-terminal state without replaying ordinary progress sounds.
    - A malformed event, duplicate event, stale event, sound failure, or presenter failure is observable through diagnostics but does not stop the daemon.

## Testing Decisions

- Tests assert external behavior at the highest seam: a validated event enters the state/attention pipeline and produces a deterministic snapshot plus notification effects. They do not assert private reducer data structures, timer implementation, or renderer internals.
- The state pipeline is tested with a fake clock, fake sound sink, fake persistence store, and deterministic event fixtures. Tests cover ordinary progress, attention transitions, terminal outcomes, duplicate delivery, stale delivery, out-of-order events, multiple sessions, acknowledgement, mute behavior, and restart recovery.
- Event ingress tests cover schema validation, size limits, unknown fields, malformed timestamps, unsupported event types, invalid transitions, and current-user transport rejection.
- Attention policy tests assert one-shot sound semantics: a duplicate completion event emits no second sound; repeated waiting updates do not spam; mute suppresses sound without suppressing state; failure in the sound sink leaves the state committed.
- Presenter contract tests assert that expanded and collapsed snapshots expose the required source, state, reason, count, and safe summary; they do not depend on terminal escape sequence layout. A small number of renderer golden tests may verify stable glyph output.
- Adapter contract tests use captured first-party payload fixtures for DSH, Codex, and Claude. They assert translation into common events and ensure adapter errors never throw into the Agent process.
- A local integration test starts the daemon and CLI emitter together, delivers events through the real local transport, and verifies acknowledgement and de-duplication. It must not launch a real Agent or access a user's transcript.
- Privacy tests assert that persisted records and diagnostics contain no raw summary, prompt, command, code, environment, credential, or terminal-output fields.
- Resource tests assert that idle and unchanged states do not cause periodic polling or unbounded event/history growth. Exact CPU and memory limits are measured during prototype validation rather than hard-coded before a baseline exists.
- Existing terminal-workflow tests may be used for the optional termfocus adapter, but the core test suite must remain runnable without a tmux server, desktop display, or audio device.

## Out of Scope

- Full pixel-agent offices, pets, furniture packs, game mechanics, character progression, or continuous high-frame-rate animation.
- Reading transcripts, prompts, commands, code, terminal output, screenshots, process tables, or arbitrary files to infer Agent state.
- Sending messages to an Agent, approving permissions, editing files, executing tools, terminating processes, or controlling a remote session.
- A full permission manager, DSH plugin marketplace, skill manager, MCP marketplace, or remote-control product.
- Automatic public social-media posting. A future release queue MCP may consume Dock events, but it must have preview, explicit approval, idempotency, and audit behavior.
- Learning/tutoring workflows. The Learning Loop Agent is a separate high-ROI product direction and should receive its own spec.
- Token accounting or cache-rate computation. A future metrics adapter may display existing DSH projections but does not belong in the Dock core.
- Cloud sync, accounts, telemetry, multi-user collaboration, fleet management, or network discovery.
- A mandatory native desktop floating ball. The first release may be terminal-only; a desktop presenter is an independent later adapter.

## Further Notes

- This spec follows the research conclusion that the strongest open-source opportunity is a small cross-agent event and attention layer, not another complete Agent visualizer. The supporting research is in [`agent-workflow-ideas-research.md`](../../../research/agent-workflow-ideas-research.md).
- Public products such as Pixel Agents, pixtuoid, Khanmigo, n8n, Buffer, and the official Claude/Codex integrations establish demand signals or adjacent capabilities. They do not prove willingness to adopt this exact product; real-user validation is still required.
- The first prototype should answer one design question: does “manually collapse to a small ball, then receive a single meaningful attention sound” reduce missed Agent handoffs without becoming another source of noise? It should not expand into a desktop shell before this behavior is validated.
- The next implementation split should follow the existing seams in this spec: event contract and reducer first, local transport second, terminal presenter third, then one adapter at a time. Learning Agent, DSH permission profiles, release broadcasting, and remote observation remain separate initiatives.

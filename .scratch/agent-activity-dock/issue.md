# Agent Activity Dock MVP: Zero-Install, Low-Interference Status Ball

**Status:** complete
**Labels:** completed

## Problem Statement

用户同时运行 Codex、Claude、DSH 或其他 Agent 时，不能一直盯着终端。用户需要一个极小的提示层，告诉他还有多少任务在工作，以及任务结束、报错或等待输入时“该回来看看了”。

用户不应该为了 Dock 重新安装 Agent，也不应该每次运行 Agent 时记住特殊前缀或包装命令。

## Solution

Dock 通过一次性的本地连接配置接入用户已经安装的 Agent。它优先使用 Agent 自己的 Hook/通知；没有 Hook 的命令行 Agent 可以使用用户级包装器，但包装器只调用原来的可执行文件，不替换或重新安装 Agent。

完成一次连接后，用户继续输入原来的 `codex`、`claude` 或其他命令。适配器只发送最小状态事件到 Dock 的本地 IPC。Dock 显示一个固定尺寸的聚合小球：边框颜色表示是否有任务工作中，数字显示 `工作数量/追踪数量`，新情况用一次提示音、短闪和持续的 `!` 标记提醒。

## User Stories

1. 作为 Agent 用户，我想继续输入原来的 Agent 命令，不想每次加 Dock 专用前缀。
2. 作为 Agent 用户，我只接受一次性连接已有 Agent，不接受重新安装或替换 Agent。
3. 作为 Agent 用户，我想看到当前有多少任务仍在工作。
4. 作为 Agent 用户，我想在工作时把 Dock 缩成一个不挡路的小球。
5. 作为 Agent 用户，我想通过边框颜色和数字快速判断整体状态。
6. 作为 Agent 用户，我想在任务正常结束、报错或等待输入时收到一次明确提示。
7. 作为 Agent 用户，即使没听到提示音，我也想看到任务留下的 `!` 标记。
8. 作为 Agent 用户，我想通过点击或热键查看任务列表，并在环境允许时回到源终端。
9. 作为 Agent 用户，我不想让 Dock 读取终端内容、提示词、代码或 transcript。
10. 作为 Agent 用户，我不想让 Dock 轮询进程或持续重绘造成干扰。
11. 作为 Agent 用户，如果事件丢失，我想用手动重置恢复小球状态。
12. 作为集成作者，我想用同一套最小事件接入不同 Agent。

## Implementation Decisions

- Dock 是独立的本地状态层，不控制 Agent，也不扫描进程或终端内容。
- 用户第一次运行 Dock 时执行一次“连接已有 Agent”流程；流程只发现本机已有命令并写入可撤销的用户级配置。
- 有原生 Hook/通知的 Agent 使用原生机制；没有的命令行 Agent 使用用户级包装器，包装器调用真实可执行文件并透传参数、退出码和信号。
- 安装或更新 Agent 不属于 Dock 的职责；用户后续仍使用原始命令名。
- Dock 核心只接收明确事件。最小事件动作是 `start`、`stop`、`waiting`、`error` 和 `reset`；内部视觉仍只有工作中/不在工作两种主状态。
- `stop`、`waiting`、`error` 从工作中切换后产生一次提示，并设置待查看标记；重复事件不重复提示。
- 任务按任务 ID 统计。聚合小球显示 `working_count/tracked_count`，而不是 Agent 类型数量。
- 小球是独立的固定尺寸置顶窗口。点击优先打开紧凑任务列表；只有能可靠获得窗口定位时才提供聚焦源终端动作。
- MVP 不做心跳、租约、超时推断、进程扫描、历史数据库或跨重启恢复。强制终止或断电导致的陈旧状态由手动重置解决。
- 本地 IPC 只允许当前用户访问，不开网络端口。事件载荷有大小限制，未知动作和非法数据不会改变现有状态。
- 事件处理、声音播放和 GUI 绘制相互隔离；声音或 presenter 失败不能影响状态提交或 Agent 进程。

## Testing Decisions

- 主测试 seam 是“公开事件入口 → 聚合快照和提示效果”。测试只验证用户/调用者可观察行为，不检查私有 reducer 字段、内部调用顺序或 GUI 工具包对象。
- CLI 到本地 IPC 到 daemon 的链路使用真实进程/套接字集成测试；声音、时钟和窗口系统只在系统边界使用 fake 或 contract test。
- 每个 ticket 按红 → 绿 → 重构推进：先写一个失败的行为测试，再写最少实现，再清理结构。
- 必测行为包括单任务生命周期、多任务计数、重复事件、非法事件、一次性提示、`!` 清除、手动重置、声音失败和连接配置不重装 Agent。
- 性能测试验证无事件时无轮询/常驻重绘，事件到状态更新小于 100ms；不预设未经测量的绝对内存上限。
- GUI 验收通过 presenter 公共快照和少量人工检查完成，不绑定特定窗口库的内部实现。

## Out of Scope

- 要求用户重装、重新下载或重新配置 Agent 本体。
- 每次运行都要求 `dock run -- codex` 之类的前缀。
- 默认扫描进程、解析终端输出或读取 Agent 内容来猜状态。
- 自动识别所有 Agent 的等待输入语义；无 Hook 的 Agent 只能先提供启动/退出能力。
- 完整 Codex、Claude、DSH 功能适配、云同步、账号、多用户、远程控制和 Agent 控制。
- 心跳、崩溃自动恢复、历史持久化、复杂设置面板和每个 Agent 一个小球。

## Completion Evidence

- Implementation: `src/agent_activity_dock/`
- Acceptance gate: `./scripts/acceptance.sh`
- Verification report: [`docs/verification.md`](../../docs/verification.md)
- Acceptance checklist: [`docs/acceptance-checklist.md`](../../docs/acceptance-checklist.md)
- Event contract: [`docs/event-contract.md`](../../docs/event-contract.md)
- Latest automated results: 51 unit/integration tests OK; ball smoke 15/15;
  visual probe 4/4; event-to-snapshot p95 <1 ms; idle CPU 0.0 s.

## Further Notes

- 原始长期规格仍在 [`spec.md`](./spec.md)；本 issue 是当前 MVP 的收窄执行范围。
- 先实现通用事件和一次性连接机制，再逐个添加 Codex、Claude、DSH 的薄适配器。
- 如果某个 Agent 没有可用 Hook，适配器必须明确标注“只能知道启动/退出”，不能通过解析终端内容假装完整支持。

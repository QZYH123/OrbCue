# Agent Activity Dock：下一会话交接文档

## 本轮实现状态（供下一个会话核对）

本轮已完成 ticket 01–07 的 MVP 代码与验证，详见
[`docs/verification.md`](./verification.md)。仓库当前可运行：

- 状态核心与本地 IPC：`python3 -m agent_activity_dock.daemon`
- 集成小球：`python3 -m agent_activity_dock.ball`
- 事件 CLI：`python3 -m agent_activity_dock.cli start|stop|waiting|error|reset|status`
- 一次性连接：`python3 -m agent_activity_dock.cli setup|connections|connect|disconnect`
- 首批适配器：Claude 原生 Hook；Codex/DSH 用户级 wrapper；DSH 显式投影 payload adapter
- 自动化测试：`PYTHONPATH=src python3 -m unittest discover -s tests -v`（55 项）
- 一键验收门禁：`./scripts/acceptance.sh`
- 性能探针：`PYTHONPATH=src python3 scripts/perf_probe.py`
- 端到端 GUI smoke：`PYTHONPATH=src python3 scripts/ball_smoke.py`
- 像素级视觉探针：`PYTHONPATH=src python3 scripts/visual_probe.py`
- 验收清单：[`docs/acceptance-checklist.md`](./acceptance-checklist.md)
- 事件协议：[`docs/event-contract.md`](./event-contract.md)

下一步只做未完成/验证类事项，不重新做已完成决策：

1. 在真实用户 shell 中先运行 `dock setup --dry-run`，确认变更后显式运行
   `dock setup`（本轮已提供 dry-run，并已在临时 HOME 验证真实 wrapper/Hook）。
2. 如需热键，再补全局热键注册及其失败诊断；当前点击已可打开任务列表。
3. 长期规格中的持久化、历史、声音设置、多用户、终端 presenter 等仍不在 MVP。
4. 观察真实 Codex/DSH/Claude 会话，确认 wrapper 与 Hook 在用户工作流中的边界。

## 先看什么

下一 Agent 不需要重新采访用户，也不要重新设计产品。按下面顺序阅读：

1. [`CONTEXT.md`](../CONTEXT.md)：项目里的说法和边界。
2. [`docs/adr/0001-independent-floating-ball-window.md`](./adr/0001-independent-floating-ball-window.md)：为什么小球是独立窗口。
3. [`docs/adr/0002-explicit-events-without-heartbeats.md`](./adr/0002-explicit-events-without-heartbeats.md)：为什么不做心跳和自动崩溃检测。
4. [`docs/adr/0003-aggregate-agents-into-one-ball.md`](./adr/0003-aggregate-agents-into-one-ball.md)：为什么多个任务共用一个小球。
5. [父 issue](../.scratch/agent-activity-dock/issue.md)：当前 MVP 的执行范围。
6. `.scratch/agent-activity-dock/issues/`：按编号和阻塞关系领取 ticket。

原始宽规格 [`spec.md`](../.scratch/agent-activity-dock/spec.md) 是长期背景，不是本次 MVP 的全部要求。不要因为它列了完整历史、声音设置、持久化或所有适配器，就把第一轮实现扩大。

## 用户最终要的体验

用户平时照常输入已有命令，例如 `codex`、`claude` 或 DSH 命令。用户不重新安装 Agent，也不需要每次加 `dock` 前缀。

Dock 第一次运行时只做一次“连接已有 Agent”的配置：发现本机已经安装的 Agent，优先接入它们提供的 Hook/通知；没有 Hook 的命令行 Agent，可以选择生成用户级包装器。包装器只调用原来的真实命令，透传参数和退出结果，并且可以撤销。

连接完成后：

- 一个任务开始，工作数量增加；
- 多个任务共用一个小球；
- 小球数字显示 `正在工作任务数/追踪任务总数`，例如 `2/3`；
- 边框颜色表示是否有任务工作中；
- 正常结束、报错或等待输入时，播放一次短提示并短闪；
- 角落保留 `!`，直到用户查看；
- 点击或热键打开任务列表，能可靠聚焦源终端时再提供该动作；
- 没有事件时不轮询、不扫描进程、不持续重绘。

## 最简使用流程

最终目标是下面这个流程，而不是让用户记住一套新命令：

1. 用户启动 Dock 一次；首次运行自动发现已有 Agent，并让用户确认要连接哪些。若实现选择单独命令入口，最多只需要一次 `dock setup`。
2. 用户继续输入原来的 `codex`、`claude` 或其他 Agent 命令。
3. 用户在其他窗口工作；小球只显示总体工作数量和边框颜色。
4. Agent 结束、出错或等待输入时，Dock 提醒一次并留下 `!`。
5. 用户点击小球查看任务；极端情况下用手动重置清除陈旧工作状态。

不要设计 `dock install codex` 这种让人误以为要重新安装 Agent 的流程。不要要求 `dock run -- codex` 作为每次运行的前缀。

## 技术边界

### 事件模式优先

Dock 核心只消费明确状态事件。最小动作是：

- `start`：任务开始工作；
- `stop`：任务正常结束；
- `waiting`：任务等待用户输入或确认；
- `error`：任务以错误结束；
- `reset`：用户手动清除陈旧状态。

视觉上仍只有“工作中”和“不在工作”两种主状态。等待、错误和正常结束通过一次性提示以及 `!` 表达“需要回来查看”，不再增加多个主颜色。

事件至少需要稳定的任务 ID、来源、事件 ID 和动作。任务 ID 是一次可独立追踪的任务，不是 Agent 类型名。同一任务重复发送停止事件不能重复提示或增加计数。

### 连接已有 Agent

适配层的职责是把已有 Agent 的生命周期翻译成上述事件，不把 Agent 的内部逻辑带进 Dock 核心。

- 有原生 Hook/通知：在用户现有配置中启用，配置必须可撤销。
- 只有进程退出信息：用用户级包装器报告开始和退出；不要声称能准确知道等待输入。
- 没有稳定接口：提供通用事件 CLI、Skill 或 MCP 示例，明确这是显式接入，不是自动探测。
- 不允许通过读取 transcript、命令、代码、终端输出或持续进程扫描来猜状态。

### 窗口与性能

- 小球是独立的约 32–48px 置顶窗口；原终端不被改形。
- 窗口尺寸固定，数字变化不能让布局抖动。
- 工作数量大于零使用工作边框色，否则使用空闲边框色；数字本身提供非颜色的线索。
- 无事件时没有轮询计时器、进程扫描或常驻动画。
- 事件到状态快照更新目标小于 100ms；声音播放不可阻塞状态提交。
- 如果当前 WSL2 环境不能可靠聚焦源终端，先交付任务列表，不要为了聚焦能力引入终端专属耦合。

## Ticket 执行顺序

按 blocker 顺序工作；每张 ticket 在一个全新 Agent 会话中完成，完成后再领取下一张：

1. `01-core-state-loop.md`：任务状态闭环。无阻塞。
2. `02-local-ipc-and-aggregation.md`：本地 IPC 和多任务计数。阻塞于 01。
3. `03-aggregate-floating-ball.md`：独立聚合小球。阻塞于 02。
4. `04-one-shot-attention.md`：一次性提示和 `!`。阻塞于 03。
5. `05-zero-install-agent-connection.md`：一次性连接已有 Agent，不重装、不加前缀。阻塞于 02。
6. `06-codex-claude-dsh-adapters.md`：Codex、Claude Code、DSH 薄适配器。阻塞于 05。
7. `07-performance-and-handoff-qa.md`：完整验收和交接。阻塞于 04、06。

05 可以在 03/04 之前完成，因为它只需要把事件送入核心；07 必须等完整提示和首批适配器都可演示。

## TDD 规则

每张 ticket 都严格执行一个小的红 → 绿循环：

1. 先写一个从公开接口观察行为的失败测试。
2. 只写让这个测试通过的最小实现。
3. 再补下一个行为测试，保持每次改动可运行。
4. 最后只做必要的整理，不提前实现后续 ticket 的能力。

首选测试 seam 是：

`公开事件入口 → 聚合快照 + 提示效果`

真实边界测试是：

`CLI → 当前用户本地 IPC → daemon → 聚合快照`

声音、时钟和窗口系统属于外部边界，可以注入 fake；不要 mock 自己的状态模块，也不要测试私有 reducer 字段、内部调用次数或具体 GUI 对象。

建议的行为测试语言：

- “开始一个任务后，快照显示 `1/1` 且边框为工作色。”
- “结束这个任务后，快照显示 `0/1`，产生一次提示并留下 `!`。”
- “重复结束同一任务不会再次提示。”
- “同时运行三个任务，其中两个未结束时显示 `2/3`。”
- “手动重置能清除丢失事件留下的工作状态。”
- “声音播放失败时，状态和 `!` 仍然可见。”

## 完成标准

下一阶段不能只说“代码写好了”，必须留下这些可验证结果：

- 状态核心和 IPC 自动化测试通过；
- 小球能在当前环境创建、置顶、保持固定尺寸并显示计数；
- 至少一个已有 Agent 能在不重装、不改变用户日常命令的情况下接入；
- 正常结束、错误和等待输入能触发一次提示；
- 重复事件不会造成重复提示或错误计数；
- 空闲时没有轮询和常驻重绘；
- 事件到状态更新小于 100ms；
- 强制终止/断电的已知限制和手动重置行为已记录；
- 完成后运行 Standards + Spec 两轴 code review，并在交接中记录残留风险。

## 已知限制与不要偷偷补的功能

- 没有 Hook 的 Agent 不能准确报告等待输入；只能先报告启动和退出。
- 强制 `kill -9`、断电或事件丢失不会自动恢复，MVP 使用手动重置。
- 不要为了“自动识别所有 Agent”偷偷加入进程扫描或终端解析。
- 不要把完整历史、持久化、云端、多用户、远程控制、复杂声音设置或所有一方 adapter 塞进本轮。
- 如果窗口工具包在 WSL2 环境不可用，先记录阻塞并做最小可行 presenter 探针，不要换成重量级桌面壳而偏离低干扰目标。

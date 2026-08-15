# MVP 验证记录（本轮实现）

验收清单见 [`docs/acceptance-checklist.md`](./acceptance-checklist.md)。

日期：本仓库工作会话实测。环境：WSL2 Ubuntu 24.04、XWayland/Weston WM、
`DISPLAY=:0`、Python 3.12.3、libX11 6。presenter 使用 libX11 + X 核心字体，
不依赖 GTK/Tk/Qt 安装。

## 自动化测试

```bash
PYTHONPATH=src python3 -m unittest discover -s tests -v
```

结果：55 个测试通过，覆盖：

- 单任务 start/stop/waiting/error/reset 闭环；
- 多任务 `2/3` 聚合、重复事件去重、未知任务/动作/坏 JSON/超大消息拒绝；
- 真实 CLI → Unix socket → daemon → snapshot 集成；
- 30 个并发本地客户端同时发事件时不丢事件，聚合为 `30/30` 后全部停止为 `0/30`；
- presenter 公开快照合同（固定 44x44、工作/空闲边框、`!`、`N/M`）；
- 一次性提示 fake sound sink、声音失败时 `!` 仍提交；
- 零安装连接：wrapper 透传参数/退出码/信号、可撤销 PATH snippet、Claude Hook
  安装与撤销不覆盖用户配置；
- `setup --dry-run` / `connect --dry-run` 只报告将创建的用户级文件和 PATH
  变更，不写入任何配置；
- 交互式 `setup` 逐个询问要连接的 Agent，只连接用户确认的项；
- 变更连接方法时先保留旧连接直到新连接成功，成功后清理旧 wrapper/Hook；
  新连接失败时旧连接保持可用；
- 同一方法重连且可执行文件路径变化时，刷新记录并重新生成 wrapper；
- wrapper 切到 native_hook 后，若不再有 wrapper 连接，会同时移除 PATH snippet；
- Codex/Claude/DSH 薄适配器 fixture 转换和 malformed payload 降级。

## 像素级视觉探针

`PYTHONPATH=src python3 scripts/visual_probe.py` 直接读取小球窗口 `(0,0)`
像素，验证真实绘制结果：初始 `0x64748b`（空闲边框）、start 后 `0x22c55e`
（工作边框）、stop 后观察到一次 `0xfbbf24`（短闪）并最终回到空闲边框。
最新一次 4/4 PASS，输出在 `docs/visual-probe-latest.txt`。

## 端到端 GUI smoke

`PYTHONPATH=src python3 scripts/ball_smoke.py` 会启动真实 ball 进程并驱动
单任务、多任务、stop/waiting/error、重复 stop、点击查看清 `!`、关闭任务列表、
reset 和优雅退出。最新一次运行 15/15 项 PASS，原始输出在
`docs/ball-smoke-latest.txt`。

## 窗口能力

- 独立小球窗口 44x44，`override_redirect`，位于 X11 右上角 `(4104,12)`。
- 计数变化不会改变窗口尺寸（`xwininfo` 复核仍为 44x44）。
- 合成 ButtonPress 后任务列表窗口 `420x54` 从 IsUnMapped 变为 IsViewable；
  再次点击关闭。WSLg 下未实现“聚焦源终端”，按 MVP 允许项只交付任务列表。
- 普通工作事件只触发一次快照重绘，不展开、不移动、不启动常驻动画。
- 空闲 2 秒内 daemon 和带球进程 CPU 时间增量均为 0.00 秒（select 阻塞，
  无轮询、无动画计时器）。

## 性能

`scripts/perf_probe.py` 实测：

| 指标 | daemon（无窗口） | daemon+小球 |
| --- | --- | --- |
| 事件→状态快照 p50 | 0.37 ms | 0.38 ms |
| 事件→状态快照 p95 | 0.52 ms | 0.61 ms |
| 事件→状态快照 max | 1.07 ms | 0.89 ms |
| CLI 完整往返 p95 | 138 ms | 137 ms |
| 空闲 2 秒 CPU 时间增量 | 0.00 s | 0.00 s |

“事件→状态快照”用同一进程直连 Unix socket 测量，排除 Python CLI 解释器
启动时间；目标 `<100ms`，实测 p95 <1ms。`scripts/perf_probe.py` 还单独启动
带真实 `paplay` 的 ball，发送 stop 并测量完整事件响应：max **1.22 ms**、
p50 **1.17 ms**；声音进程 spawn 不阻塞状态提交。

## 真实 Agent 连接验证（临时 HOME，不改真实配置）

- `ConnectionManager` 发现并连接真实 `codex`、`dsh`（用户级 wrapper）和
  `claude`（用户级 settings.json Hook）。
- 在模拟新 shell 中把生成的 wrapper 目录置于 PATH 前，仍使用原命令名：
  - `codex --version` 输出 `codex-cli 0.147.0`；
  - `dsh --version` 输出 `0.1.0-rc.6`；
  - daemon 快照从 `0/0` 变为 `0/2` 且两个任务都有 `!`，说明 wrapper 上报了
    start 与 stop，真实可执行文件未被替换。
- 生成后的临时 `settings.json` 交给真实 `claude --settings <file> --version`
  可正常解析（退出码 0）；`--version` 不实际执行 hooks，因此 hook 行为仍由
  下面的 fixture 手工验证覆盖。
- 隐私边界测试：Hook payload 即使携带 `transcript_path`，生成的 hook 脚本也只
  发送 session_id/action，日志中不出现 `transcript` 或路径内容。
- Claude 原生 Hook 脚本用真实 payload fixture 手工喂给 daemon：
  `SessionStart` → `1/1 working`，`PermissionRequest` → `0/1 idle + !`，
  `PreToolUse` → `1/1 working`（同一 session 恢复，不新增任务），
  `SessionEnd` → `0/1 idle + !`。

## 一次性提示与 `!`

- `stop`、`waiting`、`error` 都只产生一次 Attention；重复同一任务不再次
  sound/flash；普通 `start` 不提示。
- 声音 sink 抛错时，状态先提交，`!` 仍可见，并通过
  `agent_activity_dock.attention` logger 记录一次诊断（测试用 assertLogs 覆盖）。
- 打开任务列表（点击小球）调用 `acknowledge_all`，`pending_count` 清零。
- 短闪使用一次 250ms 的 select timeout，没有常驻动画循环。

## 已知限制

- 无 Hook 的 wrapper Agent（Codex、DSH）只能报告启动、正常结束和错误退出，
  无法准确检测等待输入；`connections` 命令中已显示该限制。
- `kill -9`、断电或事件丢失不会自动恢复；MVP 通过 `dock reset`/`--all`
  手动恢复。实测 daemon 被 `kill -9` 后，下一次启动可清掉陈旧 socket。
- Claude `PermissionRequest` 映射为 `waiting`，批准后的下一次 `PreToolUse`
  以 start 恢复同一 session；deny 且不再有工具调用时需查看/重置。
- 事件处理只读取结构化 hook stdin；不读取 transcript 路径、提示词、命令、
  代码或终端输出。
- 当前无持久化、跨重启恢复、云同步、多用户或完整历史。

## 两轴 code review 记录

**Standards 轴**

- 所有自动化测试从公开 seam 观察（事件入口 → 快照/提示；CLI → IPC → daemon；
  presenter 快照合同），没有断言私有 reducer 字段或 GUI 对象。
- IPC 为 Unix domain socket，目录 0700、socket 0600；载荷上限 16 KiB；
  未知动作/坏 JSON/超大消息不污染状态。
- daemon 与 ball 的循环都阻塞在 `select`，无轮询计时器、无进程扫描；flash
  只是一次 250ms timeout，结束后恢复无超时 select。
- 声音和 presenter 回调异常被隔离，不阻塞响应，不影响 Agent。

**Spec 轴**

- 只消费明确事件；不读取 transcript、提示词、命令、代码、终端输出。
- 视觉仍只有 working/idle 两个主状态；`!` 是待查看标记，不是第三状态。
- 重复事件不重复提示/计数；`reset` 可清指定任务或全部。
- 连接只发现 PATH 上的已有命令并写用户级可撤销配置；wrapper 不替换原程序。

**残留风险**

- X11 `override_redirect` 小球在当前 WSLg/Weston 下保持在普通窗口之上；其他
  窗口管理器需重新验证。若某 WM 不保证置顶，需要改为 managed dock 窗口。
- wrapper 的 PATH snippet 只影响新启动的 shell；当前已开 shell 需重开或 source。
- Claude Hook schema 随版本可能变化；`install_claude_hooks` 已做备份和按脚本
  路径撤销，但升级 Claude 后应复测 `SessionStart/SessionEnd/PermissionRequest`。
- CLI 完整往返约 120ms（主要是解释器启动）；状态提交本身 <1ms。对延迟极敏感
  的 adapter 可直连 Unix socket 或常驻 emitter。
- 未实现全局热键和聚焦源终端；点击任务列表已满足 MVP 主要流程。
- 真实 `codex --version`/`dsh --version` 已验证 wrapper，但尚未长时间观察真实
  编码会话中的所有生命周期边角。

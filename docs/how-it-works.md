# OrbCue 是怎么工作的

会用看 [README](../README.md)。这篇只讲内部怎么转：事件进状态机、球上出现数字、点回去。安装与打包见 [dev.md](dev.md)，字段与各工具能力见 [event-contract.md](event-contract.md)，术语与边界见 [domain.md](agents/domain.md)。

## 一圈

连接页确认之后，**新开**的 Agent 才会出现。已在跑的会话不回填。

```
工具 hook ──stdin JSON──► orb hook ──译成 DockEvent──► 本机 IPC ──► DockState.apply()
                                                              │
                                              快照 / 一次 cue ◄┘
                                                              │
                                              Tauri emit ────► 球 / 面板（只渲染，不推演）
```

界面从不「发现」工具。它只订阅快照。点「清除」只改 Dock 列表，不向 Agent 发命令。「已读」只去掉 `?` / `!`。

完成只出声，等待和失败才弹系统通知。这是提示策略，不是漏了 `completed`。

## 分层

| 层 | 做什么 | 不做什么 |
| --- | --- | --- |
| `dock-adapters` | 各工具 stdin → `DockEvent` | 打开文件、改配置 |
| `orb` CLI | 子命令、WSL→Windows trampoline、`orb run` | 窗口 |
| `dock-ipc` | 一行 JSON、管道/socket | 生命周期含义 |
| `dock-core` | `apply()` → 快照、跳回/通知决策 | IO、Win32 |
| `dock-service` | 听端口、落盘、活性收割 | UI |
| `src-tauri` | 窗、HWND、toast、WSL 装 `orb` | 自己发明状态 |
| `frontend` | 球/面板、主题、贴边 | 推断 working/waiting |

crate 目录仍叫 `dock-*`，包名是 `orbcue-*`。改领域行为先读 domain / event-contract，再动 `dock-core`。状态机的标准答案是 `crates/dock-core/tests/state_behavior.rs`。

## 为什么是独立小球

终端没有可靠的「变成 56px 圆窗」接口，所以收缩态和面板是两扇独立窗。见 [ADR 0001](adr/0001-independent-floating-ball-window.md)。所有 Agent 共用一个球（[ADR 0003](adr/0003-aggregate-agents-into-one-ball.md)）。状态只在明确事件到达时变，不用心跳猜死活（[ADR 0002](adr/0002-explicit-events-without-heartbeats.md)）；后来只加了「记下的那一个 PID 还是不是原进程」这一条，死了才补 `session.closed`。

## WSL：转发，不是第二份 Dock

状态服务只在 Windows 桌面进程里，听 `\\.\pipe\orbcue`。WSL 里的 `orb` 把事件和 `status` 转到 `orb.exe`。`connect` / `agents` / `run` / `alias` 仍在工具所在的系统上执行。同一用户不要跑两份 daemon。`ORBCUE_BACKEND=wsl` 已删除。

路径按发送侧原样保存，不做盘符翻译。

## 连接

不替换 Agent 可执行文件。只在工具自己的 hook 配置里登记 `orb hook <工具>`。WSL / Linux 脚本必须 `exec orb`，否则活性检查会把短命 hook 壳当成 Agent。

| 工具 | 改谁 |
| --- | --- |
| Claude | `~/.claude/settings.json` |
| Grok | `~/.grok/hooks/orbcue.json` |
| Codex | `~/.codex/hooks.json` |
| Cursor CLI | `~/.cursor/hooks.json` |

改之前留 `*.orbcue.bak`。断开只撤 OrbCue 自己写的那段。新连接不创建 wrapper；连接页「断开」仍能清掉遗留包装。各工具能报到哪一步，以 event-contract 的适配器表为准：没发的节点不能假装看见。

## 跳回

点「回去」才执行，不做状态推断。阶梯在 `dock-core` 的 `focus_attempts`，Win32 在 presenter：

1. `deep_link`
2. `terminal_id` 为 `orb:` + 6 位十六进制 → 按标题/标签名聚焦（`orb run` 造的通道）
3. 新主会话或转入 working 时记下的前台终端 HWND（窗口还在且仍是终端类才用；不冒充标签级精确）
4. 找不到就老实说，并提示用 `orb run`

Windows Terminal 一个 HWND 对应整窗；WSL 里 OSC 改标题常被中继吞掉。所以不能靠模糊匹配项目名。手开的终端没有 `orb:`，只能走第 3 档。一直 working 不会反复覆盖 HWND。

两个终端 `grok --resume` 同一段对话会在列表里占两行（两个活进程），但跳回/通知仍按这段对话（`source` + `session_id`）。这不是「请双开 resume」的产品。

## 球停住时先问谁没说话

**故意留空：** 连接前已在跑的会话；手开终端只能窗口级跳回；完成不弹系统通知；面板不展示对话。

**工具没发的节点：** Codex 打断/报错常停在工作中；Cursor 选择题不走 hook；Claude/Codex 授权框点拒绝往往没 follow-up。连接行上有 limitation；「清除」和进程退出后的活性检查是出口。

**部署：** 两份 daemon；WSL 的 `orb` 没转到 Windows 管道；桌面 PATH 看不到 fnm 临时路径。先看当前 `orb` 连的是命名管道还是 WSL socket，有没有第二份 `orbd`。不要先改状态机。

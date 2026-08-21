# 08 — 从 Dock 跳回源终端

**What to build:** 面板会话卡片上有一个「回去」动作：尽力把该会话对应的终端窗口提到前台；做不到就说一句原因，绝不乱切窗口。

**Blocked by:** 无（03 已完成；presenter 现运行在 Windows 侧，具备 Win32 窗口聚焦能力）

**Status:** complete

- [x] 定位只用事件里已有的字段（`cwd` / `workspaceRoot` / `deep_link` / 可选窗口标题），不扫进程表、不读终端内容或 transcript。
- [x] **主机制：事件时刻前台捕获（窗口级，非标签级）。** presenter 在快照出现新会话或转入 `working` 时立刻 `GetForegroundWindow()`，用终端类名/进程名校验后把 HWND 记进内存表（键 `source+session_id`）。点「回去」优先聚焦该 HWND。前提是事件发生时 presenter 已在运行。捕获不做状态推断。
- [x] 标题级联是兜底：只在终端类窗口里按项目名/标题再 `source` 唯一匹配；HWND 失效或缺失才走这条。有 `deep_link` 时级联仍优先 `deep_link`。
- [x] 匹配不唯一或失败时：面板保持打开，卡片上显示一句失败原因；不猜测、不启动任何新进程代替跳回。
- [x] 普通工作事件、空闲刷新不触发跳回。
- [x] 行为测试：有定位信息则尝试聚焦；无定位信息或聚焦失败时面板可用且状态不变。
- [x] Dock hook / 主会话事件（含 `completed` / `failed` / `waiting_input` / `permission_requested`）在能拿到控制终端或祖先 pts 时写入 OSC 标题（`{source} · {项目路径末段}`），与 `project_path_hint` 同源；`AGENT_ACTIVITY_DOCK_NO_TITLE=1` 可跳过。
- [x] 标题兜底：先按项目名/窗口标题唯一匹配，零匹配再用 `source` 子串；不唯一不切。

## 已知局限

- **捕获是窗口级，不是标签级。** 记下的是当时的 Windows Terminal 窗口。同一窗口里换标签仍会聚焦该窗口，不能点回某个后台标签。
- **presenter 必须在事件发生时已运行。** 启动前已存在的会话没有 HWND，直到下一次新会话或转入 `working`。
- **部分 WSL+WT 环境 OSC 标题转发不通**（手写 OSC 后标签仍是发行版名，如 `Ubuntu-24.04`）。标题写入仍保留，但对跳回不是前提；该环境下标题级联几乎必然失败，靠前台捕获。
- 标题兜底仍只看终端窗口，不含 Edge / QQ。多个同名 agent 终端窗口在走兜底时仍会报「终端窗口匹配不唯一」。
- `/clear` 同类顶替靠 `terminal_id`（自身/祖先 tty 设备路径，`WT_SESSION` 仅垫底），不靠窗口标题或 HWND。同一 WT 标签里的不同 pane 是不同终端。

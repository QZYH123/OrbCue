# 跳回 v2：dock run 专属标签 + 诚实降级

**Status:** ready-for-agent
**Labels:** ready-for-agent

## Problem Statement

实测证明：WT 不暴露 pts↔ConPTY↔标签映射；WSL 内部（shell、hook、interop 子进程）写的 OSC 标题被中继整层吞掉，外部对「用户随手开的 WSL 标签」做标签级跳回不可实现。现有两条路都有伤：前台捕获（HWND）在标签撕出/合并后指错窗口；项目名/source 标题级联在标题恒为发行版名的环境里只产生「匹配不唯一」和误报。

同一组实验也给出钥匙：`wt.exe nt --title` 起的标签，标题由 Windows 侧命名、WSL 内没人能覆盖（恰因 OSC 被吞）、且跟随标签撕出/合并迁移；UIA 能跨窗口按标签名枚举与切换。

## Solution

**不变量：dock 启动的会话获得「耐久的 dock 命名落点」+「聚焦该落点的手段」。**

1. `dock run <agent>` 用 `wt.exe nt --title "… — dock:xxxxxx"` 开真实 WT 标签，并在标签环境注入 `AGENT_ACTIVITY_DOCK_TERMINAL_ID=dock:xxxxxx`——复用现有最高优先级覆盖，事件契约零改动；环境变量穿透 setsid，比 tty 走查更稳；state.json 已持久化 terminal_id，presenter 重启后精确跳回仍可用。
2. 跳回阶梯重排：deep_link → dock 标签精确通道（窗口标题含 marker，否则 UIA 按 TabItem 名匹配并切换）→ 捕获 HWND（校验后聚焦，明确标注「窗口级」）→ 诚实失败并提示 dock run。移除项目名/source 模糊标题级联。
3. 终端前端抽象成 TerminalAdapter seam（spawn + focus），本期只做 WT；Linux（wmctrl/OSC）、macOS（AppleScript）留作后续适配器。

## Out of Scope

- 内置终端 / PTY 仿真。
- WezTerm/Alacritty 等其他终端适配器（seam 就绪后另开 ticket）。
- 自动把用户手开的会话迁入专属标签。

## Tickets

- [15](./issues/15-jump-fallback-honesty.md)
- [16](./issues/16-dock-run-dedicated-tab.md)

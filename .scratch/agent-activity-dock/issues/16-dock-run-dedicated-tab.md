# 16 — dock run：专属 WT 标签与标签级精确跳回

**What to build:** `dock run <agent> [args…]` 在新的 Windows Terminal 标签里启动 agent，标签由 dock 命名并与会话绑定；面板「回去」对这类会话做到标签级、防撕出/合并的精确聚焦。

**Blocked by:** 15（阶梯重排先落）

**Status:** complete

## 机制

- marker：`dock:` + 6 位十六进制，随机且进程内防重。
- 启动：`wt.exe nt --title "{agent} · {cwd 末段} — {marker}" -- wsl.exe -d $WSL_DISTRO_NAME --cd {cwd} -- $SHELL -lc 'export AGENT_ACTIVITY_DOCK_TERMINAL_ID={marker}; {agent} {args}; exec $SHELL -l'`。agent 退出后标签留一个可用登录 shell。
- 绑定：环境变量是现有 terminal_id 的最高优先级覆盖——该标签里所有事件（含 setsid hook，env 穿透一切 spawn）自动带 marker；`/clear` 顶替、state.json 持久化、presenter 重启后的跳回全部免费获得；事件契约零改动。
- 聚焦：会话 `terminal_id` 以 `dock:` 开头时走精确通道——先枚举 WT 窗口标题含 marker（活动标签直接命中）；否则 UIA（IUIAutomation）遍历各 WT 窗口 TabItem，Name 含 marker 者 SelectionItem.Select 后聚焦窗口；找不到报「该标签已关闭」。

## 验收

- [x] `dock run` 校验 agent（已连接或 PATH 可见），错误信息清晰；args 原样透传；cwd 保留。
- [x] wt.exe 不可用（非 Win+WSL 环境）时明确报错；终端前端封装为 TerminalAdapter seam（spawn/focus 两个方法），本期仅 WT 实现。
- [x] e2e：`dock run` 一个脚本假 agent（发 start/complete）→ 事件 `terminal_id == marker`；窗口枚举可见含 marker 的标题；同 marker 两个 session_id 模拟 `/clear` 后只剩一条。
- [x] UIA 实机断言：两个 dock run 标签同窗，聚焦后台标签成功（日志 + 目检）；撕出到独立窗口后仍能命中。
- [x] presenter 重启后（无捕获记录）对 dock run 会话仍能精确跳回。
- [x] 面板对 `dock:` 会话显示精确跳回徽标（审美部分归主会话）。
- [x] README / event-contract 跳回段更新：精确通道、窗口级兜底、诚实失败三层如实描述。

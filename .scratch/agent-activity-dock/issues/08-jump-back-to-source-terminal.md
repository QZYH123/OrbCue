# 08 — 从 Dock 跳回源终端

**What to build:** 面板会话卡片上有一个「回去」动作：尽力把该会话对应的终端窗口提到前台；做不到就说一句原因，绝不乱切窗口。

**Blocked by:** 无（03 已完成；presenter 现运行在 Windows 侧，具备 Win32 窗口聚焦能力）

**Status:** ready

- [ ] 定位只用事件里已有的字段（`cwd` / `workspaceRoot` / `deep_link` / 可选窗口标题），不扫进程表、不读终端内容或 transcript。
- [ ] presenter 在 Windows 侧按窗口标题匹配尝试聚焦（Windows Terminal 标签页标题通常含路径或命令名）；有 `deep_link` 时优先走 `deep_link`。
- [ ] 匹配不唯一或失败时：面板保持打开，卡片上显示一句失败原因；不猜测、不启动任何新进程代替跳回。
- [ ] 普通工作事件、空闲刷新不触发跳回。
- [ ] 行为测试：有定位信息则尝试聚焦；无定位信息或聚焦失败时面板可用且状态不变。

# 06 — 添加首批常用 Agent 薄适配器

**What to build:** 在统一事件协议之上分别接入 Codex、Claude Code 和 DSH；每个适配器只翻译已有 Hook、通知或显式回调，不把来源 Agent 的逻辑带进 Dock 核心。

**Blocked by:** 05 — 一次性连接已有 Agent

**Status:** complete

- [x] Codex 使用已有安装和配置，能在普通 `codex` 命令下报告启动、结束和可用的等待/错误事件。
- [x] Claude Code 使用已有安装和 Hook，能翻译停止、等待或权限相关事件（以实际公开接口为准）。
- [x] DSH 通过显式 session 回调或投影事件接入，不解析终端输出。
- [x] 每个适配器都有真实 payload fixture 和公开事件转换测试。
- [x] 任一适配器缺失、过期或报错时，只降级该来源，不阻断 Dock。
- [x] 不为了适配器改变 Dock 的两种主视觉状态、计数或提示规则。

# MCP / Skill 接入说明

Skill 或 MCP 集成方不需要 import OrbCue 内部模块。在新会话开始时调用一次：

```
orb start <stable-session-id> --source <tool-name>
```

在等待、结束或失败时对**同一个** session id 调用：

```
orb waiting <stable-session-id> --source <tool-name>
orb permission <stable-session-id> --source <tool-name>
orb complete <stable-session-id> --source <tool-name>
orb fail <stable-session-id> --source <tool-name>
```

`orb stop` / `orb completed` 与 `orb complete` 效果相同，`orb error` 与 `orb fail` 效果相同。`event_id` 可省略（CLI 会生成），但重试时应复用同一 `event_id`；Dock 会对重复事件去重，不会重复提示或计数。

Dock 只在当前用户范围内收事件：Linux 用 Unix domain socket，Windows 用命名管道；没有网络监听。请求上限 16 KiB，未知动作和非法 JSON 会返回 rejected，不会改变现有状态。

完整字段、拒绝规则和各工具能报告到哪一步，见 [`docs/event-contract.md`](../docs/event-contract.md)。

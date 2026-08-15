# MCP / Skill 接入说明

Skill 或 MCP 集成方不需要 import Dock 内部模块。只需要在新会话开始时调用一次
`dock start <stable-session-id> --source <tool-name>`，在结束/失败/等待输入时调用
`dock stop|error|waiting <same-session-id>`。`event_id` 可省略（CLI 自动生成），
但重试时应复用同一 `event_id`；Dock 会对重复事件去重，不会重复提示或计数。

Dock 通过当前用户可写的 Unix domain socket 接收事件；没有网络监听。
事件载荷上限 16 KiB，未知动作和非法 JSON 会返回 rejected，不会改变现有状态。

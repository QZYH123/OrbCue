# Use Explicit State Events Without Heartbeats in the MVP

The MVP changes the ball state only when an Agent or its Skill/MCP integration sends an explicit state event. It does not send periodic heartbeats or scan processes to detect crashes; this keeps the low-interference prototype small and avoids timer, lease, reconnect, and false-timeout behavior. A lost event may leave the ball showing `working` until the user performs a manual reset; automatic crash recovery can be added after the core interaction is validated.

# 09 — 可关闭的系统通知

**What to build:** 小球在后台时，`?`（等待输入/授权）和 `!`（失败）走 Windows toast 提醒一次；点击通知打开面板并落到对应会话。其余状态一律静默。

**Blocked by:** 无（03 已完成；presenter 在 Windows 侧，toast 原生可用）

**Status:** ready

- [ ] 仅进入等待输入/授权或失败时发通知；`working` / `idle` / `completed` / `cancelled` 默认静默。
- [ ] 通知由 Windows presenter 发送（Tauri notification 插件或 WinRT），不在 WSL 侧实现。
- [ ] 同一会话同一原因只通知一次，与现有提示音的一次性策略共用判定，不重复轰炸。
- [ ] 点击通知打开任务面板并高亮对应 `source + session_id`；会话已不存在则只打开面板。
- [ ] 设置里独立开关，与三种提示音开关分开。
- [ ] 通知失败只记一次诊断，不回滚状态、不影响 Agent。
- [ ] 行为测试用可注入的通知接口，不依赖真实通知中心。

# 11 — 连接前预览将改写的 Hook 文件

**What to build:** 点「连接」或跑 `dock connect` 之前，先看到 Dock 会创建/修改哪些文件、写入哪些条目，以及明确不会动什么。这是把现有 dry-run 说清楚，不是新连接机制。

**Blocked by:** 05 — 一次性连接已有 Agent（已完成）

**Status:** ready

- [ ] `dock connect <name> --dry-run` 列出将写入的文件路径与条目（wrapper 路径、Hook 文件、事件名），预览零副作用。
- [ ] 桌面确认框与 dry-run 同一数据源、同一内容；写明不替换 Agent 本体、不动用户其他 Hook；Claude 需说明首次修改前备份 `settings.json`。
- [ ] 实际连接写入的文件集合与预览完全一致；`dock disconnect` 只清理预览中属于 Dock 的条目。
- [ ] 行为测试：dry-run 无副作用；确认后的产物路径与预览匹配；非法或非 Dock 文件仍拒绝覆盖。

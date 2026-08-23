# 领域语言与边界

修改领域行为或公共事件契约前必读。架构决策见 [`docs/adr/`](../adr/)，事件契约见 [`docs/event-contract.md`](../event-contract.md)。

## 术语

- **Agent**：能向 Dock 发出状态事件的外部工具（Claude、Codex、Cursor、DSH、Grok 等）。
- **Dock**：独立的本地状态提示层；只收明确事件，不读 Agent 内容、不控制 Agent、不扫进程猜状态。
- **小球**：收缩呈现；尽量不占空间，不是桌宠或完整桌面壳。
- **工作中 / 不在工作**：主状态只有这两种。结束、出错、等待输入都算不在工作，小球不要求再拆一套主视觉。
- **提示**：从工作中切走后的一次可感知提醒。不要通知轰炸。
- **状态事件**：Agent 或其 hook **主动发送**的状态变化。Dock 不从终端输出或全进程表推断 working/waiting。允许的唯一进程派生事件是：对 hook 已记录的那一个 PID+starttime 查询「是否仍是原进程」，若已死亡则补发 `session.closed`。
- **主会话 / 子代理**：主会话对应终端里的一次 Agent 调用并参与计数；带 `parent_session_id` 的子代理事件折叠进父会话，不单独计数。
- **手动重置**：事件丢失或强制终止后，把陈旧「工作中」清掉的显式操作。
- **工作数量 / 追踪数量**：正在工作的主会话数 / Dock 当前展示的主会话总数，小球显示为 `工作数量/追踪数量`。
- **待查看标记**：停止后留下的小 `!`，不是第三种工作状态。

## 不要违反的边界

- 不读取 transcript、prompt、命令、代码、终端输出；不扫描进程表来猜 working/waiting；允许对 hook 记录的单一 PID 做活性查询，且只用于 `session.closed`；
- 不替换 Agent 可执行文件，不要求用户重新安装 Agent；
- 不把声音、窗口和 Agent adapter 的失败传播回事件发送方；
- 不把摘要或原始 payload 写入默认持久化；
- 修改 Claude `settings.json`、Codex `~/.codex/hooks.json`、Cursor `~/.cursor/hooks.json` 前保留一次用户可恢复的备份，断开时只清理 Dock 自己的 Hook；
- 不让两个 `dockd` 同时服务同一用户。Presenter 在 GUI OS 上 `attach_or_listen`；仅当 Agent 跑在另一 OS（WSL）时由该 OS 的 `dock` trampoline 把事件送到这个 daemon。`AGENT_ACTIVITY_DOCK_BACKEND=wsl` 可显式回滚。禁止在 WSL-canonical 与 GUI-OS-canonical 之间静默探测切换。

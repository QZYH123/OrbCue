# 05 — 一次性连接已有 Agent

**What to build:** Dock 第一次运行时发现用户已经安装的常用 Agent，提供一次性的可撤销连接；连接完成后用户继续输入原来的 `codex`、`claude` 或其他命令，不重装 Agent、不加每次运行前缀。

**Blocked by:** 02 — 接入本地 IPC 和多任务聚合

**Status:** complete

- [x] 连接流程只使用本机已有 Agent，不下载、重装或替换 Agent 本体。
- [x] 有原生 Hook/通知的 Agent 优先写入用户级现有配置，并能撤销。
- [x] 没有 Hook 的命令行 Agent 可选用用户级包装器；包装器透传参数、退出码和信号，并调用原始可执行文件。
- [x] 连接后用户仍使用原命令名，不需要 `dock run -- ...` 前缀。
- [x] 启动和退出事件至少可用；不具备 Hook 时明确标注等待输入无法准确检测。
- [x] 连接失败只影响该 Agent，不影响 Dock 核心或其他 Agent。
- [x] 提供一个通用自定义 Agent 的手动事件入口或 Skill/MCP 示例。

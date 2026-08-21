# Agent Activity Dock 交接

## 当前主线

正式实现是：

- `crates/dock-core`：状态机、一次性提醒、时间校验、边界和持久化投影；
- `crates/dock-ipc`：版本化 JSON 协议、Unix socket/Windows named pipe transport、snapshot/subscribe/ack/reset；
- `crates/dock-service`：事件驱动 daemon、订阅推送、持久化和优雅退出；
- `crates/dock-adapters`：Claude/Codex/DSH 结构化 payload 转换；
- `crates/dock-connect`：PATH 发现、可撤销 wrapper、Claude Hook；
- `crates/dock-cli`：`dock` 事件、查询、连接、恢复，以及 `dock bridge`（stdio ↔ socket 的一行一条连接）；
- `src-tauri` + `frontend`：Windows 原生 Tauri 2（WebView2）presenter；Linux/macOS 仍走本地 `attach_or_listen`。

## 开发顺序

Win+WSL 的最低门槛是：WSL 终端和 Windows 终端里的 Agent 都能把事件送到同一个 WSL `dockd`。小球 GUI 是后续能力，且必须跑在 Windows 侧。

1. `bash scripts/install-cli.sh` 然后 `./start-dock.sh`，确认 `dock status` / `dock start` / `dock complete` 在 zsh 或 bash 中可用。
2. `dock connect` 必须写入 **zsh/bash/fish/Linux pwsh** 的现有 profile，不能只写 `.bashrc`。Windows pwsh 使用 `scripts/windows/dock.ps1` 转发到 WSL。
3. 用真实 `claude` / `codex` 在 WSL 终端跑一次连接、开始、结束、断开。
4. 桌面小球 = Windows presenter exe + `dock bridge`。交叉编译：`npm run tauri -- build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles none`。从 Windows 启动 exe，或从 WSL 经 interop 启动。不要走 WSLg / Linux WebKitGTK。
5. 最后再迁移或删除旧 Python/Electron 入口。

## 不要违反的边界

- 不读取 transcript、prompt、命令、代码、终端输出或进程表来猜状态；
- 不替换 Agent 可执行文件，不要求用户重新安装 Agent；
- 不把声音、窗口和 Agent adapter 的失败传播回事件发送方；
- 不把摘要或原始 payload 写入默认持久化；
- 修改 Claude `settings.json` 前保留一次用户可恢复的备份，断开时只清理 Dock 自己的 Hook；
- 不让两个 `dockd` 同时绑定同一 socket。Windows presenter 经 `dock bridge` attach，不再自己 listen named pipe。

## 本次交接状态

- Win+WSL 最低路径已对齐：daemon 在 WSL，`dock connect` 会写入 zsh/bash/fish/Linux pwsh 的现有 profile，不再只写 `.bashrc`。
- `start-dock.sh` / `stop-dock.sh` 已改为管理 Rust `dockd`；`scripts/install-cli.sh` 安装到 `~/.local/bin`。
- Windows 终端用 `scripts/windows/dock.ps1` / `dock.cmd` 转发到 WSL `dock`；不要同时跑 Windows named-pipe `dockd`。
- 桌面路径已改为 Windows Tauri + `dock bridge`。WSLg / native GTK presenter 已退役。
- 本机已验证：`dockd` + `dock start/waiting/complete/reset`，以及 `wsl.exe` 转发。尚未对用户本机 Claude/Codex 执行 `dock connect`。
- 桌面小球仍是可选项，不是终端 Agent 能否工作的前提。

# Agent Activity Dock

Agent Activity Dock 是一个本地、事件驱动的 Agent 状态层。它把 Claude、Codex、DSH 或其他自动化任务聚合到一个低干扰的小球和任务面板中：工作时只显示负载，真正需要用户回来时才提醒一次。

Dock 不读取 transcript、prompt、命令、代码、终端输出或凭据，也不扫描进程和控制 Agent。默认不联网，状态和 IPC 都限定在当前用户。

## 技术栈

- Tauri 2：轻量桌面壳、透明悬浮球、系统托盘、单实例和自启动
- Rust workspace：状态机、持久化、本地 IPC、适配器、连接管理和 CLI
- Svelte 5 + TypeScript + Vite：任务面板、连接引导和设置
- 跨平台当前用户 IPC：Unix domain socket 或 Windows named pipe，统一 newline-delimited JSON 事件协议
- JSON 状态文件：只保存最小生命周期状态，不保存摘要或 Agent 内容

## 开发运行

主目标环境是 **Windows + WSL**：WSL 里的 zsh/bash/fish、WSL 里的 pwsh，以及 Windows 终端里的 PowerShell/cmd，都应能把 Agent 状态送到同一个 Dock daemon。daemon 跑在 WSL；Windows 终端通过 `scripts/windows/dock.ps1` / `dock.cmd` 转发。

环境需要 Rust 1.80+、Node.js 20+、npm。

先装 CLI，然后在任意目录启动 daemon（不必 `cd` 到仓库）：

```bash
bash scripts/install-cli.sh
dock up
dock status
dock start task-1 --source my-agent
dock complete task-1 --source my-agent
```

连接本机已有 Agent 后，在 **新的** zsh/bash/pwsh 会话里继续输入原来的 `claude` / `grok` / `codex` / `cursor-agent` / `dsh`：

```bash
dock agents
dock connect claude --dry-run
dock connect claude
dock connect grok
dock connect codex
dock connect cursor
```

Windows PowerShell 把 `scripts/windows/dock.ps1` 放到 PATH（或保存成 `dock.ps1` 后 `. $PROFILE` 里定义 `Set-Alias dock ...`），即可在 pwsh 里执行同样的 `dock start` / `dock status`。默认走同目录 `dock.exe` 连 named pipe；`AGENT_ACTIVITY_DOCK_BACKEND=wsl` 或 `AGENT_ACTIVITY_DOCK_FORWARD=wsl` 时转发到 WSL `~/.local/bin/dock`。`AGENT_ACTIVITY_DOCK_WSL_DISTRO` 对 WSL 转发生效。

停止 daemon：

```bash
dock down
```

Win+WSL 的桌面路径默认是 **Windows presenter 对本机 named pipe `attach_or_listen`**。WSL `dock` 把 emit/status/up/down/bridge 显式 trampoline 到 `dock.exe`（`run` / `connect` / `agents` 仍留在 WSL）。拓扑持久在 `%LOCALAPPDATA%\Agent Activity Dock\backend`；`AGENT_ACTIVITY_DOCK_BACKEND=wsl` 可回滚到 `dock bridge`。升级顺序：先装带 trampoline 和 hop 的 WSL `dock`，再装会 listen named pipe 的 presenter。旧 shim + 新 presenter listen 会裂脑。`AGENT_ACTIVITY_DOCK_HOP` 已有值时既不 trampoline 也不向 WSL 回跳。presenter 不开网络端口。

从 Windows 启动已经打好的 exe（或把 WSL 里交叉编译出的 exe 交给 Windows interop 启动）。不要用 WSL 里的 Linux Tauri / WSLg 显示悬浮窗。

在 WSL 里交叉编译绿色 exe：

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
npm install --prefix frontend
npm run tauri -- build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles none
```

产物在 `target/x86_64-pc-windows-msvc/release/`。NSIS 安装包由 Windows CI 打。也可用已有 `.github/workflows/ci.yml` 的 Windows job。

不要同时启动两个 `dockd`。规范 daemon 在 GUI OS；设 `AGENT_ACTIVITY_DOCK_BACKEND=wsl` 才回滚到 WSL `dockd` + `dock bridge`。桌面版不是终端 Agent 的最低门槛。

## 连接已有 Agent

桌面版第一次打开连接页面时，会分别发现 WSL 登录 PATH（并忽略 `/mnt/*`）和 Windows PATH 上的 `claude`、`grok`、`codex`、`cursor-agent`（显示为 Cursor）和 `dsh`；同名工具两侧各算一条。逐个显示变更内容并等待确认。连接只生成可撤销的用户级 wrapper 或 Hook（Claude `settings.json`、Grok 独立 hook 文件、Codex `~/.codex/hooks.json`、Cursor `~/.cursor/hooks.json`），不替换原始可执行文件：

```bash
cargo run -p agent-activity-dock-cli -- agents
cargo run -p agent-activity-dock-cli -- connect cursor --dry-run
cargo run -p agent-activity-dock-cli -- connect cursor
cargo run -p agent-activity-dock-cli -- disconnect cursor
```

连接 Claude 前会严格解析 `settings.json`，并在首次修改前保留一份
`settings.json.agent-activity-dock.bak`。断开连接只移除 Dock 自己写入的 Hook，不会用备份覆盖用户后来对设置做的修改。

没有原生 Hook 的 wrapper 只能可靠报告开始、正常结束和错误退出；页面会明确显示这个限制。连接失败不会下载、重装或修改 Agent 本体。

## 事件协议

集成作者请阅读 [`docs/event-contract.md`](docs/event-contract.md)。最小事件包含版本、事件 ID、来源、session ID、时间戳和生命周期类型；请求超过 16 KiB、时间过旧/过新、字段越界或格式错误时会被拒绝且不改变当前状态。

发送事件的常用命令：

```bash
dock start my-task --source claude
dock waiting my-task --source claude
dock permission my-task --source claude
dock complete my-task --source claude
dock fail my-task --source claude
dock acknowledge --source claude --session-id my-task
dock reset --source claude --session-id my-task
dock run grok
```

`dock reset` 是显式的陈旧状态恢复操作，不会向 Agent 发送任何控制命令。reset 之后迟到的 `complete` / `waiting` / `failed` 不会复活该会话。带 `parent_session_id` 的子代理事件不计入小球上的主会话数。

Win+WSL 下用 `dock run <agent> [args…]` 在新的 Windows Terminal 标签里启动 Agent。设置里可以给它起一个短别名（例如 `dr`），写入 `~/.local/bin`，新开的终端里 `dr grok` 等同 `dock run grok`。默认沿用当前标签的配置文件（`WT_PROFILE_ID`）；可用 `dock run --profile "Ubuntu-24.04" grok` 或 `AGENT_ACTIVITY_DOCK_WT_PROFILE` 覆盖。`BACKEND=local` 时 WSL 只解析 Linux 命令并写启动脚本，开窗由 `dock.exe` 执行。纯 Windows 的 `dock.exe run grok` 直接在新标签里跑 Windows PATH 上的 Agent，不经过 `wsl.exe`。`dock run --close grok` 在新标签启动成功后结束当前交互式 shell，从而关掉启动页；管道或脚本里即使带 `--close` 也不会关。标签标题带 `dock:xxxxxx` 标记，标签环境注入 `AGENT_ACTIVITY_DOCK_TERMINAL_ID`，面板「回去」可做标签级精确跳回（撕出/合并后仍有效）。手开的会话仍走窗口级 HWND 兜底：成功时标明「已回到最近交互的窗口」，HWND 失效则诚实失败并提示 `dock run`。不再按项目名或 source 做模糊标题匹配。`AGENT_ACTIVITY_DOCK_NO_TITLE=1` 可关闭写标题。

## 隐私与可靠性

- Unix 状态文件默认位于 `$XDG_STATE_HOME/agent-activity-dock/state.json`；Windows 默认位于 `%LOCALAPPDATA%\\Agent Activity Dock\\state.json`。
- Unix socket 目录权限为 0700、socket 权限为 0600；Windows 使用本机 named pipe。服务不会监听网络端口。
- 持久化保留来源、session、状态、时间、确认标记、终端标记和项目路径；摘要、prompt、命令和 transcript 只在内存中短暂存在。
- 重启会恢复最小状态，但不会重播旧声音；陈旧事件会被拒绝。
- 审计页只保留当前运行期间最近 128 条状态变更，不写入状态文件。
- 声音、前端和 Agent 连接失败不会让状态服务退出；用户可以用面板或 CLI 手动 reset。

## 当前平台范围

Win+WSL 的最低可用路径是 WSL 中的 `dock` CLI + 可撤销连接；规范 daemon 在 Windows presenter。回滚：`AGENT_ACTIVITY_DOCK_BACKEND=wsl` 仍用 WSL `dockd` + `dock bridge`。Unix 使用当前用户 socket。macOS 安装包尚未验证。

`start-dock.sh` / `stop-dock.sh` 调用 `dock up` / `dock down`。默认 trampoline 到 Windows；`BACKEND=wsl` 时仍起 WSL `dockd`。

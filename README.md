# Agent Activity Dock

同时开着几个终端里的 Agent，窗口一多就很难看出谁在干活、谁卡在等你。

Dock 在桌面放一个小球，把 Claude、Grok、Codex、Cursor、DSH 的任务收在一起：**干活时只显示数量，真正需要你回来时才提醒一次。** 点小球打开任务面板，按项目分组，可以从列表跳回对应终端。

它不读取对话、提示词、命令或代码，也不替你操作这些工具。默认不联网，状态只留在当前用户本机。

## 会做 / 不会做

**会做**

- 多个 Agent 共用一个小球：数字是「正在工作 / 正在追踪」
- 等待输入、等待授权、失败时弹一次系统通知并播提示音；正常完成只播提示音
- 连接本机已经装好的工具，不替换原程序
- 用 `dock run` 开专用终端标签，面板「回去」可以精确跳回去

**不会做**

- 不读对话记录、提示词、命令、代码、终端输出或凭据
- 不靠扫进程表猜测「是不是在干活」
- 不替换 Agent 的可执行文件；连接失败也不会下载或重装它们
- 不对外监听网络端口

## 界面

| 部分 | 作用 |
| --- | --- |
| 小球 | 桌面常驻。数字是工作中 / 追踪中。右上角 `?` 表示有人在等你，`!` 表示有失败。 |
| 动态 | 当前任务，按项目分组。可筛选全部 / 工作中 / 未工作。「回去」跳到对应终端。 |
| 审计 | 本次运行里最近的完成、失败、等待和关闭，最多 128 条，不写入磁盘。 |
| 连接 | 检测本机已有工具，逐个确认后再连接或断开。 |
| 设置 | 外观、提示音、系统通知、开机自启、快捷键、启动别名。 |

默认快捷键是 `Ctrl+Shift+Space`，打开或收起面板。托盘可以隐藏 / 显示小球，或退出。设置里可以把小球右上角的圆标关掉。

外观可在设置里切换：**原型 / Fluent / Glyph / Braun / Glass**。

## 开始使用

小球和面板始终跑在 Windows 桌面上。有没有 WSL，用法一样：连接本机已有的 Agent，看同一个小球，用 `dock run` 精确跳回。同一用户不要同时开两份 Dock。不要用 WSL / WSLg 开 Linux 悬浮窗。

### 1. 打开桌面程序

在 Windows 启动 **Agent Activity Dock**。第一次启动会把 `dock.exe` 装到 `%LOCALAPPDATA%\Agent Activity Dock`，并写入当前用户 PATH。设置里可以打开开机自启。

### 2. 命令行

**新开**一个 PowerShell / cmd / Windows Terminal，然后：

```bat
dock status
```

找不到 `dock` 时，把 `%LOCALAPPDATA%\Agent Activity Dock` 加进用户 PATH 后再开一个终端。桌面程序会自己写这一项，但已经开着的窗口读不到。

如果还要用 WSL 里的 Agent，在 WSL 里额外装一次命令行：

```bash
bash scripts/install-cli.sh
dock status
```

WSL 的 `dock` 装到 `~/.local/bin`。Windows 上的 Agent 和 WSL 里的 Agent 会出现在同一个小球上，连接页按「Windows / WSL」分开。

### 3. 连接已有工具

面板「连接」页会列出本机 PATH 上的工具，点连接即可。也可以：

```bash
dock agents
dock connect grok
dock connect claude
dock connect codex
dock connect cursor
```

在哪边敲命令，就连哪边的工具。连接前会列出将要改动的文件，等你确认。

连上之后，在**新开的**终端里继续用原来的 `claude`、`grok`、`codex`、`cursor-agent`、`dsh`。已经开着的终端可能还看不到这次连接。

### 4. 停止

托盘退出桌面程序，或：

```bash
dock down
```

`start-dock.sh` / `stop-dock.sh` 分别对应 `dock up` / `dock down`。

## 连接时改了什么

第一次打开连接页时，会查看 Windows 的 `PATH`；如果装了 WSL，再查看 WSL 登录 shell 的 `PATH`（忽略 `/mnt/*` 下的 Windows 程序）。没有 WSL 时只显示 Windows 侧，不要求安装 WSL。

同名工具在两侧各算一条。支持的命令是 `claude`、`grok`、`codex`、`cursor-agent`（界面显示为 Cursor）和 `dsh`。

| 工具 | 连接方式 | 能报到 Dock 的状态 |
| --- | --- | --- |
| Claude、Grok、Codex、Cursor | 写入该工具自己的 hook 配置 | 开始、等待、完成、失败（以及关闭） |
| DSH 等没有原生 hook 的 | 一层启动脚本，原命令和参数不变 | 只能可靠报告开始、正常结束、出错 |

没有 hook 的工具看不到「正在等你输入」。连接页会写明这条限制。Cursor 的命令行如果漏发结束事件，任务可能停在「工作中」，直到进程退出。

Claude / Codex / Cursor 首次改配置前会留一份备份（例如 `settings.json.agent-activity-dock.bak`）。断开时只移除 Dock 自己写入的 hook，不会用备份覆盖你后来改过的设置。

先预览、再连接、再断开：

```bash
dock connect cursor --dry-run
dock connect cursor
dock disconnect cursor
```

## 跳回终端

自己开的终端，「回去」只能回到最近交互过的窗口。窗口已经没了会明确失败，并提示用 `dock run`。Dock 不会按项目名或工具名去猜窗口标题。

要精确跳回，用 Dock 开专用的 Windows Terminal 标签：

```bash
dock run grok
```

设置里可以把 `dock run` 收成短命令（例如 `dr`）。Windows 写到 `%LOCALAPPDATA%\Agent Activity Dock`，若有 WSL 再写到 `~/.local/bin`。新开的终端里 `dr grok` 等于 `dock run grok`。也可以：

```bash
dock alias dr
```

`dock run --close grok` 会在新标签起来后关掉当前这个启动页。管道或脚本里即使带 `--close` 也不会关。

默认沿用当前 Windows Terminal 标签的配置。可以用 `dock run --profile "Ubuntu-24.04" grok` 指定。

## 常用命令

```bash
dock status
dock agents
dock run grok
dock down
```

需要手动报状态，或给其他工具接入时：

```bash
dock start my-task --source claude
dock waiting my-task --source claude
dock permission my-task --source claude
dock complete my-task --source claude
dock fail my-task --source claude
dock acknowledge --source claude --session-id my-task
dock reset --source claude --session-id my-task
```

面板上的「已读」和「清除」对应 `acknowledge` 和 `reset`。`reset` / 清除只拿掉 Dock 这边的追踪，不会向 Agent 发任何命令。清掉之后，迟到的完成、等待、失败不会把这条任务救回来。

子任务会叠进父任务，不单独占小球上的数字。

## 给其他工具接入

Dock 只接收工具主动发来的状态，不读它们的工作内容。用上面的 `dock start` / `complete` 等命令即可，不必自己写通信代码。完整字段、拒绝规则和各工具能报到哪一步，见 [`docs/event-contract.md`](docs/event-contract.md)。

要点：

- 只走当前用户本机通道（Linux 用本地 socket，Windows 用 named pipe），不监听网络端口
- 单条请求超过 16 KiB、时间过旧或过新、字段不合法时会被拒绝，且不改变现有状态
- 重复的事件 ID 会去重，不会反复提醒或重复计数

## 隐私与本地数据

- Linux 状态文件默认 `$XDG_STATE_HOME/agent-activity-dock/state.json`；Windows 默认 `%LOCALAPPDATA%\Agent Activity Dock\state.json`
- Linux 的 socket 目录权限 0700、socket 0600；Windows 使用本机 named pipe。服务不监听网络端口
- 写到磁盘的只有来源、任务 ID、状态、时间、已读标记、终端标记和项目路径。摘要、提示词、命令和对话记录只在内存里短暂存在
- 重启会恢复这份最小状态，但不会重播旧声音；过期事件会被丢掉
- 审计页只保留当前这次运行里最近 128 条变更，不写入状态文件
- 声音、界面或某个 Agent 连接失败，不会把 Dock 带停。卡住时可以用面板或 `dock reset` 清掉过期追踪

已连接的 Agent 进程退出后，对应任务会从列表里拿掉。Dock 只查询 hook 记下的那一个进程是不是还在，不会扫整张进程表。

## 从源码开发

需要 Rust 1.80+、Node.js 20+、npm。桌面壳是 Tauri，界面是 Svelte。

```bash
bash scripts/install-cli.sh
npm install --prefix frontend
npm --prefix frontend run dev
```

检查与测试：

```bash
npm --prefix frontend run check
npm --prefix frontend run test
cargo test --workspace
```

在 WSL 里交叉编译不带安装包的 Windows 可执行文件：

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
npm run tauri -- build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle
```

产物在 `target/x86_64-pc-windows-msvc/release/`。NSIS 安装包由 Windows CI 打。

WSL 里的 `dock` 会把查询状态、上报事件、启动 / 停止送到 Windows 上的桌面程序；`connect`、`agents`、`run`、`alias` 仍在 Agent 所在的系统执行。需要临时改回 WSL 本地后台时：`AGENT_ACTIVITY_DOCK_BACKEND=wsl`。

领域边界见 [`docs/agents/domain.md`](docs/agents/domain.md)，设计取舍见 [`docs/adr/`](docs/adr/)。

## 当前平台

Windows 桌面程序是日常入口：没有 WSL 也可以安装、连接和使用。WSL 是可选的第二侧，两边的任务进同一个小球。macOS 安装包尚未验证。

## 许可

MIT

# Agent Activity Dock

同时开着几个终端里的 AI 助手，窗口一多就很难看出谁在干活、谁在等你。

Agent Activity Dock（下称 Dock）在桌面上放一个小球，把 Claude、Grok、Codex、Cursor、DSH 的任务收在一起：**工作时只显示数量，需要你处理时才提醒一次。** 点开小球是任务面板，按项目分组，可以从列表直接跳回对应的终端。

Dock 不读取对话、提示词、命令或代码，也不替你操作这些工具。默认不联网，所有状态只保存在本机当前用户目录下。

## 它做什么、不做什么

**做**

- 多个工具共用一个小球，数字含义是「正在工作 / 正在追踪」
- 等待输入、等待授权或失败时，弹一次系统通知并播提示音；正常完成只播提示音
- 连接本机已装好的工具，不替换、不修改原程序
- 用 `dock run` 在专属终端标签里启动工具，之后可以从面板精确跳回

**不做**

- 不读取对话记录、提示词、命令、代码、终端输出或凭据
- 不扫描进程列表去猜测工具是否在工作
- 不替换工具的可执行文件；连接失败也不会下载或重装任何东西
- 不监听网络端口，不上传任何数据

## 界面

| 页面 | 作用 |
| --- | --- |
| 小球 | 桌面常驻。数字是「工作中 / 追踪中」；右上角 `?` 表示有任务在等你，`!` 表示有失败。 |
| 动态 | 当前任务列表，按项目分组，可筛选全部 / 工作中 / 未工作；「回去」跳到对应终端。 |
| 审计 | 本次运行里最近的完成、失败、等待和关闭记录，最多 128 条，只在内存中。 |
| 连接 | 列出本机检测到的工具，逐个确认后连接或断开。 |
| 设置 | 外观主题、提示音、系统通知、开机自启、快捷键，以及给 `dock run` 设置短命令。 |

默认快捷键 `Ctrl+Shift+Space` 打开或收起面板。托盘菜单可以隐藏小球或退出程序。小球右上角的圆标可以在设置里关掉。

外观主题有五套：原型、Fluent、Glyph、Braun、Glass。

## 系统要求

- Windows 10 / 11。桌面程序（小球和面板）只在 Windows 上运行
- WSL 可选：装了 WSL 时，WSL 里的工具会和 Windows 上的出现在同一个小球里；没装也完全不影响使用
- macOS 与 Linux 桌面暂未正式支持

## 安装

安装分两步：装桌面程序，然后连接你已有的工具。无论工具装在 Windows、WSL 还是两边都有，步骤都相同。

### 第一步：安装并启动桌面程序

安装包从 [GitHub Releases](https://github.com/QZYH123/agent-activity-dock/releases) 下载（也可以按下文从源码构建）。

首次启动时它会自动完成命令行的准备工作：

- 把 `dock` 命令装到 `%LOCALAPPDATA%\Agent Activity Dock`，并加入当前用户的 PATH
- 检测到 WSL 时，把 Linux 版 `dock` 装到 WSL 的 `~/.local/bin`

设置页可以打开「开机自启」。同一个用户只运行一份 Dock。

### 第二步：连接工具

**新开**一个终端（PowerShell、cmd、Windows Terminal 或 WSL 均可；已经开着的终端读不到新写入的 PATH），确认命令可用：

```
dock status
```

然后在面板「连接」页逐个连接检测到的工具，或者用命令行：

```
dock agents
dock connect claude
dock connect grok
dock connect codex
dock connect cursor
```

在哪个系统里执行 `dock connect`，连接的就是那个系统里的工具。连接前会先列出将要修改的文件，经你确认才动手。

连接完成后，在**新开的**终端里照常使用 `claude`、`grok`、`codex`、`cursor-agent`、`dsh`，任务状态会自动出现在小球上。

想退出 Dock：托盘菜单退出，或执行 `dock down`。

## 日常使用

### 跳回终端

面板里每个任务都有「回去」，行为分两种：

- 用 `dock run` 启动的工具（例如 `dock run grok`）：Dock 会开一个专属的 Windows Terminal 标签，「回去」能精确回到这个标签，标签被拖出或合并过也有效
- 自己手动开的终端：「回去」只能回到该窗口最近交互过的位置；窗口已经关掉时会明确提示失败，不会跳错地方

`dock run` 的补充用法：

```
dock run --close grok                    # 新标签启动成功后，关掉当前这个旧标签（脚本和管道里不生效）
dock run --profile "Ubuntu-24.04" grok   # 指定 Windows Terminal 配置文件
dock alias dr                            # 给 dock run 起短命令，之后 dr grok 即可
```

短命令也可以在设置页里改。

### 处理面板上的任务

- 「已读」：确认一条等待你的任务，去掉提醒标记（对应命令 `dock acknowledge`）
- 「清除」：把一条卡住的任务从列表拿掉（对应命令 `dock reset`）。只影响 Dock 的显示，不会向工具发送任何命令；清掉之后这条任务不会再回来
- 工具进程退出后，对应任务会自动从列表消失
- 子任务计入所属的主任务，不单独占小球上的数字

## 连接时改了什么

连接方式取决于工具是否提供钩子（hook，即工具自带的事件通知机制）：

| 工具 | 连接方式 | 能报告的状态 |
| --- | --- | --- |
| Claude、Grok、Cursor | 在该工具自己的配置里登记钩子 | 开始、等待、完成、失败、关闭 |
| Codex | 在该工具自己的配置里登记钩子 | 开始、等待、完成、关闭；打断和报错看不到 |
| DSH 等没有钩子的工具 | 加一层启动脚本，原命令和参数不变 | 开始、正常结束、出错 |

几点说明：

- 没有钩子的工具看不到「正在等你输入」，连接页会标明这一限制
- Cursor 命令行偶尔不会通知已经结束，此时任务会停在「工作中」，直到进程退出才消失
- Codex 用 Esc 或 Ctrl+C 打断当前回复时，不会通知 Dock，任务会停在「工作中」；对话报错也不会显示为失败。用面板「清除」拿掉即可；退出 Codex 后任务也会从列表消失
- 首次修改 Claude / Codex / Cursor 的配置前，会保留一份备份（例如 `settings.json.agent-activity-dock.bak`）
- 断开连接只移除 Dock 自己写入的内容，不会动你后来改过的其他设置

先看后连：

```
dock connect cursor --dry-run   # 只显示将要改什么，不实际修改
dock connect cursor
dock disconnect cursor
```

连接页会查看：桌面程序能看到的 PATH、用户 PATH，以及常见安装目录（例如 `%USERPROFILE%\.local\bin`、Grok 的 `%USERPROFILE%\.grok\bin`、Cursor 命令行的 `%LOCALAPPDATA%\cursor-agent`）。装了 WSL 时再查看 WSL 里的 PATH（WSL 里能看到的 Windows 程序不重复计）。同名工具两边都装了就各算一条，分开连接。找不到时，可在连接页选择包含可执行文件的文件夹。Cursor 编辑器本身不会被当成命令行工具。

## 隐私与数据

- 只在本机、当前用户范围内通信：Linux 用 Unix socket（目录权限 0700、socket 0600），Windows 用命名管道；不监听任何网络端口
- 写入磁盘的只有：来源、任务 ID、状态、时间、已读标记、终端标记、项目路径。状态文件在 Windows 是 `%LOCALAPPDATA%\Agent Activity Dock\state.json`，在 Linux 是 `$XDG_STATE_HOME/agent-activity-dock/state.json`（默认 `~/.local/state` 下）
- 任务摘要只在内存里短暂存在，不写盘；对话、提示词、命令一概不经手
- 审计页只保留本次运行的最近 128 条记录，重启即清空
- 重启后会恢复上述最小状态，但不会重播提醒；过期事件直接丢弃
- 判断工具进程是否退出时，只查询连接时记下的那一个进程，不扫描整个进程列表
- 提示音、界面或某个连接出问题，不影响状态服务本身；卡住的任务可以随时「清除」

## 常见问题

**新终端里找不到 `dock` 命令？**
先确认桌面程序启动过一次（命令行由它负责安装），然后再开一个新终端。仍然不行时，手动把 `%LOCALAPPDATA%\Agent Activity Dock`（Windows）或 `~/.local/bin`（WSL）加入 PATH。

**任务一直显示「工作中」？**
工具可能被强制结束，或没有把「结束了」告诉 Dock（例如在 Codex 里按 Esc 打断）。用面板「清除」或 `dock reset` 拿掉即可；进程真正退出后 Dock 也会自动清理。

**没装 WSL 能用吗？**
能。WSL 完全可选，没装时连接页只显示 Windows 上的工具。

**连接页找不到 Windows 上明明能用的工具？**
像 fnm、nvm 这类只在某个终端里临时加路径的，桌面程序看不到。官方安装一般会写进用户目录，刷新连接页即可。仍没有时，在连接页选那个可执行文件所在的文件夹。

**Dock 能看到我的对话内容吗？**
不能。Dock 只接收工具主动发来的状态变化（开始了、在等你、完成了），不读取工具的输入输出，见「隐私与数据」。

## 卸载

1. 在面板「连接」页断开所有工具（或对每个工具执行 `dock disconnect <名字>`），移除 Dock 写入的钩子和启动脚本
2. 在 Windows「设置 → 应用」里卸载 Agent Activity Dock
3. 如需彻底清理，手动删除 `%LOCALAPPDATA%\Agent Activity Dock`（内含状态文件和命令行）、WSL 里的 `~/.local/bin/dock`，以及 shell 配置文件中 `# >>> agent-activity-dock PATH >>>` 标记的段落

## 给其他工具接入

Dock 只接收工具主动发来的状态，不读取工作内容。任何能执行命令的工具都可以接入，不需要写通信代码：

```
dock start my-task --source my-tool
dock waiting my-task --source my-tool
dock permission my-task --source my-tool
dock complete my-task --source my-tool
dock fail my-task --source my-tool
dock acknowledge --source my-tool --session-id my-task
dock reset --source my-tool --session-id my-task
```

约束：

- 单条请求超过 16 KiB、时间戳过旧或过新、字段不合法时会被拒绝，且不影响现有状态
- 重复的事件 ID 会去重，不会重复提醒或重复计数

完整的字段定义、拒绝规则和各工具能报告到哪一步，见 [`docs/event-contract.md`](docs/event-contract.md)。

## 从源码开发

需要 Rust 1.80+、Node.js 20+、npm。桌面壳是 Tauri 2，界面是 Svelte 5。

```bash
bash scripts/install-cli.sh        # 编译并安装 dock / dockd 到 ~/.local/bin（开发用）
npm install --prefix frontend
npm --prefix frontend run dev
```

检查与测试：

```bash
npm --prefix frontend run check
npm --prefix frontend run test
cargo test --workspace
```

在 WSL 里交叉编译免安装的 Windows 可执行文件：

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
npm run tauri -- build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle
```

产物在 `target/x86_64-pc-windows-msvc/release/`。免安装 exe 需要把 Linux 版命令行 `dock-wsl` 和主程序放在同一目录，才具备自动安装 WSL 侧命令行的能力；Windows 安装包（NSIS）由 CI 的 `package-windows` 任务构建。

架构上，Windows 桌面程序是唯一的状态服务：WSL 里的 `dock` 把查询和事件转发给它，只有 `connect`、`agents`、`run`、`alias` 在工具所在的系统上执行。开发调试时可用环境变量 `AGENT_ACTIVITY_DOCK_BACKEND=wsl` 临时切回 WSL 本地服务。

术语与边界见 [`docs/agents/domain.md`](docs/agents/domain.md)，设计决策见 [`docs/adr/`](docs/adr/)。

## 许可

MIT

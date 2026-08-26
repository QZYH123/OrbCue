# 从源码开发

给要改代码、跑测试或打安装包的人。用户安装与日常使用见仓库根目录 [README](../README.md)。

## 环境

- Rust 1.80+
- Node.js 20+、npm
- 桌面壳：Tauri 2；界面：Svelte 5

按你要做的事再补：

| 目标 | 额外依赖 |
| --- | --- |
| Windows 本机跑桌面 / 打 NSIS | WebView2（Windows 10/11 通常已有） |
| Linux 上 `cargo check -p orbcue` | `libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`、`pkg-config` |
| 在 WSL / Linux 上交叉编译 Windows exe | `cargo-xwin`，以及 `rustup target add x86_64-pc-windows-msvc` |

## 仓库结构

| 路径 | 职责 |
| --- | --- |
| `crates/dock-core` | 状态机、跳回决策、通知语义 |
| `crates/dock-ipc` | 本机协议（Unix socket / Windows 命名管道） |
| `crates/dock-service` | 本地状态服务，给桌面进程和无头模式用 |
| `crates/dock-adapters` | 各工具的结构化 payload → Dock 事件 |
| `crates/dock-connect` | 发现已装工具、写 hook / wrapper、PATH |
| `crates/dock-cli` | `orb` 命令 |
| `src-tauri` | Windows 桌面壳；默认就是状态服务 |
| `frontend` | 小球和面板（Svelte 5） |

根目录的 `src/agent_activity_dock/` 和 `tests/` 是旧实验，不是当前构建路径，改功能时不要走那里。

改领域行为或事件契约前读 [`docs/agents/domain.md`](agents/domain.md) 和 [`docs/event-contract.md`](event-contract.md)。

## 跑起来

三件事不要混：

1. **Windows 本机：真正的桌面程序**
2. **浏览器：只看 UI，数据是假的**
3. **Linux / WSL：只装 `orb` / `orbd` CLI**

### Windows 本机（桌面）

```bash
npm ci --prefix frontend
npm run tauri -- dev
```

这会编译 sidecar、起 Vite，再启动 Tauri。小球和面板、连接、通知、跳回都走这条路径。

### 浏览器预览（假数据）

```bash
npm ci --prefix frontend
npm --prefix frontend run dev
```

打开 <http://127.0.0.1:1420/> 看面板；`?label=ball` 看小球；`?theme=glyph`（或 `fluent` / `braun` / `glass` / `prototype`）切主题。没有 Tauri 桥，连接、跳回、通知都不会真的执行。

### 只装 Linux / WSL 命令行

```bash
bash scripts/install-cli.sh
```

编译并安装 `orb`、`orbd` 到 `~/.local/bin`（可用 `ORBCUE_BIN` 改目录）。这是开发路径，也是 `ORBCUE_BACKEND=wsl` 回滚路径。普通用户的 `orb` 由 Windows 桌面程序在首次启动时安装。

## 检查与测试

与 CI 对齐的核心命令：

```bash
npm --prefix frontend run check
npm --prefix frontend run test
cargo fmt --all -- --check
cargo test --workspace --exclude orbcue
```

Tauri 包 `orbcue` 在 Linux 上需要 GTK/WebKit 才能编过。有系统库时再跑：

```bash
cargo check -p orbcue
```

在 Windows 上还可以：

```bash
npm run tauri -- build --debug --no-bundle
```

CI 配置见 [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)。frontend job 目前跑 `check` 和 `build`；本地改 UI 请额外跑 `npm --prefix frontend run test`。

## 构建 Windows 桌面程序

### 在 Windows 上打 NSIS 安装包

```bash
npm ci --prefix frontend
npm run tauri -- build --bundles nsis
```

产物在 `target/release/bundle/nsis/`。

首次启动若要自动给 WSL 安装 `orb`，构建时需要 Linux 版命令行 `src-tauri/resources/orb-wsl`。本机没有这个文件时，安装包往往仍能打出来，但 WSL 侧不会自动装 CLI。CI 会先编 musl 的 `orb`，再交给 Windows job。

### 在 WSL / Linux 上交叉编译免安装 exe

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
npm ci --prefix frontend
npm run tauri -- build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle
```

产物在 `target/x86_64-pc-windows-msvc/release/`。免安装 exe 要把 Linux 版 `orb-wsl` 和主程序放在同一目录，才具备自动安装 WSL 侧命令行的能力。

推送 `v*` tag（例如 `v0.2.0`）会跑 [`.github/workflows/release.yml`](../.github/workflows/release.yml)：测试通过后打 NSIS，并创建一个 GitHub Release，只挂 Windows 安装包。

## 架构

Windows 桌面程序是唯一的状态服务：对本机 named pipe 做 `attach_or_listen`。WSL 里的 `orb` 把查询和事件转发给它。`connect`、`agents`、`run`、`alias` 在工具所在的系统上执行。

同一用户不要同时跑两份 daemon。开发调试时可用 `ORBCUE_BACKEND=wsl` 强制切到 WSL 本地 `orbd`。这是显式回滚，不要和默认的 Windows presenter 同时开，否则两边各有一份状态（裂脑）。不要在两条路径之间做静默探测切换。

排「状态不更新」时先确认：当前 `orb` 连的是 Windows 命名管道，还是 WSL 的 socket；以及有没有残留的 `orbd`。

## 给另一个工具接入

能执行命令就够，不必改 crate，也不要 import 状态机内部类型：

```
orb start <session-id> --source <tool>
orb waiting <session-id> --source <tool>
orb complete <session-id> --source <tool>
```

完整字段和拒绝规则见 [`docs/event-contract.md`](event-contract.md)。调用示例见 [`examples/mcp-skill-note.md`](../examples/mcp-skill-note.md)。

要做一等适配器：payload 翻译放 `crates/dock-adapters`，发现与改配置放 `crates/dock-connect`。适配器只读结构化 stdin，即使 payload 里有 `transcript_path` 也不打开。

## 相关文档

- [`docs/agents/domain.md`](agents/domain.md) — 术语与不可违反的边界
- [`docs/event-contract.md`](event-contract.md) — 稳定集成契约
- [`docs/adr/`](adr/) — 架构决策
- [`docs/design-language.md`](design-language.md) — 界面约束
- [`docs/agents/issue-tracker.md`](agents/issue-tracker.md) — 本地 ticket 入口

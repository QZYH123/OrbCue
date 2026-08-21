# 验证记录

本记录对应 Rust/Tauri 主线。旧 Python/Electron 验收脚本仍保留在仓库中，但不再是正式桌面入口。

Win+WSL 桌面路径是 **Windows presenter exe + WSL `dock bridge`**。WSLg / Linux WebKitGTK 呈现层已退役。

## 已通过

在当前 WSL2 环境（Rust 1.97、Node 24、Python 3、cargo-xwin、x86_64-pc-windows-msvc target）执行：

```bash
cargo fmt --all
cargo test --workspace --exclude agent-activity-dock
cargo check -p agent-activity-dock --target x86_64-pc-windows-msvc
npm run build
```

交叉编译绿色 exe：

```bash
npm run tauri -- build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle
```

结果：

- Rust workspace 测试全部通过，含新增 `dock bridge` 集成测试（subscribe / ack / reset 快照流）。
- Windows MSVC target 的 `agent-activity-dock` check 通过（本机 `llvm-rc` 需在 PATH；见下方交叉编译说明）。
- Vite production build 成功。
- 交叉编译产物：`target/x86_64-pc-windows-msvc/release/agent-activity-dock.exe`（约 9.3 MiB）。本机 Linux 上 Tauri 只提供 deb/rpm/appimage bundle 选项，NSIS 安装包留给 Windows CI。

覆盖的行为包括：

- 多会话聚合、重复事件幂等、waiting/permission/completed/failed/cancelled；
- 事件大小、字段长度、metadata 数量、时间戳格式、过旧/过新事件拒绝；
- acknowledgement 与通配符 reset；
- Unix socket 与 Windows named pipe 共用 snapshot/subscribe/acknowledge/reset、持久化和重启恢复协议；
- `dock bridge` 在 stdin/stdout 与当前用户 socket 之间转发 NDJSON，一条进程一条连接；
- 状态持久化不包含摘要、prompt、命令或 transcript；
- Claude/Codex/DSH 结构化 payload adapter；
- wrapper 参数和退出码透传、真实 Agent 可执行文件不被替换、最后一个 wrapper 断开时清理 PATH 注入；
- Claude `settings.json` 非法时拒绝覆盖，首次修改前保留备份，断开只移除 Dock 自己的 Hook。

## 交叉编译与 interop 启动

本机交叉编译需要：

- `cargo install cargo-xwin` 与 `rustup target add x86_64-pc-windows-msvc`
- `llvm-rc` 在 PATH（`tauri-winres` 生成 Windows resource）
- 能预处理 `.rc` 的 `clang-cl` 或等价包装（cargo-xwin 会把 `CC` 指到它）

从 WSL 直接执行构建出的 exe（Windows interop，绕过 WSLg）后的文本断言：

```text
# presenter stderr
wsl: Failed to translate 'E:\mingw64\bin'
Agent Activity Dock attached via WSL dock bridge

# pgrep -a -f '/.local/bin/dock bridge'
36398 /home/qingz/.local/bin/dock bridge

# 已有 dockd 未被替换
8080 /home/qingz/.local/bin/dockd

# dock start smoke-cross --source probe
accepted — 1/2 · pending 1 ?
  [?] grok:... — needsattention
  [ ] probe:smoke-cross — working

# dock --json status → count_label 1/2, sessions 含 probe:smoke-cross

# dock reset --source probe --session-id smoke-cross
accepted — 0/1 · pending 1 ?
  [?] grok:... — needsattention
```

随后用 PowerShell `Stop-Process -Name agent-activity-dock` 结束 exe。用户本机 `dockd`（pid 8080）仍在；常驻 `dock bridge` 随 presenter 退出。

`scripts/prepare-sidecar.mjs` 在从 Linux 打 Windows 目标时走 `cargo xwin build`，产出 `src-tauri/binaries/dock-x86_64-pc-windows-msvc.exe`。

## 当前无法在本机完成的验证

- Linux 原生 Tauri（WebKitGTK）本机仍缺系统 GTK/WebKit 开发包；真 Linux 桌面走标准 Tauri Linux 构建，不再使用 WSLg sysroot。
- NSIS 安装包不能在 Linux 上打（当前 Tauri CLI 在 Linux 宿主上只列出 deb/rpm/appimage）。Windows CI job（`.github/workflows/ci.yml` 的 `windows-rust`）继续打 Windows 包。
- macOS 安装包尚未做 CI/真机验证。

## 已知风险

- 不要同时跑两个 `dockd`。Windows presenter 经 `dock bridge` attach 到 WSL daemon，不再 listen named pipe。
- wrapper 对没有原生 Hook 的 Agent 只能报告开始和退出，不能可靠判断等待输入。
- 强制终止、断电或事件丢失仍需要用户执行 `dock reset`。
- Claude settings schema 随 Claude 版本变化，连接前应保留用户配置备份并在升级后复测 Hook。
- presenter 依赖 WSL 内 `$HOME/.local/bin/dock`；可用 `AGENT_ACTIVITY_DOCK_WSL_DISTRO` 和 `AGENT_ACTIVITY_DOCK_BRIDGE_COMMAND` 覆盖。

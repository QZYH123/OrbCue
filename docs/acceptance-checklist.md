# Release Acceptance Checklist

这是 Rust/Tauri 主线的发布门槛。旧 Python/Electron 验收脚本只用于迁移回归，不代表新的桌面入口已经通过系统级 GUI 验证。

## 自动检查

```bash
cargo fmt --all
cargo test --workspace --exclude agent-activity-dock
cargo check -p agent-activity-dock
npm run check
npm run build
python3 -m pytest -q
```

打包前端、CLI sidecar 和 Tauri native binary：

```bash
npm run tauri -- build --debug --no-bundle
```

完整安装包需要 Linux GTK/WebKitGTK 依赖，见 [`README.md`](../README.md) 和 [`verification.md`](verification.md)。

## 行为门槛

| 项目 | 状态 | 证据 |
| --- | --- | --- |
| 状态机覆盖工作、等待、授权、完成、失败、取消 | PASS | `crates/dock-core/tests` |
| 重复事件幂等、事件 ID 去重 | PASS | core/service tests |
| 过旧、过新、越界、坏 JSON 拒绝 | PASS | core/IPC tests |
| snapshot、subscribe、acknowledge、reset | PASS | service local socket tests |
| 审计状态流映射和面板只读展示 | PASS | core/IPC tests + Svelte audit page |
| 持久化不包含摘要或 Agent 内容 | PASS | core/service tests |
| wrapper 不替换原 Agent，断开可撤销 | PASS | connect behavior test |
| 首次连接确认和未在 PATH 的旧连接仍可断开 | PASS | Svelte flow / manager behavior |
| 完成/等待/失败提示音可独立关闭 | PASS | Svelte settings implementation |
| 手动清除陈旧状态 | PASS | CLI + IPC + Tauri command |
| Claude settings 严格解析、首次备份和精确卸载 Hook | PASS | dock-connect unit/integration tests |
| Tauri 退出清理 socket | PASS | service shutdown API |
| Linux 真实桌面窗口和安装包 | BLOCKED BY HOST | 缺少 GTK/WebKitGTK 开发库 |
| Windows named pipe 与 Rust workspace 交叉编译 | PARTIAL | `dock-ipc` 使用 interprocess；本机 Windows target `cargo check` 通过，Windows runtime 仍需 CI/真机 |
| Windows Tauri 桌面构建 | CI CONFIGURED | Windows runner 先生成 CLI sidecar，再执行 Tauri `--no-bundle` 构建 |
| macOS 构建与安装包 | NOT VERIFIED | 尚无 macOS runner 或真机证据 |

## 真实链路冒烟

启动 headless daemon 后，下面命令应依次显示 `1/1`、待授权和 `0/0`：

```bash
AGENT_ACTIVITY_DOCK_SOCKET=/tmp/dock.sock dock start s1 --source claude
AGENT_ACTIVITY_DOCK_SOCKET=/tmp/dock.sock dock permission s1 --source claude
AGENT_ACTIVITY_DOCK_SOCKET=/tmp/dock.sock dock reset --source claude --session-id s1
AGENT_ACTIVITY_DOCK_SOCKET=/tmp/dock.sock dock status
```

## 发布前人工检查

- 小球固定尺寸，普通进度不会强制打开面板；
- 待查看任务有 `!`，逐条确认和全部确认都能清除；
- 连接页面显示能力限制、确认对话框和错误，不会静默改写用户命令；
- 关闭声音后状态和待查看标记仍然出现；
- 断开最后一个 wrapper 后 shell 配置中的 Dock PATH block 被移除；
- 无桌面环境下 CLI/daemon 仍可独立工作。

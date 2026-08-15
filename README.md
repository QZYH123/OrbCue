# Agent Activity Dock

本地、事件驱动、低干扰的 Agent 状态小球。它只消费 Agent 明确发送的状态事件，
不读取 transcript、提示词、代码或终端输出，也不扫描进程。

## 快速开始

```bash
# 1. 启动小球一次（集成 daemon + X11 presenter；无 GUI 的环境用 daemon 命令）
python3 -m agent_activity_dock.ball

# 2. 第一次运行连接已经安装的 Agent；此流程只发现 PATH 上的现有命令，
#    写入可撤销的用户级 wrapper 或 Claude 原生 Hook。
#    想先看会改哪些用户文件而不实际写入，用：
python3 -m agent_activity_dock.cli setup --dry-run
#    交互式 setup 会逐个询问要连接哪些 Agent；--yes 则一次确认全部。
python3 -m agent_activity_dock.cli setup

# 3. 像平时一样继续输入原命令，例如 codex、claude、dsh
```

连接后，也可以手动或由 Skill/MCP 发送事件：

```bash
dock start my-task --source my-agent
dock stop my-task
dock waiting my-task
dock error my-task
dock reset my-task       # 或 dock reset --all 清除陈旧状态
dock status
```

响应是 accepted/rejected JSON 或人类可读摘要。`dock` 命令会在安装包后由
`pyproject.toml` 提供；源码运行时也可用 `python3 -m agent_activity_dock.cli`。

## 事件协议

集成作者请阅读 [`docs/event-contract.md`](./docs/event-contract.md)：最小事件
字段、五种动作、去重规则、IPC 响应和原始 socket 示例。

## 开发测试

```bash
PYTHONPATH=src python3 -m unittest discover -s tests -v
```

## 已知限制（MVP）

- 无 Hook 的命令行 Agent（Codex、DSH wrapper）只能报告 start/stop/error，
  不能准确报告等待输入。
- 强制 `kill -9`、断电或事件丢失不会自动恢复；用 `dock reset` 手动恢复。
- 不做心跳、租约、进程扫描、历史持久化或跨重启恢复。
- X11 presenter 使用 libX11 核心字体，WSLg/X11 环境可运行；终端 SSH 环境
  可先使用无窗口 daemon + `dock status`，后续再增加终端 presenter。

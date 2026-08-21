# 小球注意力体验：跳回、通知、项目分组、连接预览

**Status:** complete
**Labels:** completed

## Problem Statement

用户把 Agent 放到后台后，已经能从小球看到负载和 `o` / `?` / `!` / `*`。还缺四件不撑大小球的事：点一下回到源终端、系统通知、按项目认人、连接前看清 Dock 会改哪些文件。

这批工作借鉴 CLI-Manager 的 Hook 通知和跳回，不引入内置终端、历史 Diff、用量看板或桌宠。

## Solution

在现有 Rust/Tauri 主线和 Win+WSL daemon 上增加：

1. 面板记录可跳回源终端（不可靠时只打开面板并说明原因）。
2. `?` / `!` 可走系统通知，点击打开面板并高亮对应会话。
3. 会话卡片显示事件里自带的项目路径，列表面板可按项目分组。
4. 连接确认列出将写入或修改的文件和条目，断开仍只清理 Dock 自己的内容。

## Out of Scope

- 读取 transcript、prompt、命令、终端输出或进程表。
- 内置 PTY、分屏、SSH 工作区、Worktree 向导。
- 再接入 Pi / OpenCode 等更多 CLI（另开 ticket）。
- 改变主计数 `工作/打开`，或让普通工作事件强制展开。（小球/面板尺寸调整属于 `issue-win-presenter-polish.md` 的 ticket 14。）

## Tickets

- [08](./issues/08-jump-back-to-source-terminal.md)
- [09](./issues/09-system-notifications.md)
- [10](./issues/10-project-path-and-grouping.md)
- [11](./issues/11-connect-hook-preview.md)

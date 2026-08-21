# Windows 呈现层打磨：发现、摆放、真圆

**Status:** complete
**Labels:** completed

## Problem Statement

呈现层换侧到 Windows 后首轮真机目检发现三类问题：连接页每次同步等 `wsl.exe` 而卡顿、且非登录 shell 的 PATH 漏检 WSL 侧 Agent；小球强制吸附左右边缘、面板不跟随小球；透明方窗 + CSS 圆仍留下圆外模糊框，且球和面板都偏大。

## Solution

1. 连接页缓存 + 后台刷新；发现用登录 shell PATH，WSL/Windows 条目去重。
2. 去掉吸附，面板每次按球当前位置定位并跟随。
3. Win32 window region 在窗口层裁出真圆，同时缩小球（112 → 64）与面板（420×580 → 约 360×500）。

## Out of Scope

- 换掉 WebView2 或引入原生绘制的第二套 presenter。
- 小球多形态、皮肤、动画常驻。
- 新的设置项。

## Tickets

- [12](./issues/12-connections-page-latency-and-wsl-discovery.md)
- [13](./issues/13-ball-free-placement-and-panel-follow.md)
- [14](./issues/14-true-circle-region-and-compact-sizing.md)

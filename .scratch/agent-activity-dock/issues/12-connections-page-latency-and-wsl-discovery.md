# 12 — 连接页即时响应与完整的 WSL Agent 发现

**What to build:** 连接页切进去就能看，不再每次同步等 `wsl.exe` 冷启动；Agent 发现覆盖用户真实登录环境，WSL 侧装的 claude/codex 不再漏检。

**Blocked by:** 无

**Status:** complete

背景：presenter 在 Windows 上经 `wsl.exe -e sh -c …` 调 `dock agents`。非登录 `sh` 的 PATH 只有系统默认值加 Windows interop 的 `/mnt/c/...` 条目，所以 Windows 侧命令能被发现，而登录 shell 才注入的 `~/.local/bin`、nvm/npm 路径下的 WSL 侧 Agent 反而漏检；且每次切页都同步 spawn 一次 `wsl.exe`，页面卡顿。

- [x] presenter 缓存最近一次 inventory；切到连接页先渲染缓存，后台刷新完成后合并，页面切换不被检测阻塞。
- [x] 提供显式「刷新」动作；检测在途只显示轻量指示，不遮挡列表。
- [x] `dock agents` 的发现 PATH 来自用户登录 shell（`$SHELL -lc` 探测一次并缓存进程内），能发现 nvm/npm/`~/.local/bin` 安装的 claude / codex / grok / dsh。
- [x] 登录 shell 探测失败（超时、非常规 shell）时回退现有 PATH 行为并给一句诊断；不读进程表、不扫全盘。
- [x] 同一 Agent 的 WSL 条目与 Windows（`/mnt/*`）条目去重，优先展示 WSL 侧；纯 Windows 侧条目标注来源，不提供不适用的连接动作。
- [x] 行为测试：缓存命中即时渲染；shell 启动横幅等 stdout 污染不破坏 PATH 探测；去重规则有覆盖。

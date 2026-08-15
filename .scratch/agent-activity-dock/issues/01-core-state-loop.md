# 01 — 建立任务状态闭环

**What to build:** 让外部调用者可以报告一个任务开始、停止、等待输入、报错或重置，并从公开快照中看到工作数量、追踪数量和待查看标记。

**Blocked by:** None — can start immediately.

**Status:** complete

- [x] 公开事件入口能处理 `start`、`stop`、`waiting`、`error`、`reset`。
- [x] 单任务从开始到停止会更新 `working_count/tracked_count`。
- [x] 停止、等待或报错只产生一次提示效果，并留下待查看标记。
- [x] 重复开始、重复停止不会重复计数或重复提示。
- [x] 手动重置能清除陈旧的工作状态和待查看标记。
- [x] 先有失败行为测试，再以最小实现让测试通过。

# MVP Acceptance Checklist

This file maps every completion standard from `docs/agent-handoff.md` and the
ticket files to the evidence produced in this repository.  It is intentionally
checkable by the next session without re-interviewing the user.

## Automated evidence

```bash
./scripts/acceptance.sh
```

The gate script also runs a read-only `dock setup --dry-run` against the real
PATH and fails if any test, smoke check, visual probe, dry-run, or performance
threshold fails.  Individual commands:

```bash
PYTHONPATH=src python3 -m unittest discover -s tests -v
PYTHONPATH=src python3 scripts/ball_smoke.py
PYTHONPATH=src python3 scripts/visual_probe.py
AADOCK_JSON_ONLY=1 PYTHONPATH=src python3 scripts/perf_probe.py
```

The gate now writes per-step latest artifacts automatically:
`docs/tests-latest.txt`, `docs/ball-smoke-latest.txt`,
`docs/visual-probe-latest.txt`, `docs/setup-dry-run-latest.txt`, and
`docs/perf-latest.json`.

## Completion standards

| # | Standard | Status | Evidence |
| --- | --- | --- | --- |
| 1 | 状态核心和 IPC 自动化测试通过 | PASS | 55 tests, OK |
| 2 | 小球能创建、置顶、固定尺寸并显示计数 | PASS | X11 44x44, xwininfo, ball smoke 15/15, visual probe 4/4 |
| 3 | 至少一个已有 Agent 不重装、不加前缀接入 | PASS | temp HOME real `codex --version`, `dsh --version`, Claude hook fixtures |
| 4 | 停止、错误、等待输入触发一次提示 | PASS | `tests/test_attention.py`, fake sound sink |
| 5 | 重复事件不重复提示或错误计数 | PASS | core/IPC/attention tests |
| 6 | 空闲无轮询和常驻重绘 | PASS | select-loop code review + idle CPU 0.0 s/2 s |
| 7 | 事件到状态更新 < 100 ms | PASS | raw-socket p95: daemon 0.52 ms, ball 0.61 ms |
| 8 | 强制终止/断电限制和手动重置已记录 | PASS | `docs/verification.md`; kill -9/restart probe |
| 9 | Standards + Spec 两轴 review 和残留风险记录 | PASS | `docs/verification.md` |

## Ticket checklist

- [x] 01 core state loop
- [x] 02 local IPC and aggregation
- [x] 03 aggregate floating ball
- [x] 04 one-shot attention and `!`
- [x] 05 zero-install agent connection
- [x] 06 Codex / Claude Code / DSH thin adapters
- [x] 07 performance and handoff QA

## Real PATH dry-run (read-only, current machine)

```text
codex: connect via wrapper — create /home/qingz/.local/share/agent-activity-dock/bin/codex
       and prepend /home/qingz/.local/share/agent-activity-dock/bin to PATH in user shell rc
claude: connect via native_hook — add revocable claude hook entries to user settings.json
       and create /home/qingz/.config/agent-activity-dock/hooks/claude-hook.py
dsh: connect via wrapper — create /home/qingz/.local/share/agent-activity-dock/bin/dsh
       and prepend /home/qingz/.local/share/agent-activity-dock/bin to PATH in user shell rc
```

This dry-run makes no writes.  The next session should show this to the user,
then run `dock setup` in the real shell only after the user confirms.

#!/usr/bin/env bash
# Full MVP acceptance gate.  Run from the repository root.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "== unit and integration tests =="
PYTHONPATH=src python3 -m unittest discover -s tests 2>&1 | tee docs/tests-latest.txt

echo "== real ball end-to-end smoke =="
PYTHONPATH=src python3 scripts/ball_smoke.py | tee docs/ball-smoke-latest.txt

if [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
  echo "== pixel-level visual probe =="
  PYTHONPATH=src python3 scripts/visual_probe.py | tee docs/visual-probe-latest.txt
else
  echo "== pixel-level visual probe skipped (no display) =="
fi

echo "== real PATH setup dry-run (read-only) =="
PYTHONPATH=src python3 -m agent_activity_dock.cli setup --dry-run | tee docs/setup-dry-run-latest.txt

echo "== performance probe =="
AADOCK_JSON_ONLY=1 PYTHONPATH=src python3 scripts/perf_probe.py > "$ROOT/docs/perf-latest.json"
python3 - <<'PY'
import json
from pathlib import Path
p = Path('docs/perf-latest.json')
data = json.loads(p.read_text())
for name, key in (('daemon', 'daemon'), ('ball', 'ball')):
    p95 = data[key]['event_to_snapshot_latency']['p95_ms']
    idle = data[key]['idle_cpu_seconds_over_2s']
    assert p95 < 100, (name, p95)
    assert idle < 0.05, (name, idle)
    print(f"{name} event->snapshot p95 {p95} ms, idle cpu {idle} s")
if data['sound']['available']:
    print(f"sound stop max {data['sound']['stop_with_sound_max_ms']} ms")
print("performance gate PASS")
PY

echo "== acceptance gate PASS =="

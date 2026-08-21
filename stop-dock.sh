#!/usr/bin/env bash
set -euo pipefail
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/agent-activity-dock"
runtime_dir="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/agent-activity-dock"

if [[ -f "$state_dir/dockd.pid" ]]; then
  pid="$(cat "$state_dir/dockd.pid" 2>/dev/null || true)"
  if [[ -n "${pid:-}" ]] && kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid" 2>/dev/null || true
  fi
  rm -f "$state_dir/dockd.pid"
fi
pkill -x dockd 2>/dev/null || true
sleep 0.3
pkill -KILL -x dockd 2>/dev/null || true
rm -f "$runtime_dir/agent-activity-dock.sock" \
  "$state_dir/agent-activity-dock.sock"
echo "Agent Activity Dock daemon stopped."

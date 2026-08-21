#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)"
dest="${AGENT_ACTIVITY_DOCK_BIN:-$HOME/.local/bin}"
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/agent-activity-dock"
mkdir -p "$state_dir"

if [[ -x "$dest/dock" ]] && "$dest/dock" status >/dev/null 2>&1; then
  echo "Dock daemon already running"
  "$dest/dock" status
  exit 0
fi

if [[ ! -x "$dest/dockd" ]]; then
  echo "Installing dock CLI and daemon to $dest"
  bash "$root/scripts/install-cli.sh"
fi

nohup "$dest/dockd" >>"$state_dir/dockd.log" 2>&1 &
echo $! >"$state_dir/dockd.pid"

for _ in 1 2 3 4 5 6 7 8 9 10; do
  if "$dest/dock" status >/dev/null 2>&1; then
    echo "dockd ready"
    "$dest/dock" status
    exit 0
  fi
  sleep 0.2
done

echo "dockd did not become ready; last log lines:" >&2
tail -n 20 "$state_dir/dockd.log" >&2 || true
exit 1

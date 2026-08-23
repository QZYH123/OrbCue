#!/usr/bin/env bash
set -euo pipefail
dest="${AGENT_ACTIVITY_DOCK_BIN:-$HOME/.local/bin}"

if [[ -x "$dest/dock" ]]; then
  exec "$dest/dock" down
fi

echo "dock is not installed at $dest" >&2
exit 1

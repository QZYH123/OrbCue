#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)"
dest="${AGENT_ACTIVITY_DOCK_BIN:-$HOME/.local/bin}"

if [[ ! -x "$dest/dock" ]]; then
  echo "Installing dock CLI to $dest"
  bash "$root/scripts/install-cli.sh"
fi

exec "$dest/dock" up

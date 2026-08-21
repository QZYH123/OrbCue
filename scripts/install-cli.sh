#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dest="${AGENT_ACTIVITY_DOCK_BIN:-$HOME/.local/bin}"
mkdir -p "$dest"
cargo build -p agent-activity-dock-cli --release --manifest-path "$root/Cargo.toml"
cargo build -p agent-activity-dock-service --release --manifest-path "$root/Cargo.toml"
install -m 755 "$root/target/release/dock" "$dest/dock"
install -m 755 "$root/target/release/dockd" "$dest/dockd"
echo "Installed $dest/dock and $dest/dockd"
if ! echo ":$PATH:" | grep -q ":$dest:"; then
  echo "Add $dest to PATH for this shell, then reopen terminals:"
  echo "  export PATH=\"$dest:\$PATH\""
fi

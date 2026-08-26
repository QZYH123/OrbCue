#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dest="${ORBCUE_BIN:-$HOME/.local/bin}"
mkdir -p "$dest"
cargo build -p orbcue-cli --release --manifest-path "$root/Cargo.toml"
cargo build -p orbcue-service --release --manifest-path "$root/Cargo.toml"
install -m 755 "$root/target/release/orb" "$dest/orb"
install -m 755 "$root/target/release/orbd" "$dest/orbd"
echo "Installed $dest/orb and $dest/orbd"
echo "This script is the developer path and the ORBCUE_BACKEND=wsl rollback path."
echo "Normal users get orb from the Windows desktop app (Windows CLI on first launch; WSL ~/.local/bin/orb when WSL is detected)."
echo "Canonical daemon is the GUI-OS presenter; WSL orbd is only for ORBCUE_BACKEND=wsl rollback."
if ! echo ":$PATH:" | grep -q ":$dest:"; then
  echo "Add $dest to PATH for this shell, then reopen terminals:"
  echo "  export PATH=\"$dest:\$PATH\""
fi

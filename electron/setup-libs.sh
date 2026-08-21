#!/usr/bin/env bash
# Download the small set of Ubuntu runtime libraries Electron needs when they
# are not already installed system-wide.  Kept in vendor-libs/ so no sudo is
# required for this dev workspace.
set -euo pipefail
cd "$(dirname "$0")"

ELECTRON_BIN=node_modules/electron/dist/electron
LIBS_DIR=vendor-libs/usr/lib/x86_64-linux-gnu

if ldd "$ELECTRON_BIN" 2>/dev/null | grep -q 'not found'; then
  :
else
  echo "electron runtime libraries already present"
  exit 0
fi

mkdir -p "$LIBS_DIR"
cd /tmp
download_and_extract() {
  local subpath="$1"
  local deb="$(basename "$subpath")"
  curl --noproxy '*' -sSL \
    "http://archive.ubuntu.com/ubuntu/pool/main/$subpath" -o "$deb"
  dpkg-deb -x "$deb" /tmp/agent-activity-dock-electron-libs
  rm -f "$deb"
}

rm -rf /tmp/agent-activity-dock-electron-libs
mkdir -p /tmp/agent-activity-dock-electron-libs
download_and_extract 'n/nspr/libnspr4_4.35-1.1build1_amd64.deb'
download_and_extract 'n/nss/libnss3_3.98-1ubuntu0.2_amd64.deb'
download_and_extract 'a/alsa-lib/libasound2t64_1.2.11-1ubuntu0.3_amd64.deb'

mkdir -p "$(dirname "$0")/$LIBS_DIR" 2>/dev/null || true
cd "$OLDPWD"
rm -rf vendor-libs
mkdir -p vendor-libs
cp -a /tmp/agent-activity-dock-electron-libs/. vendor-libs/
rm -rf /tmp/agent-activity-dock-electron-libs
echo "electron runtime libraries installed under electron/vendor-libs"

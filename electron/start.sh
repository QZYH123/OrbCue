#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
if [[ ! -x node_modules/.bin/electron ]]; then
  echo "installing electron dependencies..."
  npm install
fi
if ! ldd node_modules/electron/dist/electron 2>/dev/null | grep -q 'not found'; then
  LIB_PRELOAD=""
else
  if [[ ! -f vendor-libs/usr/lib/x86_64-linux-gnu/libnss3.so ]]; then
    echo "installing electron runtime libraries..."
    bash setup-libs.sh
  fi
  export LD_LIBRARY_PATH="$PWD/vendor-libs/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
exec ./node_modules/.bin/electron .

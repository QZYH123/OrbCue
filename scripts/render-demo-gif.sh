#!/usr/bin/env bash
# 3 帧状态变化 → docs/screenshots/demo.gif
# 用法：scripts/render-demo-gif.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
src="$root/scripts/demo-gif.html"
out="$root/docs/screenshots/demo.gif"
chrome="${CHROME:-google-chrome}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# scene → ball class
render() {
  local scene="$1" ball="$2"
  sed -e "s/__SCENE__/${scene}/" -e "s/__BALL__/${ball}/" "$src" > "$tmp/${scene}.html"
  "$chrome" \
    --headless=new \
    --disable-gpu \
    --hide-scrollbars \
    --no-first-run \
    --force-device-scale-factor=2 \
    --window-size=480,448 \
    --virtual-time-budget=800 \
    --screenshot="$tmp/${scene}.png" \
    "file://${tmp}/${scene}.html" >/dev/null
}

render working working
render wait wait
render panel wait

python3 - "$tmp" "$out" <<'PY'
from pathlib import Path
import sys
from PIL import Image

tmp, out = Path(sys.argv[1]), Path(sys.argv[2])
rgb = [Image.open(tmp / f"{name}.png").convert("RGB") for name in ("working", "wait", "panel")]
frames = [
    im.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.FLOYDSTEINBERG)
    for im in rgb
]
frames[0].save(
    out,
    save_all=True,
    append_images=frames[1:],
    duration=[1200, 1400, 2200],
    loop=0,
    optimize=True,
    disposal=2,
)
PY

echo "$out ($(du -h "$out" | cut -f1))"

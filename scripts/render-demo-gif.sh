#!/usr/bin/env bash
# 截真实预览：原型 工作中 → 等待 → 五套主题的小球+面板 → docs/screenshots/demo.gif
# 用法：scripts/render-demo-gif.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/docs/screenshots/demo.gif"
chrome="${CHROME:-google-chrome}"
base="http://127.0.0.1:1420"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"; if [[ -n "${vite_pid:-}" ]]; then kill "$vite_pid" 2>/dev/null || true; fi' EXIT

started_vite=0
if ! curl -sf -o /dev/null --max-time 1 "$base/"; then
  npm --prefix "$root/frontend" run dev -- --host 127.0.0.1 --port 1420 >/tmp/orbcue-demo-vite.log 2>&1 &
  vite_pid=$!
  started_vite=1
  for _ in $(seq 1 50); do
    curl -sf -o /dev/null --max-time 1 "$base/" && break
    sleep 0.2
  done
  curl -sf -o /dev/null --max-time 1 "$base/"
fi

shot() {
  local file="$1" w="$2" h="$3" url="$4"
  "$chrome" \
    --headless=new \
    --disable-gpu \
    --hide-scrollbars \
    --no-first-run \
    --no-default-browser-check \
    --user-data-dir="$tmp/chrome-$file" \
    --force-device-scale-factor=2 \
    --window-size="$w,$h" \
    --virtual-time-budget=4000 \
    --screenshot="$tmp/${file}.png" \
    "$url" >/dev/null
}

themes=(prototype fluent glyph braun glass)

shot working 480 448 "$base/?label=ball&theme=prototype&cue=working&onboarding=0&demo=ball"
shot wait 480 448 "$base/?label=ball&theme=prototype&cue=wait&onboarding=0&demo=ball"

for theme in "${themes[@]}"; do
  shot "panel-$theme" 480 448 "$base/?label=panel&theme=$theme&page=activity&onboarding=0&demo=panel"
  shot "orb-$theme" 80 80 "$base/?label=ball&theme=$theme&cue=wait&onboarding=0"
done

python3 - "$tmp" "$out" <<'PY'
from pathlib import Path
import sys
from PIL import Image, ImageDraw

tmp, out = Path(sys.argv[1]), Path(sys.argv[2])
DPR = 2
STAGE = (480 * DPR, 448 * DPR)
BALL_CSS = 56
ORB_SLOT = round(56 * 1.8 * DPR)
PAD = 24 * DPR
def load(name: str) -> Image.Image:
    return Image.open(tmp / f"{name}.png").convert("RGB")


def crop_centered(im: Image.Image, css: int) -> Image.Image:
    side = css * DPR
    x = (im.width - side) // 2
    y = (im.height - side) // 2
    return im.crop((x, y, x + side, y + side))


def circle_mask(size: int) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).ellipse((0, 0, size - 1, size - 1), fill=255)
    return mask


def compose_panel(theme: str) -> Image.Image:
    canvas = fit(load(f"panel-{theme}"))
    orb = crop_centered(load(f"orb-{theme}"), BALL_CSS).resize(
        (ORB_SLOT, ORB_SLOT), Image.Resampling.LANCZOS
    )
    x = PAD
    y = (STAGE[1] - ORB_SLOT) // 2
    canvas.paste(orb, (x, y), circle_mask(ORB_SLOT))
    return canvas


def fit(im: Image.Image) -> Image.Image:
    if im.size != STAGE:
        return im.resize(STAGE, Image.Resampling.LANCZOS)
    return im


rgb = [
    fit(load("working")),
    fit(load("wait")),
    compose_panel("prototype"),
    compose_panel("fluent"),
    compose_panel("glyph"),
    compose_panel("braun"),
    compose_panel("glass"),
]
frames = [
    im.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.FLOYDSTEINBERG)
    for im in rgb
]
frames[0].save(
    out,
    save_all=True,
    append_images=frames[1:],
    duration=[1200, 1400, 2000, 1600, 1600, 1600, 2000],
    loop=0,
    optimize=True,
    disposal=2,
)
PY

echo "$out ($(du -h "$out" | cut -f1))"

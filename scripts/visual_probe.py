#!/usr/bin/env python3
"""Verify actual X11 pixels: idle/working border and one-shot flash.

The ball is an external boundary, so this is a small manual-acceptance probe.
It starts the real ball, sends events through the real CLI, reads pixel (0,0)
with XGetImage, and expects:

* initial idle border
* working border after start
* one amber flash after stop, then idle border
"""
from __future__ import annotations

import ctypes
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "src"

from agent_activity_dock.presenter import (
    FLASH_BORDER_COLOR,
    IDLE_BORDER_COLOR,
    WORKING_BORDER_COLOR,
)
from agent_activity_dock.x11_presenter import (
    Display,
    Window,
    _parse_color,
    _x11,
)


class XImage(ctypes.Structure):
    pass


_x11.XGetImage.argtypes = [
    Display, Window, ctypes.c_int, ctypes.c_int,
    ctypes.c_uint, ctypes.c_uint, ctypes.c_ulong, ctypes.c_int,
]
_x11.XGetImage.restype = ctypes.c_void_p
_x11.XGetPixel.argtypes = [ctypes.POINTER(XImage), ctypes.c_int, ctypes.c_int]
_x11.XGetPixel.restype = ctypes.c_ulong
_x11.XDestroyImage.argtypes = [ctypes.c_void_p]
_x11.XDestroyImage.restype = ctypes.c_int

ALL_PLANES = ctypes.c_ulong(~0).value
ZPixmap = 2


def env():
    value = os.environ.copy()
    value["PYTHONPATH"] = str(SRC) + os.pathsep + value.get("PYTHONPATH", "")
    return value


def wait_ready(ready_file, proc, timeout=8.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if ready_file.exists():
            return
        if proc.poll() is not None:
            out, err = proc.communicate()
            raise RuntimeError(f"ball exited early: {out=} {err=}")
        time.sleep(0.03)
    raise RuntimeError("ball did not become ready")


def cli(socket_path, *args):
    proc = subprocess.run(
        [sys.executable, "-m", "agent_activity_dock.cli", "--socket",
         str(socket_path), "--json", *args],
        env=env(), text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        timeout=5,
    )
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr)
    return json.loads(proc.stdout)


def window_id(title):
    out = subprocess.run(
        ["/usr/bin/xwininfo", "-root", "-tree"],
        text=True, stdout=subprocess.PIPE, check=True, timeout=5,
    ).stdout
    for line in out.splitlines():
        if f'"{title}"' in line:
            return line.split()[0]
    raise RuntimeError(f"window {title!r} not found")


def expected_pixels(display):
    colormap = _x11.XDefaultColormap(display, _x11.XDefaultScreen(display))
    return {
        "idle": _parse_color(display, colormap, IDLE_BORDER_COLOR),
        "working": _parse_color(display, colormap, WORKING_BORDER_COLOR),
        "flash": _parse_color(display, colormap, FLASH_BORDER_COLOR),
    }


def read_pixel(display, win_id):
    image = _x11.XGetImage(
        display, int(win_id, 16), 0, 0, 1, 1, ALL_PLANES, ZPixmap
    )
    if not image:
        raise RuntimeError("XGetImage failed")
    try:
        return int(_x11.XGetPixel(ctypes.cast(image, ctypes.POINTER(XImage)), 0, 0))
    finally:
        _x11.XDestroyImage(image)


def main():
    if not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")):
        print("SKIP: no X display")
        return 0
    display = _x11.XOpenDisplay(None)
    if not display:
        print("SKIP: cannot open X display")
        return 0
    expected = expected_pixels(display)
    try:
        with tempfile.TemporaryDirectory(prefix="aadock-visual-") as tmp:
            tmp = pathlib.Path(tmp)
            socket_path = tmp / "dock.sock"
            ready_file = tmp / "ready"
            proc = subprocess.Popen(
                [sys.executable, "-m", "agent_activity_dock.ball",
                 "--socket", str(socket_path), "--ready-file", str(ready_file),
                 "--no-sound"],
                env=env(), stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                text=True,
            )
            wait_ready(ready_file, proc)
            try:
                win_id = window_id("Agent Activity Dock")
                checks = []
                initial = read_pixel(display, win_id)
                checks.append(("initial idle border", initial == expected["idle"], hex(initial)))

                cli(socket_path, "start", "visual-task", "--source", "visual-probe")
                time.sleep(0.15)
                working = read_pixel(display, win_id)
                checks.append(("working border after start", working == expected["working"], hex(working)))

                cli(socket_path, "stop", "visual-task")
                samples = []
                deadline = time.monotonic() + 0.7
                while time.monotonic() < deadline:
                    samples.append((time.monotonic(), read_pixel(display, win_id)))
                    time.sleep(0.02)
                saw_flash = any(pixel == expected["flash"] for _, pixel in samples)
                ended_idle = samples[-1][1] == expected["idle"] if samples else False
                checks.append(("one-shot amber flash observed", saw_flash, ""))
                checks.append(("flash reverts to idle border", ended_idle, hex(samples[-1][1]) if samples else "none"))
            finally:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill(); proc.wait(timeout=5)
                if proc.stdout is not None: proc.stdout.close()
                if proc.stderr is not None: proc.stderr.close()
    finally:
        _x11.XCloseDisplay(display)

    failed = [item for item in checks if not item[1]]
    for name, ok, detail in checks:
        print(f"{'PASS' if ok else 'FAIL'} {name}" + (f" ({detail})" if detail else ""))
    if failed:
        return 1
    print(f"PASS: {len(checks)} visual checks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

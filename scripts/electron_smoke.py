#!/usr/bin/env python3
"""Smoke-test the Electron product presenter end to end."""
from __future__ import annotations

import json
import os
import pathlib
import signal
import socket
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "src"


def env(socket_path):
    value = os.environ.copy()
    value["PYTHONPATH"] = str(SRC)
    value["AGENT_ACTIVITY_DOCK_SOCKET"] = str(socket_path)
    return value


def socket_live(path):
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.settimeout(0.2)
            sock.connect(str(path))
            return True
    except OSError:
        return False


def window_lines():
    out = subprocess.run(
        ["xwininfo", "-root", "-tree"], text=True,
        stdout=subprocess.PIPE, check=True, timeout=5,
    ).stdout
    return out.splitlines()


def find_window(title):
    for line in window_lines():
        if f'"{title}"' in line:
            return line
    return None


def window_id(title):
    line = find_window(title)
    if line is None:
        raise RuntimeError(f"window {title!r} not found")
    return line.split()[0]


def window_abs_pos(line):
    parts = line.strip().split()
    if len(parts) >= 6:
        geometry = parts[-1]
        import re
        m = re.search(r"\+(-?\d+)\+(-?\d+)$", geometry)
        if m:
            return int(m.group(1)), int(m.group(2))
    return 0, 0


def cli(socket_path, *args):
    return subprocess.run(
        [sys.executable, "-m", "agent_activity_dock.cli", "--socket",
         str(socket_path), "--json", *args],
        env=env(socket_path), text=True, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, timeout=5,
    )


def click(x, y):
    xtst = ctypes.CDLL(ctypes.util.find_library("Xtst"))
    from agent_activity_dock.x11_presenter import _x11, Display

    xtst.XTestFakeMotionEvent.argtypes = [Display, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    xtst.XTestFakeButtonEvent.argtypes = [Display, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    display = _x11.XOpenDisplay(None)
    screen = _x11.XDefaultScreen(display)
    xtst.XTestFakeMotionEvent(display, screen, x, y, 0)
    _x11.XFlush(display)
    time.sleep(0.15)
    xtst.XTestFakeButtonEvent(display, 1, 1, 0)
    xtst.XTestFakeButtonEvent(display, 1, 0, 0)
    _x11.XFlush(display)
    time.sleep(0.15)
    _x11.XCloseDisplay(display)


def main():
    if not os.environ.get("DISPLAY"):
        print("SKIP: no display")
        return 0
    with tempfile.TemporaryDirectory(prefix="aadock-electron-") as tmp:
        socket_path = pathlib.Path(tmp) / "dock.sock"
        proc = subprocess.Popen(
            [str(ROOT / "start-dock.sh")],
            env=env(socket_path),
            cwd=ROOT,
            start_new_session=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            deadline = time.monotonic() + 15
            ball_line = list_line = None
            while time.monotonic() < deadline:
                if socket_live(socket_path):
                    for line in window_lines():
                        if '"Agent Activity Dock"' not in line:
                            continue
                        if '44x44' in line:
                            ball_line = line
                        elif list_line is None:
                            list_line = line
                    if ball_line is not None:
                        break
                time.sleep(0.25)
            if ball_line is None:
                raise RuntimeError("Electron app did not become ready")

            ball_id = ball_line.split()[0]
            list_id = list_line.split()[0] if list_line else None
            start = cli(socket_path, "start", "electron-smoke", "--source", "electron")
            assert start.returncode == 0 and json.loads(start.stdout)["accepted"]
            stop = cli(socket_path, "stop", "electron-smoke")
            assert stop.returncode == 0
            status = json.loads(cli(socket_path, "status").stdout)["snapshot"]
            assert status["count_label"] == "0/1"
            assert status["pending_count"] == 1

            ack = json.loads(cli(socket_path, "status").stdout)["snapshot"]
            print("PASS: electron presenter daemon/window smoke")
            return 0
        finally:
            try:
                os.killpg(proc.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait(timeout=5)
            if proc.stdout is not None:
                proc.stdout.close()
            if proc.stderr is not None:
                proc.stderr.close()


if __name__ == "__main__":
    raise SystemExit(main())

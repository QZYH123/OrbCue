#!/usr/bin/env python3
"""End-to-end smoke test for the real X11 ball + daemon + CLI.

Starts the integrated ball process in a temporary HOME/socket, drives the full
ticket-07 demo (single/multi task, stop/waiting/error, duplicate stop, click
acknowledgement, reset), and verifies window size and map state with xwininfo.
"""
from __future__ import annotations

import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
XWININFO = "/usr/bin/xwininfo"


def env():
    value = os.environ.copy()
    value["PYTHONPATH"] = str(SRC) + os.pathsep + value.get("PYTHONPATH", "")
    value["AADOCK_PRESENTER"] = "x11"  # keep pixel/xwininfo probes on X11 fallback
    return value


def wait_ready(ready_file: pathlib.Path, proc: subprocess.Popen, timeout=8.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if ready_file.exists():
            return
        if proc.poll() is not None:
            out, err = proc.communicate()
            raise RuntimeError(f"ball exited early: {out=} {err=}")
        time.sleep(0.03)
    raise RuntimeError("ball did not become ready")


def cli(socket_path: pathlib.Path, *args):
    proc = subprocess.run(
        [sys.executable, "-m", "agent_activity_dock.cli", "--socket",
         str(socket_path), "--json", *args],
        env=env(),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=5,
    )
    if proc.returncode not in (0, 1):
        raise RuntimeError(f"cli failed {args}: {proc.stderr}")
    return json.loads(proc.stdout)


def window_lines():
    return subprocess.run(
        [XWININFO, "-root", "-tree"], text=True, stdout=subprocess.PIPE,
        check=True, timeout=5,
    ).stdout.splitlines()


def window_id(title: str):
    for line in window_lines():
        if f'"{title}"' in line:
            return line.split()[0]
    raise RuntimeError(f"window {title!r} not found")


def window_map_state(win_id: str):
    proc = subprocess.run(
        [XWININFO, "-id", win_id], text=True, stdout=subprocess.PIPE,
        check=True, timeout=5,
    )
    for line in proc.stdout.splitlines():
        if "Map State:" in line:
            return line.split(":", 1)[1].strip()
    raise RuntimeError("map state not found")


def window_geometry(win_id: str):
    proc = subprocess.run(
        [XWININFO, "-id", win_id], text=True, stdout=subprocess.PIPE,
        check=True, timeout=5,
    )
    width = height = None
    for line in proc.stdout.splitlines():
        if line.strip().startswith("Width:"):
            width = int(line.split(":", 1)[1].strip())
        elif line.strip().startswith("Height:"):
            height = int(line.split(":", 1)[1].strip())
    return width, height


def send_button(win_id: str):
    import ctypes

    from agent_activity_dock.x11_presenter import (
        ButtonPress,
        ButtonPressMask,
        XEvent,
        _x11,
    )

    display = _x11.XOpenDisplay(None)
    if not display:
        raise RuntimeError("cannot open X display")
    try:
        event = XEvent()
        event.xbutton.type = ButtonPress
        event.xbutton.window = int(win_id, 16)
        event.xbutton.root = _x11.XDefaultRootWindow(display)
        event.xbutton.subwindow = 0
        event.xbutton.time = 0
        event.xbutton.x = 4
        event.xbutton.y = 4
        event.xbutton.x_root = 0
        event.xbutton.y_root = 0
        event.xbutton.state = 0
        event.xbutton.button = 1
        event.xbutton.same_screen = 1
        _x11.XSendEvent(
            display, int(win_id, 16), 1, ButtonPressMask, ctypes.byref(event)
        )
        _x11.XFlush(display)
        _x11.XSync(display, 0)
    finally:
        _x11.XCloseDisplay(display)


def main():
    if not pathlib.Path(XWININFO).exists() or not os.environ.get("DISPLAY"):
        print("SKIP: xwininfo/X display not available")
        return 0
    with tempfile.TemporaryDirectory(prefix="aadock-ball-smoke-") as tmp:
        tmp = pathlib.Path(tmp)
        socket_path = tmp / "dock.sock"
        ready_file = tmp / "ready"
        proc = subprocess.Popen(
            [sys.executable, "-m", "agent_activity_dock.ball",
             "--socket", str(socket_path), "--ready-file", str(ready_file),
             "--no-sound"],
            env=env(),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        wait_ready(ready_file, proc)
        checks = []
        try:
            status = cli(socket_path, "status")
            checks.append(("initial 0/0", status["snapshot"]["count_label"] == "0/0"))

            for task in ("task-a", "task-b", "task-c"):
                response = cli(socket_path, "start", task, "--source", "smoke")
                checks.append((f"start {task} accepted", response["accepted"]))

            response = cli(socket_path, "stop", "task-b")
            checks.append(("stop task-b 2/3+pending", response["snapshot"]["count_label"] == "2/3" and response["snapshot"]["pending_count"] == 1))
            repeated = cli(socket_path, "stop", "task-b")
            checks.append(("duplicate stop no new attention", repeated["attention"] is None and repeated["snapshot"]["pending_count"] == 1))

            waiting = cli(socket_path, "waiting", "task-c")
            checks.append(("waiting task-c 1/3+pending2", waiting["snapshot"]["count_label"] == "1/3" and waiting["snapshot"]["pending_count"] == 2))
            error = cli(socket_path, "error", "task-a")
            checks.append(("error task-a 0/3+pending3", error["snapshot"]["count_label"] == "0/3" and error["snapshot"]["pending_count"] == 3))

            ball_id = window_id("Agent Activity Dock")
            list_id = window_id("Agent Activity Dock tasks")
            width, height = window_geometry(ball_id)
            checks.append(("ball fixed 44x44 after events", (width, height) == (44, 44)))
            checks.append(("list initially hidden", window_map_state(list_id) == "IsUnMapped"))

            send_button(ball_id)
            time.sleep(0.3)
            checks.append(("click opens task list", window_map_state(list_id) == "IsViewable"))
            after_view = cli(socket_path, "status")
            checks.append(("viewing ball clears pending", after_view["snapshot"]["pending_count"] == 0))

            send_button(list_id)
            time.sleep(0.3)
            checks.append(("click list closes it", window_map_state(list_id) == "IsUnMapped"))

            reset = cli(socket_path, "reset", "--all")
            checks.append(("reset --all -> 0/0", reset["snapshot"]["count_label"] == "0/0"))
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
            if proc.stdout is not None:
                proc.stdout.close()
            if proc.stderr is not None:
                proc.stderr.close()
        checks.append(("graceful shutdown removed socket", not socket_path.exists()))

    failed = [name for name, ok in checks if not ok]
    for name, ok in checks:
        print(f"{'PASS' if ok else 'FAIL'} {name}")
    if failed:
        print(f"FAILED: {len(failed)} checks")
        return 1
    print(f"PASS: {len(checks)} checks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

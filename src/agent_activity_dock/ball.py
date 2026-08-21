"""Integrated Dock ball: current-user IPC daemon + X11 presenter.

Start with ``python -m agent_activity_dock.ball`` once, then send events with
the normal ``dock`` CLI or an adapter.  The loop blocks on the X connection
and the Unix socket; there is no periodic poll or idle redraw timer.  A short
flash is implemented as one bounded select timeout and disappears after it.
"""
from __future__ import annotations

import argparse
import json
import os
import select
import signal
import sys
import time
from pathlib import Path

from .attention import AttentionDispatcher
from .core import Attention, DockCore, Snapshot
from .ipc import IpcServer, default_socket_path
from .presenter import BallView
from .sound import NullSoundSink, PaplaySoundSink
from .x11_presenter import X11BallPresenter

FLASH_SECONDS = 0.25


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="dock-ball")
    parser.add_argument("--socket", type=Path, default=None)
    parser.add_argument("--ready-file", type=Path, default=None)
    parser.add_argument(
        "--no-sound",
        action="store_true",
        help="Disable the one-shot local sound (visible ! still works).",
    )
    return parser


class BallApplication:
    def __init__(
        self,
        socket_path: Path,
        ready_file: Path | None = None,
        sound_enabled: bool = True,
    ) -> None:
        self.core = DockCore()
        self.server = IpcServer(self.core, socket_path)
        self.sound = PaplaySoundSink() if sound_enabled else NullSoundSink()
        self.presenter = X11BallPresenter(
            BallView.from_snapshot(self.core.snapshot),
            on_ball_click=self._on_ball_click,
        )
        self.dispatcher = AttentionDispatcher(self.presenter, self.sound)
        self.server.set_state_listener(self._on_state_change)
        self.ready_file = ready_file
        self._flash_until: float | None = None

    def bind(self) -> None:
        self.server.bind()

    def run(self) -> None:
        self.server.bind()
        if self.ready_file is not None:
            self.ready_file.write_text(
                json.dumps(
                    {
                        "pid": os.getpid(),
                        "socket": str(self.server.socket_path),
                    },
                    sort_keys=True,
                )
                + "\n"
            )
        print(
            f"agent-activity-dock ball ready on {self.server.socket_path}",
            flush=True,
        )
        while True:
            timeout = None
            now = time.monotonic()
            if self._flash_until is not None:
                timeout = max(0.0, self._flash_until - now)
            readable, _, _ = select.select(
                [self.server.fileno(), self.presenter.fileno()],
                [],
                [],
                timeout,
            )
            if self.server.fileno() in readable:
                self.server.accept_and_handle_pending()
            if self.presenter.fileno() in readable:
                self.presenter.process_pending_events()
            if self._flash_until is not None and time.monotonic() >= self._flash_until:
                self._flash_until = None
                self.presenter.clear_flash()

    def _on_state_change(
        self, snapshot: Snapshot, attention: Attention | None
    ) -> None:
        self.dispatcher.on_state_change(snapshot, attention)
        if attention is not None:
            self._flash_until = time.monotonic() + FLASH_SECONDS

    def _on_ball_click(self) -> None:
        # Opening the task list counts as viewing the aggregate ball: clear
        # every pending mark through the same public core API.
        self.core.acknowledge_all()
        self.presenter.update(BallView.from_snapshot(self.core.snapshot))

    def close(self) -> None:
        self.server.close()
        try:
            self.presenter.close()
        finally:
            self.sound.close()


def _main_x11(argv: list[str] | None = None) -> int:
    args = _build_parser().parse_args(argv)
    socket_path = args.socket or default_socket_path()

    def stop_cleanly(signum, frame):  # noqa: ARG001
        raise SystemExit(0)

    previous_term = signal.signal(signal.SIGTERM, stop_cleanly)
    previous_int = signal.signal(signal.SIGINT, stop_cleanly)
    app: BallApplication | None = None
    try:
        app = BallApplication(
            socket_path,
            ready_file=args.ready_file,
            sound_enabled=not args.no_sound,
        )
        app.run()
    except KeyboardInterrupt:
        pass
    except Exception as exc:
        print(f"dock ball failed: {exc}", file=sys.stderr)
        return 1
    finally:
        if app is not None:
            app.close()
        signal.signal(signal.SIGTERM, previous_term)
        signal.signal(signal.SIGINT, previous_int)
    return 0


def main(argv: list[str] | None = None) -> int:
    return _main_x11(argv)


if __name__ == "__main__":
    raise SystemExit(main())

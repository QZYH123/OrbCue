"""Run the Agent Activity Dock daemon.

Example:
    python -m agent_activity_dock.daemon
"""
from __future__ import annotations

import argparse
import json
import signal
import sys
from pathlib import Path

from .core import DockCore
from .ipc import IpcServer, default_socket_path


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="agent-activity-dock-daemon")
    parser.add_argument(
        "--socket",
        type=Path,
        default=None,
        help="Unix domain socket path (default: current user runtime dir).",
    )
    parser.add_argument(
        "--ready-file",
        type=Path,
        default=None,
        help="Write a JSON ready marker to this path after bind().",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _build_parser().parse_args(argv)
    socket_path = args.socket or default_socket_path()
    core = DockCore()
    server = IpcServer(core, socket_path)

    def stop_cleanly(signum, frame):  # noqa: ARG001
        raise SystemExit(0)

    previous_term = signal.signal(signal.SIGTERM, stop_cleanly)
    previous_int = signal.signal(signal.SIGINT, stop_cleanly)
    try:
        server.bind()
        if args.ready_file is not None:
            args.ready_file.write_text(
                json.dumps(
                    {
                        "pid": __import__("os").getpid(),
                        "socket": str(server.socket_path),
                    },
                    sort_keys=True,
                )
                + "\n"
            )
        print(
            f"agent-activity-dock daemon ready on {server.socket_path}",
            flush=True,
        )
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    except Exception as exc:
        print(f"daemon failed: {exc}", file=sys.stderr)
        return 1
    finally:
        server.close()
        signal.signal(signal.SIGTERM, previous_term)
        signal.signal(signal.SIGINT, previous_int)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

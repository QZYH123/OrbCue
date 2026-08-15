"""Thin CLI emitter for Agents, skills, adapters, and human smoke tests.

Examples:
    dock start task-1 --source codex
    dock stop task-1
    dock reset task-1
    dock reset --all
    dock status
"""
from __future__ import annotations

import argparse
import json
import os
import socket
import sys
import uuid

from .connections import ConnectionManager, PREFERRED_METHOD
from .ipc import default_socket_path
from .native_hooks import CLAUDE_HOOK_ACTION

_ACTIONS_WITH_TASK = ("start", "stop", "waiting", "error")


def _add_socket_options(parser: argparse.ArgumentParser, suppress: bool) -> None:
    default = argparse.SUPPRESS if suppress else None
    parser.add_argument(
        "--socket",
        default=default,
        help="Dock Unix socket path (or use AGENT_ACTIVITY_DOCK_SOCKET).",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=argparse.SUPPRESS if suppress else 5.0,
        help="Seconds to wait for the daemon response.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        default=argparse.SUPPRESS if suppress else False,
        help="Print the daemon JSON response instead of a human summary.",
    )


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="dock")
    _add_socket_options(parser, suppress=False)
    sub = parser.add_subparsers(dest="command", required=True)

    for action in _ACTIONS_WITH_TASK:
        cmd = sub.add_parser(action, help=f"Send a {action} event.")
        _add_socket_options(cmd, suppress=True)
        cmd.add_argument("task_id", help="Stable task/session identifier.")
        cmd.add_argument("--source", default="cli")
        cmd.add_argument("--event-id", default=None)
        cmd.add_argument("--terminal", default=None)
        cmd.add_argument("--occurred-at", default=None)

    reset = sub.add_parser("reset", help="Reset one task, or all with --all.")
    _add_socket_options(reset, suppress=True)
    reset.add_argument("task_id", nargs="?", help="Task id to reset.")
    reset.add_argument("--all", action="store_true", dest="reset_all")
    reset.add_argument("--source", default="user")
    reset.add_argument("--event-id", default=None)
    reset.add_argument("--occurred-at", default=None)

    status = sub.add_parser("status", help="Print the aggregate snapshot.")
    _add_socket_options(status, suppress=True)

    setup = sub.add_parser(
        "setup", help="Discover installed Agents and connect them once."
    )
    setup.add_argument(
        "--yes",
        action="store_true",
        help="Connect every discovered Agent without prompting.",
    )
    setup.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the exact user-level changes without writing anything.",
    )

    connections = sub.add_parser(
        "connections", help="List current zero-install Agent connections."
    )
    connections.add_argument("--json", action="store_true")

    connect = sub.add_parser(
        "connect", help="Connect one already-installed Agent by name."
    )
    connect.add_argument("name")
    connect.add_argument("--original", default=None)
    connect.add_argument(
        "--method",
        choices=("native_hook", "wrapper"),
        default=None,
        help="Connection method (default: per-Agent preference).",
    )
    connect.add_argument("--yes", action="store_true")
    connect.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the connection change without writing anything.",
    )

    disconnect = sub.add_parser(
        "disconnect", help="Revoke one zero-install Agent connection."
    )
    disconnect.add_argument("name")

    hook = sub.add_parser(
        "hook",
        help="Translate one native hook JSON payload from stdin to Dock.",
    )
    _add_socket_options(hook, suppress=True)
    hook.add_argument("--source", default="claude")
    hook.add_argument("--event", default=None, help="Override hook_event_name.")
    hook.add_argument("--session-id", default=None, help="Override session_id.")
    return parser


def _resolve_socket(args: argparse.Namespace) -> str:
    value = getattr(args, "socket", None)
    if value:
        return str(value)
    env_value = os.environ.get("AGENT_ACTIVITY_DOCK_SOCKET")
    if env_value:
        return env_value
    return str(default_socket_path())


def _event_payload(args: argparse.Namespace) -> dict:
    if args.command == "reset":
        if args.reset_all:
            task_id = "*"
        elif args.task_id:
            task_id = args.task_id
        else:
            raise ValueError("reset requires TASK_ID or --all")
    else:
        task_id = args.task_id

    return {
        "task_id": task_id,
        "source": args.source,
        "event_id": args.event_id or uuid.uuid4().hex,
        "action": args.command,
        **(
            {"occurred_at": args.occurred_at}
            if getattr(args, "occurred_at", None) is not None
            else {}
        ),
        **(
            {"terminal": args.terminal}
            if getattr(args, "terminal", None) is not None
            else {}
        ),
    }


def _send(socket_path: str, payload: dict, timeout: float = 5.0) -> dict:
    request = json.dumps(payload, sort_keys=True).encode("utf-8") + b"\n"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.settimeout(timeout)
        sock.connect(socket_path)
        sock.sendall(request)
        sock.shutdown(socket.SHUT_WR)
        file = sock.makefile("rb")
        with file:
            response_bytes = file.readline(64 * 1024)
    if not response_bytes:
        raise RuntimeError("daemon closed the connection without a response")
    return json.loads(response_bytes.decode("utf-8"))


def _human_summary(response: dict) -> str:
    snapshot = response.get("snapshot", {})
    lines = [
        f"{snapshot.get('count_label', '?')} {snapshot.get('border_state', '?')} "
        f"pending={snapshot.get('pending_count', '?')}"
    ]
    for task in snapshot.get("tasks", []):
        mark = "!" if task.get("needs_attention") else " "
        state = "working" if task.get("working") else task.get("last_action")
        lines.append(
            f"  [{mark}] {task.get('task_id')} ({task.get('source')}): {state}"
        )
    return "\n".join(lines)


def _run_connection_command(args: argparse.Namespace) -> int:
    manager = ConnectionManager()
    if args.command == "setup":
        if getattr(args, "dry_run", False):
            plans = manager.plan_setup()
            if not plans:
                print("no supported Agents found on PATH")
            for plan in plans:
                print(f"{plan.name}: {plan.status} via {plan.method} — {plan.describe()}")
            return 0
        plans = manager.plan_setup()
        if not plans:
            print("no supported Agents found on PATH")
            return 0
        yes = getattr(args, "yes", False)
        selected = list(plans)
        if not yes:
            selected = []
            for plan in plans:
                if plan.status == "keep":
                    print(f"{plan.name}: already connected ({plan.method}); skipping")
                    continue
                answer = input(
                    f"Connect {plan.name} via {plan.method}? [y/N] "
                ).strip().lower()
                if answer in ("y", "yes"):
                    selected.append(plan)
                else:
                    print(f"{plan.name}: skipped")
            if not selected:
                print("no Agents selected; nothing changed")
                return 0
        failed = 0
        for plan in selected:
            if plan.status == "keep":
                print(f"{plan.name}: already connected ({plan.method})")
                continue
            try:
                manager.connect(plan.name, plan.original, method=plan.method)
            except Exception as exc:
                print(f"{plan.name}: failed ({exc})")
                failed += 1
            else:
                print(f"{plan.name}: connected ({plan.method})")
        return 1 if failed else 0
    if args.command == "connections":
        records = manager.agents
        if getattr(args, "json", False):
            print(json.dumps({name: record.to_dict() for name, record in records.items()}, indent=2, sort_keys=True))
        elif not records:
            print("no Agents connected yet; run: dock setup")
        else:
            for record in records.values():
                print(
                    f"{record.name}: {record.method} "
                    f"capabilities={','.join(record.capabilities)} "
                    f"original={record.original}"
                )
                if record.limitation:
                    print(f"  limitation: {record.limitation}")
        return 0
    if args.command == "connect":
        original = args.original
        if original is None:
            discovered = {item.name: item.path for item in manager.discover_agents()}
            original = discovered.get(args.name)
            if original is None:
                print(f"error: {args.name} not found on PATH", file=sys.stderr)
                return 1
        method = args.method or PREFERRED_METHOD.get(args.name, "wrapper")
        if getattr(args, "dry_run", False):
            if method == "wrapper":
                target = manager.bin_dir / args.name
                action = (
                    f"create {target} and prepend {manager.bin_dir} to PATH "
                    "in user shell rc"
                )
            else:
                hook = manager.hook_dir / f"{args.name}-hook.py"
                action = f"add revocable hook entries and create {hook}"
            print(f"{args.name}: would connect via {method} — {action}")
            print(f"  original executable: {original}")
            return 0
        record = manager.connect(args.name, original, method=method)
        print(f"{record.name}: connected via {record.method}")
        if record.limitation:
            print(f"  limitation: {record.limitation}")
        print("start a new shell so the user-level wrapper is active, if one was installed")
        return 0
    if args.command == "disconnect":
        if manager.disconnect(args.name):
            print(f"{args.name}: disconnected")
        else:
            print(f"{args.name}: was not connected")
        return 0
    raise AssertionError(f"unhandled connection command {args.command}")


def _run_hook_command(args: argparse.Namespace, socket_path: str) -> int:
    try:
        payload_in = json.load(sys.stdin)
    except (json.JSONDecodeError, UnicodeDecodeError):
        print("error: hook stdin is not valid JSON", file=sys.stderr)
        return 2
    if not isinstance(payload_in, dict):
        payload_in = {}
    event_name = args.event or payload_in.get("hook_event_name") or payload_in.get("hook_event")
    action = CLAUDE_HOOK_ACTION.get(event_name)
    if action is None:
        if getattr(args, "json", False):
            print(json.dumps({"accepted": False, "rejection_reason": "unmapped_hook_event"}))
        return 0
    task_id = args.session_id or payload_in.get("session_id") or f"hook-{uuid.uuid4().hex}"
    payload = {
        "task_id": str(task_id),
        "source": args.source,
        "event_id": uuid.uuid4().hex,
        "action": action,
    }
    try:
        response = _send(socket_path, payload, getattr(args, "timeout", 5.0))
    except (OSError, RuntimeError, json.JSONDecodeError) as exc:
        print(f"error: cannot reach Dock daemon on {socket_path}: {exc}", file=sys.stderr)
        return 2
    if getattr(args, "json", False):
        print(json.dumps(response, sort_keys=True))
    else:
        print("accepted" if response.get("accepted") else f"rejected: {response.get('rejection_reason')}")
    return 0 if response.get("ok") and response.get("accepted") else 1


def main(argv: list[str] | None = None) -> int:
    args = _build_parser().parse_args(argv)
    if args.command in ("setup", "connections", "connect", "disconnect"):
        return _run_connection_command(args)
    socket_path = _resolve_socket(args)
    if args.command == "status":
        payload = {"query": "snapshot"}
    elif args.command == "hook":
        return _run_hook_command(args, socket_path)
    else:
        try:
            payload = _event_payload(args)
        except ValueError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 2

    try:
        response = _send(socket_path, payload, getattr(args, "timeout", 5.0))
    except (OSError, RuntimeError, json.JSONDecodeError) as exc:
        print(f"error: cannot reach Dock daemon on {socket_path}: {exc}", file=sys.stderr)
        return 2

    if getattr(args, "json", False):
        print(json.dumps(response, sort_keys=True))
    else:
        if response.get("accepted"):
            print("accepted")
        else:
            print(f"rejected: {response.get('rejection_reason')}")
        print(_human_summary(response))

    return 0 if response.get("ok") and response.get("accepted") else 1


if __name__ == "__main__":
    raise SystemExit(main())

"""Current-user-only local IPC for the Dock daemon.

The transport is a newline-delimited JSON stream over a Unix domain socket.
There is no network listener and no periodic poll: a blocking accept loop
wakes only when a local client connects.
"""
from __future__ import annotations

import json
import os
import select
import socket
from pathlib import Path
from typing import Callable, Optional

from .core import Attention, DockCore, Snapshot
from .events import dock_event_from_dict

DEFAULT_SOCKET_NAME = "agent-activity-dock.sock"
MAX_REQUEST_BYTES = 16 * 1024


def default_socket_path() -> Path:
    """Return a current-user-only socket location.

    ``XDG_RUNTIME_DIR`` is preferred because it is private to the user and
    disappears when the session ends.  The fallback directory is created with
    mode 0700 by :meth:`IpcServer.bind`.
    """
    runtime_dir = os.environ.get("XDG_RUNTIME_DIR")
    if runtime_dir and Path(runtime_dir).is_dir():
        base = Path(runtime_dir)
    else:
        base = Path.home() / ".local" / "state" / "agent-activity-dock"
    return base / "agent-activity-dock" / DEFAULT_SOCKET_NAME


def snapshot_to_dict(snapshot: Snapshot) -> dict:
    """JSON-safe public snapshot for CLI responses and contract tests."""
    return {
        "working_count": snapshot.working_count,
        "tracked_count": snapshot.tracked_count,
        "pending_count": snapshot.pending_count,
        "count_label": snapshot.count_label,
        "border_state": snapshot.border_state,
        "tasks": [
            {
                "task_id": task.task_id,
                "source": task.source,
                "working": task.working,
                "needs_attention": task.needs_attention,
                "last_action": task.last_action,
                "terminal": task.terminal,
            }
            for task in snapshot.tasks
        ],
    }


class IpcServer:
    """Owns the Unix socket and translates requests to DockCore.apply."""

    def __init__(
        self,
        core: DockCore,
        socket_path: str | Path,
        max_request_bytes: int = MAX_REQUEST_BYTES,
    ) -> None:
        self.core = core
        self.socket_path = Path(socket_path)
        self.max_request_bytes = max_request_bytes
        self._socket: Optional[socket.socket] = None
        self._running = False
        self._owns_socket = False
        self._state_listener: Optional[
            Callable[[Snapshot, Optional[Attention]], None]
        ] = None

    def set_state_listener(
        self,
        listener: Callable[[Snapshot, Optional[Attention]], None],
    ) -> None:
        """Subscribe to accepted state changes; errors are isolated."""
        self._state_listener = listener

    def bind(self) -> None:
        path = self.socket_path
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        try:
            path.parent.chmod(0o700)
        except OSError:
            pass

        if path.exists():
            if self._is_live_socket(path):
                raise RuntimeError(
                    f"another Dock daemon appears to be listening on {path}"
                )
            path.unlink()

        old_umask = os.umask(0o077)
        try:
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            sock.bind(str(path))
        finally:
            os.umask(old_umask)
        try:
            os.chmod(path, 0o600)
        except OSError:
            pass
        sock.listen(16)
        sock.setblocking(False)
        self._socket = sock
        self._owns_socket = True
        self._running = True

    def fileno(self) -> int:
        if self._socket is None:
            raise RuntimeError("bind() must be called before fileno()")
        return self._socket.fileno()

    def accept_and_handle_pending(self) -> None:
        """Accept every currently pending local client connection."""
        if self._socket is None:
            raise RuntimeError("bind() must be called first")
        sock = self._socket
        while self._running:
            try:
                conn, _ = sock.accept()
            except BlockingIOError:
                return
            except InterruptedError:
                continue
            except OSError:
                if self._running:
                    raise
                return
            with conn:
                self._handle_connection(conn)

    def serve_forever(self) -> None:
        if self._socket is None:
            raise RuntimeError("bind() must be called before serve_forever()")
        while self._running:
            self.accept_and_handle_pending()
            if self._running:
                select.select([self.fileno()], [], [])

    def close(self) -> None:
        self._running = False
        if self._socket is not None:
            try:
                self._socket.close()
            finally:
                self._socket = None
        if self._owns_socket:
            try:
                self.socket_path.unlink()
            except FileNotFoundError:
                pass
            self._owns_socket = False

    def _handle_connection(self, conn: socket.socket) -> None:
        try:
            request = self._read_request(conn)
        except RequestTooLarge:
            response = self._rejection("message_too_large")
        except Exception:
            # A broken local client must never take down the daemon.
            response = self._rejection("connection_error")
        else:
            response = self.dispatch(request)
        try:
            conn.sendall(
                json.dumps(response, sort_keys=True).encode("utf-8") + b"\n"
            )
        except OSError:
            pass

    def _read_request(self, conn: socket.socket) -> bytes:
        # Read at most max_request_bytes plus the trailing newline.  When a
        # client sends more, drain a bounded tail before responding so the
        # normal close does not race the client's read of the rejection.
        chunks: list[bytes] = []
        total = 0
        while True:
            chunk = conn.recv(4096)
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
            if b"\n" in chunk:
                break
            if total > self.max_request_bytes + 1:
                self._drain_request_tail(conn, total)
                raise RequestTooLarge
        request = b"".join(chunks)
        if request.endswith(b"\n"):
            request = request[:-1]
        if len(request) > self.max_request_bytes:
            self._drain_request_tail(conn, total)
            raise RequestTooLarge
        return request

    def _drain_request_tail(self, conn: socket.socket, total: int) -> None:
        # Keep this bound small: it exists only to let a rejected local
        # client read the complete JSON error response before close.
        drain_limit = max(64 * 1024, self.max_request_bytes * 4)
        while total < drain_limit:
            chunk = conn.recv(4096)
            if not chunk:
                break
            total += len(chunk)
            if b"\n" in chunk:
                break

    def dispatch(self, request: bytes) -> dict:
        if len(request) > self.max_request_bytes:
            return self._rejection("message_too_large")

        try:
            payload = json.loads(request.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            return self._rejection("invalid_json")

        if isinstance(payload, dict) and payload.get("query") == "snapshot":
            return {
                "ok": True,
                "accepted": True,
                "rejection_reason": None,
                "attention": None,
                "snapshot": snapshot_to_dict(self.core.snapshot),
            }

        try:
            event = dock_event_from_dict(payload)
        except ValueError:
            return self._rejection("invalid_event")

        result = self.core.apply(event)
        self._notify_state_change(result.snapshot, result.attention)
        return {
            "ok": result.accepted,
            "accepted": result.accepted,
            "rejection_reason": result.rejection_reason,
            "attention": (
                {
                    "task_id": result.attention.task_id,
                    "reason": result.attention.reason,
                }
                if result.attention is not None
                else None
            ),
            "snapshot": snapshot_to_dict(result.snapshot),
        }

    def _notify_state_change(
        self, snapshot: Snapshot, attention: Optional[Attention]
    ) -> None:
        if self._state_listener is None:
            return
        try:
            self._state_listener(snapshot, attention)
        except Exception:
            # A broken presenter or sound sink must never change the state
            # commit or the response returned to the Agent.
            pass

    def _rejection(self, reason: str) -> dict:
        return {
            "ok": False,
            "accepted": False,
            "rejection_reason": reason,
            "attention": None,
            "snapshot": snapshot_to_dict(self.core.snapshot),
        }

    @staticmethod
    def _is_live_socket(path: Path) -> bool:
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as probe:
                probe.settimeout(0.2)
                probe.connect(str(path))
        except OSError:
            return False
        return True


class RequestTooLarge(ValueError):
    """Raised internally when a client line exceeds the byte limit."""

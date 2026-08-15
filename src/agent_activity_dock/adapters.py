"""Thin public-payload adapters for the first batch of Agents.

An adapter only translates a source's existing public hook, notification, or
explicit projection payload into :class:`DockEvent`.  It never imports source
internals, scans processes, or parses transcripts/terminal output.
"""
from __future__ import annotations

import uuid
from typing import Optional

from .events import DockEvent
from .native_hooks import CLAUDE_HOOK_ACTION

DSH_PROJECTION_ACTION = {
    "session.started": "start",
    "session.waiting_input": "waiting",
    "session.completed": "stop",
    "session.failed": "error",
}


def _new_event_id(prefix: str) -> str:
    return f"{prefix}-{uuid.uuid4().hex}"


class ClaudeHookAdapter:
    """Translate Claude Code's settings.json hook JSON into Dock events."""

    source = "claude"

    @classmethod
    def to_dock_event(
        cls, payload: dict, event_id: Optional[str] = None
    ) -> Optional[DockEvent]:
        if not isinstance(payload, dict):
            return None
        event_name = payload.get("hook_event_name") or payload.get("hook_event")
        action = CLAUDE_HOOK_ACTION.get(event_name)
        task_id = payload.get("session_id")
        if action is None or not isinstance(task_id, str) or not task_id:
            return None
        return DockEvent(
            task_id=task_id,
            source=cls.source,
            event_id=event_id or _new_event_id("claude"),
            action=action,
        )


class CodexWrapperAdapter:
    """Codex has no stable lifecycle Hook in the current public CLI.

    The zero-install user wrapper reports explicit start/stop/error around the
    real ``codex`` executable.  These helpers express that translation so it
    can be tested and replaced later when Codex publishes a Hook payload.
    """

    source = "codex"

    @classmethod
    def start_event(
        cls,
        task_id: str,
        event_id: Optional[str] = None,
        terminal: Optional[str] = None,
    ) -> DockEvent:
        return DockEvent(
            task_id=task_id,
            source=cls.source,
            event_id=event_id or _new_event_id("codex"),
            action="start",
            terminal=terminal,
        )

    @classmethod
    def exit_event(
        cls,
        task_id: str,
        exit_code: int,
        event_id: Optional[str] = None,
        terminal: Optional[str] = None,
    ) -> DockEvent:
        return DockEvent(
            task_id=task_id,
            source=cls.source,
            event_id=event_id or _new_event_id("codex"),
            action="error" if exit_code != 0 else "stop",
            terminal=terminal,
        )


class DshProjectionAdapter:
    """Translate an explicit DSH session projection payload.

    DSH does not publish a lifecycle Hook in its current CLI, so this adapter
    defines the explicit projection contract an integration callback must
    emit.  It does not parse DSH terminal output.
    """

    source = "dsh"

    @classmethod
    def to_dock_event(
        cls, payload: dict, event_id: Optional[str] = None
    ) -> Optional[DockEvent]:
        if not isinstance(payload, dict):
            return None
        event_name = payload.get("event")
        action = DSH_PROJECTION_ACTION.get(event_name)
        task_id = payload.get("session_id")
        if action is None or not isinstance(task_id, str) or not task_id:
            return None
        return DockEvent(
            task_id=task_id,
            source=cls.source,
            event_id=event_id or _new_event_id("dsh"),
            action=action,
        )

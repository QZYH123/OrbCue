"""Public event contract for the Dock MVP.

The contract is deliberately small: every event carries a stable task id,
source, event id, and action.  ``occurred_at`` and ``terminal`` are optional
and are only used for display or future source-terminal focusing.
"""
from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Optional


class Action(StrEnum):
    """Minimal lifecycle actions accepted by the Dock core."""

    START = "start"
    STOP = "stop"
    WAITING = "waiting"
    ERROR = "error"
    RESET = "reset"


VALID_ACTIONS = frozenset(action.value for action in Action)

# ``reset`` with this task id clears every currently tracked task.
RESET_ALL_TASK_ID = "*"


@dataclass(frozen=True, slots=True)
class DockEvent:
    """One explicit state event from an Agent or adapter."""

    task_id: str
    source: str
    event_id: str
    action: str
    occurred_at: Optional[str] = None
    terminal: Optional[str] = None


def dock_event_from_dict(data: object) -> DockEvent:
    """Parse a bounded, already-decoded JSON object into a DockEvent."""
    if not isinstance(data, dict):
        raise ValueError("invalid_event")
    required = ("task_id", "source", "event_id", "action")
    for key in required:
        value = data.get(key)
        if not isinstance(value, str) or not value:
            raise ValueError("invalid_event")
    for key in ("occurred_at", "terminal"):
        value = data.get(key)
        if value is not None and not isinstance(value, str):
            raise ValueError("invalid_event")
    return DockEvent(
        task_id=data["task_id"],
        source=data["source"],
        event_id=data["event_id"],
        action=data["action"],
        occurred_at=data.get("occurred_at"),
        terminal=data.get("terminal"),
    )


def dock_event_to_dict(event: DockEvent) -> dict:
    """Serialize one DockEvent for the newline-delimited JSON IPC."""
    payload = {
        "task_id": event.task_id,
        "source": event.source,
        "event_id": event.event_id,
        "action": event.action,
    }
    if event.occurred_at is not None:
        payload["occurred_at"] = event.occurred_at
    if event.terminal is not None:
        payload["terminal"] = event.terminal
    return payload

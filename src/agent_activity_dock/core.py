"""Ticket 01: task state loop, aggregate snapshot, and attention policy.

The core consumes explicit :class:`DockEvent` values and exposes only
observable results: a read-only aggregate snapshot plus, for the event that
changed a task from working to not-working, a single attention effect.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from .events import VALID_ACTIONS, RESET_ALL_TASK_ID, DockEvent

_SEEN_EVENT_LIMIT = 8192


@dataclass(frozen=True, slots=True)
class TaskInfo:
    """Public, read-only view of one tracked task."""

    task_id: str
    source: str
    working: bool
    needs_attention: bool
    last_action: str
    terminal: Optional[str] = None


@dataclass(frozen=True, slots=True)
class Snapshot:
    """Aggregate view consumed by presenters and tests."""

    working_count: int
    tracked_count: int
    pending_count: int
    tasks: tuple[TaskInfo, ...]

    @property
    def is_working(self) -> bool:
        return self.working_count > 0

    @property
    def border_state(self) -> str:
        return "working" if self.is_working else "idle"

    @property
    def count_label(self) -> str:
        return f"{self.working_count}/{self.tracked_count}"

    def task_by_id(self, task_id: str) -> Optional[TaskInfo]:
        for task in self.tasks:
            if task.task_id == task_id:
                return task
        return None


@dataclass(frozen=True, slots=True)
class Attention:
    """One-shot attention request emitted only on a working->stopped change."""

    task_id: str
    reason: str


@dataclass(frozen=True, slots=True)
class EventResult:
    """Outcome of one public event entry."""

    accepted: bool
    snapshot: Snapshot
    attention: Optional[Attention] = None
    rejection_reason: Optional[str] = None


@dataclass(slots=True)
class _TaskRecord:
    source: str
    working: bool
    needs_attention: bool
    last_action: str
    terminal: Optional[str]


class DockCore:
    """In-memory task registry and deterministic state aggregator."""

    def __init__(self) -> None:
        self._tasks: dict[str, _TaskRecord] = {}
        self._seen_event_ids: set[str] = set()

    @property
    def snapshot(self) -> Snapshot:
        return self._make_snapshot()

    def apply(self, event: DockEvent) -> EventResult:
        """Apply one event and return the new snapshot plus attention effect."""
        validation_error = self._validate(event)
        if validation_error is not None:
            return EventResult(False, self.snapshot, None, validation_error)

        action = str(event.action)
        if action not in VALID_ACTIONS:
            return EventResult(False, self.snapshot, None, "unknown_action")

        if event.event_id in self._seen_event_ids:
            # An already-processed event is idempotent: accept it and do
            # nothing, especially no second sound or flash.
            return EventResult(True, self.snapshot, None, None)

        attention = None
        if action == "reset":
            self._apply_reset(event)
        elif action == "start":
            self._apply_start(event)
        else:
            task = self._tasks.get(event.task_id)
            if task is None:
                return EventResult(False, self.snapshot, None, "unknown_task")
            attention = self._apply_terminal(event, task)

        self._remember_event(event.event_id)
        return EventResult(True, self.snapshot, attention, None)

    def acknowledge(self, task_id: str) -> Snapshot:
        """Clear the pending mark after the user has viewed one task."""
        if task_id == RESET_ALL_TASK_ID:
            return self.acknowledge_all()
        task = self._tasks.get(task_id)
        if task is not None:
            task.needs_attention = False
        return self.snapshot

    def acknowledge_all(self) -> Snapshot:
        """Clear every pending mark after the user has viewed the list."""
        for task in self._tasks.values():
            task.needs_attention = False
        return self.snapshot

    @staticmethod
    def _validate(event: DockEvent) -> Optional[str]:
        if not isinstance(event, DockEvent):
            return "invalid_event"
        if not event.task_id or not event.source or not event.event_id:
            return "invalid_event"
        return None

    def _apply_start(self, event: DockEvent) -> None:
        task = self._tasks.get(event.task_id)
        if task is None:
            self._tasks[event.task_id] = _TaskRecord(
                source=event.source,
                working=True,
                needs_attention=False,
                last_action="start",
                terminal=event.terminal,
            )
            return
        # A task id is a single independently-trackable run.  Replayed or
        # duplicate starts do not create another task.  The one exception is
        # a task that was ``waiting``: an Agent resumes that same task with a
        # normal start event after the user provides input or approval.
        if task.working:
            if event.terminal is not None and task.terminal is None:
                task.terminal = event.terminal
        elif task.last_action == "waiting":
            task.working = True
            task.last_action = "start"
            if event.terminal is not None and task.terminal is None:
                task.terminal = event.terminal

    def _apply_terminal(
        self, event: DockEvent, task: _TaskRecord
    ) -> Optional[Attention]:
        if not task.working:
            # Duplicate stop/waiting/error for a stopped task: no second
            # sound, no counter change, and the existing mark stays intact.
            return None
        task.working = False
        task.needs_attention = True
        task.last_action = str(event.action)
        if event.terminal is not None and task.terminal is None:
            task.terminal = event.terminal
        return Attention(task_id=event.task_id, reason=str(event.action))

    def _apply_reset(self, event: DockEvent) -> None:
        if event.task_id == RESET_ALL_TASK_ID:
            self._tasks.clear()
        else:
            self._tasks.pop(event.task_id, None)

    def _remember_event(self, event_id: str) -> None:
        if len(self._seen_event_ids) >= _SEEN_EVENT_LIMIT:
            # MVP state is intentionally ephemeral.  Bounding the duplicate
            # guard is more important than remembering every historical id.
            self._seen_event_ids.clear()
        self._seen_event_ids.add(event_id)

    def _make_snapshot(self) -> Snapshot:
        tasks = tuple(
            TaskInfo(
                task_id=task_id,
                source=record.source,
                working=record.working,
                needs_attention=record.needs_attention,
                last_action=record.last_action,
                terminal=record.terminal,
            )
            for task_id, record in sorted(self._tasks.items())
        )
        working_count = sum(1 for task in tasks if task.working)
        pending_count = sum(1 for task in tasks if task.needs_attention)
        return Snapshot(
            working_count=working_count,
            tracked_count=len(tasks),
            pending_count=pending_count,
            tasks=tasks,
        )

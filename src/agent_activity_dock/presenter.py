"""Presenter contract and pure view projection.

GUI toolkits are an external boundary.  Everything here consumes the public
:class:`~agent_activity_dock.core.Snapshot` and returns plain data; the X11
presenter renders :class:`BallView` without importing core internals.
"""
from __future__ import annotations

from dataclasses import dataclass

from .core import Snapshot

# Fixed collapsed window size.  The window is deliberately not resized when
# the count label changes, so layout never jumps.
BALL_SIZE = 44

WORKING_BORDER_COLOR = "#22c55e"
IDLE_BORDER_COLOR = "#64748b"
BALL_BACKGROUND_COLOR = "#0f172a"
TEXT_COLOR = "#f8fafc"
FLASH_BORDER_COLOR = "#fbbf24"

FRAME_WIDTH = 4


@dataclass(frozen=True, slots=True)
class BallView:
    """Read-only data needed to draw the aggregate ball and task list."""

    count_label: str
    border_color: str
    working: bool
    show_bang: bool
    tasks: tuple
    width: int = BALL_SIZE
    height: int = BALL_SIZE

    @classmethod
    def from_snapshot(cls, snapshot: Snapshot) -> "BallView":
        return cls(
            count_label=snapshot.count_label,
            border_color=(
                WORKING_BORDER_COLOR
                if snapshot.is_working
                else IDLE_BORDER_COLOR
            ),
            working=snapshot.is_working,
            show_bang=snapshot.pending_count > 0,
            tasks=snapshot.tasks,
        )

"""Ticket 04: one-shot attention policy between core and external effects.

The dispatcher is deliberately dumb: it projects the public snapshot for the
presenter first, then attempts the sound/flash.  A broken sound sink cannot
roll back the visible state.
"""
from __future__ import annotations

import logging
from typing import Optional, Protocol

from .core import Attention, Snapshot
from .presenter import BallView

logger = logging.getLogger(__name__)


class PresenterSink(Protocol):
    def update(self, view: BallView) -> None: ...

    def flash(self) -> None: ...


class SoundSink(Protocol):
    def play(self, reason: str) -> None: ...


class AttentionDispatcher:
    """Translates one accepted event into view update plus one-shot effects."""

    def __init__(self, presenter: PresenterSink, sound: SoundSink) -> None:
        self.presenter = presenter
        self.sound = sound

    def on_state_change(
        self, snapshot: Snapshot, attention: Optional[Attention]
    ) -> None:
        view = BallView.from_snapshot(snapshot)
        self.presenter.update(view)
        if attention is None:
            return
        try:
            self.sound.play(attention.reason)
        except Exception:
            # The visible ! mark has already been committed above; record one
            # diagnostic but never let audio failure affect the Agent or Dock.
            logger.warning(
                "sound sink failed for attention reason %r",
                attention.reason,
                exc_info=True,
            )
        self.presenter.flash()

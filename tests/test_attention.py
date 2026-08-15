"""Ticket 04: one-shot attention is an external seam, not core internals."""
import unittest

from agent_activity_dock.attention import AttentionDispatcher
from agent_activity_dock.core import DockCore
from agent_activity_dock.events import Action, DockEvent
from agent_activity_dock.presenter import WORKING_BORDER_COLOR, BallView


class FakeSoundSink:
    def __init__(self, error=None):
        self.calls = []
        self.error = error

    def play(self, reason):
        self.calls.append(reason)
        if self.error is not None:
            raise self.error


class FakePresenter:
    def __init__(self):
        self.views = []
        self.flashes = 0

    def update(self, view):
        self.views.append(view)

    def flash(self):
        self.flashes += 1


class AttentionDispatcherTests(unittest.TestCase):
    def setUp(self):
        self.core = DockCore()
        self.sound = FakeSoundSink()
        self.presenter = FakePresenter()
        self.dispatcher = AttentionDispatcher(self.presenter, self.sound)

    def feed(self, action, task_id="task-1", event_id=None):
        event = DockEvent(task_id, "itest", event_id or f"{action}-{task_id}", action)
        result = self.core.apply(event)
        self.dispatcher.on_state_change(result.snapshot, result.attention)
        return result

    def test_ordinary_start_does_not_sound_or_flash(self):
        self.feed(Action.START)

        self.assertEqual([], self.sound.calls)
        self.assertEqual(0, self.presenter.flashes)
        self.assertEqual("1/1", self.presenter.views[-1].count_label)

    def test_each_terminal_reason_sounds_and_flashes_once(self):
        for action in (Action.STOP, Action.WAITING, Action.ERROR):
            with self.subTest(action=action):
                core = DockCore()
                sound = FakeSoundSink()
                presenter = FakePresenter()
                dispatcher = AttentionDispatcher(presenter, sound)
                core.apply(DockEvent("t", "itest", "s", Action.START))
                result = core.apply(DockEvent("t", "itest", f"e-{action}", action))
                dispatcher.on_state_change(result.snapshot, result.attention)

                self.assertEqual([str(action)], sound.calls)
                self.assertEqual(1, presenter.flashes)
                self.assertTrue(presenter.views[-1].show_bang)

                repeated = core.apply(DockEvent("t", "itest", f"e2-{action}", action))
                dispatcher.on_state_change(repeated.snapshot, repeated.attention)
                self.assertEqual([str(action)], sound.calls)
                self.assertEqual(1, presenter.flashes)

    def test_sound_failure_still_commits_visible_bang(self):
        self.sound.error = RuntimeError("no audio device")
        self.feed(Action.START)
        with self.assertLogs("agent_activity_dock.attention", level="WARNING") as captured:
            result = self.feed(Action.STOP)

        self.assertFalse(result.snapshot.is_working)
        self.assertEqual(1, result.snapshot.pending_count)
        self.assertTrue(self.presenter.views[-1].show_bang)
        self.assertEqual("0/1", self.presenter.views[-1].count_label)
        self.assertTrue(any("sound sink failed" in line for line in captured.output))

    def test_view_is_public_snapshot_projection_not_core_object(self):
        self.feed(Action.START)
        view = self.presenter.views[-1]
        self.assertIsInstance(view, BallView)
        self.assertEqual(WORKING_BORDER_COLOR, view.border_color)


if __name__ == "__main__":
    unittest.main()

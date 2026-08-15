"""Ticket 01: public event entry -> aggregate snapshot + attention effects.

The tests observe the same public seam that later IPC, adapters, and the
presenter will use.  They deliberately avoid private reducer fields.
"""
import unittest

from agent_activity_dock.core import DockCore
from agent_activity_dock.events import DockEvent, Action


def event(action, task_id="task-1", event_id=None, source="test-agent"):
    return DockEvent(
        task_id=task_id,
        source=source,
        event_id=event_id or f"{action}-{task_id}",
        action=action,
    )


class SingleTaskLifecycleTests(unittest.TestCase):
    def setUp(self):
        self.dock = DockCore()

    def test_start_shows_working_one_over_one(self):
        result = self.dock.apply(event(Action.START))

        self.assertTrue(result.accepted)
        self.assertEqual("1/1", result.snapshot.count_label)
        self.assertEqual(1, result.snapshot.working_count)
        self.assertEqual(1, result.snapshot.tracked_count)
        self.assertTrue(result.snapshot.is_working)
        self.assertEqual("working", result.snapshot.border_state)
        self.assertIsNone(result.attention)

    def test_stop_moves_to_zero_working_and_requests_attention_once(self):
        self.dock.apply(event(Action.START))
        result = self.dock.apply(event(Action.STOP))

        self.assertTrue(result.accepted)
        self.assertEqual("0/1", result.snapshot.count_label)
        self.assertFalse(result.snapshot.is_working)
        self.assertEqual("idle", result.snapshot.border_state)
        self.assertEqual(1, result.snapshot.pending_count)
        self.assertIsNotNone(result.attention)
        self.assertEqual(Action.STOP, result.attention.reason)

        repeated = self.dock.apply(event(Action.STOP, event_id="stop-again"))
        self.assertEqual("0/1", repeated.snapshot.count_label)
        self.assertEqual(1, repeated.snapshot.pending_count)
        self.assertIsNone(repeated.attention)

    def test_waiting_and_error_each_trigger_one_attention(self):
        for action in (Action.WAITING, Action.ERROR):
            with self.subTest(action=action):
                dock = DockCore()
                dock.apply(event(Action.START))
                result = dock.apply(event(action))

                self.assertFalse(result.snapshot.is_working)
                self.assertEqual(1, result.snapshot.pending_count)
                self.assertIsNotNone(result.attention)
                self.assertEqual(action, result.attention.reason)

                repeated = dock.apply(event(action, event_id=f"{action}-again"))
                self.assertIsNone(repeated.attention)
                self.assertEqual(1, repeated.snapshot.pending_count)

    def test_repeated_start_does_not_add_tasks_or_attention(self):
        self.dock.apply(event(Action.START))
        result = self.dock.apply(event(Action.START, event_id="start-again"))

        self.assertTrue(result.accepted)
        self.assertEqual("1/1", result.snapshot.count_label)
        self.assertIsNone(result.attention)

    def test_start_after_waiting_resumes_same_task_without_adding_one(self):
        self.dock.apply(event(Action.START))
        self.dock.apply(event(Action.WAITING, event_id="waiting-1"))

        resumed = self.dock.apply(event(Action.START, event_id="resume-1"))

        self.assertTrue(resumed.accepted)
        self.assertEqual("1/1", resumed.snapshot.count_label)
        self.assertTrue(resumed.snapshot.is_working)
        self.assertEqual(1, resumed.snapshot.pending_count)


class MultiTaskAggregationTests(unittest.TestCase):
    def test_two_of_three_tasks_working(self):
        dock = DockCore()
        dock.apply(event(Action.START, "task-1", "e1"))
        dock.apply(event(Action.START, "task-2", "e2"))
        result = dock.apply(event(Action.START, "task-3", "e3"))
        dock.apply(event(Action.STOP, "task-2", "e4"))

        snapshot = result.snapshot if False else dock.snapshot
        self.assertEqual("2/3", snapshot.count_label)
        self.assertEqual(2, snapshot.working_count)
        self.assertEqual(3, snapshot.tracked_count)
        self.assertTrue(snapshot.is_working)
        self.assertEqual("working", snapshot.border_state)

    def test_all_stopped_idle_snapshot_keeps_pending_marks(self):
        dock = DockCore()
        for task_id in ("task-1", "task-2", "task-3"):
            dock.apply(event(Action.START, task_id, f"start-{task_id}"))
        dock.apply(event(Action.STOP, "task-1", "stop-1"))
        result = dock.apply(event(Action.ERROR, "task-2", "error-2"))

        self.assertEqual("1/3", result.snapshot.count_label)
        self.assertEqual(2, result.snapshot.pending_count)
        dock.apply(event(Action.STOP, "task-3", "stop-3"))
        self.assertEqual("0/3", dock.snapshot.count_label)
        self.assertFalse(dock.snapshot.is_working)
        self.assertEqual(3, dock.snapshot.pending_count)


class ResetTests(unittest.TestCase):
    def test_reset_specified_task_clears_stale_work_and_pending_mark(self):
        dock = DockCore()
        dock.apply(event(Action.START, "task-1", "s1"))
        dock.apply(event(Action.START, "task-2", "s2"))
        dock.apply(event(Action.STOP, "task-2", "st2"))

        result = dock.apply(event(Action.RESET, "task-1", "r1"))

        self.assertTrue(result.accepted)
        self.assertEqual(1, result.snapshot.tracked_count)
        self.assertEqual(0, result.snapshot.working_count)
        # Only task-1 was reset; task-2's earlier stop mark remains visible.
        self.assertEqual(1, result.snapshot.pending_count)
        self.assertEqual("idle", result.snapshot.border_state)
        self.assertIsNone(result.attention)

        after_reset_pending = dock.apply(event(Action.RESET, "task-2", "r2"))
        self.assertEqual("0/0", after_reset_pending.snapshot.count_label)
        self.assertEqual(0, after_reset_pending.snapshot.pending_count)

    def test_reset_all_clears_every_task(self):
        dock = DockCore()
        dock.apply(event(Action.START, "task-1", "s1"))
        dock.apply(event(Action.START, "task-2", "s2"))
        dock.apply(event(Action.WAITING, "task-2", "w2"))

        result = dock.apply(event(Action.RESET, "*", "r-all"))

        self.assertEqual("0/0", result.snapshot.count_label)
        self.assertEqual(0, result.snapshot.pending_count)
        self.assertFalse(result.snapshot.is_working)


class ValidationTests(unittest.TestCase):
    def test_unknown_action_is_rejected_without_state_change(self):
        dock = DockCore()
        dock.apply(event(Action.START, "task-1", "s1"))
        before = dock.snapshot

        result = dock.apply(DockEvent("task-1", "test-agent", "bad-1", "explode"))

        self.assertFalse(result.accepted)
        self.assertEqual("unknown_action", result.rejection_reason)
        self.assertEqual(before, dock.snapshot)

    def test_missing_identity_fields_are_rejected_without_state_change(self):
        dock = DockCore()

        result = dock.apply(DockEvent("", "", "bad-2", Action.START))

        self.assertFalse(result.accepted)
        self.assertIsNotNone(result.rejection_reason)
        self.assertEqual("0/0", result.snapshot.count_label)

    def test_terminal_event_for_unknown_task_does_not_pollute_registry(self):
        dock = DockCore()

        result = dock.apply(event(Action.STOP, "never-started", "orphan-stop"))

        self.assertFalse(result.accepted)
        self.assertEqual("0/0", result.snapshot.count_label)


class AcknowledgementTests(unittest.TestCase):
    def test_acknowledge_task_clears_only_that_pending_mark(self):
        dock = DockCore()
        dock.apply(event(Action.START, "task-1", "s1"))
        dock.apply(event(Action.START, "task-2", "s2"))
        dock.apply(event(Action.STOP, "task-1", "st1"))
        dock.apply(event(Action.ERROR, "task-2", "e2"))

        snapshot = dock.acknowledge("task-1")

        self.assertEqual(1, snapshot.pending_count)
        task_1 = snapshot.task_by_id("task-1")
        task_2 = snapshot.task_by_id("task-2")
        self.assertFalse(task_1.needs_attention)
        self.assertTrue(task_2.needs_attention)

    def test_acknowledge_all_clears_every_pending_mark(self):
        dock = DockCore()
        dock.apply(event(Action.START, "task-1", "s1"))
        dock.apply(event(Action.STOP, "task-1", "st1"))
        dock.apply(event(Action.START, "task-2", "s2"))
        dock.apply(event(Action.WAITING, "task-2", "w2"))

        snapshot = dock.acknowledge_all()

        self.assertEqual(0, snapshot.pending_count)


if __name__ == "__main__":
    unittest.main()

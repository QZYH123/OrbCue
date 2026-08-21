"""Ticket 03: presenter contract is public-snapshot based, toolkit agnostic."""
import unittest

from agent_activity_dock.core import DockCore
from agent_activity_dock.events import Action, DockEvent
from agent_activity_dock.presenter import BallView, WORKING_BORDER_COLOR, IDLE_BORDER_COLOR
from agent_activity_dock.x11_presenter import parse_monitor_rect_from_xrandr_line


def start(dock, task_id):
    dock.apply(DockEvent(task_id, "itest", f"start-{task_id}", Action.START))


class VisibleMonitorParsingTests(unittest.TestCase):
    def test_xrandr_monitor_line_uses_output_offset(self):
        rect = parse_monitor_rect_from_xrandr_line(
            " 0: +*XWAYLAND0 2560/597x1440/336+1600+512  XWAYLAND0"
        )

        self.assertEqual((1600, 512, 2560, 1440), rect)

    def test_non_monitor_line_returns_none(self):
        self.assertIsNone(parse_monitor_rect_from_xrandr_line("Monitors: 1"))


class BallViewContractTests(unittest.TestCase):
    def test_empty_snapshot_has_idle_border_and_no_bang(self):
        view = BallView.from_snapshot(DockCore().snapshot)

        self.assertEqual("0/0", view.count_label)
        self.assertEqual(IDLE_BORDER_COLOR, view.border_color)
        self.assertFalse(view.show_bang)
        self.assertEqual(44, view.width)
        self.assertEqual(44, view.height)

    def test_working_snapshot_uses_working_border_and_keeps_label(self):
        dock = DockCore()
        start(dock, "task-1")
        dock.apply(DockEvent("task-1", "itest", "s2", Action.START))
        dock.apply(DockEvent("task-2", "itest", "s3", Action.START))
        dock.apply(DockEvent("task-3", "itest", "s4", Action.START))
        dock.apply(DockEvent("task-2", "itest", "stop-2", Action.STOP))
        dock.apply(DockEvent("task-3", "itest", "wait-3", Action.WAITING))
        dock.apply(DockEvent("task-1", "itest", "s5", Action.START))
        dock.apply(DockEvent("task-4", "itest", "s6", Action.START))
        dock.apply(DockEvent("task-5", "itest", "s7", Action.START))
        dock.apply(DockEvent("task-6", "itest", "s8", Action.START))
        dock.apply(DockEvent("task-7", "itest", "s9", Action.START))
        dock.apply(DockEvent("task-8", "itest", "s10", Action.START))
        dock.apply(DockEvent("task-9", "itest", "s11", Action.START))
        dock.apply(DockEvent("task-10", "itest", "s12", Action.START))
        dock.apply(DockEvent("task-4", "itest", "stop-4", Action.STOP))

        view = BallView.from_snapshot(dock.snapshot)

        self.assertEqual("7/10", view.count_label)
        self.assertEqual(WORKING_BORDER_COLOR, view.border_color)
        self.assertTrue(view.show_bang)
        self.assertEqual(10, len(view.tasks))

    def test_count_label_is_preserved_for_common_widths(self):
        cases = {
            "0/0": DockCore().snapshot,
        }
        dock = DockCore()
        start(dock, "only")
        cases["1/1"] = dock.snapshot

        for expected, snapshot in cases.items():
            with self.subTest(expected=expected):
                view = BallView.from_snapshot(snapshot)
                self.assertEqual(expected, view.count_label)

    def test_view_is_a_read_only_projection(self):
        dock = DockCore()
        start(dock, "task-1")
        view = BallView.from_snapshot(dock.snapshot)

        with self.assertRaises(AttributeError):
            view.border_color = IDLE_BORDER_COLOR

        self.assertEqual("1/1", view.count_label)


if __name__ == "__main__":
    unittest.main()

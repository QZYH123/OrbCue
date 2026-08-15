"""Ticket 06: thin adapters translate public source payloads to Dock events."""
import json
import pathlib
import unittest

from agent_activity_dock.adapters import (
    ClaudeHookAdapter,
    CodexWrapperAdapter,
    DshProjectionAdapter,
)
from agent_activity_dock.events import Action

FIXTURES = pathlib.Path(__file__).parent / "fixtures"


class ClaudeHookAdapterTests(unittest.TestCase):
    def test_session_start_fixture_maps_to_start(self):
        payload = json.loads((FIXTURES / "claude-session-start.json").read_text())
        event = ClaudeHookAdapter.to_dock_event(payload, event_id="claude-s-1")

        self.assertIsNotNone(event)
        self.assertEqual(Action.START, event.action)
        self.assertEqual("claude", event.source)
        self.assertEqual("claude-session-123", event.task_id)
        self.assertEqual("claude-s-1", event.event_id)

    def test_permission_request_maps_to_waiting_and_next_tool_use_can_resume(self):
        permission = json.loads((FIXTURES / "claude-permission-request.json").read_text())
        waiting = ClaudeHookAdapter.to_dock_event(permission, event_id="c-w-1")
        tool_use = ClaudeHookAdapter.to_dock_event(permission | {"hook_event_name": "PreToolUse"}, event_id="c-s-2")

        self.assertEqual(Action.WAITING, waiting.action)
        self.assertEqual(Action.START, tool_use.action)

    def test_session_end_and_stop_failure_fixtures(self):
        for filename, expected in (
            ("claude-session-end.json", Action.STOP),
            ("claude-stop-failure.json", Action.ERROR),
        ):
            with self.subTest(filename=filename):
                payload = json.loads((FIXTURES / filename).read_text())
                event = ClaudeHookAdapter.to_dock_event(payload, event_id="e-1")
                self.assertEqual(expected, event.action)
                self.assertEqual("claude-session-123", event.task_id)

    def test_malformed_or_unknown_hook_returns_none(self):
        self.assertIsNone(ClaudeHookAdapter.to_dock_event({"session_id": "x"}))
        self.assertIsNone(
            ClaudeHookAdapter.to_dock_event(
                {"hook_event_name": "UnknownEvent", "session_id": "x"}
            )
        )


class CodexWrapperAdapterTests(unittest.TestCase):
    def test_zero_exit_maps_to_stop_and_nonzero_to_error(self):
        stop = CodexWrapperAdapter.exit_event("codex-1", 0, event_id="c-stop")
        error = CodexWrapperAdapter.exit_event("codex-1", 2, event_id="c-error")
        start = CodexWrapperAdapter.start_event("codex-1", event_id="c-start")

        self.assertEqual(Action.START, start.action)
        self.assertEqual(Action.STOP, stop.action)
        self.assertEqual(Action.ERROR, error.action)
        self.assertEqual("codex", start.source)
        self.assertEqual("codex-1", error.task_id)


class DshProjectionAdapterTests(unittest.TestCase):
    def test_explicit_projection_fixtures_map_all_supported_actions(self):
        cases = {
            "dsh-session-started.json": Action.START,
            "dsh-session-waiting-input.json": Action.WAITING,
            "dsh-session-completed.json": Action.STOP,
            "dsh-session-failed.json": Action.ERROR,
        }
        for filename, expected in cases.items():
            with self.subTest(filename=filename):
                payload = json.loads((FIXTURES / filename).read_text())
                event = DshProjectionAdapter.to_dock_event(payload, event_id=f"d-{filename}")
                self.assertEqual(expected, event.action)
                self.assertEqual("dsh", event.source)
                self.assertEqual("dsh-session-1", event.task_id)

    def test_unknown_or_missing_projection_returns_none(self):
        self.assertIsNone(DshProjectionAdapter.to_dock_event({"event": "noise"}))
        self.assertIsNone(DshProjectionAdapter.to_dock_event({"session_id": "x"}))


if __name__ == "__main__":
    unittest.main()

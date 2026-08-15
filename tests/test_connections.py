"""Ticket 05: zero-install connection flow is user-level and revocable."""
import io
import json
import os
import pathlib
import subprocess
import sys
import textwrap
import unittest
from unittest import mock

from agent_activity_dock import cli as cli_module
from agent_activity_dock.connections import ConnectionManager


class TempHomeTestCase(unittest.TestCase):
    def setUp(self):
        self._old_env = os.environ.copy()
        self.home = pathlib.Path(
            subprocess.run(
                ["mktemp", "-d", "/tmp/aadock-connection-test-XXXXXX"],
                check=True,
                text=True,
                stdout=subprocess.PIPE,
            ).stdout.strip()
        )
        os.environ["HOME"] = str(self.home)
        os.environ["XDG_CONFIG_HOME"] = str(self.home / ".config")
        os.environ["XDG_DATA_HOME"] = str(self.home / ".local/share")
        (self.home / ".bashrc").write_text("# user bashrc\n")
        self.dock_log = self.home / "dock-events.jsonl"
        self.fake_dock = self.home / "fake-dock"
        self.fake_dock.write_text(
            "#!/bin/sh\n"
            f"printf '%s\\n' \"$*\" >> {self.dock_log}\n"
            "exit 0\n"
        )
        self.fake_dock.chmod(0o755)
        self.dock_candidates = [[str(self.fake_dock)]]

    def tearDown(self):
        os.environ.clear()
        os.environ.update(self._old_env)
        subprocess.run(["rm", "-rf", str(self.home)], check=False)

    def manager(self):
        return ConnectionManager(dock_cli_candidates=self.dock_candidates)

    def make_original(self, name, exit_code=7):
        original = self.home / "originals" / name
        original.parent.mkdir(exist_ok=True)
        original.write_text(
            "#!/bin/sh\n"
            f"printf 'arg:<%s>\\n' \"$*\" >> {self.home / (name + '.log')}\n"
            f"exit {exit_code}\n"
        )
        original.chmod(0o755)
        return original

    def run_wrapper(self, manager, name, *args):
        wrapper = manager.bin_dir / name
        return subprocess.run(
            [str(wrapper), *args],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=5,
        )


class WrapperConnectionTests(TempHomeTestCase):
    def test_wrapper_transparently_calls_original_and_reports_lifecycle(self):
        original = self.make_original("fake-agent", exit_code=7)
        manager = self.manager()
        record = manager.connect("fake-agent", original, method="wrapper")

        proc = self.run_wrapper(manager, "fake-agent", "one", "--flag=2")

        self.assertEqual(7, proc.returncode)
        self.assertEqual(f"arg:<one --flag=2>\n", (self.home / "fake-agent.log").read_text())
        events = [line for line in self.dock_log.read_text().splitlines() if line.strip()]
        self.assertTrue(any("start" in line for line in events), events)
        self.assertTrue(any("error" in line for line in events), events)
        self.assertTrue(any("fake-agent" in line for line in events), events)
        self.assertEqual("wrapper", record.method)
        self.assertEqual(str(original), record.original)

    def test_disconnect_removes_wrapper_path_snippet_and_record_not_original(self):
        original = self.make_original("fake-agent", exit_code=0)
        before = original.read_text()
        manager = self.manager()
        manager.connect("fake-agent", original, method="wrapper")
        self.assertTrue((manager.bin_dir / "fake-agent").exists())
        self.assertIn("agent-activity-dock", (self.home / ".bashrc").read_text())

        manager.disconnect("fake-agent")

        self.assertFalse((manager.bin_dir / "fake-agent").exists())
        self.assertNotIn("agent-activity-dock", (self.home / ".bashrc").read_text())
        self.assertNotIn("fake-agent", manager.load().agents)
        self.assertEqual(before, original.read_text())

    def test_wrapper_forwards_termination_signal(self):
        original = self.home / "originals" / "slow-agent"
        original.parent.mkdir(exist_ok=True)
        original.write_text(
            "#!/bin/sh\n"
            "while :; do sleep 1; done\n"
        )
        original.chmod(0o755)
        manager = self.manager()
        manager.connect("slow-agent", original, method="wrapper")
        wrapper = str(manager.bin_dir / "slow-agent")

        proc = subprocess.Popen([wrapper], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        import time
        time.sleep(0.4)
        proc.terminate()
        proc.wait(timeout=5)
        if proc.stdout is not None:
            proc.stdout.close()
        if proc.stderr is not None:
            proc.stderr.close()

        self.assertNotEqual(0, proc.returncode)

    def test_missing_original_never_replaces_or_downloads_agent(self):
        original = self.home / "not-installed" / "ghost-agent"
        manager = self.manager()
        with self.assertRaises(FileNotFoundError):
            manager.connect("ghost-agent", original, method="wrapper")
        self.assertNotIn("ghost-agent", manager.load().agents)
        self.assertFalse((manager.bin_dir / "ghost-agent").exists())


class DiscoveryAndSetupTests(TempHomeTestCase):
    def test_setup_connects_discovered_agents_without_reinstalling(self):
        original_dir = self.home / "path-bin"
        original_dir.mkdir()
        for name in ("codex", "dsh", "claude"):
            cmd = original_dir / name
            cmd.write_text("#!/bin/sh\nexit 0\n")
            cmd.chmod(0o755)
        old_path = os.environ.get("PATH", "")
        os.environ["PATH"] = f"{original_dir}{os.pathsep}{old_path}"

        manager = self.manager()
        results = manager.setup_all(yes=True)

        self.assertEqual({"codex", "claude", "dsh"}, {r.name for r in results})
        for name in ("codex", "dsh"):
            self.assertTrue((manager.bin_dir / name).exists())
            record = manager.load().agents[name]
            self.assertEqual("wrapper", record.method)
        claude_record = manager.load().agents["claude"]
        self.assertEqual("native_hook", claude_record.method)
        settings = json.loads((self.home / ".claude" / "settings.json").read_text())
        self.assertIn("SessionStart", settings["hooks"])
        self.assertIn("SessionEnd", settings["hooks"])

    def test_setup_dry_run_reports_plan_without_writing_user_files(self):
        original_dir = self.home / "path-bin"
        original_dir.mkdir()
        for name in ("codex", "claude"):
            cmd = original_dir / name
            cmd.write_text("#!/bin/sh\nexit 0\n")
            cmd.chmod(0o755)
        old_path = os.environ.get("PATH", "")
        os.environ["PATH"] = str(original_dir)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(pathlib.Path(__file__).resolve().parents[1] / "src")
        proc = subprocess.run(
            [sys.executable, "-m", "agent_activity_dock.cli", "setup", "--dry-run"],
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=5,
        )
        os.environ["PATH"] = old_path

        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertIn("codex: connect via wrapper", proc.stdout)
        self.assertIn("claude: connect via native_hook", proc.stdout)
        self.assertFalse((self.home / ".config").exists())
        self.assertFalse((self.home / ".local").exists())
        self.assertEqual("# user bashrc\n", (self.home / ".bashrc").read_text())

    def test_interactive_setup_connects_only_confirmed_agents(self):
        original_dir = self.home / "path-bin"
        original_dir.mkdir()
        for name in ("codex", "claude"):
            cmd = original_dir / name
            cmd.write_text("#!/bin/sh\nexit 0\n")
            cmd.chmod(0o755)
        old_path = os.environ.get("PATH", "")
        os.environ["PATH"] = str(original_dir)
        with mock.patch("builtins.input", side_effect=["n", "y"]), \
                mock.patch("sys.stdout", new=io.StringIO()):
            code = cli_module.main(["setup"])
        os.environ["PATH"] = old_path

        self.assertEqual(0, code)
        manager = self.manager()
        self.assertNotIn("codex", manager.load().agents)
        self.assertIn("claude", manager.load().agents)
        self.assertEqual("native_hook", manager.load().agents["claude"].method)

    def test_setup_skips_already_connected_agent(self):
        original_dir = self.home / "path-bin"
        original_dir.mkdir()
        original = original_dir / "codex"
        original.write_text("#!/bin/sh\nexit 0\n")
        original.chmod(0o755)
        old_path = os.environ.get("PATH", "")
        os.environ["PATH"] = str(original_dir)
        manager = self.manager()
        manager.connect("codex", original, method="wrapper")
        mtime = (manager.bin_dir / "codex").stat().st_mtime_ns

        results = manager.setup_all(yes=True)
        os.environ["PATH"] = old_path

        self.assertEqual(["codex"], [r.name for r in results])
        self.assertEqual("already_connected", results[0].status)
        self.assertEqual(mtime, (manager.bin_dir / "codex").stat().st_mtime_ns)


class ReconnectionTests(TempHomeTestCase):
    def test_changing_method_removes_previous_connection_artifacts(self):
        claude_dir = self.home / ".claude"
        claude_dir.mkdir()
        claude = self.make_original("claude", exit_code=0)
        manager = self.manager()

        native = manager.connect("claude", claude, method="native_hook")
        self.assertTrue(native.hook_script)
        settings = json.loads((claude_dir / "settings.json").read_text())
        self.assertIn("SessionStart", settings.get("hooks", {}))

        wrapper = manager.connect("claude", claude, method="wrapper")

        self.assertEqual("wrapper", wrapper.method)
        self.assertTrue((manager.bin_dir / "claude").exists())
        settings_after = json.loads((claude_dir / "settings.json").read_text())
        self.assertNotIn("SessionStart", settings_after.get("hooks", {}))
        self.assertNotIn("native_hook", manager.load().agents["claude"].method)

    def test_wrapper_to_native_switch_removes_orphan_path_snippet(self):
        claude_dir = self.home / ".claude"
        claude_dir.mkdir()
        claude = self.make_original("claude", exit_code=0)
        manager = self.manager()
        manager.connect("claude", claude, method="wrapper")
        self.assertIn("agent-activity-dock", (self.home / ".bashrc").read_text())
        self.assertTrue((manager.bin_dir / "claude").exists())

        record = manager.connect("claude", claude, method="native_hook")

        self.assertEqual("native_hook", record.method)
        self.assertFalse((manager.bin_dir / "claude").exists())
        self.assertNotIn("agent-activity-dock", (self.home / ".bashrc").read_text())

    def test_same_method_reconnect_refreshes_moved_executable_path(self):
        original_1 = self.make_original("codex", exit_code=0)
        manager = self.manager()
        manager.connect("codex", original_1, method="wrapper")

        original_2 = self.home / "new-path" / "codex"
        original_2.parent.mkdir()
        original_2.write_text("#!/bin/sh\nexit 0\n")
        original_2.chmod(0o755)
        refreshed = manager.connect("codex", original_2, method="wrapper")

        self.assertEqual(str(original_2), refreshed.original)
        wrapper_text = (manager.bin_dir / "codex").read_text()
        self.assertIn(str(original_2), wrapper_text)
        self.assertNotIn(str(original_1), wrapper_text)

    def test_failed_reconnect_keeps_previous_connection(self):
        claude_dir = self.home / ".claude"
        claude_dir.mkdir()
        claude = self.make_original("claude", exit_code=0)
        manager = self.manager()
        manager.connect("claude", claude, method="native_hook")

        with mock.patch(
            "agent_activity_dock.connections.ConnectionManager._install_wrapper",
            side_effect=RuntimeError("wrapper install failed"),
        ):
            with self.assertRaises(RuntimeError):
                manager.connect("claude", claude, method="wrapper")

        settings = json.loads((claude_dir / "settings.json").read_text())
        self.assertIn("SessionStart", settings.get("hooks", {}))
        self.assertEqual("native_hook", manager.load().agents["claude"].method)


class AdapterFailureIsolationTests(TempHomeTestCase):
    def test_one_broken_adapter_does_not_block_other_connections(self):
        original_dir = self.home / "path-bin"
        original_dir.mkdir()
        for name in ("codex", "claude"):
            cmd = original_dir / name
            cmd.write_text("#!/bin/sh\nexit 0\n")
            cmd.chmod(0o755)
        old_path = os.environ.get("PATH", "")
        os.environ["PATH"] = str(original_dir)
        manager = self.manager()
        with mock.patch(
            "agent_activity_dock.connections.install_claude_hooks",
            side_effect=RuntimeError("Claude hook schema changed"),
        ):
            results = manager.setup_all(yes=True)
        os.environ["PATH"] = old_path

        by_name = {result.name: result for result in results}
        self.assertEqual("connected", by_name["codex"].status)
        self.assertEqual("failed", by_name["claude"].status)
        self.assertIn("codex", manager.load().agents)
        self.assertNotIn("claude", manager.load().agents)
        self.assertTrue((manager.bin_dir / "codex").exists())


class CorruptManifestTests(TempHomeTestCase):
    def test_broken_connection_manifest_is_isolated(self):
        manager = self.manager()
        manager.config_dir.mkdir(parents=True, exist_ok=True)
        manager.config_path.write_text("{not-json")
        reloaded = ConnectionManager(dock_cli_candidates=self.dock_candidates)

        self.assertEqual({}, reloaded.agents)
        original = self.make_original("fake-agent", exit_code=0)
        reloaded.connect("fake-agent", original, method="wrapper")
        self.assertIn("fake-agent", reloaded.load().agents)


class NativeHookConnectionTests(TempHomeTestCase):
    def test_hook_script_does_not_read_or_forward_transcript_path(self):
        claude_dir = self.home / ".claude"
        claude_dir.mkdir()
        claude = self.make_original("claude", exit_code=0)
        manager = self.manager()
        record = manager.connect("claude", claude, method="native_hook")

        payload = json.dumps({
            "hook_event_name": "SessionStart",
            "session_id": "private-session-1",
            "transcript_path": "/home/user/.claude/projects/secret/transcript.jsonl",
            "cwd": "/home/user/secret-project",
        })
        proc = subprocess.run(
            [sys.executable, str(record.hook_script)],
            input=payload,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=5,
        )

        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual("{}\n", proc.stdout)
        dock_events = self.dock_log.read_text()
        self.assertIn("start", dock_events)
        self.assertIn("private-session-1", dock_events)
        self.assertNotIn("transcript", dock_events)
        self.assertNotIn("secret", dock_events)

    def test_claude_native_hook_install_and_disconnect_are_revocable(self):
        claude_dir = self.home / ".claude"
        claude_dir.mkdir()
        original_settings = {"theme": "dark", "env": {"KEEP": "yes"}}
        (claude_dir / "settings.json").write_text(json.dumps(original_settings))
        claude = self.make_original("claude", exit_code=0)

        manager = self.manager()
        record = manager.connect("claude", claude, method="native_hook")

        self.assertEqual("native_hook", record.method)
        settings = json.loads((claude_dir / "settings.json").read_text())
        self.assertEqual("dark", settings["theme"])
        self.assertEqual({"KEEP": "yes"}, settings["env"])
        for event in ("SessionStart", "SessionEnd"):
            self.assertTrue(settings["hooks"][event])
        hook_script = pathlib.Path(settings["hooks"]["SessionStart"][0]["hooks"][0]["command"])
        self.assertTrue(hook_script.exists())

        manager.disconnect("claude")

        self.assertEqual(original_settings, json.loads((claude_dir / "settings.json").read_text()))
        self.assertNotIn("claude", manager.load().agents)


if __name__ == "__main__":
    unittest.main()

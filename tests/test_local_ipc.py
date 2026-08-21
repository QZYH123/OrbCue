"""Ticket 02: real CLI -> current-user-only local IPC -> daemon -> snapshot."""
import concurrent.futures
import json
import os
import pathlib
import socket
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "src"


class RealDaemonTestCase(unittest.TestCase):
    """Start one real daemon process for the whole test class."""

    @classmethod
    def setUpClass(cls):
        cls.tmpdir = tempfile.TemporaryDirectory(prefix="aadock-ipc-test-")
        cls.socket_path = pathlib.Path(cls.tmpdir.name) / "dock.sock"
        cls.ready_file = pathlib.Path(cls.tmpdir.name) / "ready.json"
        env = os.environ.copy()
        env["PYTHONPATH"] = str(SRC) + os.pathsep + env.get("PYTHONPATH", "")
        cls.daemon = subprocess.Popen(
            [
                sys.executable,
                "-m",
                "agent_activity_dock.daemon",
                "--socket",
                str(cls.socket_path),
                "--ready-file",
                str(cls.ready_file),
            ],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline:
            if cls.ready_file.exists():
                break
            if cls.daemon.poll() is not None:
                break
            time.sleep(0.02)
        if not cls.ready_file.exists():
            out, err = cls.daemon.communicate(timeout=2)
            raise RuntimeError(f"daemon did not become ready: {out=} {err=}")

    @classmethod
    def tearDownClass(cls):
        cls.daemon.terminate()
        try:
            cls.daemon.wait(timeout=5)
        except subprocess.TimeoutExpired:
            cls.daemon.kill()
            cls.daemon.wait(timeout=5)
        if cls.daemon.stdout is not None:
            cls.daemon.stdout.close()
        if cls.daemon.stderr is not None:
            cls.daemon.stderr.close()
        cls.tmpdir.cleanup()

    def setUp(self):
        reset = self.cli("reset", "--all")
        self.assertEqual(0, reset.returncode, reset.stderr)

    def cli(self, *args):
        env = os.environ.copy()
        env["PYTHONPATH"] = str(SRC) + os.pathsep + env.get("PYTHONPATH", "")
        return subprocess.run(
            [sys.executable, "-m", "agent_activity_dock.cli"]
            + ["--socket", str(self.socket_path), "--json"]
            + list(args),
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=5,
        )

    def raw_event(self, action, task_id):
        payload = json.dumps({
            "task_id": task_id,
            "source": "concurrent-test",
            "event_id": f"{action}-{task_id}",
            "action": action,
        }).encode() + b"\n"
        return json.loads(self.raw_request(payload))

    def raw_request(self, payload: bytes):
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.connect(str(self.socket_path))
            sock.sendall(payload)
            sock.shutdown(socket.SHUT_WR)
            chunks = []
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                chunks.append(chunk)
        return b"".join(chunks)

    def parse(self, proc):
        self.assertEqual("", proc.stderr.strip(), proc.stderr)
        return json.loads(proc.stdout)


class CliLifecycleTests(RealDaemonTestCase):
    def test_subscription_pushes_snapshots_and_acknowledge_clears_pending(self):
        import socket as socket_module

        with socket_module.socket(socket_module.AF_UNIX, socket_module.SOCK_STREAM) as sub:
            sub.connect(str(self.socket_path))
            sub.settimeout(5)
            sub.sendall(b'{"query":"subscribe"}\n')
            file = sub.makefile("rb")
            first = json.loads(file.readline())
            self.assertEqual("subscribed", first["type"])
            self.assertEqual("0/0", first["snapshot"]["count_label"])

            start = self.parse(self.cli("start", "sub-task", "--source", "sub"))
            self.assertTrue(start["accepted"])

            pushed = json.loads(file.readline())
            self.assertEqual("snapshot", pushed["type"])
            self.assertEqual("1/1", pushed["snapshot"]["count_label"])
            self.assertIsNone(pushed["attention"])

            stop = self.parse(self.cli("stop", "sub-task"))
            stopped = json.loads(file.readline())
            self.assertEqual("stop", stopped["attention"]["reason"])
            self.assertEqual(1, stopped["snapshot"]["pending_count"])

            ack_response = json.loads(
                self.raw_request(b'{"query":"acknowledge","task_id":"*"}\n')
            )
            self.assertTrue(ack_response["accepted"])
            acked = json.loads(file.readline())
            self.assertEqual(0, acked["snapshot"]["pending_count"])
    def test_many_concurrent_clients_aggregate_without_lost_events(self):
        task_ids = [f"concurrent-{index}" for index in range(30)]

        def start(task_id):
            return self.raw_event("start", task_id)

        def stop(task_id):
            return self.raw_event("stop", task_id)

        with concurrent.futures.ThreadPoolExecutor(max_workers=10) as pool:
            starts = list(pool.map(start, task_ids))
        self.assertTrue(all(item["accepted"] for item in starts))
        status = self.parse(self.cli("status"))
        self.assertEqual("30/30", status["snapshot"]["count_label"])
        self.assertEqual(30, status["snapshot"]["working_count"])

        with concurrent.futures.ThreadPoolExecutor(max_workers=10) as pool:
            stops = list(pool.map(stop, task_ids))
        self.assertTrue(all(item["accepted"] for item in stops))
        status = self.parse(self.cli("status"))
        self.assertEqual("0/30", status["snapshot"]["count_label"])
        self.assertEqual(30, status["snapshot"]["pending_count"])
    def test_start_stop_and_status_follow_public_snapshot(self):
        start = self.parse(self.cli("start", "task-1", "--source", "itest"))
        self.assertEqual(0, self.cli("start", "task-1", "--source", "itest").returncode)
        self.assertTrue(start["accepted"])
        self.assertEqual("1/1", start["snapshot"]["count_label"])

        stop = self.parse(self.cli("stop", "task-1"))
        self.assertTrue(stop["accepted"])
        self.assertEqual("0/1", stop["snapshot"]["count_label"])
        self.assertEqual("idle", stop["snapshot"]["border_state"])
        self.assertEqual("stop", stop["attention"]["reason"])

        status = self.parse(self.cli("status"))
        self.assertEqual("0/1", status["snapshot"]["count_label"])
        self.assertEqual(1, status["snapshot"]["pending_count"])

    def test_multiple_tasks_aggregate_as_two_of_three(self):
        for task_id in ("task-1", "task-2", "task-3"):
            proc = self.cli("start", task_id, "--source", "itest")
            self.assertEqual(0, proc.returncode)
        self.assertEqual(0, self.cli("stop", "task-2").returncode)

        status = self.parse(self.cli("status"))
        self.assertEqual("2/3", status["snapshot"]["count_label"])
        self.assertEqual("working", status["snapshot"]["border_state"])
        self.assertEqual(2, status["snapshot"]["working_count"])
        self.assertEqual(3, status["snapshot"]["tracked_count"])

    def test_repeated_start_does_not_pollute(self):
        self.cli("start", "stable-task", "--source", "itest")

        repeated = self.parse(self.cli("start", "stable-task"))
        self.assertTrue(repeated["accepted"])
        self.assertEqual("1/1", repeated["snapshot"]["count_label"])
        self.assertEqual(1, repeated["snapshot"]["tracked_count"])

    def test_reset_round_trip_through_real_cli(self):
        self.cli("start", "reset-me", "--source", "itest")
        reset = self.parse(self.cli("reset", "reset-me"))
        self.assertTrue(reset["accepted"])
        self.assertEqual("0/0", reset["snapshot"]["count_label"])

        status = self.parse(self.cli("status"))
        self.assertEqual("0/0", status["snapshot"]["count_label"])


class InvalidRequestTests(RealDaemonTestCase):
    def test_unknown_action_is_rejected_and_state_unchanged(self):
        self.cli("start", "keep", "--source", "itest")
        unknown = json.dumps({
            "task_id": "keep", "source": "itest",
            "event_id": "unknown-action-1", "action": "explode",
        }).encode() + b"\n"
        response = json.loads(self.raw_request(unknown))
        self.assertFalse(response["accepted"])
        self.assertEqual("unknown_action", response["rejection_reason"])
        self.assertEqual("1/1", response["snapshot"]["count_label"])

    def test_malformed_json_gets_rejection_without_state_change(self):
        self.cli("start", "keep", "--source", "itest")
        response = json.loads(self.raw_request(b"{not-json\n"))
        self.assertFalse(response["accepted"])
        self.assertIn("invalid_json", response["rejection_reason"])
        self.assertEqual("1/1", response["snapshot"]["count_label"])

    def test_oversized_message_is_rejected_without_state_change(self):
        self.cli("start", "keep", "--source", "itest")
        big = json.dumps({"action": "stop", "task_id": "keep", "source": "x",
                          "event_id": "big-1", "padding": "x" * 32768}).encode() + b"\n"
        response = json.loads(self.raw_request(big))
        self.assertFalse(response["accepted"])
        self.assertEqual("message_too_large", response["rejection_reason"])
        status = self.parse(self.cli("status"))
        self.assertEqual("1/1", status["snapshot"]["count_label"])

    def test_terminal_event_for_unknown_task_is_rejected_without_tracking(self):
        response = self.parse(self.cli("stop", "ghost-task"))
        self.assertFalse(response["accepted"])
        self.assertEqual("unknown_task", response["rejection_reason"])
        self.assertEqual("0/0", response["snapshot"]["count_label"])


class SocketOwnershipTests(RealDaemonTestCase):
    def test_socket_directory_and_socket_are_current_user_only(self):
        st = self.socket_path.stat()
        self.assertEqual(0o600, st.st_mode & 0o777)
        self.assertEqual(0o700, self.socket_path.parent.stat().st_mode & 0o777)


if __name__ == "__main__":
    unittest.main()

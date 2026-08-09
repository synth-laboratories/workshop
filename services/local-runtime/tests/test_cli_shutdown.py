from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path


class RuntimeCliShutdownTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.connection = self.root / "connection.json"
        self.process: subprocess.Popen[str] | None = None

    def tearDown(self) -> None:
        if self.process is not None and self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=5)
        self.temp.cleanup()

    def start_runtime(self) -> subprocess.Popen[str]:
        env = os.environ.copy()
        env.update(
            {
                "SYNTH_INTERN_DEMO": "1",
                "SYNTH_RUNTIME_TOKEN": "shutdown-test-token",
            }
        )
        self.process = subprocess.Popen(
            [
                sys.executable,
                "-m",
                "synth_local_runtime",
                "--host",
                "127.0.0.1",
                "--port",
                "0",
                "--data-dir",
                str(self.root / "data"),
                "--connection-file",
                str(self.connection),
            ],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                stdout, stderr = self.process.communicate()
                self.fail(f"runtime exited during startup: {stdout}\n{stderr}")
            if self.connection.exists():
                json.loads(self.connection.read_text(encoding="utf-8"))
                return self.process
            time.sleep(0.025)
        self.fail("runtime did not write its isolated connection file")

    def assert_clean_exit(self) -> None:
        assert self.process is not None
        _stdout, stderr = self.process.communicate(timeout=5)
        self.assertEqual(self.process.returncode, 0, stderr)
        self.assertFalse(self.connection.exists())

    def test_sigterm_exits_without_serving_thread_deadlock(self) -> None:
        process = self.start_runtime()
        process.send_signal(signal.SIGTERM)
        self.assert_clean_exit()

    def test_stop_command_exits_runtime_cleanly(self) -> None:
        self.start_runtime()
        stopped = subprocess.run(
            [
                sys.executable,
                "-m",
                "synth_local_runtime",
                "--stop",
                "--connection-file",
                str(self.connection),
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
        self.assertEqual(stopped.returncode, 0, stopped.stderr)
        self.assertEqual(json.loads(stopped.stdout), {"stopping": True})
        self.assert_clean_exit()


if __name__ == "__main__":
    unittest.main()

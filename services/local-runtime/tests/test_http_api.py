from __future__ import annotations

import json
import tempfile
import threading
import unittest
import urllib.request
from pathlib import Path

from synth_local_runtime.api import RuntimeHTTPServer
from synth_local_runtime.config import RuntimeConfig
from synth_local_runtime.service import RuntimeService


class RuntimeHttpTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        config = RuntimeConfig(
            host="127.0.0.1",
            port=0,
            data_dir=Path(self.temp.name),
            runtime_token="test-token",
            connection_file=None,
            backend_url="https://example.invalid",
            synth_api_key=None,
            intern_demo=True,
            laguna_base_url=None,
            laguna_stub_delay_ms=0,
            openrouter_api_key=None,
            laguna_model_path=None,
            visuals_root=None,
            workshop_root=None,
        )
        self.service = RuntimeService(config)
        self.server = RuntimeHTTPServer(("127.0.0.1", 0), self.service, token="test-token")
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        host, port = self.server.server_address[:2]
        self.url = f"http://{host}:{port}"

    def tearDown(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)
        self.service.store.close_thread_connection()
        self.temp.cleanup()

    def request(self, method: str, path: str, body: object | None = None) -> object:
        data = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(
            f"{self.url}{path}",
            data=data,
            method=method,
            headers={
                "Authorization": "Bearer test-token",
                "Content-Type": "application/json",
            },
        )
        with urllib.request.urlopen(request, timeout=5) as response:
            return json.loads(response.read().decode())

    def test_health_create_and_replay(self) -> None:
        health = self.request("GET", "/v1/health")
        self.assertEqual(health["protocolVersion"], "synth.desktop-runtime.v1")
        session = self.request(
            "POST",
            "/v1/sessions",
            {"target": {"kind": "local", "model": "laguna-xs-2.1", "adapter": None}},
        )
        page = self.request("GET", f"/v1/sessions/{session['id']}/events?after_sequence=0")
        self.assertEqual(page["events"][0]["eventKind"], "session.created")

    def test_sse_replays_from_cursor(self) -> None:
        session = self.request(
            "POST",
            "/v1/sessions",
            {"target": {"kind": "local", "model": "laguna-xs-2.1", "adapter": None}},
        )
        request = urllib.request.Request(
            f"{self.url}/v1/sessions/{session['id']}/events/stream?after_sequence=0",
            headers={
                "Authorization": "Bearer test-token",
                "Accept": "text/event-stream",
            },
        )
        with urllib.request.urlopen(request, timeout=5) as response:
            lines: list[str] = []
            for _ in range(8):
                line = response.readline().decode().strip()
                lines.append(line)
                if line.startswith("data:"):
                    break
        data_line = next(line for line in lines if line.startswith("data:"))
        event = json.loads(data_line.removeprefix("data:").strip())
        self.assertEqual(event["eventKind"], "session.created")
        self.assertEqual(event["sequence"], 1)


if __name__ == "__main__":
    unittest.main()

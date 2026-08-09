from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from synth_local_runtime.codex.config import CodexLaunchConfig
from synth_local_runtime.codex.session import CodexAgentSession


class FakeClient:
    def __init__(self, **kwargs):
        self.kwargs = kwargs
        self.requests = []

    def start(self): pass
    def initialize(self): return {}
    def close(self): pass

    def request(self, method, params, timeout=120):
        self.requests.append((method, params))
        if method == "thread/start":
            return {"thread": {"id": "thr_1"}}
        if method == "thread/resume":
            return {"thread": {"id": params["threadId"]}}
        if method == "turn/start":
            self.kwargs["on_notification"]("turn/completed", {"turn": {"id": "turn_1"}})
            return {"turn": {"id": "turn_1"}}
        return {}


class CodexAgentSessionTests(unittest.TestCase):
    def test_starts_thread_and_runs_responses_backed_turn(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            config = CodexLaunchConfig(
                codex_home=root / "codex", laguna_base_url="http://127.0.0.1:7333",
                laguna_api_key="test", model="laguna", workspace=root,
                enable_visuals_mcp=False,
            )
            with patch("synth_local_runtime.codex.session.resolve_codex_bin", return_value="codex"):
                session = CodexAgentSession(config, client_factory=FakeClient)
            self.assertEqual(session.start(), "thr_1")
            self.assertEqual(session.run_turn("inspect files"), "turn_1")
            methods = [method for method, _ in session.client.requests]
            self.assertEqual(methods, ["thread/start", "turn/start"])
            config_text = (root / "codex" / "config.toml").read_text()
            self.assertIn('wire_api = "responses"', config_text)

    def test_resumes_persisted_thread_before_turn(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            config = CodexLaunchConfig(
                codex_home=root / "codex", laguna_base_url="http://127.0.0.1:7333",
                laguna_api_key="test", model="laguna", workspace=root,
                enable_visuals_mcp=False,
            )
            with patch("synth_local_runtime.codex.session.resolve_codex_bin", return_value="codex"):
                session = CodexAgentSession(
                    config, thread_id="thr_saved", client_factory=FakeClient
                )
            self.assertEqual(session.start(), "thr_saved")
            method, params = session.client.requests[0]
            self.assertEqual(method, "thread/resume")
            self.assertEqual(params["model"], "laguna")
            self.assertEqual(params["cwd"], str(root))


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import json
import tempfile
import time
import unittest
from pathlib import Path

from fastapi.testclient import TestClient

from laguna_daemon.app import build_app
from laguna_daemon.config import LagunaConfig
from laguna_daemon.responses import (
    build_chat_body_from_responses,
    responses_input_to_messages,
)


def _config(tmp: Path, *, api_key: str = "synth-test-key") -> LagunaConfig:
    models = tmp / "models"
    models.mkdir(parents=True, exist_ok=True)
    data = tmp / "data"
    data.mkdir(parents=True, exist_ok=True)
    return LagunaConfig(
        host="127.0.0.1",
        port=7333,
        backend="mock",
        api_key=api_key,
        models_dir=models,
        default_model="poolside/Laguna-XS-2.1-NVFP4-mlx",
        model="poolside/Laguna-XS-2.1-NVFP4-mlx",
        revision=None,
        draft_model=None,
        adapter=None,
        upstream_host="127.0.0.1",
        upstream_port=17999,
        external_url=None,
        upstream_api_key=None,
        data_dir=data,
        auto_load=True,
        idle_unload_after_seconds=900,
        context_length=262144,
        started_at=time.time(),
    )


class ResponsesShimTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp(prefix="synth-laguna-resp-"))
        self.api_key = "synth-test-key"
        self.client = TestClient(build_app(_config(self.tmp, api_key=self.api_key)))

    def test_input_string_to_messages(self) -> None:
        messages = responses_input_to_messages(
            {"instructions": "Be brief.", "input": "list files"}
        )
        self.assertEqual(messages[0]["role"], "system")
        self.assertEqual(messages[-1], {"role": "user", "content": "list files"})

    def test_build_chat_body_with_tools(self) -> None:
        body = build_chat_body_from_responses(
            {
                "model": "laguna-xs-2.1",
                "input": "hi",
                "tools": [
                    {
                        "type": "function",
                        "name": "shell",
                        "description": "run shell",
                        "parameters": {"type": "object", "properties": {}},
                    }
                ],
            },
            default_model="poolside/Laguna-XS-2.1-NVFP4-mlx",
        )
        self.assertEqual(body["model"], "poolside/Laguna-XS-2.1-NVFP4-mlx")
        self.assertEqual(body["tools"][0]["function"]["name"], "shell")

    def test_responses_non_stream(self) -> None:
        response = self.client.post(
            "/v1/responses",
            headers={"Authorization": f"Bearer {self.api_key}"},
            json={"model": "laguna-xs-2.1", "input": "hello", "stream": False},
        )
        self.assertEqual(response.status_code, 200)
        payload = response.json()
        self.assertEqual(payload["object"], "response")
        self.assertEqual(payload["status"], "completed")
        self.assertTrue(payload["output"])
        text = payload["output"][0]["content"][0]["text"]
        self.assertIn("hello", text.lower())

    def test_responses_stream(self) -> None:
        with self.client.stream(
            "POST",
            "/v1/responses",
            headers={"Authorization": f"Bearer {self.api_key}"},
            json={"model": "laguna-xs-2.1", "input": "stream me", "stream": True},
        ) as response:
            self.assertEqual(response.status_code, 200)
            body = "".join(response.iter_text())
        self.assertIn("response.created", body)
        self.assertIn("response.output_text.delta", body)
        self.assertIn("response.completed", body)


if __name__ == "__main__":
    unittest.main()

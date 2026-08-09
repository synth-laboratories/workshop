from __future__ import annotations

import tempfile
import time
import unittest
from pathlib import Path

from fastapi.testclient import TestClient

from laguna_daemon.app import build_app
from laguna_daemon.config import LagunaConfig


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


class SidecarApiCompatTests(unittest.TestCase):
    """Poolside-compatible OpenAI shapes on the independent Synth sidecar."""

    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp(prefix="synth-laguna-"))
        self.api_key = "synth-test-key"
        self.client = TestClient(build_app(_config(self.tmp, api_key=self.api_key)))

    def test_401_without_bearer(self) -> None:
        response = self.client.get("/v1/models")
        self.assertEqual(response.status_code, 401)
        body = response.json()
        self.assertEqual(body["error"]["code"], "401")
        self.assertIn("bearer", body["error"]["message"])

    def test_health_and_models_shape(self) -> None:
        headers = {"Authorization": f"Bearer {self.api_key}"}
        health = self.client.get("/health", headers=headers).json()
        for key in (
            "status",
            "modelsDirectory",
            "defaultModel",
            "loadedModel",
            "memoryBytes",
            "idleUnloadAfterSeconds",
        ):
            self.assertIn(key, health)
        models = self.client.get("/v1/models", headers=headers).json()
        self.assertEqual(models["object"], "list")
        item = models["data"][0]
        self.assertEqual(item["id"], "poolside/Laguna-XS-2.1-NVFP4-mlx")
        self.assertEqual(item["context_length"], 262144)
        self.assertEqual(item["details"]["format"], "safetensors")

    def test_chat_completion_mock(self) -> None:
        headers = {"Authorization": f"Bearer {self.api_key}"}
        response = self.client.post(
            "/v1/chat/completions",
            headers=headers,
            json={
                "model": "laguna-xs-2.1",
                "messages": [{"role": "user", "content": "hi"}],
                "stream": False,
            },
        )
        self.assertEqual(response.status_code, 200)
        body = response.json()
        self.assertEqual(body["object"], "chat.completion")
        self.assertEqual(body["model"], "poolside/Laguna-XS-2.1-NVFP4-mlx")
        self.assertEqual(body["choices"][0]["message"]["role"], "assistant")
        self.assertIn("usage", body)

    def test_unknown_route_404(self) -> None:
        headers = {"Authorization": f"Bearer {self.api_key}"}
        response = self.client.get("/nope", headers=headers)
        self.assertEqual(response.status_code, 404)
        self.assertIn("unknown route", response.json()["error"]["message"])


if __name__ == "__main__":
    unittest.main()

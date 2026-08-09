from __future__ import annotations

import tempfile
import time
import unittest
import subprocess
import sys
from pathlib import Path

from fastapi.testclient import TestClient

from laguna_daemon.app import build_app
from laguna_daemon.config import LagunaConfig
from laguna_daemon.manager import LagunaProcessManager


def _config(
    tmp: Path, *, api_key: str = "synth-test-key", idle_seconds: int = 900
) -> LagunaConfig:
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
        idle_unload_after_seconds=idle_seconds,
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
        self.assertIs(health["responsesApi"], True)
        for key in (
            "status",
            "modelsDirectory",
            "defaultModel",
            "loadedModel",
            "memoryBytes",
            "idleSeconds",
            "idleUnloadAfterSeconds",
            "lastUsedAt",
            "freeAt",
        ):
            self.assertIn(key, health)
        models = self.client.get("/v1/models", headers=headers).json()
        self.assertEqual(models["object"], "list")
        item = models["data"][0]
        self.assertEqual(item["id"], "poolside/Laguna-XS-2.1-NVFP4-mlx")
        self.assertEqual(item["context_length"], 262144)
        self.assertEqual(item["details"]["format"], "safetensors")

    def test_health_advertises_the_configured_idle_unload_delay(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synth-laguna-timeout-") as tmp:
            config = _config(Path(tmp), idle_seconds=30)
            with TestClient(build_app(config)) as client:
                health = client.get(
                    "/health", headers={"Authorization": f"Bearer {self.api_key}"}
                ).json()
        self.assertEqual(health["idleUnloadAfterSeconds"], 30)

    def test_native_health_is_ready_but_reports_nonresident_weights(self) -> None:
        headers = {"Authorization": f"Bearer {self.api_key}"}
        self.client.app.state.manager.state = "unloaded"
        health = self.client.get("/health", headers=headers).json()
        self.assertEqual(health["status"], "ok")
        self.assertIsNone(health["loadedModel"])
        self.assertEqual(health["memoryBytes"], 0)

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
        health = self.client.get("/health", headers=headers).json()
        self.assertLessEqual(health["idleSeconds"], 1)

    def test_unknown_route_404(self) -> None:
        headers = {"Authorization": f"Bearer {self.api_key}"}
        response = self.client.get("/nope", headers=headers)
        self.assertEqual(response.status_code, 404)
        self.assertIn("unknown route", response.json()["error"]["message"])


class SidecarLifecycleTests(unittest.IsolatedAsyncioTestCase):
    async def test_short_idle_timeout_terminates_owned_process(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synth-laguna-owned-") as tmp:
            manager = LagunaProcessManager(_config(Path(tmp), idle_seconds=1))
            process = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(60)"],
                start_new_session=True,
            )
            manager.process = process
            manager.state = "ready"
            manager.last_used_at = time.time() - 2

            self.assertTrue(await manager.unload_if_idle())
            self.assertEqual(manager.state, "unloaded")
            self.assertIsNone(manager.process)
            self.assertIsNotNone(process.poll())
            self.assertIsNone(manager.status()["pid"])

    async def test_unloads_owned_model_after_fifteen_minutes(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synth-laguna-lifecycle-") as tmp:
            manager = LagunaProcessManager(_config(Path(tmp)))
            manager.state = "ready"
            manager.last_used_at = 100.0

            self.assertFalse(await manager.unload_if_idle(now=999.9))
            self.assertTrue(await manager.unload_if_idle(now=1000.0))
            self.assertEqual(manager.state, "unloaded")

    async def test_active_response_blocks_eviction_and_next_prompt_reloads(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synth-laguna-active-") as tmp:
            manager = LagunaProcessManager(_config(Path(tmp), idle_seconds=1))
            manager.state = "ready"
            manager.begin_request()
            manager.last_used_at = 100.0

            self.assertFalse(await manager.unload_if_idle(now=200.0))
            manager.end_request()
            self.assertTrue(await manager.unload_if_idle(now=200.0))
            self.assertEqual(manager.state, "unloaded")

            manager.begin_request()
            status = await manager.ensure_ready()
            manager.end_request()
            self.assertEqual(status["state"], "ready")


if __name__ == "__main__":
    unittest.main()

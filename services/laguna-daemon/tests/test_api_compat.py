from __future__ import annotations

import asyncio
import tempfile
import threading
import time
import unittest
from dataclasses import replace
from functools import partial
from pathlib import Path

from fastapi.testclient import TestClient

from laguna_daemon.app import build_app
from laguna_daemon.config import LagunaConfig
from laguna_daemon.responses_api.backends.mlx import NativeMlxBackend
from laguna_daemon.responses_api.telemetry import GenerationTiming


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
        external_url=None,
        upstream_api_key=None,
        data_dir=data,
        auto_load=True,
        idle_unload_after_seconds=idle_seconds,
        context_length=262144,
        started_at=time.time(),
    )


class SidecarApiCompatTests(unittest.TestCase):
    """OpenAI shapes on the single self-contained Synth daemon."""

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
        self.assertIs(health["chatCompletionsApi"], True)
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

    def test_health_no_longer_advertises_a_second_engine(self) -> None:
        """There is one runtime, so there is no engine to choose or report."""
        headers = {"Authorization": f"Bearer {self.api_key}"}
        health = self.client.get("/health", headers=headers).json()
        self.assertNotIn("responsesEngine", health)

    def test_health_advertises_the_configured_idle_unload_delay(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synth-laguna-timeout-") as tmp:
            config = _config(Path(tmp), idle_seconds=30)
            with TestClient(build_app(config)) as client:
                health = client.get(
                    "/health", headers={"Authorization": f"Bearer {self.api_key}"}
                ).json()
        self.assertEqual(health["idleUnloadAfterSeconds"], 30)

    def test_health_is_ready_but_reports_nonresident_weights(self) -> None:
        """Weights load lazily, so readiness must not require residency."""
        headers = {"Authorization": f"Bearer {self.api_key}"}
        health = self.client.get("/health", headers=headers).json()
        self.assertEqual(health["status"], "ok")
        self.assertIsNone(health["loadedModel"])
        self.assertEqual(health["memoryBytes"], 0)

    def test_native_health_reports_missing_weights_as_not_installed(self) -> None:
        """A live HTTP server is not inference-ready without its local model."""
        with tempfile.TemporaryDirectory(prefix="synth-laguna-missing-") as tmp:
            config = replace(
                _config(Path(tmp), api_key=self.api_key), backend="mlx_lm"
            )
            with TestClient(build_app(config)) as client:
                health = client.get(
                    "/health", headers={"Authorization": f"Bearer {self.api_key}"}
                ).json()
        self.assertEqual(health["status"], "not_installed")
        self.assertIsNone(health["loadedModel"])

    def test_chat_completion_over_the_native_core(self) -> None:
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


class NativeResidencyLifecycleTests(unittest.IsolatedAsyncioTestCase):
    """Residency now belongs to the in-process backend, not a child process.

    These are the successors to the old owned-subprocess lifecycle tests: the
    behavior that mattered — idle eviction, an eviction guard while work is in
    flight, and staying alive so the next prompt can reload — is unchanged, but
    it is now about MLX weights inside this daemon rather than a second server.
    """

    def _backend(self) -> NativeMlxBackend:
        backend = NativeMlxBackend(model_path=Path("/nonexistent"), context_length=1024)
        # Stand in for loaded weights. `_release_model_memory` only drops
        # references and clears caches, so it needs no real MLX runtime.
        backend._model = object()
        backend._tokenizer = object()
        return backend

    async def test_recent_use_blocks_eviction(self) -> None:
        backend = self._backend()
        backend._last_used_at = time.time()
        self.assertFalse(await backend.unload_if_idle(900))
        self.assertIsNotNone(backend._model)
        await backend.close()

    async def test_idle_weights_are_released_without_stopping_the_daemon(self) -> None:
        backend = self._backend()
        backend._last_used_at = time.time() - 2
        self.assertTrue(await backend.unload_if_idle(1))
        self.assertIsNone(backend._model)
        self.assertIsNone(backend._tokenizer)
        # The backend object itself survives, so the next prompt reloads.
        self.assertFalse(backend.diagnostics()["loaded"])
        await backend.close()

    async def test_in_flight_generation_guards_against_eviction(self) -> None:
        backend = self._backend()
        backend._last_used_at = time.time() - 100
        backend._inflight_generations = 1
        self.assertFalse(await backend.unload_if_idle(1))
        self.assertIsNotNone(backend._model)

        backend._inflight_generations = 0
        self.assertTrue(await backend.unload_if_idle(1))
        self.assertIsNone(backend._model)
        await backend.close()

    async def test_zero_delay_disables_automatic_eviction(self) -> None:
        backend = self._backend()
        backend._last_used_at = time.time() - 10_000
        self.assertFalse(await backend.unload_if_idle(0))
        self.assertIsNotNone(backend._model)
        await backend.close()

    async def test_residency_reports_free_at_only_while_loaded(self) -> None:
        backend = self._backend()
        loaded = backend.residency(900)
        self.assertTrue(loaded["loaded"])
        self.assertIsNotNone(loaded["free_at"])

        backend._last_used_at = time.time() - 2
        await backend.unload_if_idle(1)
        unloaded = backend.residency(900)
        self.assertFalse(unloaded["loaded"])
        self.assertIsNone(unloaded["free_at"])
        await backend.close()


class GenerationSlotOwnershipTests(unittest.IsolatedAsyncioTestCase):
    """The admission slot's lifetime must equal the worker thread's lifetime.

    Regression coverage for a live-MLX hang: the slot used to be released in a
    `finally` that awaited the worker future. When the caller was already being
    cancelled — the ordinary client-disconnect path — that await returned
    without joining the thread, so the slot reopened while an orphaned
    generation still owned the backend's single executor thread. The next
    request then took the slot, queued its own worker behind the orphan, and
    sat in `prefill` forever with the whole queue stalled behind it.
    """

    def _backend(self) -> NativeMlxBackend:
        return NativeMlxBackend(model_path=Path("/nonexistent"), context_length=1024)

    def _register(self, backend: NativeMlxBackend, generation_id: str) -> GenerationTiming:
        timing = GenerationTiming(generation_id=generation_id, queued_at=time.monotonic())
        timing.phase = "prefill"
        backend._generations[generation_id] = timing
        backend._inflight_generations += 1
        return timing

    async def test_tokenizer_work_uses_the_owned_mlx_thread(self) -> None:
        """Concurrent request compilation must not borrow the fast tokenizer."""
        backend = self._backend()
        self.addAsyncCleanup(backend.close)

        class RecordingTokenizer:
            def __init__(self) -> None:
                self.threads: list[str] = []

            def apply_chat_template(self, *_args: object, **_kwargs: object) -> str:
                self.threads.append(threading.current_thread().name)
                return "compiled prompt"

            def encode(self, *_args: object, **_kwargs: object) -> list[int]:
                self.threads.append(threading.current_thread().name)
                return [1, 2, 3]

        tokenizer = RecordingTokenizer()
        backend._model = object()
        backend._tokenizer = tokenizer
        turn = await backend.compile_messages(
            messages=[{"role": "user", "content": "hello"}],
            model="laguna",
            generation_id="gen_tokenizer_thread",
        )
        usage = await backend.count_tokens(turn)

        self.assertEqual(usage.input_tokens, 3)
        self.assertEqual(len(tokenizer.threads), 2)
        self.assertTrue(
            all(name.startswith("laguna-mlx") for name in tokenizer.threads),
            tokenizer.threads,
        )

    async def test_slot_stays_closed_until_the_worker_thread_finishes(self) -> None:
        backend = self._backend()
        self.addAsyncCleanup(backend.close)
        await backend._generation_slot.acquire()
        timing = self._register(backend, "gen_hold")

        gate = threading.Event()
        loop = asyncio.get_running_loop()
        worker = loop.run_in_executor(backend._executor, gate.wait)
        worker.add_done_callback(
            partial(backend._retire_generation, "gen_hold", timing)
        )

        # The worker is still running, so the slot must not be available.
        await asyncio.sleep(0.05)
        self.assertTrue(backend._generation_slot.locked())
        self.assertFalse(backend.diagnostics()["generation_slot_available"])

        gate.set()
        await worker
        await asyncio.sleep(0)  # let the done callback run on the loop

        self.assertFalse(backend._generation_slot.locked())
        self.assertTrue(backend.diagnostics()["generation_slot_available"])
        self.assertEqual(backend._inflight_generations, 0)
        self.assertEqual(timing.phase, "complete")

    async def test_a_cancelled_consumer_cannot_reopen_the_slot_early(self) -> None:
        """Cancelling the awaiting coroutine must not hand the slot away."""
        backend = self._backend()
        self.addAsyncCleanup(backend.close)
        await backend._generation_slot.acquire()
        timing = self._register(backend, "gen_cancel")

        gate = threading.Event()
        loop = asyncio.get_running_loop()
        worker = loop.run_in_executor(backend._executor, gate.wait)
        worker.add_done_callback(
            partial(backend._retire_generation, "gen_cancel", timing)
        )

        async def consumer() -> None:
            try:
                await asyncio.sleep(30)
            finally:
                # Mirrors the production teardown: signal, then leave the join
                # to the done callback rather than awaiting it here.
                gate_seen.set()

        gate_seen = threading.Event()
        task = asyncio.create_task(consumer())
        await asyncio.sleep(0.05)
        task.cancel()
        with self.assertRaises(asyncio.CancelledError):
            await task

        # The consumer is gone, but the thread has not stopped, so the slot
        # must still be closed. This is the exact condition that used to let a
        # following request wedge the queue.
        self.assertTrue(backend._generation_slot.locked())

        gate.set()
        await worker
        await asyncio.sleep(0)
        self.assertFalse(backend._generation_slot.locked())

    async def test_retire_is_idempotent_bookkeeping(self) -> None:
        backend = self._backend()
        self.addAsyncCleanup(backend.close)
        await backend._generation_slot.acquire()
        timing = self._register(backend, "gen_once")
        backend._cancel_flags["gen_once"] = threading.Event()

        backend._retire_generation("gen_once", timing, None)

        self.assertNotIn("gen_once", backend._generations)
        self.assertNotIn("gen_once", backend._cancel_flags)
        self.assertIn("gen_once", backend._recent_generations)
        self.assertEqual(backend._inflight_generations, 0)
        self.assertFalse(backend._generation_slot.locked())


class MemoryReportingTests(unittest.IsolatedAsyncioTestCase):
    """`residentBytes` must measure memory, not the filesystem.

    It previously summed the model files' on-disk size, so an unloaded or
    partially-loaded runtime still advertised ~20 GB "resident".
    """

    async def test_unloaded_backend_reports_zero(self) -> None:
        backend = NativeMlxBackend(model_path=Path("/nonexistent"), context_length=1024)
        self.addAsyncCleanup(backend.close)
        self.assertEqual(backend.memory_bytes(), 0)

    async def test_loaded_backend_reports_allocator_bytes_or_nothing(self) -> None:
        backend = NativeMlxBackend(model_path=Path("/nonexistent"), context_length=1024)
        self.addAsyncCleanup(backend.close)
        backend._model = object()
        measured = backend.memory_bytes()
        # Either a real allocator figure or an explicit "cannot measure".
        # Never the size of the weights on disk.
        self.assertTrue(measured is None or isinstance(measured, int))
        if measured is not None:
            self.assertGreaterEqual(measured, 0)

    async def test_snapshot_never_reports_disk_size_as_memory(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synth-mem-") as tmp:
            from laguna_daemon.responses_api import ResponsesService

            service = ResponsesService(_config(Path(tmp)))
            await service.start()
            try:
                snapshot = service.inference_snapshot()
            finally:
                # Close inside the temp dir's lifetime; the SQLite writer needs
                # the file to still exist.
                await service.close()
            # The mock backend holds no weights, so nothing may claim memory.
            self.assertEqual(snapshot["residentBytes"], 0)


if __name__ == "__main__":
    unittest.main()

"""Contract tests for the /v1/synth sidecar control API.

Everything runs against the deterministic FakeBackend plus an injected fake
downloader — no weights, no network, no live daemon. The live counterpart is
tests/integration/test_sidecar_lifecycle.py.
"""

from __future__ import annotations

import asyncio
import json
import tempfile
import threading
import time
import unittest
from datetime import datetime
from pathlib import Path

from fastapi.testclient import TestClient

from laguna_daemon.app import build_app
from laguna_daemon.config import LagunaConfig
from laguna_daemon.synth_control import CANONICAL_STATES, SynthControl


MODEL = "poolside/Laguna-XS-2.1-NVFP4-mlx"
GIB = 1024**3

ERROR_CODES = {
    "model_not_found",
    "download_in_progress",
    "download_failed",
    "invalid_model",
    "insufficient_memory",
    "load_failed",
    "unload_in_progress",
    "generation_busy",
    "invalid_state_transition",
    "invalid_setting",
    "settings_write_failed",
    "openapi_unavailable",
}

STATUS_FIELDS = {
    "schema_version",
    "sidecar_version",
    "backend",
    "model",
    "state",
    "state_since",
    "memory",
    "generation",
    "reasoning",
    "idle_unload_after_seconds",
    "settings_schema_version",
}


def _config(tmp: Path) -> LagunaConfig:
    models = tmp / "models"
    models.mkdir(parents=True, exist_ok=True)
    data = tmp / "data"
    data.mkdir(parents=True, exist_ok=True)
    return LagunaConfig(
        host="127.0.0.1",
        port=7333,
        backend="mock",
        api_key=None,
        models_dir=models,
        default_model=MODEL,
        model=MODEL,
        revision=None,
        draft_model=None,
        adapter=None,
        external_url=None,
        upstream_api_key=None,
        data_dir=data,
        auto_load=True,
        idle_unload_after_seconds=900,
        context_length=262144,
        started_at=time.time(),
    )


def _install_fake_weights(models_dir: Path, model: str = MODEL) -> Path:
    """A directory that looks like an indexed checkpoint to the daemon."""
    path = models_dir / model
    path.mkdir(parents=True, exist_ok=True)
    (path / "model.safetensors.index.json").write_text(
        json.dumps({"metadata": {"total_size": 8 * GIB}, "weight_map": {}}),
        encoding="utf-8",
    )
    return path


class FakeDownloader:
    """Deterministic Downloader: byte progress, optional failure, optional gate."""

    def __init__(
        self,
        *,
        fail: bool = False,
        gate: threading.Event | None = None,
        bytes_total: int = 1000,
    ) -> None:
        self.fail = fail
        self.gate = gate
        self.bytes_total = bytes_total
        self.calls: list[str] = []

    def download(self, model, destination, progress) -> None:
        self.calls.append(model)
        if self.gate is not None:
            assert self.gate.wait(timeout=10), "test gate never released"
        if self.fail:
            raise RuntimeError("network unreachable")
        progress(100, self.bytes_total)
        progress(self.bytes_total, self.bytes_total)
        _install_fake_weights(destination.parent.parent, model)


class ControlApiTestCase(unittest.TestCase):
    """Shared plumbing: app on the mock backend with deterministic memory."""

    install_weights = True

    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="synth-control-")
        self.addCleanup(self.temp.cleanup)
        self.config = _config(Path(self.temp.name))
        if self.install_weights:
            _install_fake_weights(self.config.models_dir)
        self.client = TestClient(build_app(self.config))
        self.control: SynthControl = self.client.app.state.synth_control
        # Real machine RAM must not decide test outcomes.
        self.control.system_memory_bytes = 64 * GIB
        self.control.available_memory_bytes = 64 * GIB
        # Real host capacity must not decide deterministic download contracts.
        self.control.free_disk_bytes = 64 * GIB

    def status(self) -> dict:
        response = self.client.get("/v1/synth/status")
        self.assertEqual(response.status_code, 200)
        return response.json()

    def assert_error(self, response, status: int, code: str) -> dict:
        self.assertEqual(response.status_code, status, response.text)
        body = response.json()
        error = body["error"]
        self.assertEqual(error["code"], code)
        self.assertIn(code, ERROR_CODES)
        self.assertIsInstance(error["message"], str)
        self.assertIsInstance(error["retryable"], bool)
        self.assertIsInstance(error["details"], dict)
        self.assertTrue(body["request_id"].startswith("req_"))
        return body

    def runtime_events(self) -> list[dict]:
        return [
            event
            for event in self.control.broker.history()
            if event["event"] == "runtime.state_changed"
        ]


class StatusContractTests(ControlApiTestCase):
    def test_status_schema(self) -> None:
        body = self.status()
        self.assertEqual(set(body), STATUS_FIELDS)
        self.assertEqual(body["schema_version"], "1.0")
        self.assertIsInstance(body["sidecar_version"], str)
        self.assertEqual(body["backend"], "mock")

        model = body["model"]
        self.assertEqual(
            set(model), {"id", "revision", "available", "resident", "resident_bytes"}
        )
        self.assertEqual(model["id"], MODEL)
        self.assertIsNone(model["revision"])
        self.assertTrue(model["available"])
        self.assertFalse(model["resident"])

        self.assertIn(body["state"], CANONICAL_STATES)
        self.assertEqual(body["state"], "unloaded")
        # state_since must parse as ISO-8601.
        datetime.fromisoformat(body["state_since"].replace("Z", "+00:00"))

        memory = body["memory"]
        self.assertEqual(set(memory), {"free_bytes", "required_bytes", "admission"})
        self.assertEqual(memory["admission"], "allowed")
        self.assertEqual(memory["required_bytes"], 32 * GIB)
        self.assertEqual(memory["free_bytes"], 64 * GIB)

        generation = body["generation"]
        self.assertEqual(set(generation), {"in_flight", "queued", "active_request_id"})
        self.assertEqual(generation["in_flight"], 0)
        self.assertEqual(generation["queued"], 0)
        self.assertIsNone(generation["active_request_id"])

        self.assertEqual(
            body["reasoning"],
            {
                "supported": ["none", "high"],
                "default": "high",
                "legacy_aliases": {"max": "high"},
            },
        )
        self.assertEqual(body["idle_unload_after_seconds"], 900)
        self.assertEqual(body["settings_schema_version"], "1.0")

    def test_status_during_generation_reports_a_generation_state(self) -> None:
        service = self.client.app.state.responses_service
        backend = service.backend
        seen: list[dict] = []
        original = backend.stream

        def instrumented(turn):
            async def wrapper():
                async for event in original(turn):
                    if not seen:
                        seen.append(self.status())
                    yield event

            return wrapper()

        backend.stream = instrumented
        self.addCleanup(setattr, backend, "stream", original)
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        self.assertTrue(seen, "no status was taken during generation")
        self.assertIn(seen[0]["state"], {"queued", "prefill", "decoding"})
        generation = seen[0]["generation"]
        self.assertEqual(generation["in_flight"], 1)
        self.assertTrue(generation["active_request_id"].startswith("sha256:"))

    def test_capabilities_schema(self) -> None:
        response = self.client.get("/v1/synth/capabilities")
        self.assertEqual(response.status_code, 200)
        body = response.json()
        self.assertEqual(body["schema_version"], "1.0")
        self.assertEqual(body["model"], MODEL)
        self.assertEqual(body["reasoning"]["legacy_aliases"], {"max": "high"})
        self.assertIn("context_length", body["capabilities"])
        self.assertEqual(
            body["control_api"]["openapi_url"], "/v1/synth/openapi.json"
        )

    def test_models_is_the_control_plane_view(self) -> None:
        body = self.client.get("/v1/synth/models").json()
        self.assertEqual(body["object"], "list")
        entry = next(item for item in body["data"] if item["id"] == MODEL)
        self.assertTrue(entry["available"])
        self.assertFalse(entry["resident"])
        self.assertTrue(entry["default"])
        # Distinct from the OpenAI serving catalog, which has its own shape.
        openai_list = self.client.get("/v1/models").json()
        self.assertNotIn("schema_version", openai_list)


class LifecycleTests(ControlApiTestCase):
    def test_load_walks_the_canonical_states(self) -> None:
        self.status()  # observe the initial unloaded state
        response = self.client.post(f"/v1/synth/models/{MODEL}/load")
        self.assertEqual(response.status_code, 200, response.text)
        body = response.json()
        self.assertTrue(body["operation_id"].startswith("op_"))
        self.assertEqual(body["state"], "resident_idle")
        self.assertTrue(body["resident"])
        self.assertFalse(body["already_resident"])
        self.assertTrue(self.status()["model"]["resident"])

        transitions = [
            (event["previous_state"], event["state"]) for event in self.runtime_events()
        ]
        self.assertEqual(
            transitions,
            [
                ("starting", "unloaded"),
                ("unloaded", "checking_memory"),
                ("checking_memory", "loading"),
                ("loading", "resident_idle"),
            ],
        )
        # One load, one operation: every transition it caused shares its id.
        operation_ids = {
            event["operation_id"]
            for event in self.runtime_events()
            if event["previous_state"] != "starting"
        }
        self.assertEqual(operation_ids, {body["operation_id"]})
        for event in self.runtime_events():
            self.assertEqual(
                set(event),
                {"event", "operation_id", "state", "previous_state", "timestamp"},
            )

    def test_load_is_idempotent(self) -> None:
        first = self.client.post(f"/v1/synth/models/{MODEL}/load").json()
        events_before = len(self.runtime_events())
        second = self.client.post(f"/v1/synth/models/{MODEL}/load")
        self.assertEqual(second.status_code, 200)
        body = second.json()
        self.assertTrue(body["already_resident"])
        self.assertEqual(body["state"], "resident_idle")
        self.assertNotEqual(body["operation_id"], first["operation_id"])
        self.assertEqual(len(self.runtime_events()), events_before)

    def test_unload_is_idempotent_and_walks_the_states(self) -> None:
        self.client.post(f"/v1/synth/models/{MODEL}/load")
        response = self.client.post(f"/v1/synth/models/{MODEL}/unload")
        self.assertEqual(response.status_code, 200, response.text)
        body = response.json()
        self.assertEqual(body["state"], "unloaded")
        self.assertFalse(body["resident"])
        self.assertFalse(body["already_unloaded"])
        transitions = [
            (event["previous_state"], event["state"]) for event in self.runtime_events()
        ]
        self.assertIn(("resident_idle", "unloading"), transitions)
        self.assertIn(("unloading", "unloaded"), transitions)

        again = self.client.post(f"/v1/synth/models/{MODEL}/unload")
        self.assertEqual(again.status_code, 200)
        self.assertTrue(again.json()["already_unloaded"])

    def test_unload_during_a_generation_is_generation_busy(self) -> None:
        service = self.client.app.state.responses_service
        backend = service.backend
        outcomes: list = []
        original = backend.stream

        def instrumented(turn):
            async def wrapper():
                async for event in original(turn):
                    if not outcomes:
                        outcomes.append(
                            self.client.post(f"/v1/synth/models/{MODEL}/unload")
                        )
                    yield event

            return wrapper()

        backend.stream = instrumented
        self.addCleanup(setattr, backend, "stream", original)
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        body = self.assert_error(outcomes[0], 409, "generation_busy")
        self.assertTrue(body["error"]["retryable"])
        # The legacy alias keeps its exact historical contract.
        legacy = self.client.post("/v1/synth/model/unload")
        self.assertEqual(legacy.status_code, 200)

    def test_load_with_insufficient_memory_is_typed_and_blocks(self) -> None:
        self.control.system_memory_bytes = 16 * GIB
        response = self.client.post(f"/v1/synth/models/{MODEL}/load")
        body = self.assert_error(response, 503, "insufficient_memory")
        details = body["error"]["details"]
        self.assertEqual(details["required_bytes"], 32 * GIB)
        self.assertEqual(details["available_bytes"], 16 * GIB)
        status = self.status()
        self.assertEqual(status["state"], "blocked_memory")
        self.assertEqual(status["memory"]["admission"], "blocked")

    def test_load_blocks_when_capacity_is_sufficient_but_available_memory_is_not(self) -> None:
        self.control.system_memory_bytes = 64 * GIB
        self.control.available_memory_bytes = 8 * GIB
        response = self.client.post(f"/v1/synth/models/{MODEL}/load")
        body = self.assert_error(response, 503, "insufficient_memory")
        self.assertEqual(body["error"]["details"]["available_bytes"], 8 * GIB)
        self.assertEqual(self.status()["state"], "blocked_memory")

    def test_load_of_an_unknown_model_is_invalid_model(self) -> None:
        response = self.client.post("/v1/synth/models/other/model/load")
        self.assert_error(response, 400, "invalid_model")

    def test_download_when_weights_exist_is_invalid_state_transition(self) -> None:
        response = self.client.post(f"/v1/synth/models/{MODEL}/download")
        self.assert_error(response, 409, "invalid_state_transition")


class DownloadTests(ControlApiTestCase):
    install_weights = False

    def wait_for_job(self, job_id: str, terminal: set[str]) -> dict:
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            response = self.client.get(f"/v1/synth/downloads/{job_id}")
            self.assertEqual(response.status_code, 200)
            job = response.json()
            if job["state"] in terminal:
                return job
            time.sleep(0.01)
        self.fail(f"download job {job_id} never reached {terminal}")

    def test_download_lifecycle_and_operation_identity(self) -> None:
        self.control.downloader = FakeDownloader()
        self.assertFalse(self.status()["model"]["available"])

        response = self.client.post(f"/v1/synth/models/{MODEL}/download")
        self.assertEqual(response.status_code, 202, response.text)
        job = response.json()
        self.assertTrue(job["job_id"].startswith("op_"))
        self.assertEqual(job["operation_id"], job["job_id"])
        self.assertEqual(job["model"], MODEL)
        self.assertIn(job["state"], {"queued", "downloading"})
        self.assertEqual(
            set(job),
            {
                "job_id",
                "operation_id",
                "model",
                "state",
                "bytes_done",
                "bytes_total",
                "error",
                "created_at",
                "updated_at",
            },
        )

        finished = self.wait_for_job(job["job_id"], {"downloaded", "failed"})
        self.assertEqual(finished["state"], "downloaded")
        self.assertEqual(finished["bytes_done"], 1000)
        self.assertEqual(finished["bytes_total"], 1000)
        self.assertIsNone(finished["error"])

        status = self.status()
        self.assertTrue(status["model"]["available"])
        self.assertEqual(status["state"], "downloaded")

        # Event ordering for the whole download, all under one operation id.
        events = [
            event
            for event in self.control.broker.history()
            if event["operation_id"] == job["job_id"]
        ]
        names_and_states = [(event["event"], event["state"]) for event in events]
        self.assertEqual(
            names_and_states,
            [
                ("runtime.state_changed", "downloading"),
                ("download.state_changed", "downloading"),
                ("download.progress", "downloading"),
                ("download.progress", "downloading"),
                ("download.state_changed", "downloaded"),
                ("runtime.state_changed", "downloaded"),
            ],
        )
        for event in events:
            self.assertIn("previous_state", event)
            self.assertIn("timestamp", event)

        # After a download, load consumes the "downloaded" state.
        load = self.client.post(f"/v1/synth/models/{MODEL}/load")
        self.assertEqual(load.status_code, 200, load.text)
        self.assertEqual(self.status()["state"], "resident_idle")

    def test_download_refuses_insufficient_disk(self) -> None:
        self.control.free_disk_bytes = 8 * GIB
        response = self.client.post(f"/v1/synth/models/{MODEL}/download")
        body = self.assert_error(response, 507, "download_failed")
        self.assertEqual(body["error"]["details"]["reason"], "insufficient_disk")
        self.assertEqual(body["error"]["details"]["free_bytes"], 8 * GIB)

    def test_download_failure_is_recorded_not_fabricated(self) -> None:
        self.control.downloader = FakeDownloader(fail=True)
        job = self.client.post(f"/v1/synth/models/{MODEL}/download").json()
        finished = self.wait_for_job(job["job_id"], {"downloaded", "failed"})
        self.assertEqual(finished["state"], "failed")
        self.assertIn("network unreachable", finished["error"])
        # Failure never invents byte counts.
        self.assertIsNone(finished["bytes_done"])
        self.assertIsNone(finished["bytes_total"])
        self.assertEqual(self.status()["state"], "error")

    def test_concurrent_download_is_download_in_progress(self) -> None:
        gate = threading.Event()
        self.addCleanup(gate.set)
        self.control.downloader = FakeDownloader(gate=gate)
        first = self.client.post(f"/v1/synth/models/{MODEL}/download")
        self.assertEqual(first.status_code, 202)
        second = self.client.post(f"/v1/synth/models/{MODEL}/download")
        body = self.assert_error(second, 409, "download_in_progress")
        self.assertEqual(body["error"]["details"]["job_id"], first.json()["job_id"])
        # Loading mid-download is refused the same way.
        load = self.client.post(f"/v1/synth/models/{MODEL}/load")
        self.assert_error(load, 409, "download_in_progress")
        self.assertEqual(self.status()["state"], "downloading")
        gate.set()
        self.wait_for_job(first.json()["job_id"], {"downloaded"})

    def test_unknown_job_is_a_typed_404(self) -> None:
        response = self.client.get("/v1/synth/downloads/op_deadbeef")
        self.assert_error(response, 404, "model_not_found")

    def test_load_without_weights_is_model_not_found(self) -> None:
        response = self.client.post(f"/v1/synth/models/{MODEL}/load")
        self.assert_error(response, 404, "model_not_found")


class MetricsMirrorTests(ControlApiTestCase):
    def test_unmeasured_metrics_stay_null(self) -> None:
        body = self.client.get("/v1/synth/metrics").json()
        self.assertEqual(body["schema_version"], "1.0")
        rolling = body["rolling"]
        for key in ("ttftP50Ms", "decodeTpsP50", "latencyP95Ms"):
            self.assertIsNone(rolling[key])
        self.assertEqual(rolling["requestsCompleted"], 0)

    def test_metrics_mirror_the_rolling_telemetry(self) -> None:
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        body = self.client.get("/v1/synth/metrics").json()
        snapshot = self.client.get("/v1/synth/inference").json()
        self.assertEqual(body["rolling"], snapshot["rolling"])
        self.assertEqual(body["resident"], snapshot["resident"])
        self.assertEqual(body["queue_capacity"], snapshot["queueCapacity"])
        self.assertEqual(body["rolling"]["requestsCompleted"], 1)
        self.assertIn(body["state"], CANONICAL_STATES)
        # The Prometheus surface is untouched by the JSON mirror.
        prometheus = self.client.get("/metrics")
        self.assertTrue(prometheus.headers["content-type"].startswith("text/plain"))


class PrefillHistogramTests(ControlApiTestCase):
    BUCKETS = ("<=1k", "<=5k", "<=10k", "<=25k", "<=50k", "<=150k", ">150k")

    def telemetry(self):
        return self.client.app.state.responses_service.coordinator.runner.telemetry

    def record(self, prompt_tokens: int, **overrides) -> None:
        """Feed one synthetic completed generation into the rolling window."""
        from laguna_daemon.responses_api.telemetry import GenerationTiming

        base = {
            "generation_id": f"gen_{prompt_tokens}",
            "queued_at": 100.0,
            "admitted_at": 100.2,
            "compiled_at": 100.5,
            "first_token_at": 102.5,
            "last_token_at": 104.5,
            "prompt_tokens": prompt_tokens,
            "cached_tokens": 0,
            "output_tokens": 20,
        }
        base.update(overrides)
        self.telemetry().record_completed(GenerationTiming(**base), latency_ms=500.0)

    def histogram(self) -> dict:
        body = self.client.get("/v1/synth/metrics").json()
        return body["prefill_histogram"]

    def test_shape_and_empty_buckets_stay_null(self) -> None:
        histogram = self.histogram()
        self.assertEqual(tuple(histogram), self.BUCKETS)
        for bucket, entry in histogram.items():
            self.assertEqual(
                set(entry),
                {"count", "cached_token_share", "ttft_p50_ms", "prefill_tps_p50"},
                bucket,
            )
            self.assertEqual(entry["count"], 0)
            # No data means null everywhere — never a fabricated zero.
            self.assertIsNone(entry["cached_token_share"])
            self.assertIsNone(entry["ttft_p50_ms"])
            self.assertIsNone(entry["prefill_tps_p50"])

    def test_bucket_assignment_across_the_full_range(self) -> None:
        for tokens in (500, 1000, 4000, 9000, 20000, 40000, 100000, 200000):
            self.record(tokens)
        histogram = self.histogram()
        counts = {bucket: entry["count"] for bucket, entry in histogram.items()}
        # 1000 sits on the boundary and belongs to <=1k.
        self.assertEqual(
            counts,
            {
                "<=1k": 2,
                "<=5k": 1,
                "<=10k": 1,
                "<=25k": 1,
                "<=50k": 1,
                "<=150k": 1,
                ">150k": 1,
            },
        )

    def test_measured_values_and_cached_share(self) -> None:
        # 8000 computed tokens over the 2.0s compile→first-token span.
        self.record(10000, cached_tokens=2000)
        entry = self.histogram()["<=10k"]
        self.assertEqual(entry["count"], 1)
        self.assertEqual(entry["cached_token_share"], 0.2)
        # ttft measures from enqueue: 102.5 - 100.0.
        self.assertEqual(entry["ttft_p50_ms"], 2500.0)
        self.assertEqual(entry["prefill_tps_p50"], 4000.0)

    def test_unmeasured_prefill_stays_null_without_erasing_the_count(self) -> None:
        # Never produced a first token: no ttft, no prefill rate — but the
        # generation still happened and must be counted.
        self.record(3000, first_token_at=None, last_token_at=None)
        # Fully cached prompt: zero computed tokens is unmeasurable throughput.
        self.record(3000, cached_tokens=3000)
        entry = self.histogram()["<=5k"]
        self.assertEqual(entry["count"], 2)
        self.assertEqual(entry["cached_token_share"], 0.5)
        # The only measurable ttft is the cached generation's.
        self.assertEqual(entry["ttft_p50_ms"], 2500.0)
        self.assertIsNone(entry["prefill_tps_p50"])

    def test_real_generations_land_in_the_small_bucket(self) -> None:
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        self.assertGreaterEqual(self.histogram()["<=1k"]["count"], 1)

    def test_prometheus_exposition_follows_the_existing_style(self) -> None:
        self.record(10000, cached_tokens=2000)
        body = self.client.get("/metrics").text
        self.assertIn("# TYPE laguna_prefill_requests_total counter", body)
        self.assertIn('laguna_prefill_requests_total{bucket="<=10k"} 1', body)
        self.assertIn('laguna_prefill_requests_total{bucket=">150k"} 0', body)
        self.assertIn(
            'laguna_prefill_cached_token_share{bucket="<=10k"} 0.2', body
        )
        # Buckets without data export a count but never a fabricated share.
        self.assertNotIn(
            'laguna_prefill_cached_token_share{bucket=">150k"}', body
        )
        # Pre-existing metric names are untouched.
        self.assertIn("laguna_queue_capacity", body)

    def test_legacy_inference_payload_is_unchanged(self) -> None:
        self.record(10000)
        snapshot = self.client.get("/v1/synth/inference").json()
        self.assertNotIn("prefill_histogram", snapshot)
        self.assertNotIn("prefill_histogram", snapshot["rolling"])


class ReasoningNormalizationTests(ControlApiTestCase):
    def test_legacy_max_is_normalized_to_high_and_echoed(self) -> None:
        response = self.client.post(
            "/v1/responses",
            json={
                "model": MODEL,
                "input": "hi",
                "store": False,
                "reasoning": {"effort": "max"},
            },
        )
        self.assertEqual(response.status_code, 200, response.text)
        self.assertEqual(response.json()["reasoning"]["effort"], "high")
        self.assertEqual(
            self.status()["reasoning"]["legacy_aliases"], {"max": "high"}
        )


class OpenApiTests(ControlApiTestCase):
    def test_openapi_document_is_served_from_the_checked_in_file(self) -> None:
        response = self.client.get("/v1/synth/openapi.json")
        self.assertEqual(response.status_code, 200)
        document = response.json()
        self.assertEqual(document["openapi"], "3.1.0")
        for path in (
            "/v1/synth/status",
            "/v1/synth/capabilities",
            "/v1/synth/models",
            "/v1/synth/models/{model}/download",
            "/v1/synth/downloads/{job_id}",
            "/v1/synth/models/{model}/load",
            "/v1/synth/models/{model}/unload",
            "/v1/synth/metrics",
            "/v1/synth/events",
            "/v1/synth/settings",
            "/v1/synth/openapi.json",
        ):
            self.assertIn(path, document["paths"])
        spec_states = set(
            document["components"]["schemas"]["State"]["enum"]
        )
        self.assertEqual(spec_states, set(CANONICAL_STATES))


class AuthTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="synth-control-auth-")
        self.addCleanup(self.temp.cleanup)
        # The config dataclass is frozen; rebuild it with a key.
        from dataclasses import replace

        config = replace(_config(Path(self.temp.name)), api_key="secret-key")
        _install_fake_weights(config.models_dir)
        self.client = TestClient(build_app(config))

    def test_control_surface_requires_the_same_bearer_key(self) -> None:
        for path in (
            "/v1/synth/status",
            "/v1/synth/capabilities",
            "/v1/synth/models",
            "/v1/synth/metrics",
            "/v1/synth/settings",
            "/v1/synth/openapi.json",
        ):
            self.assertEqual(self.client.get(path).status_code, 401, path)
            self.assertEqual(
                self.client.get(
                    path, headers={"Authorization": "Bearer secret-key"}
                ).status_code,
                200,
                path,
            )
        self.assertEqual(
            self.client.post(f"/v1/synth/models/{MODEL}/load").status_code, 401
        )


class SettingsTests(ControlApiTestCase):
    DEFAULTS = {
        "default_temperature": 1.0,
        "default_top_p": 1.0,
        "default_top_k": 0,
        "default_reasoning_effort": "high",
        "default_max_output_tokens": 8192,
        "idle_unload_after_seconds": 900,
        "prompt_cache_slots": 2,
        "queue_capacity": 9,
    }

    def compiled_turns(self) -> list:
        """Capture the CompiledTurn of every generation run after this call."""
        backend = self.client.app.state.responses_service.backend
        turns: list = []
        original = backend.stream

        def instrumented(turn):
            turns.append(turn)
            return original(turn)

        backend.stream = instrumented
        self.addCleanup(setattr, backend, "stream", original)
        return turns

    def test_get_defaults_shape(self) -> None:
        response = self.client.get("/v1/synth/settings")
        self.assertEqual(response.status_code, 200)
        body = response.json()
        self.assertEqual(body["schema_version"], "1.0")
        self.assertEqual(body["settings"], self.DEFAULTS)
        self.assertTrue(body["source"]["path"].endswith("settings.toml"))
        datetime.fromisoformat(body["source"]["loaded_at"].replace("Z", "+00:00"))
        self.assertEqual(body["startup"]["default_model"], MODEL)
        self.assertEqual(body["startup"]["backend"], "mock")
        self.assertNotIn("api_key", json.dumps(body))

    def test_put_roundtrip_persists_and_changes_effective_behavior(self) -> None:
        turns = self.compiled_turns()
        response = self.client.put(
            "/v1/synth/settings",
            json={
                "default_temperature": 0.4,
                "default_reasoning_effort": "none",
                "default_max_output_tokens": 2048,
                "idle_unload_after_seconds": 60,
            },
        )
        self.assertEqual(response.status_code, 200, response.text)
        effective = response.json()["settings"]
        self.assertEqual(effective["default_temperature"], 0.4)
        self.assertEqual(effective["default_reasoning_effort"], "none")

        # Persisted: the TOML on disk re-parses to the effective values.
        import tomllib

        on_disk = tomllib.loads(
            (self.config.data_dir / "settings.toml").read_text(encoding="utf-8")
        )
        self.assertEqual(on_disk["default_temperature"], 0.4)
        self.assertEqual(on_disk["default_reasoning_effort"], "none")
        self.assertEqual(on_disk["idle_unload_after_seconds"], 60)

        # Effective on the Responses surface: absent fields take the new
        # defaults, explicit values still win.
        self.client.post(
            "/v1/responses", json={"model": MODEL, "input": "hi", "store": False}
        )
        self.assertEqual(turns[-1].temperature, 0.4)
        self.assertEqual(turns[-1].max_output_tokens, 2048)
        self.assertFalse(turns[-1].enable_thinking)
        self.client.post(
            "/v1/responses",
            json={
                "model": MODEL,
                "input": "hi",
                "store": False,
                "temperature": 1.3,
                "reasoning": {"effort": "high"},
            },
        )
        self.assertEqual(turns[-1].temperature, 1.3)
        self.assertTrue(turns[-1].enable_thinking)
        # And on the Chat surface, which shares the same defaults object.
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        self.assertEqual(turns[-1].temperature, 0.4)
        self.assertFalse(turns[-1].enable_thinking)

        # Advertised model defaults stay consistent with the setting.
        models = self.client.get("/v1/models").json()
        self.assertEqual(models["models"][0]["default_reasoning_level"], "none")
        # Status reflects the runtime idle deadline.
        self.assertEqual(self.status()["idle_unload_after_seconds"], 60)
        self.assertEqual(
            self.client.get("/health").json()["idleUnloadAfterSeconds"], 60
        )

    def test_put_survives_a_restart(self) -> None:
        self.client.put("/v1/synth/settings", json={"default_top_k": 40})
        restarted = TestClient(build_app(self.config))
        body = restarted.get("/v1/synth/settings").json()
        self.assertEqual(body["settings"]["default_top_k"], 40)

    def test_out_of_range_values_are_typed_and_atomic(self) -> None:
        for payload, field in (
            ({"default_temperature": 2.5}, "default_temperature"),
            ({"default_top_p": 0}, "default_top_p"),
            ({"default_top_k": -1}, "default_top_k"),
            ({"default_reasoning_effort": "max"}, "default_reasoning_effort"),
            ({"default_max_output_tokens": 40000}, "default_max_output_tokens"),
            ({"idle_unload_after_seconds": -1}, "idle_unload_after_seconds"),
            ({"prompt_cache_slots": 0}, "prompt_cache_slots"),
            ({"prompt_cache_slots": 3}, "prompt_cache_slots"),
            ({"queue_capacity": 33}, "queue_capacity"),
            ({"nonsense_knob": 1}, "nonsense_knob"),
        ):
            response = self.client.put("/v1/synth/settings", json=payload)
            body = self.assert_error(response, 400, "invalid_setting")
            self.assertEqual(body["error"]["details"]["field"], field)
            self.assertIn(field, body["error"]["message"])
        # A batch with one bad value applies nothing.
        response = self.client.put(
            "/v1/synth/settings",
            json={"default_temperature": 0.5, "default_top_p": 5},
        )
        self.assert_error(response, 400, "invalid_setting")
        settings = self.client.get("/v1/synth/settings").json()["settings"]
        self.assertEqual(settings["default_temperature"], 1.0)
        # Nothing was persisted either.
        self.assertFalse((self.config.data_dir / "settings.toml").exists())

    def test_unknown_toml_key_fails_startup_naming_the_key(self) -> None:
        temp = tempfile.TemporaryDirectory(prefix="synth-settings-")
        self.addCleanup(temp.cleanup)
        config = _config(Path(temp.name))
        (config.data_dir / "settings.toml").write_text(
            'defualt_temperature = 0.5\n', encoding="utf-8"
        )
        with self.assertRaises(Exception) as caught:
            build_app(config)
        self.assertIn("defualt_temperature", str(caught.exception))

    def test_toml_wins_over_the_env_fallback(self) -> None:
        from dataclasses import replace

        temp = tempfile.TemporaryDirectory(prefix="synth-settings-")
        self.addCleanup(temp.cleanup)
        # Simulate SYNTH_LAGUNA_IDLE_UNLOAD_SECONDS: config carries the env value.
        config = replace(_config(Path(temp.name)), idle_unload_after_seconds=123)
        client = TestClient(build_app(config))
        self.assertEqual(
            client.get("/v1/synth/settings").json()["settings"][
                "idle_unload_after_seconds"
            ],
            123,
        )
        # A settings file overrides the env-derived fallback.
        (config.data_dir / "settings.toml").write_text(
            "idle_unload_after_seconds = 456\n", encoding="utf-8"
        )
        client = TestClient(build_app(config))
        self.assertEqual(
            client.get("/v1/synth/settings").json()["settings"][
                "idle_unload_after_seconds"
            ],
            456,
        )

    def test_backend_bounds_are_applied_to_the_live_backend(self) -> None:
        backend = self.client.app.state.responses_service.backend
        # The mock backend has no cache/queue attributes; simulate the native
        # backend's knobs so the wiring is observable.
        backend._max_prompt_caches = 2
        backend._max_inflight_generations = 9
        response = self.client.put(
            "/v1/synth/settings",
            json={"prompt_cache_slots": 1, "queue_capacity": 3},
        )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(backend._max_prompt_caches, 1)
        self.assertEqual(backend._max_inflight_generations, 3)


class EventStreamTests(ControlApiTestCase):
    """Drive ASGI directly, as the telemetry stream test does: TestClient
    blocks on an open SSE stream, while raw ASGI lets the test read real
    frames and prove disconnect stops the generator."""

    def collect_frames(self, expected_frames: int) -> list[dict]:
        app = self.client.app
        messages: list[dict] = []

        def body_frames() -> int:
            return sum(
                1
                for message in messages
                if message["type"] == "http.response.body" and message.get("body")
            )

        request_sent = False

        async def receive() -> dict:
            # ASGI semantics: exactly one request message, then (eventually)
            # a disconnect. Repeating http.request trips the auth middleware's
            # cached-receive guard.
            nonlocal request_sent
            if not request_sent:
                request_sent = True
                return {"type": "http.request", "body": b"", "more_body": False}
            while body_frames() < expected_frames:
                await asyncio.sleep(0.01)
            return {"type": "http.disconnect"}

        async def send(message: dict) -> None:
            messages.append(message)

        scope = {
            "type": "http",
            "asgi": {"version": "3.0", "spec_version": "2.3"},
            "http_version": "1.1",
            "method": "GET",
            "scheme": "http",
            "path": "/v1/synth/events",
            "raw_path": b"/v1/synth/events",
            "query_string": b"",
            "root_path": "",
            "headers": [(b"host", b"127.0.0.1")],
            "client": ("127.0.0.1", 50000),
            "server": ("127.0.0.1", 7333),
        }

        async def drive() -> None:
            await app(scope, receive, send)

        asyncio.run(asyncio.wait_for(drive(), timeout=15))

        start = next(m for m in messages if m["type"] == "http.response.start")
        self.assertEqual(start["status"], 200)
        headers = {key.decode(): value.decode() for key, value in start["headers"]}
        self.assertTrue(headers["content-type"].startswith("text/event-stream"))

        raw = b"".join(
            m.get("body", b"") for m in messages if m["type"] == "http.response.body"
        ).decode()
        return [
            json.loads(line[len("data: ") :])
            for line in raw.splitlines()
            if line.startswith("data: ")
        ]

    def test_sse_replays_load_transitions_in_order(self) -> None:
        self.client.post(f"/v1/synth/models/{MODEL}/load")
        history = self.control.broker.history()
        self.assertTrue(history)
        frames = self.collect_frames(len(history))
        self.assertEqual(frames, history)
        for frame in frames:
            self.assertEqual(
                {"event", "operation_id", "state", "previous_state", "timestamp"}
                & set(frame),
                {"event", "operation_id", "state", "previous_state", "timestamp"},
            )

    def test_sse_replays_download_progress_in_order(self) -> None:
        # Fresh app without weights so a download is legal.
        temp = tempfile.TemporaryDirectory(prefix="synth-control-dl-")
        self.addCleanup(temp.cleanup)
        client = TestClient(build_app(_config(Path(temp.name))))
        control: SynthControl = client.app.state.synth_control
        control.system_memory_bytes = 64 * GIB
        control.free_disk_bytes = 64 * GIB
        control.downloader = FakeDownloader()
        job = client.post(f"/v1/synth/models/{MODEL}/download").json()
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if client.get(f"/v1/synth/downloads/{job['job_id']}").json()[
                "state"
            ] == "downloaded":
                break
            time.sleep(0.01)
        self.client = client
        self.control = control
        history = control.broker.history()
        frames = self.collect_frames(len(history))
        self.assertEqual(frames, history)
        download_events = [
            frame["event"] for frame in frames if frame["operation_id"] == job["job_id"]
        ]
        self.assertEqual(
            download_events,
            [
                "runtime.state_changed",
                "download.state_changed",
                "download.progress",
                "download.progress",
                "download.state_changed",
                "runtime.state_changed",
            ],
        )


if __name__ == "__main__":
    unittest.main()

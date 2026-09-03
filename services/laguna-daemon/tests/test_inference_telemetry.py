from __future__ import annotations

import asyncio
import json
import tempfile
import time
import unittest
from pathlib import Path

from fastapi.testclient import TestClient

from laguna_daemon.app import build_app
from laguna_daemon.config import LagunaConfig
from laguna_daemon.responses_api.telemetry import (
    GenerationTiming,
    InferenceTelemetry,
    percentile,
)


MODEL = "poolside/Laguna-XS-2.1-NVFP4-mlx"

#: Every field the Desktop monitor is built against. Drift here silently blanks
#: the panel, so the contract is asserted rather than assumed.
ACTIVE_FIELDS = {
    "generationId",
    "phase",
    "queuedAt",
    "startedAt",
    "firstTokenAt",
    "lastTokenAt",
    "promptTokens",
    "cachedTokens",
    "outputTokens",
    "cacheHitRatio",
    "prefillTokensPerSecond",
    "decodeTokensPerSecond",
    "elapsedMs",
}
ROLLING_FIELDS = {
    "requestsCompleted",
    "requestsFailed",
    "requestsCancelled",
    "lastFailureReason",
    "inputTokens",
    "outputTokens",
    "cachedTokens",
    "ttftP50Ms",
    "ttftP95Ms",
    "decodeTpsP50",
    "decodeTpsP95",
    "latencyP50Ms",
    "latencyP95Ms",
}

#: Substrings that must never reach telemetry under any circumstances.
SECRET_MARKERS = (
    "SUPERSECRET",
    "my private prompt",
    "Checked the request contract",
)


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


class GenerationTimingTests(unittest.TestCase):
    """Derived metrics must be measured or absent — never guessed."""

    def _timing(self, **overrides) -> GenerationTiming:
        base = {
            "generation_id": "gen_1",
            "queued_at": 100.0,
            "admitted_at": 100.5,
            "first_token_at": 102.5,
            "last_token_at": 106.5,
            "prompt_tokens": 1000,
            "cached_tokens": 200,
            "output_tokens": 41,
        }
        base.update(overrides)
        return GenerationTiming(**base)

    def test_ttft_measures_from_enqueue(self) -> None:
        """Queue wait is part of what a user experiences as time-to-first-token."""
        self.assertEqual(self._timing().ttft_ms(), 2500.0)

    def test_ttft_is_none_before_the_first_token(self) -> None:
        self.assertIsNone(self._timing(first_token_at=None).ttft_ms())

    def test_prefill_rate_excludes_cached_tokens(self) -> None:
        """Cached prefix tokens are not recomputed; counting them inflates the rate."""
        # (1000 - 200) computed tokens over 2.0s of prefill.
        self.assertEqual(self._timing().prefill_tokens_per_second(), 400.0)

    def test_prefill_rate_is_none_on_a_fully_cached_prompt(self) -> None:
        timing = self._timing(prompt_tokens=500, cached_tokens=500)
        self.assertIsNone(timing.prefill_tokens_per_second())

    def test_decode_rate_excludes_the_first_token(self) -> None:
        """The first token's cost belongs to prefill, not decode."""
        # (41 - 1) tokens over 4.0s.
        self.assertEqual(self._timing().decode_tokens_per_second(), 10.0)

    def test_decode_rate_prefers_the_mlx_source_measurement(self) -> None:
        timing = self._timing(measured_decode_tps=47.1254)
        self.assertEqual(timing.decode_tokens_per_second(), 47.125)

    def test_decode_rate_is_none_for_a_single_token(self) -> None:
        self.assertIsNone(self._timing(output_tokens=1).decode_tokens_per_second())

    def test_decode_rate_is_none_without_a_measured_token_interval(self) -> None:
        timing = self._timing(first_token_at=102.5, last_token_at=102.5)
        self.assertIsNone(timing.decode_tokens_per_second())

    def test_decode_rate_rejects_a_sub_resolution_interval(self) -> None:
        """A burst callback cannot truthfully establish per-token throughput."""
        timing = self._timing(first_token_at=102.5, last_token_at=102.501)
        self.assertIsNone(timing.decode_tokens_per_second())

    def test_decode_progress_tracks_source_tokens_without_display_text(self) -> None:
        """Structured/tool turns still expose speed before a text delta exists."""
        timing = self._timing(
            first_token_at=None,
            last_token_at=None,
            output_tokens=0,
            measured_decode_tps=None,
        )

        timing.record_decode_progress(
            sampled_at=103.0,
            output_tokens=2,
            prompt_tokens=1_200,
            cached_tokens=200,
            measured_decode_tps=47.1254,
        )

        self.assertEqual(timing.phase, "decode")
        self.assertEqual(timing.first_token_at, 103.0)
        self.assertEqual(timing.last_token_at, 103.0)
        self.assertEqual(timing.output_tokens, 2)
        self.assertEqual(timing.prompt_tokens, 1_200)
        self.assertEqual(timing.cached_tokens, 200)
        self.assertEqual(timing.decode_tokens_per_second(), 47.125)

    def test_decode_progress_preserves_per_token_policy_samples(self) -> None:
        timing = self._timing(
            first_token_at=None,
            last_token_at=None,
            output_tokens=0,
            measured_decode_tps=None,
            decode_latencies=[],
        )
        timing.record_decode_progress(
            sampled_at=103.0,
            output_tokens=1,
            prompt_tokens=100,
            cached_tokens=0,
            measured_decode_tps=None,
        )
        timing.record_decode_progress(
            sampled_at=103.2,
            output_tokens=5,
            prompt_tokens=100,
            cached_tokens=0,
            measured_decode_tps=None,
        )
        self.assertEqual(len(timing.decode_latencies), 4)
        for latency in timing.decode_latencies:
            self.assertAlmostEqual(latency, 0.05)

    def test_cache_hit_ratio(self) -> None:
        self.assertEqual(self._timing().cache_hit_ratio(), 0.2)
        self.assertEqual(self._timing(prompt_tokens=0).cache_hit_ratio(), 0.0)

    def test_zero_duration_never_divides_by_zero(self) -> None:
        timing = self._timing(admitted_at=102.5, first_token_at=102.5)
        self.assertIsNone(timing.prefill_tokens_per_second())


class PercentileTests(unittest.TestCase):
    def test_empty_series_has_no_percentile(self) -> None:
        self.assertIsNone(percentile([], 0.5))

    def test_nearest_rank(self) -> None:
        values = [1.0, 2.0, 3.0, 4.0, 5.0]
        self.assertEqual(percentile(values, 0.0), 1.0)
        self.assertEqual(percentile(values, 0.5), 3.0)
        self.assertEqual(percentile(values, 1.0), 5.0)

    def test_single_sample(self) -> None:
        self.assertEqual(percentile([7.0], 0.95), 7.0)


class RollingAggregateTests(unittest.TestCase):
    def test_counters_and_percentiles(self) -> None:
        telemetry = InferenceTelemetry()
        for index in range(4):
            timing = GenerationTiming(
                generation_id=f"gen_{index}",
                queued_at=0.0,
                admitted_at=0.0,
                first_token_at=float(index + 1),
                last_token_at=float(index + 2),
                prompt_tokens=10,
                cached_tokens=2,
                output_tokens=11,
            )
            telemetry.record_completed(timing, latency_ms=float(index + 1) * 100)
        telemetry.record_failed()
        telemetry.record_cancelled()

        snapshot = telemetry.snapshot()
        self.assertEqual(snapshot["requestsCompleted"], 4)
        self.assertEqual(snapshot["requestsFailed"], 1)
        self.assertEqual(snapshot["requestsCancelled"], 1)
        self.assertEqual(snapshot["inputTokens"], 40)
        self.assertEqual(snapshot["outputTokens"], 44)
        self.assertEqual(snapshot["cachedTokens"], 8)
        self.assertIsNotNone(snapshot["ttftP50Ms"])
        self.assertIsNotNone(snapshot["latencyP95Ms"])
        self.assertTrue(snapshot["resetsOnRestart"])

    def test_unmeasured_percentiles_stay_null(self) -> None:
        """No requests means no data — not zero."""
        snapshot = InferenceTelemetry().snapshot()
        for key in ("ttftP50Ms", "decodeTpsP50", "latencyP95Ms"):
            self.assertIsNone(snapshot[key])

    def test_failures_do_not_pollute_token_totals(self) -> None:
        telemetry = InferenceTelemetry()
        telemetry.record_failed(RuntimeError("The model selected unknown tool 'container_list' with argument keys []."))
        telemetry.record_cancelled()
        snapshot = telemetry.snapshot()
        self.assertEqual(snapshot["inputTokens"], 0)
        self.assertEqual(snapshot["outputTokens"], 0)
        self.assertEqual(snapshot["requestsCompleted"], 0)
        self.assertEqual(snapshot["lastFailureReason"], "Unknown tool: container_list")


class InferenceEndpointTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="synth-telemetry-")
        self.addCleanup(self.temp.cleanup)
        self.client = TestClient(build_app(_config(Path(self.temp.name))))

    def snapshot(self) -> dict:
        response = self.client.get("/v1/synth/inference")
        self.assertEqual(response.status_code, 200)
        return response.json()

    def test_idle_snapshot_matches_the_contract(self) -> None:
        body = self.snapshot()
        self.assertEqual(body["model"], MODEL)
        self.assertFalse(body["resident"])
        self.assertIsNone(body["active"])
        self.assertEqual(set(body["rolling"]) & ROLLING_FIELDS, ROLLING_FIELDS)
        # Apple GPU counters are unavailable; the field says so rather than
        # reporting a number derived from process CPU.
        self.assertIsNone(body["gpuUtilization"])

    def test_completed_turns_move_the_rolling_counters(self) -> None:
        before = self.snapshot()["rolling"]["requestsCompleted"]
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        after = self.snapshot()["rolling"]
        self.assertEqual(after["requestsCompleted"], before + 1)
        self.assertGreater(after["inputTokens"], 0)
        self.assertGreater(after["outputTokens"], 0)
        self.assertIsNotNone(after["latencyP50Ms"])

    def test_both_surfaces_feed_one_set_of_aggregates(self) -> None:
        """Chat and Responses share a runner, so they must share counters."""
        before = self.snapshot()["rolling"]["requestsCompleted"]
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        self.client.post(
            "/v1/responses", json={"model": MODEL, "input": "hi", "store": False}
        )
        after = self.snapshot()["rolling"]["requestsCompleted"]
        self.assertEqual(after, before + 2)

    def test_residency_appears_after_first_use_and_clears_on_unload(self) -> None:
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        self.assertTrue(self.snapshot()["resident"])

        unload = self.client.post("/v1/synth/model/unload")
        self.assertEqual(unload.status_code, 200)
        self.assertTrue(unload.json()["unloaded"])

        after = self.snapshot()
        self.assertFalse(after["resident"])
        # Eviction releases weights but must not erase completed work history.
        self.assertGreater(after["rolling"]["requestsCompleted"], 0)

    def test_active_generation_is_reported_with_the_contract_fields(self) -> None:
        """Snapshot the panel's live view from inside a running generation."""
        service = self.client.app.state.responses_service
        backend = service.backend
        seen: list[dict] = []
        original = backend.stream

        def instrumented(turn):
            async def wrapper():
                async for event in original(turn):
                    if not seen:
                        seen.append(service.inference_snapshot())
                    yield event

            return wrapper()

        backend.stream = instrumented
        self.addCleanup(setattr, backend, "stream", original)
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        self.assertTrue(seen, "no snapshot was taken during generation")
        active = seen[0]["active"]
        self.assertIsNotNone(active)
        self.assertEqual(set(active) & ACTIVE_FIELDS, ACTIVE_FIELDS)
        self.assertIn(
            active["phase"],
            {"queued", "loading", "compiling", "prefill", "decode", "complete"},
        )
        self.assertEqual(seen[0]["queueDepth"], 1)
        self.assertGreaterEqual(seen[0]["queueCapacity"], 1)

    def test_generation_id_is_redacted(self) -> None:
        service = self.client.app.state.responses_service
        backend = service.backend
        seen: list[dict] = []
        original = backend.stream

        def instrumented(turn):
            async def wrapper():
                async for event in original(turn):
                    if not seen:
                        seen.append((service.inference_snapshot(), turn.generation_id))
                    yield event

            return wrapper()

        backend.stream = instrumented
        self.addCleanup(setattr, backend, "stream", original)
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        snapshot, generation_id = seen[0]
        reported = snapshot["active"]["generationId"]
        self.assertTrue(reported.startswith("sha256:"))
        self.assertNotIn(generation_id, reported)

    def test_no_prompt_or_reasoning_text_can_appear(self) -> None:
        self.client.post(
            "/v1/chat/completions",
            json={
                "model": MODEL,
                "messages": [
                    {"role": "system", "content": "key is SUPERSECRET"},
                    {"role": "user", "content": "my private prompt"},
                ],
            },
        )
        serialized = json.dumps(self.snapshot())
        for marker in SECRET_MARKERS:
            self.assertNotIn(marker, serialized)
        self.assertNotIn("SUPERSECRET", self.client.get("/metrics").text)

    def test_unload_is_refused_while_a_generation_is_in_flight(self) -> None:
        service = self.client.app.state.responses_service
        backend = service.backend
        outcomes: list = []
        original = backend.stream

        def instrumented(turn):
            async def wrapper():
                async for event in original(turn):
                    if not outcomes:
                        outcomes.append(self.client.post("/v1/synth/model/unload"))
                    yield event

            return wrapper()

        backend.stream = instrumented
        self.addCleanup(setattr, backend, "stream", original)
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        self.assertEqual(outcomes[0].status_code, 409)
        self.assertEqual(
            outcomes[0].json()["error"]["code"], "generation_in_flight"
        )

    def test_metrics_endpoint_is_prometheus_text(self) -> None:
        self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        response = self.client.get("/metrics")
        self.assertEqual(response.status_code, 200)
        self.assertTrue(response.headers["content-type"].startswith("text/plain"))
        body = response.text
        self.assertIn("# TYPE laguna_requests_total counter", body)
        self.assertIn('laguna_requests_total{outcome="completed"}', body)
        self.assertIn("laguna_queue_capacity", body)

    def test_metrics_omits_unmeasured_percentiles(self) -> None:
        """An absent measurement must not be exported as zero."""
        body = self.client.get("/metrics").text
        self.assertNotIn("laguna_ttft_ms_p50", body)

    def test_stream_endpoint_emits_snapshots_and_stops_on_disconnect(self) -> None:
        """Drive the ASGI app directly.

        `TestClient` runs the app in a portal and blocks on close until the body
        iterator finishes, which an open SSE stream never does. Speaking ASGI
        lets the test both read a real frame and assert that reporting a client
        disconnect actually ends the generator.
        """
        app = self.client.app
        messages: list[dict] = []
        disconnected = False

        async def receive() -> dict:
            nonlocal disconnected
            if messages:
                # One frame has been produced; report the client going away.
                disconnected = True
                return {"type": "http.disconnect"}
            return {"type": "http.request", "body": b"", "more_body": False}

        async def send(message: dict) -> None:
            messages.append(message)

        scope = {
            "type": "http",
            "asgi": {"version": "3.0", "spec_version": "2.3"},
            "http_version": "1.1",
            "method": "GET",
            "scheme": "http",
            "path": "/v1/synth/inference/stream",
            "raw_path": b"/v1/synth/inference/stream",
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

        frames = b"".join(
            m.get("body", b"") for m in messages if m["type"] == "http.response.body"
        ).decode()
        self.assertIn("event: inference", frames)
        payload = json.loads(
            next(
                line[len("data: ") :]
                for line in frames.splitlines()
                if line.startswith("data: ")
            )
        )
        self.assertEqual(set(payload["rolling"]) & ROLLING_FIELDS, ROLLING_FIELDS)
        self.assertIn("resident", payload)
        # The generator must actually stop rather than poll a departed client.
        self.assertTrue(disconnected)


if __name__ == "__main__":
    unittest.main()

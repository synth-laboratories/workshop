#!/usr/bin/env python3
"""Standalone client-side benchmark for a live Laguna daemon.

Not part of the deterministic suite: the filename does not match test
discovery, and the embedded smoke test skips itself unless
SYNTH_LAGUNA_BENCH_BASE_URL is set.

Drives N concurrent streaming requests (chat or responses surface) against a
base URL and reports client-observed TTFT, decode tokens/second, and request
latency percentiles alongside the daemon's own /v1/synth/metrics rolling
aggregates taken before and after the run. Never point it at a daemon whose
numbers someone else is currently measuring.

    .venv/bin/python tests/performance/benchmark_mlx.py \
        --base-url http://127.0.0.1:7340 --api-key ... \
        --concurrency 4 --requests 16 --surface chat \
        --output /tmp/bench.json
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
import time
import unittest
from typing import Any

import httpx


def percentile(values: list[float], fraction: float) -> float | None:
    """Nearest-rank, mirroring laguna_daemon.responses_api.telemetry."""
    if not values:
        return None
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, round(fraction * (len(ordered) - 1))))
    return round(ordered[index], 3)


def _one_stream(
    client: httpx.Client, surface: str, model: str, prompt: str, max_tokens: int
) -> dict[str, Any]:
    """Run one streaming request; every number is measured on this client."""
    if surface == "chat":
        path = "/v1/chat/completions"
        body: dict[str, Any] = {
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "stream": True,
            "max_tokens": max_tokens,
        }
    else:
        path = "/v1/responses"
        body = {
            "model": model,
            "input": prompt,
            "stream": True,
            "store": False,
            "max_output_tokens": max_tokens,
        }
    started = time.monotonic()
    first_token_at: float | None = None
    last_token_at: float | None = None
    chunks = 0
    output_tokens: int | None = None
    with client.stream("POST", path, json=body) as response:
        response.raise_for_status()
        for line in response.iter_lines():
            if not line.startswith("data: ") or line == "data: [DONE]":
                continue
            now = time.monotonic()
            event = json.loads(line[len("data: ") :])
            if surface == "chat":
                delta = (event.get("choices") or [{}])[0].get("delta") or {}
                if delta.get("content") or delta.get("reasoning_content"):
                    chunks += 1
                    first_token_at = first_token_at or now
                    last_token_at = now
                usage = event.get("usage")
                if isinstance(usage, dict) and usage.get("completion_tokens"):
                    output_tokens = int(usage["completion_tokens"])
            else:
                kind = event.get("type") or ""
                if kind.endswith(".delta"):
                    chunks += 1
                    first_token_at = first_token_at or now
                    last_token_at = now
                if kind == "response.completed":
                    usage = (event.get("response") or {}).get("usage") or {}
                    if usage.get("output_tokens"):
                        output_tokens = int(usage["output_tokens"])
    completed = time.monotonic()
    ttft_ms = (
        round((first_token_at - started) * 1000, 3)
        if first_token_at is not None
        else None
    )
    decode_tps = None
    if (
        output_tokens
        and output_tokens > 1
        and first_token_at is not None
        and last_token_at is not None
        and last_token_at - first_token_at > 0.01
    ):
        decode_tps = round((output_tokens - 1) / (last_token_at - first_token_at), 3)
    return {
        "latency_ms": round((completed - started) * 1000, 3),
        "ttft_ms": ttft_ms,
        "decode_tps": decode_tps,
        "output_tokens": output_tokens,
        "stream_chunks": chunks,
    }


def run_benchmark(
    *,
    base_url: str,
    api_key: str | None,
    surface: str,
    concurrency: int,
    requests: int,
    max_tokens: int,
    prompt: str,
    timeout: float,
) -> dict[str, Any]:
    headers = {"Authorization": f"Bearer {api_key}"} if api_key else {}

    def daemon_metrics(client: httpx.Client) -> dict[str, Any] | None:
        response = client.get("/v1/synth/metrics")
        return response.json() if response.status_code == 200 else None

    with httpx.Client(base_url=base_url, headers=headers, timeout=timeout) as probe:
        model = probe.get("/health").json()["defaultModel"]
        before = daemon_metrics(probe)

    results: list[dict[str, Any]] = []
    errors: list[str] = []
    started = time.monotonic()

    def worker(_: int) -> None:
        # One client per worker: connection reuse inside a worker, isolation
        # across workers.
        with httpx.Client(
            base_url=base_url, headers=headers, timeout=timeout
        ) as client:
            try:
                results.append(
                    _one_stream(client, surface, model, prompt, max_tokens)
                )
            except (httpx.HTTPError, json.JSONDecodeError) as error:
                errors.append(str(error))

    with concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as pool:
        list(pool.map(worker, range(requests)))
    wall_seconds = round(time.monotonic() - started, 3)

    with httpx.Client(base_url=base_url, headers=headers, timeout=timeout) as probe:
        after = daemon_metrics(probe)

    latency = [entry["latency_ms"] for entry in results]
    ttft = [entry["ttft_ms"] for entry in results if entry["ttft_ms"] is not None]
    decode = [
        entry["decode_tps"] for entry in results if entry["decode_tps"] is not None
    ]
    return {
        "config": {
            "base_url": base_url,
            "surface": surface,
            "concurrency": concurrency,
            "requests": requests,
            "max_tokens": max_tokens,
            "model": model,
        },
        "wall_seconds": wall_seconds,
        "completed": len(results),
        "failed": len(errors),
        "errors": errors[:10],
        "client": {
            "latency_ms_p50": percentile(latency, 0.50),
            "latency_ms_p95": percentile(latency, 0.95),
            "ttft_ms_p50": percentile(ttft, 0.50),
            "ttft_ms_p95": percentile(ttft, 0.95),
            "decode_tps_p50": percentile(decode, 0.50),
            "decode_tps_p95": percentile(decode, 0.95),
        },
        "daemon_metrics_before": before,
        "daemon_metrics_after": after,
        "samples": results,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--api-key", default=os.getenv("SYNTH_LAGUNA_API_KEY"))
    parser.add_argument("--surface", choices=("chat", "responses"), default="chat")
    parser.add_argument("--concurrency", type=int, default=2)
    parser.add_argument("--requests", type=int, default=8)
    parser.add_argument("--max-tokens", type=int, default=256)
    parser.add_argument(
        "--prompt", default="Summarize what a sidecar daemon is in two sentences."
    )
    parser.add_argument("--timeout", type=float, default=600.0)
    parser.add_argument("--output", default=None, help="write the JSON report here")
    arguments = parser.parse_args()

    report = run_benchmark(
        base_url=arguments.base_url.rstrip("/"),
        api_key=arguments.api_key,
        surface=arguments.surface,
        concurrency=arguments.concurrency,
        requests=arguments.requests,
        max_tokens=arguments.max_tokens,
        prompt=arguments.prompt,
        timeout=arguments.timeout,
    )
    rendered = json.dumps(report, indent=2, sort_keys=True)
    print(rendered)
    if arguments.output:
        with open(arguments.output, "w", encoding="utf-8") as handle:
            handle.write(rendered + "\n")
    return 0 if report["failed"] == 0 else 1


class BenchmarkSmokeTest(unittest.TestCase):
    """Env-gated smoke run so `unittest` invocations skip rather than fail."""

    @unittest.skipUnless(
        os.getenv("SYNTH_LAGUNA_BENCH_BASE_URL"),
        "set SYNTH_LAGUNA_BENCH_BASE_URL to run the benchmark smoke test",
    )
    def test_smoke_two_requests(self) -> None:
        report = run_benchmark(
            base_url=os.environ["SYNTH_LAGUNA_BENCH_BASE_URL"].rstrip("/"),
            api_key=os.getenv("SYNTH_LAGUNA_BENCH_API_KEY")
            or os.getenv("SYNTH_LAGUNA_API_KEY"),
            surface="chat",
            concurrency=2,
            requests=2,
            max_tokens=64,
            prompt="Reply with the word pong.",
            timeout=600.0,
        )
        self.assertEqual(report["failed"], 0, report["errors"])
        self.assertIsNotNone(report["client"]["latency_ms_p50"])


if __name__ == "__main__":
    raise SystemExit(main())

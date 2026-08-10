#!/usr/bin/env python3
"""Bounded reliability and performance soak for a live native MLX daemon."""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import statistics
import tempfile
import time
from pathlib import Path
from typing import Any

import httpx


MODEL = "poolside/Laguna-XS-2.1-NVFP4-mlx"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://127.0.0.1:7333")
    parser.add_argument("--model", default=MODEL)
    parser.add_argument("--iterations", type=int, default=12)
    parser.add_argument("--concurrency", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=600)
    parser.add_argument("--min-decode-tps", type=float, default=20.0)
    parser.add_argument("--max-resident-growth-mib", type=float, default=1024.0)
    parser.add_argument("--api-key-env", default="SYNTH_LAGUNA_API_KEY")
    parser.add_argument(
        "--report",
        default=str(Path(tempfile.gettempdir()) / "laguna-soak-report.json"),
    )
    return parser.parse_args()


def percentile(values: list[float], quantile: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, int(len(ordered) * quantile + 0.999) - 1))
    return ordered[index]


async def inference(client: httpx.AsyncClient) -> dict[str, Any]:
    response = await client.get("/v1/synth/inference")
    response.raise_for_status()
    return response.json()


async def wait_for_idle(client: httpx.AsyncClient, timeout: float = 30) -> dict[str, Any]:
    deadline = time.monotonic() + timeout
    last: dict[str, Any] = {}
    while time.monotonic() < deadline:
        last = await inference(client)
        if (
            last.get("active") is None
            and last.get("queueDepth") == 0
        ):
            return last
        await asyncio.sleep(0.1)
    raise RuntimeError(f"generation slot did not recover: {last}")


async def chat_turn(
    client: httpx.AsyncClient, model: str, index: int
) -> dict[str, Any]:
    started = time.monotonic()
    response = await client.post(
        "/v1/chat/completions",
        json={
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": f"Reply in one short sentence: soak turn {index} is healthy.",
                }
            ],
            "reasoning_effort": "none",
            "temperature": 0.7,
            "top_p": 0.95,
            "max_tokens": 128,
            "prompt_cache_key": f"soak-chat-{index % 2}",
        },
    )
    elapsed = time.monotonic() - started
    response.raise_for_status()
    body = response.json()
    content = body["choices"][0]["message"].get("content") or ""
    if not content.strip():
        raise RuntimeError(f"chat turn {index} returned no assistant content")
    return {
        "surface": "chat",
        "index": index,
        "latencySeconds": round(elapsed, 3),
        "outputTokens": int(body.get("usage", {}).get("completion_tokens") or 0),
    }


async def responses_turn(
    client: httpx.AsyncClient, model: str, index: int
) -> dict[str, Any]:
    started = time.monotonic()
    response = await client.post(
        "/v1/responses",
        json={
            "model": model,
            "input": f"Reply in one short sentence: responses soak turn {index} is healthy.",
            "reasoning": {"effort": "none"},
            "temperature": 0.7,
            "top_p": 0.95,
            "max_output_tokens": 128,
            "prompt_cache_key": f"soak-responses-{index % 2}",
            "store": False,
        },
    )
    elapsed = time.monotonic() - started
    response.raise_for_status()
    body = response.json()
    content = "".join(
        str(part.get("text") or "")
        for item in body.get("output") or []
        if item.get("type") == "message"
        for part in item.get("content") or []
        if part.get("type") == "output_text"
    )
    if not content.strip():
        raise RuntimeError(f"Responses turn {index} returned no assistant content")
    return {
        "surface": "responses",
        "index": index,
        "latencySeconds": round(elapsed, 3),
        "outputTokens": int(body.get("usage", {}).get("output_tokens") or 0),
    }


async def abandon_stream(client: httpx.AsyncClient, model: str) -> None:
    async with client.stream(
        "POST",
        "/v1/chat/completions",
        json={
            "model": model,
            "messages": [{"role": "user", "content": "Write a very long essay."}],
            "stream": True,
            "max_tokens": 4096,
        },
    ) as response:
        response.raise_for_status()
        async for line in response.aiter_lines():
            if line.startswith("data: ") and line != "data: [DONE]":
                return
    raise RuntimeError("disconnect probe received no streamed event")


async def run(args: argparse.Namespace) -> dict[str, Any]:
    if args.iterations < 2:
        raise ValueError("--iterations must be at least 2")
    if not 1 <= args.concurrency <= 8:
        raise ValueError("--concurrency must be between 1 and 8")
    api_key = os.environ.get(args.api_key_env, "").strip()
    headers = {"Authorization": f"Bearer {api_key}"} if api_key else {}
    timeout = httpx.Timeout(args.timeout)
    async with httpx.AsyncClient(
        base_url=args.base_url.rstrip("/"), headers=headers, timeout=timeout
    ) as client:
        health = await client.get("/health")
        health.raise_for_status()

        # Warm model load and allocator state before taking the memory baseline.
        await chat_turn(client, args.model, -1)
        before = await wait_for_idle(client)

        completed: list[dict[str, Any]] = []
        for start in range(0, args.iterations, args.concurrency):
            batch = []
            for index in range(start, min(start + args.concurrency, args.iterations)):
                turn = chat_turn if index % 2 == 0 else responses_turn
                batch.append(turn(client, args.model, index))
            completed.extend(await asyncio.gather(*batch))

        await abandon_stream(client, args.model)
        recovered = await wait_for_idle(client)
        follow_up = await chat_turn(client, args.model, args.iterations)
        after = await wait_for_idle(client)

    latencies = [float(item["latencySeconds"]) for item in completed]
    before_rolling = before["rolling"]
    after_rolling = after["rolling"]
    if after_rolling["requestsFailed"] != before_rolling["requestsFailed"]:
        raise RuntimeError("the soak added a failed daemon request")
    expected_completed = args.iterations + 1
    completed_delta = (
        int(after_rolling["requestsCompleted"])
        - int(before_rolling["requestsCompleted"])
    )
    if completed_delta < expected_completed:
        raise RuntimeError(
            f"only {completed_delta}/{expected_completed} expected turns completed"
        )
    cancelled_delta = int(after_rolling["requestsCancelled"]) - int(
        before_rolling["requestsCancelled"]
    )
    if cancelled_delta < 1:
        raise RuntimeError("the abandoned stream was not recorded as cancelled")
    decode_p50 = after_rolling.get("decodeTpsP50")
    if decode_p50 is None or float(decode_p50) < args.min_decode_tps:
        raise RuntimeError(
            f"decode throughput {decode_p50!r} is below {args.min_decode_tps:.1f} tps"
        )
    resident_growth = int(after.get("residentBytes") or 0) - int(
        before.get("residentBytes") or 0
    )
    max_growth = int(args.max_resident_growth_mib * 1024 * 1024)
    if resident_growth > max_growth:
        raise RuntimeError(
            f"resident memory grew by {resident_growth} bytes; limit is {max_growth}"
        )

    report = {
        "passed": True,
        "baseUrl": args.base_url,
        "model": args.model,
        "iterations": args.iterations,
        "concurrency": args.concurrency,
        "latencySeconds": {
            "median": round(statistics.median(latencies), 3),
            "p95": round(percentile(latencies, 0.95) or 0, 3),
            "max": round(max(latencies), 3),
        },
        "daemon": {
            "decodeTpsP50": decode_p50,
            "decodeTpsP95": after_rolling.get("decodeTpsP95"),
            "requestsCompletedDelta": completed_delta,
            "requestsCancelledDelta": cancelled_delta,
            "residentBytesBefore": int(before.get("residentBytes") or 0),
            "residentBytesAfter": int(after.get("residentBytes") or 0),
            "residentBytesGrowth": resident_growth,
            "slotRecoveredAfterDisconnect": (
                recovered.get("active") is None and recovered.get("queueDepth") == 0
            ),
        },
        "turns": completed,
        "followUp": follow_up,
    }
    return report


def main() -> int:
    args = parse_args()
    report = asyncio.run(run(args))
    rendered = json.dumps(report, indent=2, sort_keys=True)
    print(rendered)
    path = Path(args.report).expanduser()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(rendered + "\n", encoding="utf-8")
    print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

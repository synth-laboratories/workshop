#!/usr/bin/env python3
"""Prefill-length sweep against the Laguna daemon: cold and warm per bucket.

For each target prefill size, one cold request (fresh prompt_cache_key,
populates the cache) and one warm request (same key, same prefix, new tail).
Reports client TTFT, derived prefill tok/s, cached tokens, decode tps.
Sequential — no queue contention pollutes the numbers.
"""
import json
import os
import time
import urllib.request

KEY = open(os.path.expanduser("~/.synth-desktop/laguna/api_key")).read().strip()
BASE = "http://127.0.0.1:7333/v1"
MODEL = "poolside/Laguna-XS-2.1-NVFP4-mlx"
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "prefill_sweep_results.json")

# ~64 chars, ~15 tokens per line of filler; sized empirically below.
FILLER = "The reviewer checked module {i} and found the invariant held under load.\n"
BUCKETS = [1_000, 5_000, 10_000, 25_000, 50_000, 150_000]


def build_prompt(target_tokens: int) -> str:
    # ~15.5 tokens/line measured for this tokenizer; overshoot slightly and
    # report the daemon's own input_tokens as the truth.
    lines = int(target_tokens / 15.5)
    return "Context log:\n" + "".join(FILLER.format(i=i) for i in range(lines))


def stream_request(prompt: str, cache_key: str, question: str):
    body = {
        "model": MODEL,
        "input": f"{prompt}\n\nQuestion: {question} Reply with one word.",
        "reasoning": {"effort": "none"},
        "stream": True,
        "store": False,
        "temperature": 1.0,
        "top_p": 1.0,
        "top_k": 20,
        "max_output_tokens": 64,
        "prompt_cache_key": cache_key,
    }
    req = urllib.request.Request(
        f"{BASE}/responses", data=json.dumps(body).encode(),
        headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json",
                 "Accept": "text/event-stream"})
    t0 = time.time()
    first_delta = None
    last = None
    usage = None
    resp = urllib.request.urlopen(req, timeout=3600)
    for raw in resp:
        line = raw.decode("utf-8", "replace").strip()
        if not line.startswith("data:"):
            continue
        payload = line[5:].strip()
        if payload == "[DONE]":
            continue
        event = json.loads(payload)
        kind = event.get("type")
        if kind == "response.output_text.delta" and first_delta is None:
            first_delta = time.time()
        if kind in {"response.completed", "response.incomplete"}:
            last = time.time()
            usage = (event.get("response") or {}).get("usage") or {}
    ttft = (first_delta or last) - t0
    total = (last or time.time()) - t0
    input_tokens = usage.get("input_tokens", 0)
    cached = (usage.get("input_tokens_details") or {}).get("cached_tokens", 0)
    output_tokens = usage.get("output_tokens", 0)
    fresh = max(1, input_tokens - cached)
    decode_s = max(0.001, total - ttft)
    return {
        "input_tokens": input_tokens,
        "cached_tokens": cached,
        "output_tokens": output_tokens,
        "ttft_s": round(ttft, 3),
        "prefill_tps": round(fresh / ttft, 1) if ttft > 0 else None,
        "decode_tps": round(output_tokens / decode_s, 1),
        "total_s": round(total, 3),
    }


def main() -> None:
    results = []
    for target in BUCKETS:
        prompt = build_prompt(target)
        key = f"sweep-{target}-{int(time.time())}"
        entry = {"target_tokens": target}
        try:
            entry["cold"] = stream_request(prompt, key, "How many modules were checked?")
            entry["warm"] = stream_request(prompt, key, "Did the invariant hold?")
            entry["warm_speedup"] = (
                round(entry["cold"]["ttft_s"] / entry["warm"]["ttft_s"], 2)
                if entry["warm"]["ttft_s"] else None
            )
        except Exception as error:  # 503/timeout at large contexts is a finding
            entry["error"] = f"{type(error).__name__}: {error}"[:300]
        results.append(entry)
        print(json.dumps(entry), flush=True)
        json.dump(results, open(OUT, "w"), indent=2)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()

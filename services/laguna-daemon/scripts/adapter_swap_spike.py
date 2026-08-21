#!/usr/bin/env python3
"""Measure whether a LoRA can be attached and detached without reloading Laguna.

`NativeMlxBackend.set_adapter` currently drops resident weights, so switching
policies costs a full cold load. If the wrappers can be swapped in place, a
policy switch is a per-request pin instead — which is the difference between a
picker that feels instant and one that stalls for ten seconds.

The spike answers three questions with measurements rather than assertions
about MLX internals:

1. Does attach/detach work at all on an NVFP4 checkpoint?
2. What does an attached adapter cost per decoded token, by rank?
3. Is the adapter actually bound? `load_weights(strict=False)` ignores keys it
   does not recognise, so a neutral adapter that silently failed to attach
   looks exactly like a successful one. The probe fixture must move the output.

Generation goes through the same `mlx_vlm.stream_generate` path the daemon
uses, so the throughput numbers are comparable with `.live-report.json`.
"""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import sys
from statistics import median
import time
from pathlib import Path

DEFAULT_MODEL = Path.home() / ".synth-desktop/models/poolside/Laguna-XS-2.1-NVFP4-mlx"
DEFAULT_FIXTURES = Path.home() / ".synth-desktop/laguna/test-adapters"
DEFAULT_REPORT = Path.home() / ".synth-desktop/laguna/adapter-swap-report.json"
PROMPT = "Write a Python function that merges two sorted lists. Explain briefly."


def say(message: str) -> None:
    """Unbuffered: a redirected run should stream rather than dump at exit."""
    print(message, flush=True)


def admission_check(model_dir: Path) -> None:
    """Refuse to load when the Mac cannot hold the weights.

    Reuses the daemon's own thresholds so the spike cannot be more permissive
    than production.
    """
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from laguna_daemon.responses_api.backends.mlx import (  # noqa: E402
        _available_memory_bytes,
        _required_available_memory_bytes,
    )

    available = _available_memory_bytes()
    required = _required_available_memory_bytes(model_dir)
    gib = 1024**3
    if available is not None and available < required:
        raise SystemExit(
            f"refusing to load: {available/gib:.1f} GiB available, "
            f"{required/gib:.1f} GiB required. Free memory or unload the "
            f"resident daemon first."
        )
    say(f"admission: {(available or 0)/gib:.1f} GiB available, {required/gib:.1f} GiB required")


def lora_target(model, sample_key: str):
    """Find the module the checkpoint's key paths are relative to.

    mlx-vlm nests a text checkpoint differently depending on how it was
    registered: Laguna is `Model.language_model.model.layers[...]`, while the
    text-only fallback is `Model.language_model._model`. Rather than assume a
    shape that a version bump can change, try the candidates and keep the one
    where a real key from this adapter resolves.
    """
    from mlx_vlm.trainer.utils import get_module_by_name

    candidates = [("<root>", model)]
    language_model = getattr(model, "language_model", None)
    if language_model is not None:
        candidates.append(("language_model", language_model))
        inner = getattr(language_model, "_model", None)
        if inner is not None:
            candidates.append(("language_model._model", inner))
    for label, candidate in candidates:
        try:
            get_module_by_name(candidate, sample_key)
        except (AttributeError, KeyError, IndexError, TypeError):
            continue
        say(f"lora target: {label}")
        return candidate
    raise SystemExit(
        f"no module tree resolves {sample_key!r}; tried {[l for l, _ in candidates]}"
    )


def attach(model, adapter: Path) -> tuple[dict, float, int]:
    import mlx.core as mx
    from mlx_vlm.trainer.utils import _to_lora, get_module_by_name, set_module_by_name

    config = json.loads((adapter / "adapter_config.json").read_text())
    params = dict(config["lora_parameters"])
    keys = params["keys"]
    target = lora_target(model, keys[0])
    start = time.perf_counter()
    originals = {}
    for name in keys:
        module = get_module_by_name(target, name)
        originals[name] = module
        set_module_by_name(target, name, _to_lora(module, params))
    target.load_weights(str(adapter / "adapters.safetensors"), strict=False)
    mx.eval(target.parameters())
    elapsed = (time.perf_counter() - start) * 1000

    # `strict=False` would swallow a key mismatch, so confirm one tensor round
    # trips from the file into the live module.
    saved = mx.load(str(adapter / "adapters.safetensors"))
    probe_key = keys[0]
    live = get_module_by_name(target, probe_key)
    bound = bool(mx.array_equal(live.lora_b, saved[f"{probe_key}.lora_b"]))
    if not bound:
        raise SystemExit(f"adapter did not bind: {probe_key}.lora_b differs from the file")
    return originals, elapsed, len(keys)


def detach(model, originals: dict) -> float:
    import mlx.core as mx
    from mlx_vlm.trainer.utils import set_module_by_name

    target = lora_target(model, next(iter(originals)))
    start = time.perf_counter()
    for name, module in originals.items():
        set_module_by_name(target, name, module)
    mx.eval(target.parameters())
    return (time.perf_counter() - start) * 1000


def measure(model, tokenizer, max_tokens: int) -> dict:
    from mlx_vlm import stream_generate
    from mlx_vlm.sample_utils import make_sampler

    sampler = make_sampler(temp=0.0)
    text = ""
    tps = 0.0
    tokens = 0
    start = time.perf_counter()
    for response in stream_generate(
        model,
        tokenizer,
        prompt=PROMPT,
        max_tokens=max_tokens,
        sampler=sampler,
        verbose=False,
    ):
        text += response.text or ""
        tokens = max(tokens, int(response.generation_tokens or 0))
        if response.generation_tps:
            tps = float(response.generation_tps)
    return {
        "decode_tps": round(tps, 3) if tps else None,
        "tokens": tokens,
        "wall_s": round(time.perf_counter() - start, 3),
        "digest": hashlib.sha256(text.encode()).hexdigest()[:16],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", type=Path, default=DEFAULT_MODEL)
    parser.add_argument("--fixtures", type=Path, default=DEFAULT_FIXTURES)
    parser.add_argument(
        "--adapters", default="neutral-r8,neutral-r32,probe-r8", help="fixture names in order"
    )
    parser.add_argument("--max-tokens", type=int, default=256)
    parser.add_argument("--repeats", type=int, default=3, help="scored passes per arm")
    parser.add_argument("--warmup", type=int, default=1, help="unscored passes per arm")
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    args = parser.parse_args()

    admission_check(args.model_dir)

    import mlx.core as mx
    from mlx_vlm import load

    gib = 1024**3
    say("loading base weights…")
    cold = time.perf_counter()
    model, tokenizer = load(str(args.model_dir), lazy=False, fix_mistral_regex=True)
    load_s = round(time.perf_counter() - cold, 3)
    say(f"loaded in {load_s}s · resident {mx.get_active_memory()/gib:.2f} GiB")

    names = [part.strip() for part in args.adapters.split(",") if part.strip()]
    arms = ["base"] + names
    samples: dict[str, list[float]] = {arm: [] for arm in arms}
    swap: dict[str, dict] = {}
    correctness: dict[str, dict] = {}
    base_digest = None

    report: dict = {
        "cold_load_s": load_s,
        "max_tokens": args.max_tokens,
        "repeats": args.repeats,
        "warmup": args.warmup,
    }
    try:
        # Interleaved rather than grouped: thermal drift and background load move
        # throughput by more than a LoRA does, so every arm has to be sampled
        # under the same conditions rather than in its own block of time.
        for pass_index in range(args.warmup + args.repeats):
            scored = pass_index >= args.warmup
            for arm in arms:
                if arm == "base":
                    row = measure(model, tokenizer, args.max_tokens)
                    if base_digest is None:
                        base_digest = row["digest"]
                    elif row["digest"] != base_digest:
                        raise SystemExit("base output is not deterministic; cannot compare arms")
                else:
                    originals, attach_ms, modules = attach(model, args.fixtures / arm)
                    row = measure(model, tokenizer, args.max_tokens)
                    detach_ms = detach(model, originals)
                    swap[arm] = {
                        "attach_ms": round(attach_ms, 1),
                        "detach_ms": round(detach_ms, 1),
                        "modules": modules,
                        "resident_gb": round(mx.get_active_memory() / gib, 3),
                    }
                    if arm not in correctness:
                        neutral = arm.startswith("neutral")
                        matches = row["digest"] == base_digest
                        after = measure(model, tokenizer, args.max_tokens)
                        correctness[arm] = {
                            "output_matches_base": matches,
                            "expected_match": neutral,
                            "verdict": "ok" if matches == neutral else "UNEXPECTED",
                            "detach_restores_base": after["digest"] == base_digest,
                        }
                if scored and row["decode_tps"]:
                    samples[arm].append(row["decode_tps"])
            say(f"pass {pass_index + 1}/{args.warmup + args.repeats}"
                + (" (warmup, discarded)" if not scored else ""))

        base_median = median(samples["base"]) if samples["base"] else None
        report["arms"] = []
        for arm in arms:
            values = sorted(samples[arm])
            row = {
                "arm": arm,
                "decode_tps_median": round(median(values), 3) if values else None,
                "decode_tps_min": round(values[0], 3) if values else None,
                "decode_tps_max": round(values[-1], 3) if values else None,
                "samples": len(values),
            }
            if arm != "base" and values and base_median:
                row["delta_pct"] = round((median(values) / base_median - 1) * 100, 2)
            row.update(swap.get(arm, {}))
            row.update(correctness.get(arm, {}))
            report["arms"].append(row)
            spread = (
                f"{row['decode_tps_min']}–{row['decode_tps_max']}" if values else "n/a"
            )
            delta = f" ({row['delta_pct']:+.2f}%)" if "delta_pct" in row else ""
            extra = (
                f" · attach {row['attach_ms']}ms · detach {row['detach_ms']}ms · {row['verdict']}"
                if arm != "base"
                else ""
            )
            say(f"{arm}: median {row['decode_tps_median']} tok/s{delta} · spread {spread}{extra}")

        # The spread across identical base passes bounds what a delta can mean.
        if samples["base"]:
            noise = (max(samples["base"]) / min(samples["base"]) - 1) * 100
            report["base_noise_pct"] = round(noise, 2)
            say(f"base-to-base noise across identical passes: {noise:.2f}%")
    finally:
        del model
        gc.collect()
        mx.clear_cache()

    args.report.write_text(json.dumps(report, indent=2) + "\n")
    say(f"report: {args.report}")


if __name__ == "__main__":
    main()

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
import time
from pathlib import Path

DEFAULT_MODEL = Path.home() / ".synth-desktop/models/poolside/Laguna-XS-2.1-NVFP4-mlx"
DEFAULT_FIXTURES = Path.home() / ".synth-desktop/laguna/test-adapters"
DEFAULT_REPORT = Path.home() / ".synth-desktop/laguna/adapter-swap-report.json"
PROMPT = "Write a Python function that merges two sorted lists. Explain briefly."


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
    print(f"admission: {(available or 0)/gib:.1f} GiB available, {required/gib:.1f} GiB required")


def lora_target(model):
    """mlx-vlm wraps a text-only checkpoint; LoRA applies to the inner module."""
    if getattr(model, "_is_text_model", False):
        return model.language_model._model
    return model


def attach(model, adapter: Path) -> tuple[dict, float, int]:
    import mlx.core as mx
    from mlx_vlm.trainer.utils import _to_lora, get_module_by_name, set_module_by_name

    config = json.loads((adapter / "adapter_config.json").read_text())
    params = dict(config["lora_parameters"])
    keys = params["keys"]
    target = lora_target(model)
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

    target = lora_target(model)
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
    parser.add_argument("--max-tokens", type=int, default=128)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    args = parser.parse_args()

    admission_check(args.model_dir)

    import mlx.core as mx
    from mlx_vlm import load

    gib = 1024**3
    print("loading base weights…")
    cold = time.perf_counter()
    model, tokenizer = load(str(args.model_dir), lazy=False, fix_mistral_regex=True)
    load_s = round(time.perf_counter() - cold, 3)
    print(f"loaded in {load_s}s · resident {mx.get_active_memory()/gib:.2f} GiB")

    report: dict = {"cold_load_s": load_s, "phases": []}
    try:
        base = measure(model, tokenizer, args.max_tokens)
        base["phase"] = "base"
        base["resident_gb"] = round(mx.get_active_memory() / gib, 3)
        report["phases"].append(base)
        print(f"base: {base['decode_tps']} tok/s · digest {base['digest']}")

        for name in [part.strip() for part in args.adapters.split(",") if part.strip()]:
            adapter = args.fixtures / name
            originals, attach_ms, modules = attach(model, adapter)
            row = measure(model, tokenizer, args.max_tokens)
            row.update(
                phase=name,
                attach_ms=round(attach_ms, 1),
                modules=modules,
                resident_gb=round(mx.get_active_memory() / gib, 3),
            )
            neutral = name.startswith("neutral")
            row["output_matches_base"] = row["digest"] == base["digest"]
            # A zero-initialised adapter is mathematically the identity; if the
            # text moved, the attach path changed more than it should have.
            row["expected_match"] = neutral
            row["verdict"] = "ok" if row["output_matches_base"] == neutral else "UNEXPECTED"
            row["detach_ms"] = round(detach(model, originals), 1)
            after = measure(model, tokenizer, args.max_tokens)
            row["detach_restores_base"] = after["digest"] == base["digest"]
            report["phases"].append(row)
            delta = (
                f"{(row['decode_tps']/base['decode_tps'] - 1) * 100:+.1f}%"
                if row["decode_tps"] and base["decode_tps"]
                else "n/a"
            )
            print(
                f"{name}: {row['decode_tps']} tok/s ({delta}) · attach {row['attach_ms']}ms · "
                f"detach {row['detach_ms']}ms · {row['verdict']} · "
                f"restored={row['detach_restores_base']}"
            )
    finally:
        del model
        gc.collect()
        mx.clear_cache()

    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(f"report: {args.report}")


if __name__ == "__main__":
    main()

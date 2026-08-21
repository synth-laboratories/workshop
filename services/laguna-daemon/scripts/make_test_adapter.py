#!/usr/bin/env python3
"""Build an `mlx-lora.v1` adapter tree for Laguna without loading the weights.

Two variants exist for different jobs:

* ``neutral`` — ``lora_b`` is all zeros, so the adapted model is mathematically
  identical to the base. Any measured difference is pure runtime cost, which is
  what the policy-switch throughput numbers need.
* ``probe`` — ``lora_b`` carries small noise, so the logits *must* move. A
  neutral adapter cannot tell a working attach path from a silently unbound
  one; the probe can.

Shapes come from the safetensors headers, so this reads a few kilobytes rather
than paging in 20 GB of weights.
"""

from __future__ import annotations

import argparse
import json
import math
import struct
from pathlib import Path

DEFAULT_MODEL = Path.home() / ".synth-desktop/models/poolside/Laguna-XS-2.1-NVFP4-mlx"
DEFAULT_OUT = Path.home() / ".synth-desktop/laguna/test-adapters"
DEFAULT_TARGETS = ("q_proj", "k_proj", "v_proj", "o_proj")
INDEX = "model.safetensors.index.json"


def read_header(shard: Path) -> dict:
    """Parse a safetensors header without reading any tensor payload."""
    with shard.open("rb") as handle:
        (length,) = struct.unpack("<Q", handle.read(8))
        return json.loads(handle.read(length))


def module_paths(weight_map: dict[str, str], targets: tuple[str, ...], layers: int) -> list[str]:
    seen: dict[int, list[str]] = {}
    for key in weight_map:
        if not key.endswith(".weight"):
            continue
        path = key[: -len(".weight")]
        parts = path.split(".")
        if parts[-1] not in targets:
            continue
        try:
            index = parts.index("layers")
            number = int(parts[index + 1])
        except (ValueError, IndexError):
            continue
        seen.setdefault(number, []).append(path)
    if not seen:
        raise SystemExit(f"no target modules matched {targets}")
    chosen = sorted(seen)[-layers:] if layers > 0 else sorted(seen)
    return [path for number in chosen for path in sorted(seen[number])]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", type=Path, default=DEFAULT_MODEL)
    parser.add_argument("--out-dir", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--name", default=None, help="defaults to <mode>-r<rank>")
    parser.add_argument("--rank", type=int, default=8)
    parser.add_argument("--scale", type=float, default=20.0)
    parser.add_argument("--dropout", type=float, default=0.0)
    parser.add_argument("--layers", type=int, default=16, help="adapt the last N blocks; 0 = all")
    parser.add_argument("--targets", default=",".join(DEFAULT_TARGETS))
    parser.add_argument("--mode", choices=["neutral", "probe"], default="neutral")
    parser.add_argument("--probe-std", type=float, default=1e-3)
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()

    import mlx.core as mx

    model_dir: Path = args.model_dir
    index_path = model_dir / INDEX
    if not index_path.is_file():
        raise SystemExit(f"missing {index_path}")
    weight_map = json.loads(index_path.read_text())["weight_map"]
    config = json.loads((model_dir / "config.json").read_text())
    quantization = config.get("quantization") or {}
    bits = int(quantization.get("bits") or 0)

    targets = tuple(part.strip() for part in args.targets.split(",") if part.strip())
    paths = module_paths(weight_map, targets, args.layers)

    headers: dict[str, dict] = {}
    tensors: dict[str, "mx.array"] = {}
    mx.random.seed(args.seed)
    total_params = 0
    for path in paths:
        weight_key = f"{path}.weight"
        shard = weight_map[weight_key]
        header = headers.setdefault(shard, read_header(model_dir / shard))
        entry = header.get(weight_key)
        if entry is None:
            raise SystemExit(f"{weight_key} missing from {shard}")
        out_features, packed_in = entry["shape"]
        # A quantized linear stores several values per 32-bit word; mlx-vlm
        # recovers the logical width the same way when it wraps the layer.
        quantized = f"{path}.scales" in header
        in_features = packed_in * 32 // bits if quantized and bits else packed_in
        bound = 1.0 / math.sqrt(in_features)
        lora_a = mx.random.uniform(low=-bound, high=bound, shape=(in_features, args.rank))
        if args.mode == "neutral":
            lora_b = mx.zeros(shape=(args.rank, out_features))
        else:
            lora_b = mx.random.normal(shape=(args.rank, out_features)) * args.probe_std
        tensors[f"{path}.lora_a"] = lora_a.astype(mx.bfloat16)
        tensors[f"{path}.lora_b"] = lora_b.astype(mx.bfloat16)
        total_params += lora_a.size + lora_b.size

    name = args.name or f"{args.mode}-r{args.rank}"
    out = args.out_dir / name
    out.mkdir(parents=True, exist_ok=True)
    mx.save_safetensors(str(out / "adapters.safetensors"), tensors)
    (out / "adapter_config.json").write_text(
        json.dumps(
            {
                "fine_tune_type": "lora",
                "num_layers": args.layers,
                "lora_parameters": {
                    "rank": args.rank,
                    "scale": args.scale,
                    "dropout": args.dropout,
                    "keys": paths,
                },
                "synth_test_fixture": {
                    "mode": args.mode,
                    "base_model": model_dir.name,
                    "probe_std": args.probe_std if args.mode == "probe" else None,
                    "seed": args.seed,
                },
            },
            indent=2,
        )
        + "\n"
    )
    size = (out / "adapters.safetensors").stat().st_size
    print(f"{name}: {len(paths)} modules, {total_params/1e6:.2f}M params, {size/1024**2:.1f} MB")
    print(f"  layers adapted: last {args.layers or 'all'} · targets {','.join(targets)}")
    print(f"  {out}")


if __name__ == "__main__":
    main()

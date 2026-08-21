#!/usr/bin/env python3
"""Publish and fetch Synth-maintained Laguna adapters.

Run locally, never as a service route: publishing production bytes is an act
someone performs, and keeping it here keeps production credentials off the
server.

The object store is addressed by digest, so a republished tree with different
bytes is a different object rather than a silent overwrite of an old one:

    s3://<bucket>/laguna-xs-2.1/ft/sha256-<hex>/{manifest,adapter_config}.json
                                               /adapters.safetensors

Wasabi and MinIO differ only by endpoint, so `--endpoint` is the whole of the
difference between rehearsing this locally and running it for real.

    synth_adapters.py publish ~/.synth-desktop/laguna/test-adapters/neutral-r8 \
        --base-revision 841778bda563a36104dd521e37d99218e46f4f25
    synth_adapters.py list
    synth_adapters.py pull sha256:<hex> --into /tmp/pulled
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

SCHEMA_VERSION = "synth-adapter.v1"
DEFAULT_BUCKET = "synth-adapters"
DEFAULT_PREFIX = "laguna-xs-2.1/ft"
DEFAULT_ENDPOINT = "http://127.0.0.1:9000"
BASE_MODEL_ID = "poolside/Laguna-XS-2.1-NVFP4-mlx"
REQUIRED_FILES = ("adapter_config.json", "adapters.safetensors")
MANIFEST = "manifest.json"


def digest_file(path: Path) -> str:
    sha = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            sha.update(block)
    return sha.hexdigest()


def digest_tree(root: Path) -> tuple[str, list[dict[str, Any]]]:
    """Digest a tree exactly as Workshop's catalog does.

    `local_lora::digest_directory` frames each top-level file as
    `len(name) || name || len(content) || content`, sorted by path. The framing
    is a contract, not an implementation detail: a second definition here would
    mean a downloaded adapter never matches the id it was published under.
    """
    if any(child.is_dir() for child in root.iterdir()):
        # The catalog digests top-level files only, so a nested file would be
        # published without ever being covered by the identity it claims.
        raise SystemExit(f"{root} has subdirectories; an mlx-lora.v1 tree is flat")
    files: list[dict[str, Any]] = []
    tree = hashlib.sha256()
    for path in sorted(child for child in root.iterdir() if child.is_file()):
        name = path.name.encode()
        content = path.read_bytes()
        tree.update(len(name).to_bytes(8, "big"))
        tree.update(name)
        tree.update(len(content).to_bytes(8, "big"))
        tree.update(content)
        files.append(
            {
                "path": path.name,
                "bytes": len(content),
                "sha256": hashlib.sha256(content).hexdigest(),
            }
        )
    return f"sha256:{tree.hexdigest()}", files


def client(args: argparse.Namespace):
    try:
        import boto3
    except ImportError:  # pragma: no cover - operator environment
        raise SystemExit("boto3 is required: pip install boto3")
    return boto3.client(
        "s3",
        endpoint_url=args.endpoint,
        aws_access_key_id=os.environ.get("SYNTH_ADAPTERS_KEY", "synth-local"),
        aws_secret_access_key=os.environ.get("SYNTH_ADAPTERS_SECRET", "synth-local-secret"),
        region_name=args.region,
    )


def object_prefix(prefix: str, digest: str) -> str:
    return f"{prefix}/{digest.replace(':', '-')}"


def build_manifest(root: Path, args: argparse.Namespace) -> dict[str, Any]:
    for required in REQUIRED_FILES:
        if not (root / required).is_file():
            raise SystemExit(f"not mlx-lora.v1: {root} is missing {required}")
    config = json.loads((root / "adapter_config.json").read_text(encoding="utf-8"))
    params = config.get("lora_parameters") or {}
    digest, files = digest_tree(root)
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "digest": digest,
        "base": {
            "model_id": config.get("base_model") or BASE_MODEL_ID,
            # The pin that makes an install refusable. An adapter trained
            # against one base revision produces plausible tokens on another,
            # so nothing downstream would catch the mismatch.
            "revision": args.base_revision,
        },
        "lora": {
            "rank": params.get("rank"),
            "scale": params.get("scale"),
            "modules": len(params.get("keys") or []),
        },
        "provenance": {
            "run_id": args.run_id,
            "algorithm": args.algorithm,
            "dataset_id": args.dataset_id,
            "step": args.step,
            "published_at": datetime.now(timezone.utc).isoformat(),
        },
        "metrics": {},
        "files": files,
    }
    if args.decode_tps_p10 is not None:
        if args.measurement_floor_pct is None:
            # A cost number without the floor it was measured against invites
            # exactly the false confidence this whole path is built to avoid.
            raise SystemExit("--decode-tps-p10 requires --measurement-floor-pct")
        manifest["metrics"]["decode_tokens_per_second_p10"] = args.decode_tps_p10
        manifest["metrics"]["measurement_floor_pct"] = args.measurement_floor_pct
    if args.notes:
        manifest["notes"] = args.notes
    return manifest


def publish(args: argparse.Namespace) -> None:
    root = args.adapter.expanduser().resolve()
    manifest = build_manifest(root, args)
    digest = manifest["digest"]
    key_prefix = object_prefix(args.prefix, digest)
    s3 = client(args)
    existing = s3.list_objects_v2(Bucket=args.bucket, Prefix=key_prefix).get("KeyCount", 0)
    if existing and not args.force:
        print(f"{digest} is already published at {key_prefix}; nothing to do")
        return
    for entry in manifest["files"]:
        source = root / entry["path"]
        s3.upload_file(str(source), args.bucket, f"{key_prefix}/{entry['path']}")
        print(f"  uploaded {entry['path']} ({entry['bytes']} bytes)")
    s3.put_object(
        Bucket=args.bucket,
        Key=f"{key_prefix}/{MANIFEST}",
        Body=json.dumps(manifest, indent=2).encode() + b"\n",
        ContentType="application/json",
    )
    print(f"published {digest}")
    print(f"  s3://{args.bucket}/{key_prefix}/")


def list_adapters(args: argparse.Namespace) -> None:
    s3 = client(args)
    paginator = s3.get_paginator("list_objects_v2")
    found = 0
    for page in paginator.paginate(Bucket=args.bucket, Prefix=args.prefix):
        for entry in page.get("Contents", []):
            if not entry["Key"].endswith(MANIFEST):
                continue
            body = s3.get_object(Bucket=args.bucket, Key=entry["Key"])["Body"].read()
            manifest = json.loads(body)
            found += 1
            metrics = manifest.get("metrics") or {}
            rate = metrics.get("decode_tokens_per_second_p10")
            floor = metrics.get("measurement_floor_pct")
            speed = f"{rate} tok/s ±{floor}%" if rate is not None else "unmeasured"
            print(
                f"{manifest['digest']}  rank {manifest['lora']['rank']}  "
                f"base {manifest['base']['revision'][:12]}  {speed}"
            )
    if not found:
        print("no adapters published")


def pull(args: argparse.Namespace) -> None:
    """Download and verify, which is the install path the app will run.

    Every file is checked against the manifest and the tree is re-digested, so
    a truncated object or a swapped byte fails here rather than at the first
    token.
    """
    s3 = client(args)
    key_prefix = object_prefix(args.prefix, args.digest)
    body = s3.get_object(Bucket=args.bucket, Key=f"{key_prefix}/{MANIFEST}")["Body"].read()
    manifest = json.loads(body)
    destination = args.into.expanduser().resolve()
    destination.mkdir(parents=True, exist_ok=True)
    for entry in manifest["files"]:
        target = destination / entry["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        s3.download_file(args.bucket, f"{key_prefix}/{entry['path']}", str(target))
        actual = digest_file(target)
        if actual != entry["sha256"]:
            raise SystemExit(
                f"{entry['path']} failed verification: expected {entry['sha256']}, got {actual}"
            )
        print(f"  verified {entry['path']}")
    recomputed, _ = digest_tree(destination)
    if recomputed != manifest["digest"]:
        raise SystemExit(f"tree digest mismatch: expected {manifest['digest']}, got {recomputed}")
    # Beside the tree, never inside it: a manifest in the directory would
    # change the digest the directory is identified by.
    destination.with_suffix(".manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    print(f"pulled {manifest['digest']} into {destination}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", default=os.environ.get("SYNTH_ADAPTERS_ENDPOINT", DEFAULT_ENDPOINT))
    parser.add_argument("--bucket", default=DEFAULT_BUCKET)
    parser.add_argument("--prefix", default=DEFAULT_PREFIX)
    parser.add_argument("--region", default="us-east-1")
    sub = parser.add_subparsers(dest="command", required=True)

    publish_cmd = sub.add_parser("publish", help="digest, manifest, and upload an adapter tree")
    publish_cmd.add_argument("adapter", type=Path)
    publish_cmd.add_argument("--base-revision", required=True)
    publish_cmd.add_argument("--run-id")
    publish_cmd.add_argument("--algorithm")
    publish_cmd.add_argument("--dataset-id")
    publish_cmd.add_argument("--step", type=int)
    publish_cmd.add_argument("--decode-tps-p10", type=float)
    publish_cmd.add_argument("--measurement-floor-pct", type=float)
    publish_cmd.add_argument("--notes")
    publish_cmd.add_argument("--force", action="store_true")
    publish_cmd.set_defaults(func=publish)

    list_cmd = sub.add_parser("list", help="list published adapters")
    list_cmd.set_defaults(func=list_adapters)

    pull_cmd = sub.add_parser("pull", help="download and verify an adapter")
    pull_cmd.add_argument("digest")
    pull_cmd.add_argument("--into", type=Path, required=True)
    pull_cmd.set_defaults(func=pull)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Generate the small, deterministic Trace V5 archive used by Desktop tests.

The fixture deliberately exercises more than manifest parsing: one model turn,
one tool call/result pair, usage, native verifier/reward evidence, and the
canonical rollout-inspector projection. It is built with synth-containers
rather than by hand so format changes fail at the producer boundary.
"""

from __future__ import annotations

import argparse
from contextlib import ExitStack
from contextlib import redirect_stdout
import io
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
from unittest.mock import patch


FIXED_TIME = "2026-08-09T00:00:00Z"


def _containers_repo() -> Path:
    override = os.environ.get("SYNTH_CONTAINERS_REPO")
    if override:
        return Path(override).expanduser().resolve()
    return Path(__file__).resolve().parents[2] / "containers"


def _load_containers() -> None:
    source = _containers_repo() / "src"
    if not source.is_dir():
        raise SystemExit(
            f"synth-containers source not found at {source}; set SYNTH_CONTAINERS_REPO"
        )
    sys.path.insert(0, str(source))


def _atif_payload() -> dict[str, object]:
    return {
        "schema_version": "ATIF-v1.7",
        "session_id": "desktop-e2e-session",
        "trajectory_id": "desktop-e2e-rollout",
        "agent": {
            "name": "synth-desktop-e2e",
            "version": "1",
            "model_name": "synth/e2e-model",
        },
        "steps": [
            {
                "step_id": 1,
                "timestamp": "2026-08-09T00:00:00Z",
                "source": "user",
                "message": "Inspect the workspace and report the answer.",
            },
            {
                "step_id": 2,
                "timestamp": "2026-08-09T00:00:01Z",
                "source": "agent",
                "message": "I will inspect the requested file.",
                "model_name": "synth/e2e-model",
                "reasoning_content": "The request requires one deterministic tool call.",
                "tool_calls": [
                    {
                        "tool_call_id": "call-read-1",
                        "function_name": "read_file",
                        "arguments": {"path": "answer.txt"},
                    }
                ],
                "observation": {
                    "results": [
                        {
                            "source_call_id": "call-read-1",
                            "content": "dogfood-ready",
                        }
                    ]
                },
                "metrics": {
                    "prompt_tokens": 17,
                    "completion_tokens": 8,
                    "cached_tokens": 3,
                    "cost_usd": 0.0012,
                },
            },
            {
                "step_id": 3,
                "timestamp": "2026-08-09T00:00:02Z",
                "source": "agent",
                "message": "The workspace reports dogfood-ready.",
                "model_name": "synth/e2e-model",
                "metrics": {"prompt_tokens": 9, "completion_tokens": 6},
            },
        ],
        "final_metrics": {
            "total_prompt_tokens": 26,
            "total_completion_tokens": 14,
            "total_cached_tokens": 3,
            "total_cost_usd": 0.0012,
            "total_steps": 3,
        },
    }


def _native_evaluation() -> dict[str, object]:
    return {
        "schema_version": "harbor.native-evaluation.v1",
        "authority": "synth-desktop-e2e-verifier",
        "task_id": "desktop/trace-v5-import",
        "benchmark_family": "desktop-e2e",
        "seed": 8,
        "rubric": {
            "id": "desktop-e2e-rubric",
            "name": "Desktop Trace V5 import",
            "pass_threshold": 0.5,
            "criteria": [
                {
                    "id": "answer",
                    "name": "Answer recovered",
                    "description": "The trace records the expected tool result.",
                    "role": "gating",
                    "pass_threshold": 0.5,
                }
            ],
        },
        "verifier": {
            "id": "desktop-e2e-verifier",
            "score": 1.0,
            "passed": True,
            "pass_threshold": 0.5,
            "criteria": [
                {
                    "id": "answer",
                    "score": 1.0,
                    "passed": True,
                    "verdict": "pass",
                }
            ],
        },
        "reward": {
            "primary_metric": "desktop_e2e_reward",
            "value": 1.0,
            "lower_bound": 0.0,
            "upper_bound": 1.0,
        },
        "metrics": {"wall_time_seconds": 2.0},
    }


def build(output: Path) -> dict[str, object]:
    _load_containers()

    from synth_containers.tracing.adapters.atif import import_atif
    from synth_containers.tracing.adapters.native import write_imported_document
    from synth_containers.tracing.canonical import bytes_digest, canonical_bytes
    from synth_containers.tracing.cli import main as trace_cli_main
    from synth_containers.tracing.native_evaluation import attach_native_evaluation
    from synth_containers.tracing.projections.inspector import load_bundle
    from synth_containers.tracing.store.bundle import LocalTraceBundle

    output = output.expanduser().resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="synth-trace-v5-e2e-"))
    try:
        root = work / "bundle"
        bundle = LocalTraceBundle(root, bundle_id="synth-desktop-e2e-v1")
        atif = _atif_payload()
        source_bytes = canonical_bytes(atif)
        source_digest = bytes_digest(source_bytes)
        source_blob_digest = bundle.blobs.put(source_bytes)
        document = import_atif(atif)

        # Every producer timestamp participating in a semantic digest is fixed.
        with ExitStack() as stack:
            for target in (
                "synth_containers.tracing.store.bundle.utc_now",
                "synth_containers.tracing.evidence_ops.utc_now",
                "synth_containers.tracing.native_evaluation.utc_now",
                "synth_containers.tracing.validation.validator.utc_now",
                "synth_containers.tracing.cli.utc_now",
            ):
                stack.enter_context(patch(target, return_value=FIXED_TIME))
            imported = write_imported_document(
                document,
                source_digest=source_digest,
                stored_source_digest=source_blob_digest,
                source_format="harbor.atif-v1.7",
                bundle=bundle,
            )
            evidence = attach_native_evaluation(
                root,
                payload=_native_evaluation(),
                source_name="desktop-e2e-native-evaluation.json",
            )
            with redirect_stdout(io.StringIO()):
                projected = trace_cli_main(
                    ["project", str(root), "--format", "rollout-inspector"]
                )
            if projected != 0:
                raise RuntimeError("rollout-inspector projection failed")

        ok, errors = bundle.verify_self_contained()
        if not ok:
            raise RuntimeError(f"generated fixture is not self-contained: {errors}")
        archive_digest = bundle.write_archive(output)
        inspected = load_bundle(root)[0]
        manifest = bundle.read_manifest()
        return {
            "schema_version": "synth.desktop-trace-fixture-build.v1",
            "path": str(output),
            "byte_size": output.stat().st_size,
            "archive_digest": archive_digest,
            "bundle_digest": manifest["content_digest"],
            "trace_id": inspected.trace.trace_id,
            "trace_digest": inspected.trace.content_digest,
            "evidence_bundle_digest": evidence["evidence_bundle_digest"],
            "event_count": len(inspected.trace.events),
            "span_count": len(inspected.trace.spans),
            "message_count": len(inspected.trace.messages),
            "projection_count": len(manifest.get("projection_digests") or ()),
            "self_contained": True,
            "import_result": imported,
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="destination .zip path")
    args = parser.parse_args()
    print(json.dumps(build(args.output), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

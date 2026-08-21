#!/usr/bin/env python3
"""Generate real turns under each policy, through the daemon.

Everything else in this lane verifies bytes and wiring. This is the only test
that answers the question that matters: does a turn actually decode with the
adapter attached, and does the pin in the request decide which one?

Three arms, and the third is the one with teeth:
  base   — the resident weights
  ft     — the published adapter, which is the identity, so its output must
           match base exactly
  probe  — noise in `lora_b`, so its output must differ; without it a policy
           that silently failed to attach would look like a success

Waits for the daemon's own admission threshold rather than assuming capacity,
and kills the daemon it started no matter how it exits.
"""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODELS = Path.home() / ".synth-desktop/models"
BASE = "poolside/Laguna-XS-2.1-NVFP4-mlx"
FT = "synth/Laguna-XS-2.1-ft"
PROBE = "synth/Laguna-XS-2.1-probe"
PORT = int(os.environ.get("POLICY_E2E_PORT", "7399"))
KEY = "policy-e2e-key"
URL = f"http://127.0.0.1:{PORT}"
PROMPT = "Reply with exactly the word: ready"


def call(path: str, payload: dict | None = None, method: str = "POST", timeout: float = 300.0):
    request = urllib.request.Request(
        f"{URL}{path}",
        data=None if payload is None else json.dumps(payload).encode(),
        headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"},
        method=method,
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read() or b"{}")


def wait_for_capacity(deadline_s: float) -> None:
    sys.path.insert(0, str(ROOT))
    from laguna_daemon.responses_api.backends.mlx import (
        _available_memory_bytes,
        _required_available_memory_bytes,
    )

    model = MODELS / BASE
    required = _required_available_memory_bytes(model)
    started = time.time()
    while True:
        available = _available_memory_bytes() or 0
        if available >= required:
            print(f"admission ok: {available/1024**3:.1f} GiB available", flush=True)
            return
        if time.time() - started > deadline_s:
            raise SystemExit(
                f"gave up waiting for memory: {available/1024**3:.1f} GiB available, "
                f"{required/1024**3:.1f} GiB required"
            )
        print(
            f"waiting for memory: {available/1024**3:.1f} of {required/1024**3:.1f} GiB",
            flush=True,
        )
        time.sleep(60)


def text_of(response: dict) -> str:
    parts = []
    for item in response.get("output") or []:
        for chunk in item.get("content") or []:
            if chunk.get("type") in {"output_text", "text"}:
                parts.append(chunk.get("text") or "")
    return "".join(parts).strip()


def main() -> None:
    wait_for_capacity(float(os.environ.get("POLICY_E2E_WAIT_SECONDS", "3600")))
    environment = {
        **os.environ,
        "PYTHONPATH": str(ROOT),
        "SYNTH_LAGUNA_PORT": str(PORT),
        "SYNTH_LAGUNA_API_KEY": KEY,
        "SYNTH_LAGUNA_BACKEND": "mlx_lm",
        "SYNTH_LAGUNA_MODELS_DIR": str(MODELS),
        "SYNTH_LAGUNA_DATA_DIR": str(Path.home() / ".synth-desktop/laguna/e2e"),
        "SYNTH_LAGUNA_AUTO_LOAD": "0",
    }
    daemon = subprocess.Popen(
        [str(Path.home() / ".synth-desktop/laguna/.venv/bin/python"), "-m", "laguna_daemon"],
        cwd=str(ROOT),
        env=environment,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.STDOUT,
    )
    report: dict = {"port": PORT}
    try:
        for _ in range(60):
            try:
                call("/health", method="GET", timeout=5)
                break
            except (urllib.error.URLError, ConnectionError, TimeoutError):
                time.sleep(1)
        else:
            raise SystemExit("daemon never became healthy")
        print("daemon healthy", flush=True)

        fixtures = Path.home() / ".synth-desktop/laguna"
        for model_id, path in (
            (FT, fixtures / "installed-ft"),
            (PROBE, fixtures / "test-adapters/probe-r8"),
        ):
            call("/v1/synth/policies", {"model_id": model_id, "adapter_path": str(path)})
            print(f"registered {model_id}", flush=True)

        listed = call("/v1/models", method="GET")
        report["models"] = [item["id"] for item in listed["data"]]
        print("models:", report["models"], flush=True)

        outputs = {}
        for model_id in (BASE, FT, PROBE):
            started = time.time()
            answer = call(
                "/v1/responses",
                {"model": model_id, "input": PROMPT, "stream": False, "max_output_tokens": 24},
            )
            if answer.get("status") != "completed":
                raise SystemExit(f"{model_id} did not complete: {json.dumps(answer)[:400]}")
            outputs[model_id] = text_of(answer)
            print(
                f"{model_id}: {time.time()-started:.1f}s -> {outputs[model_id]!r}",
                flush=True,
            )
        report["outputs"] = outputs
        report["ft_matches_base"] = outputs[FT] == outputs[BASE]
        report["probe_differs_from_base"] = outputs[PROBE] != outputs[BASE]

        # An unregistered id must be refused rather than quietly served base.
        try:
            call("/v1/responses", {"model": "synth/not-registered", "input": "hi", "stream": False})
            report["unknown_model_refused"] = False
        except urllib.error.HTTPError as error:
            report["unknown_model_refused"] = error.code == 404

        report["telemetry"] = call("/v1/synth/inference", method="GET").get("policies")
    finally:
        daemon.send_signal(signal.SIGTERM)
        try:
            daemon.wait(timeout=30)
        except subprocess.TimeoutExpired:
            daemon.kill()
            daemon.wait(timeout=10)
        print("daemon stopped", flush=True)

    out = Path.home() / ".synth-desktop/laguna/policy-e2e-report.json"
    out.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({k: v for k, v in report.items() if k != "telemetry"}, indent=2))
    print("report:", out)
    ok = (
        report.get("ft_matches_base")
        and report.get("probe_differs_from_base")
        and report.get("unknown_model_refused")
    )
    print("VERDICT:", "pass" if ok else "FAIL")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()

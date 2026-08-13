#!/usr/bin/env python3
"""Dogfood local Laguna stub + OpenRouter Luna / Laguna S 2.1."""

from __future__ import annotations

import json
import os
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "services" / "local-runtime" / "src"))

from synth_local_runtime.api import RuntimeHTTPServer, write_connection_file  # noqa: E402
from synth_local_runtime.config import RuntimeConfig  # noqa: E402
from synth_local_runtime.service import RuntimeService  # noqa: E402


def http_json(url: str, *, method: str = "GET", body: dict | None = None, token: str | None = None):
    data = None if body is None else json.dumps(body).encode("utf-8")
    headers = {"Accept": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if body is not None:
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    with urllib.request.urlopen(req, timeout=90) as resp:
        return json.loads(resp.read().decode("utf-8"))


def wait_for_run(base: str, session_id: str, token: str, *, timeout: float = 60.0) -> list[dict]:
    deadline = time.time() + timeout
    cursor = 0
    events: list[dict] = []
    while time.time() < deadline:
        page = http_json(
            f"{base}/v1/sessions/{session_id}/events?after_sequence={cursor}&limit=200",
            token=token,
        )
        batch = page.get("events") or []
        if batch:
            events.extend(batch)
            cursor = max(cursor, max(int(e["sequence"]) for e in batch))
            kinds = {e["eventKind"] for e in events}
            if "run.completed" in kinds or "run.failed" in kinds or "run.cancelled" in kinds:
                return events
        time.sleep(0.25)
    return events


def main() -> int:
    openrouter_key = os.getenv("OPENROUTER_API_KEY")
    data_dir = Path(tempfile.mkdtemp(prefix="synth-desktop-dogfood-"))
    token = "dogfood-token"
    os.environ["SYNTH_RUNTIME_TOKEN"] = token
    os.environ["SYNTH_INTERN_DEMO"] = "1"
    os.environ["SYNTH_WORKSHOP_ROOT"] = str(ROOT)
    os.environ["SYNTH_VISUALS_ROOT"] = str(ROOT / "visuals")
    if openrouter_key:
        os.environ["OPENROUTER_API_KEY"] = openrouter_key

    config = RuntimeConfig.from_env(host="127.0.0.1", port=0, data_dir=data_dir)
    service = RuntimeService(config)
    server = RuntimeHTTPServer(("127.0.0.1", 0), service, token=token)
    host, port = server.server_address[:2]
    base = f"http://{host}:{port}"
    write_connection_file(
        data_dir / "connection.json",
        url=base,
        token=token,
        service=service,
    )

    import threading

    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    print(f"[dogfood] runtime at {base}")
    print(f"[dogfood] health={json.dumps(http_json(f'{base}/v1/health', token=token), indent=2)}")

    results: dict[str, str] = {}

    # 1) Local Laguna stub
    local = http_json(
        f"{base}/v1/sessions",
        method="POST",
        token=token,
        body={"target": {"kind": "local", "model": "laguna-xs-2.1", "adapter": None}},
    )
    http_json(
        f"{base}/v1/sessions/{local['id']}/messages",
        method="POST",
        token=token,
        body={"body": "Say hello from local Laguna XS 2.1 and mention Trace V5 briefly."},
    )
    events = wait_for_run(base, local["id"], token)
    ok = any(e["eventKind"] == "run.completed" for e in events)
    results["local_laguna"] = "PASS" if ok else "FAIL"
    print(f"[dogfood] local_laguna → {results['local_laguna']} ({len(events)} events)")

    # 2) Inventory
    containers = http_json(f"{base}/v1/containers", token=token)["containers"]
    traces = http_json(f"{base}/v1/traces", token=token)["traces"]
    templates = http_json(f"{base}/v1/visuals/templates", token=token)["templates"]
    results["inventory"] = (
        "PASS" if containers and traces and len(templates) >= 5 else "FAIL"
    )
    print(
        f"[dogfood] inventory → {results['inventory']} "
        f"(containers={len(containers)} traces={len(traces)} templates={len(templates)})"
    )

    # 3) Create Craftax visual + save TSX
    visual = http_json(
        f"{base}/v1/visuals",
        method="POST",
        token=token,
        body={
            "templateId": "craftax.eval_matrix.v1",
            "title": "Dogfood Craftax matrix",
            "bindings": {"kind": "fixture"},
            "sessionId": local["id"],
        },
    )
    saved = http_json(
        f"{base}/v1/visuals/{visual['id']}/save-tsx",
        method="POST",
        token=token,
        body={},
    )
    results["visual_tsx"] = "PASS" if saved.get("tsxPath") and Path(saved["tsxPath"]).exists() else "FAIL"
    print(f"[dogfood] visual_tsx → {results['visual_tsx']} path={saved.get('tsxPath')}")

    # 4) Live eval stream visual
    live = http_json(
        f"{base}/v1/visuals/simulate-live",
        method="POST",
        token=token,
        body={"kind": "harbor"},
    )
    results["live_visual"] = "PASS" if live.get("visual", {}).get("id") else "FAIL"
    print(f"[dogfood] live_visual → {results['live_visual']}")

    # 5) OpenRouter models when key present
    if openrouter_key:
        for label, model in (
            ("openrouter_luna", "moonshotai/kimi-k2.5"),
            ("openrouter_laguna_s21", "poolside/laguna-s-2.1"),
        ):
            session = http_json(
                f"{base}/v1/sessions",
                method="POST",
                token=token,
                body={
                    "target": {
                        "kind": "remote",
                        "provider": "openrouter",
                        "model": model,
                        "adapter": None,
                    }
                },
            )
            try:
                http_json(
                    f"{base}/v1/sessions/{session['id']}/messages",
                    method="POST",
                    token=token,
                    body={
                        "body": (
                            f"Reply in one short sentence confirming you are {model} "
                            "and that Synth Desktop can track usage locally."
                        )
                    },
                )
                events = wait_for_run(base, session["id"], token, timeout=90)
                ok = any(e["eventKind"] == "run.completed" for e in events)
                failed = any(e["eventKind"] == "run.failed" for e in events)
                results[label] = "PASS" if ok and not failed else "FAIL"
                print(f"[dogfood] {label} → {results[label]} ({len(events)} events)")
            except urllib.error.HTTPError as exc:
                results[label] = f"FAIL HTTP {exc.code}"
                print(f"[dogfood] {label} → {results[label]}")
    else:
        results["openrouter_luna"] = "SKIP (no OPENROUTER_API_KEY)"
        results["openrouter_laguna_s21"] = "SKIP (no OPENROUTER_API_KEY)"
        print("[dogfood] OpenRouter skipped — set OPENROUTER_API_KEY to exercise Luna + Laguna S 2.1")

    # Intern demo
    sync = http_json(
        f"{base}/v1/sessions",
        method="POST",
        token=token,
        body={"target": {"kind": "intern", "mode": "sync"}},
    )
    http_json(
        f"{base}/v1/sessions/{sync['id']}/messages",
        method="POST",
        token=token,
        body={"body": "Demo sync hello"},
    )
    events = wait_for_run(base, sync["id"], token, timeout=20)
    results["intern_sync_demo"] = (
        "PASS" if any(e["eventKind"] == "run.completed" for e in events) else "FAIL"
    )
    print(f"[dogfood] intern_sync_demo → {results['intern_sync_demo']}")

    print("\n=== DOGFOOD SUMMARY ===")
    for key, value in results.items():
        print(f"  {key}: {value}")

    failed = [k for k, v in results.items() if v.startswith("FAIL")]
    server.shutdown()
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())

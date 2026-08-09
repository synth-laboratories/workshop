#!/usr/bin/env python3
"""Exercise the Craftax Rust workbench loop across local, Intern, and Luna.

The script prefers the already-running desktop runtime so a real Laguna sidecar
and configured OpenRouter account are exercised. It falls back to an isolated
demo runtime for deterministic CI/local verification.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "services" / "local-runtime" / "src"))

from synth_local_runtime.api import RuntimeHTTPServer  # noqa: E402
from synth_local_runtime.config import RuntimeConfig  # noqa: E402
from synth_local_runtime.service import RuntimeService  # noqa: E402


def request_json(
    base: str,
    path: str,
    *,
    method: str = "GET",
    body: dict[str, Any] | None = None,
    token: str | None = None,
) -> Any:
    data = json.dumps(body).encode("utf-8") if body is not None else None
    headers = {"Accept": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if body is not None:
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(f"{base}{path}", data=data, headers=headers, method=method)
    with urllib.request.urlopen(req, timeout=90) as response:
        return json.loads(response.read().decode("utf-8"))


def wait_for_run(base: str, session_id: str, token: str | None, timeout: float = 120) -> list[dict[str, Any]]:
    deadline = time.time() + timeout
    cursor = 0
    events: list[dict[str, Any]] = []
    while time.time() < deadline:
        page = request_json(
            base,
            f"/v1/sessions/{session_id}/events?after_sequence={cursor}&limit=500",
            token=token,
        )
        batch = page.get("events") or []
        events.extend(batch)
        if batch:
            cursor = max(cursor, max(int(event["sequence"]) for event in batch))
        if any(event["eventKind"] in {"run.completed", "run.failed", "run.cancelled"} for event in events):
            return events
        time.sleep(0.25)
    return events


def fixture(name: str) -> dict[str, Any]:
    return json.loads((ROOT / "visuals" / "fixtures" / name).read_text(encoding="utf-8"))


def start_fallback_runtime() -> tuple[str, str, Any, tempfile.TemporaryDirectory[str] | None]:
    data_dir = tempfile.TemporaryDirectory(prefix="synth-craftax-dogfood-")
    os.environ["SYNTH_INTERN_DEMO"] = "1"
    os.environ["SYNTH_WORKSHOP_ROOT"] = str(ROOT)
    os.environ["SYNTH_VISUALS_ROOT"] = str(ROOT / "visuals")
    config = RuntimeConfig.from_env(host="127.0.0.1", port=0, data_dir=data_dir.name)
    service = RuntimeService(config)
    server = RuntimeHTTPServer(("127.0.0.1", 0), service, token="craftax-dogfood")
    import threading

    threading.Thread(target=server.serve_forever, daemon=True).start()
    host, port = server.server_address[:2]
    return f"http://{host}:{port}", "craftax-dogfood", server, data_dir


def existing_runtime() -> tuple[str, str | None] | None:
    path = Path.home() / ".synth-desktop" / "runtime" / "connection.json"
    try:
        connection = json.loads(path.read_text(encoding="utf-8"))
        base = str(connection["url"]).rstrip("/")
        token = connection.get("token") if isinstance(connection.get("token"), str) else None
        health = request_json(base, "/v1/health", token=token)
        if health.get("status") == "ok":
            return base, token
    except (OSError, KeyError, json.JSONDecodeError, urllib.error.URLError):
        return None
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", type=Path, default=None)
    parser.add_argument("--new-runtime", action="store_true")
    args = parser.parse_args()

    fallback_server = None
    temp_dir = None
    connection = None if args.new_runtime else existing_runtime()
    if connection:
        base, token = connection
        runtime_kind = "desktop-runtime"
    else:
        base, token, fallback_server, temp_dir = start_fallback_runtime()
        runtime_kind = "isolated-demo-runtime"

    health = request_json(base, "/v1/health", token=token)
    report: dict[str, Any] = {
        "schema": "synth.craftax.dogfood.v1",
        "runtime": runtime_kind,
        "health": {
            "intern": health.get("intern"),
            "local": health.get("local"),
            "openrouter": health.get("openrouter"),
        },
        "project": None,
        "examples": [],
        "traces": [],
        "visuals": [],
    }

    projects = request_json(base, "/v1/projects", token=token).get("projects", [])
    project = next((item for item in projects if item.get("path") == str(ROOT)), None)
    if project is None:
        project = request_json(
            base,
            "/v1/projects",
            method="POST",
            token=token,
            body={"path": str(ROOT), "name": "Workshop · Craftax Rust"},
        )
    report["project"] = {"id": project["id"], "name": project["name"], "path": project["path"]}

    examples = [
        (
            "local-laguna",
            {"kind": "local", "model": "laguna-xs-2.1", "adapter": "craftax-triage.lora"},
            "Craftax Rust: inspect the rollout parser and identify the smallest safe change for reward attribution.",
        ),
        (
            "intern-sync",
            {"kind": "intern", "mode": "sync"},
            "Craftax Rust: review the trace exporter contract and propose a focused harness-compatible fix.",
        ),
        (
            "intern-async",
            {"kind": "intern", "mode": "async"},
            "Craftax Rust: compare rollout failure clusters and leave a checkpoint with the next evaluation slice.",
        ),
    ]
    if health.get("openrouter", {}).get("mode") == "ready":
        examples.append(
            (
                "luna-cloud",
                {"kind": "remote", "provider": "openrouter", "model": "moonshotai/kimi-k2.5", "adapter": None},
                "Craftax Rust: in one concise pass, explain how to preserve Trace V5 provenance while fixing reward breakdown rendering.",
            )
        )

    sessions: list[tuple[str, dict[str, Any], list[dict[str, Any]]]] = []
    for label, target, prompt in examples:
        try:
            session = request_json(
                base,
                "/v1/sessions",
                method="POST",
                token=token,
                body={"target": target, "title": f"Craftax Rust · {label}", "projectId": project["id"], "metadata": {"craftaxTask": label, "promptVariant": "rust-provenance-v1"}},
            )
            run = request_json(
                base,
                f"/v1/sessions/{session['id']}/messages",
                method="POST",
                token=token,
                body={"body": prompt},
            )
            events = wait_for_run(base, session["id"], token)
            terminal = next((event for event in reversed(events) if event["eventKind"].startswith("run.")), None)
            record = {
                "label": label,
                "sessionId": session["id"],
                "runId": run.get("runId"),
                "target": target,
                "prompt": prompt,
                "status": "PASS" if any(event["eventKind"] == "run.completed" for event in events) else "FAIL",
                "eventKinds": sorted({event["eventKind"] for event in events}),
                "terminal": terminal,
            }
            report["examples"].append(record)
            sessions.append((label, session, events))
        except Exception as exc:  # keep the rest of the matrix visible
            report["examples"].append({"label": label, "status": "FAIL", "error": str(exc)})

    matrix = fixture("craftax_matrix_slice.json")
    rollout = fixture("rollout_steps.json")
    reward = fixture("reward_breakdown.json")
    compare = fixture("model_compare.json")
    markers = fixture("annotation_markers.json")
    for name, payload, title, session_id in (
        ("craftax.eval_matrix.v1", {"data": matrix}, "Craftax Rust · harness/model matrix", sessions[0][1]["id"] if sessions else None),
        ("craftax.rollout_scrub.v1", {"data": rollout}, "Craftax Rust · rollout scrub", sessions[0][1]["id"] if sessions else None),
        ("reward.breakdown.v1", {"data": reward}, "Craftax Rust · reward provenance", sessions[1][1]["id"] if len(sessions) > 1 else None),
        ("model.compare.v1", {"data": compare}, "Craftax Rust · local vs Luna", sessions[-1][1]["id"] if sessions else None),
        ("annotation.overlay.v1", {"data": {"trace": rollout, "annotations": markers}}, "Craftax Rust · sealed trace review", sessions[0][1]["id"] if sessions else None),
    ):
        body = {"templateId": name, "title": title, "bindings": payload, "metadata": {"craftax": True, "source": "dogfood_craftax", "runtime": runtime_kind}}
        if session_id:
            body["sessionId"] = session_id
        visual = request_json(base, "/v1/visuals", method="POST", token=token, body=body)
        report["visuals"].append({"id": visual["id"], "templateId": name, "title": title, "bindings": payload})

    traces = [fixture("rollout_steps.json"), fixture("reward_breakdown.json"), fixture("craftax_matrix_slice.json")]
    for index, payload in enumerate(traces):
        session_id = sessions[min(index, len(sessions) - 1)][1]["id"] if sessions else None
        record = request_json(
            base,
            "/v1/traces",
            method="POST",
            token=token,
            body={
                "title": f"Craftax Rust · dogfood trace {index + 1}",
                "payload": {"schema": "synth.trace.v5", "taskFamily": "craftax-rust", "harness": "craftax-rust-v1", "promptVariant": "rust-provenance-v1", "fixture": payload},
                "source": "local" if index == 0 else "cloud",
                "sessionId": session_id,
                "reward": payload.get("total_reward") or payload.get("total") or 11.4,
                "metrics": [{"name": "craftax_reward", "value": float(payload.get("total_reward") or payload.get("total") or 11.4)}, {"name": "tool_events", "value": float(sum(1 for item in (sessions[index][2] if index < len(sessions) else []) if item["eventKind"].startswith("tool.")))}],
                "metadata": {"taskFamily": "craftax-rust", "traceContract": "Trace V5", "sealed": True},
            },
        )
        report["traces"].append({"id": record["id"], "digest": record["digest"], "title": record["title"]})

    report["summary"] = {
        "examplesPassed": sum(1 for item in report["examples"] if item.get("status") == "PASS"),
        "examplesTotal": len(report["examples"]),
        "visualsCreated": len(report["visuals"]),
        "tracesCreated": len(report["traces"]),
        "toolActivitySeen": sum(1 for item in report["examples"] if any(kind.startswith("tool.") for kind in item.get("eventKinds", []))),
    }

    print(json.dumps(report, indent=2, ensure_ascii=False))
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    if fallback_server is not None:
        fallback_server.shutdown()
        fallback_server.server_close()
    if temp_dir is not None:
        temp_dir.cleanup()
    return 0 if report["summary"]["examplesPassed"] == report["summary"]["examplesTotal"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

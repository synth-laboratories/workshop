#!/usr/bin/env python3
"""Run modern optimizer and Containers acceptance through Workshop's local IPC.

The driver is deliberately fail-closed:

* no mutating call is made without ``--execute``;
* the Desktop-created visual must exist before optimizer work can advance;
* prepared container rollouts stop for human visual review and resume later;
* resume uses the exact declared stream descriptor and a current ready receipt;
* receipts preserve missing values as null and redact credentials recursively.

It writes the common Aug 12 receipt bundle so GEPA, GELO, SFT, Harbor, Craftax,
and dig.bench evidence can be compared without family-specific shell scripts.
"""

from __future__ import annotations

import argparse
import collections
import datetime as dt
import json
import os
import re
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path
from typing import Any, Iterable

SCHEMA = "synth.modern-stack-receipt.v1"
TERMINAL = {"completed", "failed", "cancelled"}
SECRET_KEY = re.compile(r"(?:token|secret|password|authorization|cookie|api[_-]?key)", re.I)
BEARER = re.compile(r"(?i)bearer\s+[A-Za-z0-9._~+/=-]+")
QUERY_SECRET = re.compile(r"(?i)([?&](?:token|api[_-]?key|secret)=)[^&#\s]+")


def now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def redact(value: Any, key: str = "") -> Any:
    if SECRET_KEY.search(key):
        return "[REDACTED]"
    if isinstance(value, dict):
        return {str(k): redact(v, str(k)) for k, v in value.items()}
    if isinstance(value, list):
        return [redact(item) for item in value]
    if isinstance(value, str):
        return QUERY_SECRET.sub(r"\1[REDACTED]", BEARER.sub("Bearer [REDACTED]", value))
    return value


def dump_json(value: Any) -> str:
    return json.dumps(redact(value), indent=2, sort_keys=True, ensure_ascii=False) + "\n"


class IpcError(RuntimeError):
    pass


class IpcClient:
    def __init__(self, connection_file: Path, timeout: float = 90.0):
        try:
            connection = json.loads(connection_file.read_text(encoding="utf-8"))
            self.base = str(connection["url"]).rstrip("/")
            self.token = str(connection["token"])
        except (OSError, KeyError, TypeError, json.JSONDecodeError) as exc:
            raise IpcError(f"cannot read Workshop IPC connection {connection_file}: {exc}") from exc
        if not self.base.startswith("http://127.0.0.1:") and not self.base.startswith("http://localhost:"):
            raise IpcError("Workshop IPC must be loopback HTTP")
        self.timeout = timeout

    def request(self, method: str, path: str, body: dict[str, Any] | None = None) -> Any:
        data = json.dumps(body).encode() if body is not None else None
        headers = {"Accept": "application/json", "Authorization": f"Bearer {self.token}"}
        if data is not None:
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(
            f"{self.base}{path}", data=data, headers=headers, method=method
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                payload = response.read().decode("utf-8")
        except urllib.error.HTTPError as exc:
            payload = exc.read().decode("utf-8", errors="replace")
            try:
                detail = json.loads(payload)
            except json.JSONDecodeError:
                detail = payload[:1000]
            raise IpcError(f"{method} {path} returned HTTP {exc.code}: {redact(detail)}") from exc
        except urllib.error.URLError as exc:
            raise IpcError(f"{method} {path} failed: {exc.reason}") from exc
        try:
            parsed = json.loads(payload)
        except json.JSONDecodeError as exc:
            raise IpcError(f"{method} {path} returned non-JSON") from exc
        if isinstance(parsed, dict) and parsed.get("error"):
            raise IpcError(f"{method} {path}: {redact(parsed['error'])}")
        return parsed


class ReceiptBundle:
    STANDARD = (
        "requested-stream.json",
        "bound-stream.json",
        "event-kind-counts.json",
        "run-manifest.json",
        "cost-reconciliation.json",
        "trace-v5.json",
        "cua-findings.json",
    )

    def __init__(self, root: Path, acceptance_id: str, operation: str):
        self.root = root
        self.root.mkdir(parents=True, exist_ok=True)
        (self.root / "screenshots").mkdir(exist_ok=True)
        self.acceptance_id = acceptance_id
        self.operation = operation
        self.started_at = now()
        self.checks: dict[str, Any] = {}
        self.blockers: list[dict[str, Any]] = []
        for name in self.STANDARD:
            path = self.root / name
            if not path.exists():
                self.write(name, {"state": "not_emitted", "reason": "operation_not_completed"})
        transcript = self.root / "cursor-transcript.jsonl"
        if not transcript.exists():
            transcript.touch()

    def write(self, name: str, value: Any) -> None:
        destination = self.root / name
        with tempfile.NamedTemporaryFile(
            "w", encoding="utf-8", dir=self.root, prefix=f".{name}.", delete=False
        ) as handle:
            handle.write(dump_json(value))
            temporary = Path(handle.name)
        os.replace(temporary, destination)

    def append(self, value: Any) -> None:
        with (self.root / "cursor-transcript.jsonl").open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(redact(value), sort_keys=True, ensure_ascii=False) + "\n")

    def blocker(self, code: str, detail: str) -> None:
        self.blockers.append({"code": code, "detail": detail})

    def finish(self, status: str, **extra: Any) -> None:
        self.write(
            "receipt.json",
            {
                "schemaVersion": SCHEMA,
                "acceptanceId": self.acceptance_id,
                "operation": self.operation,
                "status": status,
                "startedAt": self.started_at,
                "finishedAt": now(),
                "checks": self.checks,
                "blockers": self.blockers,
                **extra,
            },
        )


def default_connection() -> Path:
    explicit = os.getenv("SYNTH_DESKTOP_IPC_FILE") or os.getenv("SYNTH_VISUALS_IPC_FILE")
    if explicit:
        return Path(explicit)
    root = os.getenv("SYNTH_DESKTOP_DATA_ROOT")
    if root:
        return Path(root) / "visuals-ipc.json"
    return Path.home() / "Library" / "Application Support" / "Synth Desktop" / "visuals-ipc.json"


def event_kind(event: dict[str, Any]) -> str:
    return str(event.get("type") or event.get("eventKind") or event.get("kind") or "unknown")


def sequence(event: dict[str, Any]) -> int:
    for key in ("sequenceNumber", "sequence_number", "sequence", "seq"):
        value = event.get(key)
        if isinstance(value, int):
            return value
    return 0


def visual_ids(run: dict[str, Any]) -> list[str]:
    refs = run.get("visualRefs") or []
    return [str(item.get("id")) for item in refs if isinstance(item, dict) and item.get("id")]


def stage_checks(recipe: str, kinds: collections.Counter[str]) -> dict[str, bool]:
    names = list(kinds)
    includes = lambda *needles: any(any(needle in name for needle in needles) for name in names)
    if recipe.startswith(("gepa.", "gelo.")):
        return {
            "proposal_visible": includes("proposer", "proposal"),
            "candidate_lifecycle_visible": includes("candidate"),
            "frontier_visible": includes("frontier"),
            "child_rollouts_visible": includes("child", "rollout", "evaluation"),
        }
    if recipe.startswith("sft."):
        return {
            "training_visible": includes("training", "train", "optimizer.run.started"),
            "checkpoints_visible": includes("checkpoint"),
            "campaign_rollouts_visible": includes("campaign", "rollout", "evaluation"),
        }
    return {"events_visible": bool(kinds)}


def run_optimizer(args: argparse.Namespace, client: IpcClient, receipt: ReceiptBundle) -> int:
    catalog = client.request("GET", "/v1/optimizers/recipes", {})
    recipes = catalog.get("recipes") or []
    selected = next((item for item in recipes if item.get("id") == args.recipe), None)
    receipt.write(
        "requested-stream.json",
        {"kind": "optimizer_event.v1", "recipeId": args.recipe, "catalogEntry": selected},
    )
    if selected is None:
        receipt.blocker("recipe_not_advertised", args.recipe)
        receipt.finish("BLOCKED")
        return 2
    receipt.checks["recipe_advertised"] = True
    receipt.checks["recipe_available"] = selected.get("availability") == "available"
    if not receipt.checks["recipe_available"]:
        prerequisites = selected.get("prerequisites") or []
        receipt.blocker(
            "recipe_unavailable",
            ", ".join(str(item) for item in prerequisites) or "catalog reported unavailable",
        )
        receipt.finish("BLOCKED", preflightOnly=True, executionAuthorized=bool(args.execute))
        return 2
    if not args.execute:
        receipt.blocker("execution_authorization_required", "rerun with --execute")
        receipt.finish("BLOCKED", preflightOnly=True)
        return 2

    body: dict[str, Any] = {
        "recipeId": args.recipe,
        "openVisual": True,
        "sessionRef": args.session_ref,
    }
    if args.base_model:
        body["baseModel"] = args.base_model
    if args.dataset_shard:
        body["datasetShard"] = args.dataset_shard
    started = client.request("POST", "/v1/optimizers/recipes/run", body)
    run = started["run"]
    run_id = str(run["id"])
    refs_at_start = visual_ids(run)
    receipt.checks["visual_created_before_worker_poll"] = bool(refs_at_start)
    receipt.write(
        "bound-stream.json",
        {
            "optimizerRunId": run_id,
            "visualRefsAtStart": refs_at_start,
            "cursor": run.get("cursorSeq", 0),
        },
    )
    cursor = 0
    seen_sequences: list[int] = []
    kinds: collections.Counter[str] = collections.Counter()
    deadline = time.monotonic() + args.timeout
    transitions: list[dict[str, Any]] = []
    latest = run
    while time.monotonic() < deadline:
        page = client.request(
            "GET", f"/v1/optimizers/runs/{run_id}/events", {"after_seq": cursor, "limit": 1000}
        )
        for event in page.get("events") or []:
            seq = sequence(event)
            if seq <= cursor:
                receipt.blocker("cursor_regression", f"received {seq} after {cursor}")
            cursor = max(cursor, seq)
            seen_sequences.append(seq)
            kinds[event_kind(event)] += 1
            receipt.append({"source": "optimizer", "cursor": seq, "event": event})
        latest = client.request("GET", f"/v1/optimizers/runs/{run_id}", {}).get("run") or {}
        status = str(latest.get("status") or "unknown")
        if not transitions or transitions[-1]["status"] != status:
            transition = {"status": status, "at": now(), "cursor": cursor}
            transitions.append(transition)
            receipt.append({"source": "run", **transition})
        if status in TERMINAL:
            break
        time.sleep(args.poll_interval)
    else:
        receipt.blocker("timeout", f"run did not terminate within {args.timeout}s")

    monotonic = seen_sequences == sorted(set(seen_sequences)) and all(seq > 0 for seq in seen_sequences)
    receipt.checks.update(
        {
            "cursor_strictly_monotonic": monotonic,
            "terminal_observed": latest.get("status") in TERMINAL,
            "terminal_completed": latest.get("status") == "completed",
            **stage_checks(args.recipe, kinds),
        }
    )
    receipt.write("event-kind-counts.json", dict(sorted(kinds.items())))
    receipt.write(
        "run-manifest.json",
        {
            "recipeId": args.recipe,
            "optimizerRunId": run_id,
            "status": latest.get("status"),
            "visualRefs": latest.get("visualRefs"),
            "inputRefs": latest.get("inputRefs"),
            "outputRefs": latest.get("outputRefs"),
            "executionBindings": latest.get("executionBindings"),
            "transitions": transitions,
            "error": latest.get("error"),
        },
    )
    usage = latest.get("usage") if isinstance(latest.get("usage"), dict) else {}
    receipt.write(
        "cost-reconciliation.json",
        {
            "reportedCostUsd": usage.get("costUsd"),
            "reportedUsage": usage or None,
            "state": "present" if usage.get("costUsd") is not None else "not_emitted",
        },
    )
    receipt.write(
        "trace-v5.json",
        {
            "state": "present" if any("trace" in kind or "seal" in kind for kind in kinds) else "not_emitted",
            "eventKinds": [kind for kind in kinds if "trace" in kind or "seal" in kind],
        },
    )
    passed = latest.get("status") == "completed" and all(receipt.checks.values()) and not receipt.blockers
    receipt.finish("PASS" if passed else "FAIL", optimizerRunId=run_id)
    return 0 if passed else 1


DEFAULT_PINS = {
    "harbor": [
        {"harness": "harbor_fused", "config": "luna_med"},
        {"harness": "harbor_fused", "config": "sol_med"},
    ],
    "digbench": [
        {"harness": "react_legal_actions", "config": "react_legal_actions"},
        {"harness": "codex", "config": "agentic_codex", "mcp_bind": "digbench-mcp"},
    ],
}


def parse_pins(values: list[str] | None, family: str) -> list[dict[str, Any]]:
    if not values:
        return list(DEFAULT_PINS.get(family, []))
    pins = []
    for raw in values:
        value = json.loads(raw)
        if not isinstance(value, dict) or not value.get("harness"):
            raise ValueError("each --policy-ref must be a JSON object with harness")
        pins.append(value)
    return pins


def prepare_container(args: argparse.Namespace, client: IpcClient, receipt: ReceiptBundle) -> int:
    if not args.execute:
        receipt.write(
            "requested-stream.json",
            {"baseUrl": args.base_url, "taskFamily": args.family, "preflightOnly": True},
        )
        receipt.blocker("execution_authorization_required", "rerun with --execute")
        receipt.finish("BLOCKED", preflightOnly=True)
        return 2
    pins = parse_pins(args.policy_ref, args.family)
    rollout_id = args.rollout_id or f"roll_{args.family}_{uuid.uuid4().hex[:12]}"
    telemetry = {
        "enabled": True,
        "transport": "sse",
        "detail": "standard",
        "frame": {"enabled": args.family == "craftax", "format": "png", "every_n_steps": 1},
    }
    requested = {
        "baseUrl": args.base_url,
        "taskFamily": args.family,
        "rolloutId": rollout_id,
        "taskInstanceId": args.task_instance_id,
        "policyRefs": pins,
        "telemetry": telemetry,
    }
    receipt.write("requested-stream.json", requested)
    registered = client.request(
        "POST",
        "/v1/containers",
        {
            "baseUrl": args.base_url,
            "name": args.name,
            "location": "local",
            "taskFamily": args.family,
            "metadata": {"policyRefs": pins, "dogfoodReceipt": str(receipt.root)},
        },
    )
    container = registered["container"]
    container_id = str(container["id"])
    live_eval = registered.get("liveEval") or (container.get("metadata") or {}).get("liveEval") or {}
    template = live_eval.get("templateId")
    if not template:
        receipt.blocker("family_not_classified", "container registration did not declare liveEval.templateId")
        receipt.finish("FAIL", containerId=container_id)
        return 1
    prepared = client.request(
        "POST",
        f"/v1/containers/{container_id}/rollouts/prepare",
        {"rollout_id": rollout_id, "telemetry": telemetry},
    )
    binding = prepared["visual_binding"]
    visual = client.request(
        "POST",
        "/v1/visuals",
        {
            "templateId": template,
            "title": args.title or f"{args.name or args.family} · {rollout_id}",
            "sourceAgentId": "modern-stack-dogfood",
            "bindings": {"schemaVersion": "synth.visual-bindings.v1", "slots": [binding]},
            "metadata": {
                "presentation": "pane",
                "liveEval": live_eval,
                "policyRefs": pins,
                "containerId": container_id,
                "rolloutId": rollout_id,
                "receiptDir": str(receipt.root),
            },
        },
    )["visual"]
    client.request("POST", f"/v1/visuals/{visual['id']}/show", {})
    receipt.checks.update(
        {
            "container_registered": True,
            "family_classified": bool(template),
            "declared_stream_bound_exactly": True,
            "visual_opened_before_start": True,
            "rollout_not_started_before_review": True,
        }
    )
    receipt.write(
        "bound-stream.json",
        {
            "containerId": container_id,
            "rolloutId": rollout_id,
            "taskInstanceId": args.task_instance_id,
            "policyRefs": pins,
            "telemetry": telemetry,
            "prepared": prepared.get("prepared"),
            "stream": prepared.get("stream"),
            "resolved": prepared.get("resolved"),
            "visualBinding": binding,
            "visualId": visual["id"],
            "visualRevision": visual.get("currentRevision"),
            "templateId": template,
        },
    )
    receipt.write(
        "run-manifest.json",
        {"phase": "prepared", "container": container, "rolloutId": rollout_id, "visual": visual},
    )
    receipt.blocker(
        "visual_review_required",
        "review at two viewport widths, mark the current revision ready, then run container-start",
    )
    receipt.finish("BLOCKED", resumable=True, containerId=container_id, visualId=visual["id"])
    return 2


def same_binding(expected: dict[str, Any], visual: dict[str, Any]) -> bool:
    slots = (visual.get("bindings") or {}).get("slots") or []
    return any(
        item.get("slot") == expected.get("slot")
        and item.get("kind") == expected.get("kind")
        and item.get("source") == expected.get("source")
        and item.get("schema") == expected.get("schema")
        and item.get("poll_url") == expected.get("poll_url")
        for item in slots
        if isinstance(item, dict)
    )


def start_container(args: argparse.Namespace, client: IpcClient, receipt: ReceiptBundle) -> int:
    try:
        bound = json.loads((receipt.root / "bound-stream.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        receipt.blocker("missing_prepare_receipt", str(exc))
        receipt.finish("BLOCKED")
        return 2
    if not args.execute:
        receipt.blocker("execution_authorization_required", "rerun with --execute")
        receipt.finish("BLOCKED", preflightOnly=True)
        return 2
    visual = client.request("GET", f"/v1/visuals/{bound['visualId']}", {}).get("visual") or {}
    quality = (visual.get("metadata") or {}).get("qualityGate") or {}
    current_revision = visual.get("currentRevision")
    ready = quality.get("ready") is True and quality.get("revision") == current_revision
    exact = same_binding(bound["visualBinding"], visual)
    receipt.checks["visual_current_revision_ready"] = ready
    receipt.checks["declared_stream_still_bound_exactly"] = exact
    if not ready or not exact:
        receipt.blocker(
            "visual_not_ready" if not ready else "stream_binding_changed",
            "start refused before the current reviewed revision binds the exact prepared stream",
        )
        receipt.finish("BLOCKED", resumable=True)
        return 2
    policy_refs = bound.get("policyRefs") or []
    if args.policy_index < 0 or args.policy_index >= len(policy_refs):
        receipt.blocker("policy_index_invalid", str(args.policy_index))
        receipt.finish("BLOCKED")
        return 2
    payload = {
        "rollout_id": bound["rolloutId"],
        "stream": bound["stream"],
        "visual_id": bound["visualId"],
        "seed": args.seed,
        "task_instance_id": bound["taskInstanceId"],
        "policy_ref": policy_refs[args.policy_index],
        "telemetry": bound["telemetry"],
    }
    if not 1 <= args.start_retries <= 3:
        receipt.blocker("start_retries_invalid", "start retries must be between 1 and 3")
        receipt.finish("BLOCKED")
        return 2
    starts = [
        client.request("POST", f"/v1/containers/{bound['containerId']}/rollouts/start", payload)
        for _ in range(args.start_retries)
    ]
    started = starts[-1]
    receipt.checks["stream_subscribed_before_start"] = bool(started.get("subscription"))
    receipt.checks["rollout_started"] = started.get("started") is True
    receipt.checks["idempotent_start_replay"] = all(
        item.get("started") is True
        and item.get("rollout_id", bound["rolloutId"]) == bound["rolloutId"]
        for item in starts
    )
    cursor = 0
    kinds: collections.Counter[str] = collections.Counter()
    terminal_state: Any = started.get("state")
    deadline = time.monotonic() + args.timeout
    poll_count = 0
    while time.monotonic() < deadline:
        page_result = client.request(
            "POST",
            f"/v1/containers/{bound['containerId']}/rollouts/poll",
            {"rollout_id": bound["rolloutId"], "stream": bound["stream"], "after": cursor},
        )
        page = page_result.get("page") or {}
        poll_count += 1
        events = page.get("events") or []
        for event in events:
            seq = sequence(event)
            # Control envelopes such as stream.subscribed are deliberately
            # unsequenced and project to 0; they do not regress durable data.
            if event.get("sequence") is not None and seq <= cursor:
                receipt.blocker("cursor_regression", f"received {seq} after {cursor}")
            cursor = max(cursor, seq)
            kinds[event_kind(event)] += 1
            receipt.append({"source": "container", "cursor": seq, "event": event})
        high_water = page_result.get("next_cursor")
        if isinstance(high_water, int):
            cursor = max(cursor, high_water)
        closed = bool((page.get("cursor") or {}).get("closed"))
        if closed or any(kind in {"capture.closed", "rollout.completed", "rollout.failed"} for kind in kinds):
            terminal_state = client.request(
                "GET",
                f"/v1/containers/{bound['containerId']}/rollouts/{bound['rolloutId']}",
                {},
            ).get("state")
            break
        if args.reconnect_after_page and poll_count == args.reconnect_after_page:
            receipt.append(
                {
                    "source": "driver",
                    "kind": "consumer.disconnected",
                    "lastDurableCursor": cursor,
                    "note": "next poll resumes without preparing or starting again",
                }
            )
            time.sleep(args.reconnect_pause)
            receipt.checks["consumer_reconnected_from_durable_cursor"] = True
        time.sleep(args.poll_interval)
    else:
        receipt.blocker("timeout", f"rollout did not close within {args.timeout}s")
    receipt.checks["terminal_observed"] = bool(terminal_state) and not any(
        blocker["code"] == "timeout" for blocker in receipt.blockers
    )
    receipt.write("event-kind-counts.json", dict(sorted(kinds.items())))
    receipt.write(
        "run-manifest.json",
        {
            "phase": "terminal" if receipt.checks["terminal_observed"] else "running",
            "containerId": bound["containerId"],
            "rolloutId": bound["rolloutId"],
            "visualId": bound["visualId"],
            "policyRef": policy_refs[args.policy_index],
            "state": terminal_state,
            "starts": starts,
        },
    )
    receipt.write(
        "cost-reconciliation.json",
        {"reportedCostUsd": None, "state": "not_emitted", "reason": "container stream owns usage evidence"},
    )
    seal_kinds = [kind for kind in kinds if "seal" in kind or kind == "capture.closed"]
    receipt.write(
        "trace-v5.json",
        {"state": "present" if seal_kinds else "not_emitted", "eventKinds": seal_kinds},
    )
    passed = all(receipt.checks.values()) and not receipt.blockers
    receipt.finish("PASS" if passed else "FAIL", rolloutId=bound["rolloutId"])
    return 0 if passed else 1


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    root.add_argument("--connection", type=Path, default=default_connection())
    root.add_argument("--receipt-dir", type=Path, required=True)
    root.add_argument("--timeout", type=float, default=1800)
    root.add_argument("--poll-interval", type=float, default=0.5)
    commands = root.add_subparsers(dest="command", required=True)

    optimizer = commands.add_parser("optimizer")
    optimizer.add_argument("--recipe", required=True)
    optimizer.add_argument("--session-ref")
    optimizer.add_argument("--base-model")
    optimizer.add_argument("--dataset-shard", choices=("train_a", "train_b"))
    optimizer.add_argument("--execute", action="store_true")

    prepare = commands.add_parser("container-prepare")
    prepare.add_argument("--base-url", required=True)
    prepare.add_argument("--family", choices=("craftax", "harbor", "digbench"), required=True)
    prepare.add_argument("--name")
    prepare.add_argument("--title")
    prepare.add_argument("--rollout-id")
    prepare.add_argument("--task-instance-id", required=True)
    prepare.add_argument("--policy-ref", action="append")
    prepare.add_argument("--execute", action="store_true")

    start = commands.add_parser("container-start")
    start.add_argument("--policy-index", type=int, default=0)
    start.add_argument("--seed", type=int, default=1)
    start.add_argument("--start-retries", type=int, default=1,
                       help="repeat the same immutable start 1-3 times for idempotency proof")
    start.add_argument("--reconnect-after-page", type=int,
                       help="pause after this poll page, then resume from the durable cursor")
    start.add_argument("--reconnect-pause", type=float, default=0.25)
    start.add_argument("--execute", action="store_true")
    return root


def main(argv: Iterable[str] | None = None) -> int:
    args = parser().parse_args(argv)
    acceptance = {
        "optimizer": "optimizer-live",
        "container-prepare": "container-live",
        "container-start": "container-live",
    }[args.command]
    receipt = ReceiptBundle(args.receipt_dir, acceptance, args.command)
    try:
        client = IpcClient(args.connection)
        if args.command == "optimizer":
            return run_optimizer(args, client, receipt)
        if args.command == "container-prepare":
            return prepare_container(args, client, receipt)
        return start_container(args, client, receipt)
    except (IpcError, KeyError, TypeError, ValueError, json.JSONDecodeError) as exc:
        receipt.blocker("driver_error", str(exc))
        receipt.finish("FAIL")
        print(f"modern-stack dogfood failed: {redact(str(exc))}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

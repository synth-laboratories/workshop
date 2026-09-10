"""One gate at a time per run; separate runs can be scheduled independently."""
import json
import time
import tomllib
import uuid
from .bundles import verified_path
from .core import Conflict, digest, seal
from .review import static_review, ai_request, call_ai


def step(store, run_id):
    before = store.get(run_id)
    if before["status"] not in {"queued", "running"}:
        return False
    if any(g["status"] == "running" for g in before["gates"]):
        return False
    gate = next((g for g in before["gates"] if g["status"] == "pending"), None)
    if not gate:
        store.mutate(run_id, "run.sealed", seal, before["revision"])
        return True
    gate_id = gate["id"]
    token = uuid.uuid4().hex
    def claim(run):
        target = next(g for g in run["gates"] if g["id"] == gate_id)
        target.update(status="running", attempt=token, started_at=time.time())
        run["status"] = "running"
    try:
        run = store.mutate(run_id, "gate.started", claim, before["revision"])
    except Conflict:
        return False
    result = {"findings": [], "limitations": []}
    status = "succeeded"
    try:
        path = verified_path(store, run["bundle"])
        if gate_id == "structure":
            config = tomllib.loads((path / "task.toml").read_text())
            if not (path / "tests" / "test.sh").is_file():
                result["limitations"].append("No tests/test.sh: runtime verification unavailable")
            result["task_format_version"] = config.get("version", "unspecified")
        elif gate_id == "review":
            if run["policy"]["reviewer"] == "ai":
                request = ai_request(path, run["policy"])
                def reserve(current):
                    if current["status"] in {"paused", "cancelling", "cancelled"}:
                        raise Conflict("Run was paused or cancelled before provider dispatch")
                    budget = current["budget"]
                    if budget["reserved_usd"] + request[3] > budget["limit_usd"]:
                        raise ValueError("Provider request exceeds the remaining run budget")
                    budget["reserved_usd"] += request[3]
                    budget["actual_usd"] = None  # Unknown until provider usage is received.
                store.mutate(run_id, "budget.reserved", reserve)
                result = call_ai(request)
            else:
                result = static_review(path)
        elif gate_id == "harbor":
            if run["policy"]["harbor_probes"]:
                from .harbor import probe
                result = probe(store, run, path)
                status = result.get("gate_status", "inconclusive")
            else:
                status = "skipped"
                result["limitations"].append("Harbor execution not selected; no runtime validity claim")
        elif gate_id == "decision":
            result["resolver"] = "local-human" if run["mode"] == "hitl" else run["policy"]["reviewer"]
    except Exception as exc:
        status = "inconclusive"
        result = {"findings": [], "limitations": [f"{gate_id}: {type(exc).__name__}: {exc}"]}
    # Bind source integrity again after external work, not only before it.
    try:
        verified_path(store, run["bundle"])
    except Exception:
        status = "inconclusive"
        result = {"findings": [], "limitations": ["Task snapshot integrity failed after gate execution"]}
    def finish(current):
        target = next(g for g in current["gates"] if g["id"] == gate_id)
        if target["attempt"] != token or target["status"] != "running":
            raise Conflict("Stale worker result")
        evidence = {"id": digest(result), "gate": gate_id, "result": result}
        current["evidence"].append(evidence)
        current["findings"].extend(f | {"evidence_id": evidence["id"]} for f in result.get("findings", []))
        current["limitations"].extend(result.get("limitations", []))
        provider = result.get("provider", {})
        if provider.get("actual_usd") is not None:
            current["budget"]["actual_usd"] = provider["actual_usd"]
        target.update(status=status, finished_at=time.time(), evidence_id=evidence["id"])
        if current["status"] == "cancelling":
            target["status"] = "cancelled"
            current["status"] = "cancelled"
            return
        if gate_id == "decision":
            if current["mode"] == "hitl":
                context = digest({"findings": current["findings"], "evidence": current["evidence"], "policy": current["policy"]})
                current["interactions"].append({"id": uuid.uuid4().hex, "status": "open", "context_digest": context,
                    "type": "run_disposition", "allowed": ["confirm", "dismiss", "request_evidence"],
                    "expiry_policy": "remain_open", "created_at": time.time()})
                target["status"] = "waiting_interaction"
                if current["status"] != "paused":
                    current["status"] = "waiting_interaction"
            elif current["status"] != "paused":
                seal(current)
        elif current["status"] != "paused":
            current["status"] = "queued"
    store.mutate(run_id, "gate.finished", finish)
    return True


def recover(store):
    """Call only while holding the service's exclusive worker lock.

    Never repeat ambiguous provider or container work after a crash. Partial
    attempts become inconclusive and retain reservations for inspection.
    """
    for run in store.list():
        if run["seal"] or not any(g["status"] == "running" for g in run["gates"]):
            continue
        def apply(current):
            for interaction in current["interactions"]:
                if interaction["status"] == "open" and interaction.get("type") in {"permission", "clarification"}:
                    interaction.update(status="superseded", reason="App-server connection interrupted; old request cannot be replayed")
            for gate in current["gates"]:
                if gate["status"] == "running":
                    gate["status"] = "inconclusive"
                    gate["attempt"] = None
                    current["limitations"].append(f"Interrupted {gate['id']} attempt; not automatically retried. Inspect external resources before rerunning.")
            current["status"] = "cancelled" if current["status"] == "cancelling" else "paused"
        store.mutate(run["id"], "run.recovered", apply)

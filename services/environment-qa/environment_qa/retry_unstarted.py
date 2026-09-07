"""Explicit recovery for capacity-denied attempts that never started a runtime."""
from .core import Conflict


def retry(store, run_id, body):
    if body.get("actor") not in {"agent-cua", "local-human"} or not str(body.get("reason", "")).strip():
        raise ValueError("Actor and reason are required")
    # Evidence is read again under the mutation's write lock below via SQL;
    # a currently running gate makes recovery ineligible.
    events = []
    cursor = 0
    while True:
        page = store.events(run_id, cursor)
        if not page: break
        events.extend(page); cursor = page[-1]["seq"]
    started = {(e["payload"].get("gate_id"),e["payload"].get("attempt")) for e in events if e["kind"] == "codex.process.started"}
    def apply(run):
        if run["status"] != "paused" or any(g["status"] == "running" for g in run["gates"]):
            raise Conflict("Pause and wait for active gates before recovery")
        candidates = []
        for gate in run["gates"]:
            if gate["status"] != "inconclusive":
                continue
            evidence = next((e for e in run["evidence"] if e["id"] == gate.get("evidence_id")), None)
            limits = (evidence or {}).get("result", {}).get("limitations", [])
            unstarted = ((gate["id"],gate.get("attempt")) not in started and
                         any("GateBlocked: Host app-server capacity exhausted" in line for line in limits))
            stopped = any(e["kind"] == "codex.process.stopped" and e["payload"].get("gate_id") == gate["id"]
                          and e["payload"].get("attempt") == gate.get("attempt") for e in events)
            interrupted_source = (body.get("include_interrupted_source") is True and stopped and gate.get("executor") in {"review", "contract_plan"}
                                  and any("ProtocolError:" in line for line in limits))
            if unstarted or interrupted_source:
                candidates.append((gate, interrupted_source))
        if not candidates:
            raise Conflict("No verified unstarted capacity-denied attempts")
        transport = run["budget"].get("transport", {}).get("calls", {}).values()
        for gate, interrupted_source in candidates:
            if not interrupted_source and any(c["gate_id"] == gate["id"] and c["attempt"] == gate["attempt"] for c in transport):
                raise Conflict("Provider activity prevents automatic retry")
            gate.setdefault("recovery", []).append({"old_attempt":gate["attempt"], "actor":body["actor"], "reason":body["reason"]})
            gate.update(status="pending", attempt=None)
        # Old evidence and failed attempt rows stay intact. No budget is refunded.
    return store.mutate(run_id,"runtime.unstarted.retry",apply,body["revision"],body["request_key"],body)

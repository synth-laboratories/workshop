"""What each gate is actually doing, assembled from records rather than inferred.

The run document says a gate is `running`; that is a claim about scheduling, not
evidence of work. This view answers the questions an operator actually asks -- which
process is this, what session, what has it done, what did it consume, and what is it
waiting for -- by reading the gate's own journal off the event log.

It never fabricates liveness. A gate that was launched but has produced no events
reports exactly that, because "started successfully" and "is doing something" are
different claims and only the second is worth showing.
"""
from __future__ import annotations

CODEX_PREFIX = "codex."

# The next thing a person has to do, per state. Absent from this map means the gate
# is proceeding on its own and needs nobody.
NEXT_ACTION = {
    "waiting_for_review": "A reviewer must resolve this gate's open interaction.",
    "waiting_for_permission": "A reviewer must approve or refuse the requested action.",
    "blocked": "Authority is missing; grant it explicitly or leave the gate blocked.",
    "failed": "Inspect the failure and start a new attempt; this one is spent.",
    "cancelled": "Cancelled. Start a new attempt if the work is still wanted.",
}


def gate_events(store, run_id):
    """Journal entries grouped by (gate, attempt), oldest first."""
    grouped = {}
    rows, cursor = [], 0
    while True:
        page = store.events(run_id, cursor)
        if not page: break
        rows.extend(page)
        cursor = page[-1]["seq"]
    for row in rows:
        if not row["kind"].startswith(CODEX_PREFIX):
            continue
        payload = row["payload"]
        key = (payload.get("gate_id"), payload.get("attempt"))
        grouped.setdefault(key, []).append({"seq": row["seq"], "at": payload.get("at", row["at"]),
                                            "kind": payload.get("kind", row["kind"]),
                                            "payload": payload.get("payload", {})})
    for entries in grouped.values():
        entries.sort(key=lambda e: e["seq"])
    return grouped


def summarise(entries):
    """Reduce one gate attempt's journal to the facts worth showing."""
    view = {"events": len(entries), "thread_id": None, "turn_ids": [], "pid": None,
            "runtime": None, "state": None, "blocked_reason": None, "usage": {},
            "rate_limits": None, "tools": [], "approvals": [], "started_at": None,
            "last_event_at": None, "declined_requests": [], "wait_seconds": 0.0}
    waiting_since = None
    for entry in entries:
        kind, payload = entry["kind"], entry["payload"]
        view["started_at"] = view["started_at"] or entry["at"]
        view["last_event_at"] = entry["at"]
        if kind == "gate.state":
            if waiting_since is not None:
                view["wait_seconds"] += max(0, entry["at"] - waiting_since)
                waiting_since = None
            if payload.get("to") in {"waiting_for_permission", "waiting_for_review"}:
                waiting_since = entry["at"]
            view["state"] = payload.get("to")
        elif kind == "server.initialized":
            view["runtime"] = payload.get("userAgent") or payload.get("serverInfo")
        elif kind == "thread.opened":
            view["thread_id"] = payload.get("threadId")
        elif kind == "turn.acknowledged" and payload.get("turnId"):
            view["turn_ids"].append(payload["turnId"])
        elif kind in {"process.started", "process.stopped"}:
            view["pid"] = payload.get("pid")
        elif kind == "thread/tokenUsage/updated":
            total = (payload.get("tokenUsage") or {}).get("total") or {}
            if total:
                view["usage"] = total
        elif kind == "account/rateLimits/updated":
            view["rate_limits"] = payload
        elif kind == "tool.result":
            view["tools"].append({"tool": payload.get("tool"), "ok": payload.get("ok")})
        elif kind == "approval.requested":
            view["approvals"].append({"method": payload.get("method"),
                                      "command": (payload.get("params") or {}).get("command")})
        elif kind == "approval.decided":
            if view["approvals"]:
                view["approvals"][-1].update(decision=payload.get("decision"), reason=payload.get("reason"),
                                             actor=payload.get("actor"), human=payload.get("human"))
        elif kind == "request.declined":
            view["declined_requests"].append(payload.get("method"))
    if waiting_since is not None and view["last_event_at"] is not None:
        view["wait_seconds"] += max(0, view["last_event_at"] - waiting_since)
    return view


def activity(store, run_id):
    """Per-gate activity for a run, joined to what the scheduler believes."""
    run = store.get(run_id)
    journals = gate_events(store, run_id)
    gates = []
    for gate in run["gates"]:
        entries = journals.get((gate["id"], gate.get("attempt")), [])
        view = summarise(entries)
        scheduled = gate.get("status")
        gates.append({
            "gate_id": gate["id"], "executor": gate.get("executor"), "attempt": gate.get("attempt"),
            "scheduled_status": scheduled, "session_state": view["state"],
            # A launch acknowledgement is not activity. Say which it is.
            "has_activity": bool(entries),
            "started_at": gate.get("started_at") or view["started_at"],
            "finished_at": gate.get("finished_at"), "last_event_at": view["last_event_at"],
            "machine_seconds": elapsed(gate, view),
            "recorded_wait_seconds": view["wait_seconds"],
            "thread_id": view["thread_id"], "turn_ids": view["turn_ids"], "pid": view["pid"],
            "runtime": view["runtime"], "usage": view["usage"], "rate_limits": view["rate_limits"],
            "tools": view["tools"], "approvals": view["approvals"],
            "declined_requests": view["declined_requests"],
            "evidence_id": gate.get("evidence_id"), "events": view["events"],
            "blocking_reason": blocking_reason(gate, view),
            "next_action": NEXT_ACTION.get(view["state"] or scheduled)})
    return {"schema": "environment-qa.activity.v1", "run_id": run_id,
            "run_status": run["status"], "revision": run["revision"],
            "profile": run["policy"].get("pipeline", {}).get("profile"),
            "profile_version": run["policy"].get("pipeline", {}).get("profile_version"),
            "policy_sha256": run["policy"].get("pipeline", {}).get("sha256"),
            "open_interactions": [{"id": i["id"], "gate_id": i.get("gate_id"), "type": i.get("type"),
                                   "expires_at": i.get("expires_at")}
                                  for i in run.get("interactions", []) if i.get("status") == "open"],
            "gates": gates}


def elapsed(gate, view):
    """Machine time only. Human waiting is not work and is reported separately."""
    started = gate.get("started_at") or view["started_at"]
    finished = gate.get("finished_at") or view["last_event_at"]
    if started is None or finished is None or finished < started:
        return None
    return round(max(0, finished - started - view.get("wait_seconds", 0)), 3)


def blocking_reason(gate, view):
    if view["blocked_reason"]:
        return view["blocked_reason"]
    for approval in reversed(view["approvals"]):
        if approval.get("decision") == "reject":
            return f"approval refused: {approval.get('reason')}"
    if gate.get("status") == "waiting_interaction":
        return "awaiting a review decision"
    return None

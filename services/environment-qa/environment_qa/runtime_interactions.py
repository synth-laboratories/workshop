"""Typed, evidence-bound decisions for an active app-server connection.

Workflow reviews never resolve these requests. The worker retains the connection
while waiting; a lost connection must be reconciled, not silently approved/replayed.
"""
import threading
import time
import uuid

from .core import Conflict, digest
from .codex_executor import GateCancelled


def wait_for_permission(store, executor, message):
    return wait_for_response(store, executor, message, "permission")


def wait_for_clarification(store, executor, message):
    questions = message.get("params", {}).get("questions")
    if not isinstance(questions, list) or not questions or len(questions) > 10:
        raise ValueError("Clarification must contain one to ten questions")
    ids = [q.get("id") for q in questions if isinstance(q, dict)]
    if len(ids) != len(questions) or any(not isinstance(q, str) or not q for q in ids) or len(set(ids)) != len(ids):
        raise ValueError("Clarification question IDs must be unique nonempty strings")
    if any(q.get("isSecret") for q in questions):
        raise ValueError("Secrets cannot be requested through the QA review UI")
    return wait_for_response(store, executor, message, "clarification")


def wait_for_response(store, executor, message, kind):
    spec = executor.spec
    request = executor.scrub({"method": message["method"], "params": message.get("params", {})})
    interaction_id = uuid.uuid4().hex
    context = digest({"request": request, "input": spec["input_sha256"], "attempt": spec["attempt"]})
    now = time.time()
    def create(run):
        gate = next(g for g in run["gates"] if g["id"] == spec["gate_id"])
        if run["mode"] != "hitl" or run["status"] in {"cancelled", "cancelling"} or gate["attempt"] != spec["attempt"]:
            raise Conflict("Permission request is no longer current")
        run["interactions"].append({"id": interaction_id, "type": kind, "status": "open",
            "gate_id": spec["gate_id"], "attempt": spec["attempt"], "request_id": message["id"],
            "context_digest": context, "input_sha256": spec["input_sha256"], "request": request,
            "allowed": ["once", "reject"] if kind == "permission" else ["answer", "reject"],
            "response_contract": f"environment-qa.{kind}.v1",
            "created_at": now, "expires_at": now + run["policy"]["pipeline"]["interaction_timeout_seconds"]})
        if run["status"] != "paused":
            run["status"] = "waiting_interaction"
    store.mutate(spec["run_id"], f"runtime.{kind}.requested", create)
    executor.transition("waiting_for_permission" if kind == "permission" else "waiting_for_review")
    waiter = threading.Event()
    started = executor.clock()
    try:
        while True:
            run = store.get(spec["run_id"])
            interaction = next(i for i in run["interactions"] if i["id"] == interaction_id)
            if run["status"] in {"cancelled", "cancelling"} or interaction["status"] == "cancelled":
                raise GateCancelled("Permission wait cancelled")
            if executor.process is None or executor.process.poll() is not None:
                raise GateCancelled("Permission connection lost; no response replayed")
            if interaction["status"] == "resolved" and run["status"] != "paused":
                executor.transition("running")
                return {"decision": interaction["decision"], "actor": interaction["actor"], "reason": interaction["reason"],
                        "answers": interaction.get("answers", {})}
            if interaction["status"] == "superseded":
                raise GateCancelled("Runtime request superseded; no response replayed")
            if interaction["status"] == "expired" or time.time() >= interaction["expires_at"]:
                def expire(current):
                    target = next(i for i in current["interactions"] if i["id"] == interaction_id)
                    if target["status"] == "open": target["status"] = "expired"
                store.mutate(spec["run_id"], f"runtime.{kind}.expired", expire)
                return {"decision": "reject", "actor": "executor", "reason": "permission expired"}
            waiter.wait(.1)
    finally:
        executor.review_wait_seconds += executor.clock() - started


def respond(store, run_id, body, kind="permission"):
    actor, reason = body.get("actor"), body.get("reason")
    if actor not in {"local-human", "agent-cua"} or not isinstance(reason, str) or not reason.strip():
        raise ValueError("A valid actor and nonempty reason are required")
    if body.get("decision") not in ({"once", "reject"} if kind == "permission" else {"answer", "reject"}):
        raise ValueError("Unknown permission decision")
    command = {k: body.get(k) for k in ("interaction_id", "context_digest", "decision", "reason", "actor", "answers")}
    def apply(run):
        i = next((i for i in run["interactions"] if i["id"] == body.get("interaction_id")), None)
        if run["mode"] != "hitl" or run["status"] in {"completed", "cancelled", "cancelling", "failed"}:
            raise Conflict("Run does not accept permission responses")
        if not i or i["type"] != kind or i["status"] != "open" or i["context_digest"] != body.get("context_digest") or time.time() >= i["expires_at"]:
            raise Conflict("Permission request expired or superseded")
        gate = next(g for g in run["gates"] if g["id"] == i["gate_id"])
        if gate["attempt"] != i["attempt"] or gate["status"] != "running":
            raise Conflict("Permission attempt is no longer active")
        if kind == "clarification" and body["decision"] == "answer":
            answers = body.get("answers")
            ids = {q["id"] for q in i["request"]["params"]["questions"]}
            if not isinstance(answers, dict) or set(answers) != ids:
                raise ValueError("Answer every requested question and no others")
            for value in answers.values():
                if (not isinstance(value, dict) or set(value) != {"answers"} or
                    not isinstance(value["answers"], list) or len(value["answers"]) != 1 or
                    not isinstance(value["answers"][0], str) or not value["answers"][0].strip() or
                    len(value["answers"][0]) > 10000):
                    raise ValueError("Each answer must contain one nonempty text value of at most 10000 characters")
            i["answers"] = answers
        i.update(status="resolved", decision=body["decision"], reason=reason, actor=actor, resolved_at=time.time())
        if run["status"] != "paused":
            run["status"] = "waiting_interaction" if any(i["status"] == "open" for i in run["interactions"]) else "running"
    key = body.get("request_key")
    if not isinstance(key, str) or not key.strip(): raise ValueError("An idempotency key is required")
    return store.mutate(run_id, f"runtime.{kind}.resolved", apply, None, key, command)

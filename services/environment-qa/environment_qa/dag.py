"""Transactional DAG dispatch, fenced completions and evidence-bound interaction gates."""
import time
import uuid
import threading
from concurrent.futures import ThreadPoolExecutor
from .core import Conflict, digest, seal
from .bundles import verified_path

TERMINAL = {"succeeded", "failed", "inconclusive", "skipped", "cancelled"}


def claim(store, run_id):
    token = uuid.uuid4().hex
    chosen = []
    def apply(run):
        if run["status"] not in {"queued", "running", "waiting_interaction"}:
            raise Conflict("Run is not dispatchable")
        running = sum(g["status"] == "running" for g in run["gates"])
        if running >= run["policy"]["pipeline"]["max_parallel"]: raise Conflict("Concurrency limit")
        states = {g["id"]: g["status"] for g in run["gates"]}
        for g in run["gates"]:
            if g["status"] != "pending" or not all(states[d] in TERMINAL for d in g["depends_on"]): continue
            optional=set(g.get('optional_evidence_dependencies',[]))
            if not g.get("allow_inconclusive_dependencies") and any(states[d] != "succeeded" and not (d in optional and states[d]=='inconclusive') for d in g["depends_on"]):
                g["status"] = "inconclusive"
                run["limitations"].append(g["id"] + ": prerequisite did not succeed")
                if all(n["status"] in TERMINAL for n in run["gates"]): seal(run)
                chosen.append(None)
                return
            g.update(status="running", attempt=token, started_at=time.time(), lease_expires_at=time.time()+60)
            g["attempts"].append({"token": token, "started_at": g["started_at"]})
            run["status"] = "running"
            chosen.append(dict(g))
            return
        if all(n["status"] in TERMINAL for n in run["gates"]):
            seal(run)
            chosen.append(None)
            return
        raise Conflict("No ready gates")
    try: store.mutate(run_id, "dag.claimed", apply)
    except Conflict: return None
    return (chosen[0], token)


def complete(store, run_id, gate, token, result):
    def apply(run):
        g = next(g for g in run["gates"] if g["id"] == gate["id"])
        if g["attempt"] != token or g["status"] != "running": raise Conflict("Stale worker completion")
        evidence = {"id": digest(result), "gate": g["id"], "attempt": token, "result": result}
        run["evidence"].append(evidence)
        for decision in result.get("dispositions",[]):
            f = next(f for f in run["findings"] if f["id"] == decision["finding_id"])
            f.setdefault("assessments",[]).append(dict(decision,actor="ai",gate_id=g["id"],evidence_id=evidence["id"]))
            previous=[a for a in f['assessments'][:-1] if a.get('gate_id')=='attribution']
            conflict=(g.get('role')=='critic' and previous and
                      (previous[-1]['status']=='dismissed') != (decision['status']=='dismissed'))
            f["disposition"] = 'unresolved' if conflict else decision["status"]
        for f in result.get("findings", []):
            f = dict(f, evidence_id=evidence["id"], gate_id=g["id"])
            f["id"] = digest([g["id"], f["id"]])[:16]
            run["findings"].append(f)
        run["limitations"].extend(result.get("limitations", []))
        g.update(status=result.get("gate_status", "succeeded"), finished_at=time.time(), evidence_id=evidence["id"])
        g["attempts"][-1].update(finished_at=g["finished_at"], status=g["status"])
        if run["status"] == "cancelling":
            g["status"] = "cancelled"
            if not any(n["status"] == "running" for n in run["gates"]): run["status"] = "cancelled"
            return
        if result.get("interaction"):
            context = digest({"gate": g["id"], "evidence": evidence["id"], "bundle": run["bundle"]["sha256"], "policy": run["policy"]})
            run["interactions"].append({"id": uuid.uuid4().hex, "gate_id": g["id"], "type": g["role"], "status": "open",
                "context_digest": context, "allowed": ["confirm", "dismiss", "request_evidence"],
                "created_at": time.time(), "expires_at": time.time()+run["policy"]["pipeline"]["interaction_timeout_seconds"]})
            g["status"] = "waiting_interaction"
        if run["status"] != "paused":
            run["status"] = "waiting_interaction" if any(i["status"] == "open" for i in run["interactions"]) else "running" if any(n["status"] == "running" for n in run["gates"]) else "queued"
        if run["status"] != "paused" and all(n["status"] in TERMINAL for n in run["gates"]): seal(run)
    return store.mutate(run_id, "dag.completed", apply)


def execute(store, run_id, gate, token, executor=None):
    if gate is None: return
    from .executors import execute_gate
    stopped = threading.Event()
    def heartbeat():
        while not stopped.wait(20):
            def renew(current):
                g = next(g for g in current["gates"] if g["id"] == gate["id"])
                if g["status"] != "running" or g["attempt"] != token: raise Conflict("Lease fenced")
                g["lease_expires_at"] = time.time()+60
            try: store.mutate(run_id,"dag.lease.renewed",renew)
            except Conflict: return
    timer = threading.Thread(target=heartbeat,daemon=True)
    timer.start()
    try:
        run = store.get(run_id)
        path = verified_path(store, run["bundle"])
        result = (executor or execute_gate)(store, run, gate, path)
        verified_path(store, run["bundle"])
    except Exception as exc:
        result = {"gate_status": "inconclusive", "findings": [], "limitations": [f"{gate['id']}: {type(exc).__name__}: {exc}"]}
    finally:
        stopped.set()
        timer.join()
        # A gate's app-server session dies with the gate. Holding it open would
        # leak a process and let the next attempt inherit this one's conversation.
        from .dispatch import SESSIONS
        SESSIONS.release(run_id, gate["id"], token)
    complete(store, run_id, gate, token, result)


def decide(store, run_id, interaction_id, decision, reason, context_digest, expected, key, actor="local-human"):
    if actor not in {"local-human", "agent-cua"}:
        raise ValueError("Invalid review actor")
    if decision not in {"confirm", "dismiss", "request_evidence"} or not isinstance(reason, str) or not reason.strip():
        raise ValueError("Valid decision and reason required")
    command = dict(interaction_id=interaction_id, decision=decision, reason=reason, context_digest=context_digest, revision=expected, actor=actor)
    def apply(run):
        if run["mode"] != "hitl" or run["status"] in {"cancelled", "cancelling"}: raise Conflict("Human decisions not allowed")
        i = next((i for i in run["interactions"] if i["id"] == interaction_id), None)
        if i and i.get("type") in {"permission", "clarification"}:
            raise Conflict("Use the typed runtime response endpoint")
        if not i or i["status"] != "open" or i["context_digest"] != context_digest or time.time() > i["expires_at"]:
            raise Conflict("Interaction expired or superseded")
        i.update(status="resolved", decision=decision, reason=reason, actor=actor, resolved_at=time.time())
        gate = next(g for g in run["gates"] if g["id"] == i["gate_id"])
        gate["status"] = {"confirm": "succeeded", "dismiss": "failed", "request_evidence": "inconclusive"}[decision]
        if decision != "confirm": run["limitations"].append(gate["id"] + ": " + reason)
        if run["status"] != "paused":
            run["status"] = "waiting_interaction" if any(i["status"] == "open" for i in run["interactions"]) else "queued"
    # Independent gates and heartbeats can advance the run revision while a
    # human reads. The immutable interaction context, not unrelated run progress,
    # is the compare-and-swap boundary; apply() validates it transactionally.
    return store.mutate(run_id, "dag.interaction.resolved", apply, None, key, command)


def expire(store, run_id):
    run = store.get(run_id)
    expired = [i for i in run["interactions"] if i["status"] == "open" and i.get("type") not in {"permission", "clarification"} and i["expires_at"] < time.time()]
    if not expired or run["seal"]: return
    def apply(current):
        for i in current["interactions"]:
            if i["status"] == "open" and i.get("type") not in {"permission", "clarification"} and i["expires_at"] < time.time():
                i["status"] = "expired"
                next(g for g in current["gates"] if g["id"] == i["gate_id"])["status"] = "inconclusive"
                current["limitations"].append(i["gate_id"] + ": interaction expired without approval")
    store.mutate(run_id, "dag.interaction.expired", apply)


def run_until_idle(store, run_id, executor=None):
    """CLI scheduler. Each completion frees a slot; gates share one atomic budget."""
    with ThreadPoolExecutor(max_workers=store.get(run_id)["policy"]["pipeline"]["max_parallel"]) as pool:
        futures = []
        while True:
            expire(store, run_id)
            for f in futures:
                if f.done(): f.result()
            futures = [f for f in futures if not f.done()]
            work = claim(store, run_id)
            if work:
                if work[0] is not None: futures.append(pool.submit(execute, store, run_id, *work, executor))
            elif not futures: break
            else: time.sleep(0.1)
    return store.get(run_id)

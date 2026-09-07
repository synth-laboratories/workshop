"""Versioned QA records and transactional run state, independent of the UI."""
from __future__ import annotations

import hashlib
import json
import math
import sqlite3
import time
import uuid
from pathlib import Path
from typing import Callable
from contextlib import contextmanager


class Conflict(ValueError):
    pass


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def digest(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


CHARTERS = {
    "terminal-bench": {
        "version": "1", "name": "Terminal-Bench validity",
        "goal": "Tasks must be solvable, fairly specified, and grade correct behavior without shortcuts.",
        "criteria": ["instruction_verifier_alignment", "verifier_validity", "solution_leakage", "reproducibility"],
    },
    "reb-systems": {
        "version": "1", "name": "Systems REB",
        "goal": "Build AI systems that deliver real search and learning for applied AI.",
        "criteria": ["instruction_verifier_alignment", "heldout_integrity", "meaningful_baselines", "demonstrated_learning", "reproducibility"],
    },
    "reb-visuals": {
        "version": "1", "name": "VisualsBench",
        "goal": "Review data, find open-ended insights, and deliver them visually in a compact, striking, low-fluff manner.",
        "criteria": ["data_grounding", "insight_quality", "visual_communication", "verifier_validity", "anti_spoofing"],
    },
}


def gate_states(run):
    return {g["id"]: g.get("status") for g in run.get("gates", [])}


def transition(before, before_status, run):
    """Describe what changed, not just where the run ended up.

    A follower reading `{"status": "running"}` could see that something happened
    but not which gate moved, so every transition looked alike. The diff is
    derived from the document itself rather than passed in by each caller, so it
    cannot drift from what actually changed, and a new call site gets it free.
    """
    after = gate_states(run)
    gates = [{"id": gid, "from": before.get(gid), "to": status,
              "attempt": next((g.get("attempt") for g in run["gates"] if g["id"] == gid), None)}
             for gid, status in after.items() if before.get(gid) != status]
    payload = {"status": run["status"], "gates": gates}
    if before_status != run["status"]:
        payload["run"] = {"from": before_status, "to": run["status"]}
    waiting = [i["id"] for i in run.get("interactions", []) if i.get("status") == "open"]
    if waiting:
        payload["waiting_on"] = waiting
    return payload


class Store:
    def __init__(self, root: Path):
        self.root = Path(root).resolve()
        self.root.mkdir(parents=True, exist_ok=True)
        self.db = self.root / "qa.sqlite3"
        with self.connect() as con:
            con.executescript("""
              PRAGMA journal_mode=WAL;
              CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY, request_key TEXT UNIQUE,
                  request_digest TEXT, revision INTEGER, document TEXT);
              CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,
                  run_id TEXT, kind TEXT, at REAL, revision INTEGER, payload TEXT);
              CREATE TABLE IF NOT EXISTS commands(run_id TEXT, key TEXT, digest TEXT,
                  PRIMARY KEY(run_id,key));
              CREATE TABLE IF NOT EXISTS provider_allowances(id TEXT PRIMARY KEY, maximum REAL, committed REAL);
            """)

    @contextmanager
    def connect(self):
        con = sqlite3.connect(self.db, timeout=30)
        con.row_factory = sqlite3.Row
        try:
            with con:
                yield con
        finally:
            con.close()

    def get(self, run_id):
        with self.connect() as con:
            row = con.execute("SELECT document FROM runs WHERE id=?", (run_id,)).fetchone()
        if not row:
            raise KeyError(run_id)
        return json.loads(row[0])

    def list(self):
        with self.connect() as con:
            return [json.loads(r[0]) for r in con.execute("SELECT document FROM runs ORDER BY rowid DESC")]

    def append_event(self, run_id, kind, payload, revision=None):
        """Record an event without rewriting the run document.

        Gate transcripts are high volume -- every tool call and agent message of
        every turn. Folding them into the document would make each one rewrite the
        whole record, so the log carries them and the document stays the state.
        """
        with self.connect() as con:
            if revision is None:
                row = con.execute("SELECT revision FROM runs WHERE id=?", (run_id,)).fetchone()
                if not row:
                    raise KeyError(run_id)
                revision = row[0]
            con.execute("INSERT INTO events(run_id,kind,at,revision,payload) VALUES(?,?,?,?,?)",
                        (run_id, kind, time.time(), revision, canonical(payload)))

    def events(self, run_id, after=0):
        with self.connect() as con:
            return [dict(r) | {"payload": json.loads(r["payload"])} for r in con.execute(
                "SELECT * FROM events WHERE run_id=? AND seq>? ORDER BY seq LIMIT 1000", (run_id, after))]

    def create(self, bundle: dict, mode="automated", charter="terminal-bench", reviewer="rules", probes=False,
               budget_usd=0.0, request_key=None, parent_id=None, overlay=None, pipeline=None, allowance_id=None):
        if mode not in {"automated", "hitl"} or charter not in CHARTERS or reviewer not in {"rules", "ai"}:
            raise ValueError("Unknown mode, charter, or reviewer")
        if not isinstance(budget_usd, (int, float)) or not math.isfinite(budget_usd) or not 0 <= budget_usd <= 50:
            raise ValueError("Prototype budget must be finite and between $0 and $50")
        if reviewer == "rules" and budget_usd:
            raise ValueError("Rules-only runs have a $0 provider budget")
        if parent_id:
            self.get(parent_id)
        if pipeline is not None:
            from .policy import validate
            pipeline = validate(pipeline)
            if reviewer != "ai":
                raise ValueError("Full QA policies require AI reviewers")
        overlay = overlay or ""
        if not isinstance(overlay, str) or len(overlay) > 8000:
            raise ValueError("Task goals must be text of at most 8000 characters")
        request = dict(bundle=bundle, mode=mode, charter=charter, reviewer=reviewer, probes=probes,
                       budget_usd=budget_usd, parent_id=parent_id, overlay=overlay)
        if pipeline is not None:
            request["pipeline"] = pipeline
        request_key = request_key or uuid.uuid4().hex
        run_id = uuid.uuid4().hex
        run = {"schema": "environment-qa.run.v1", "id": run_id, "revision": 0,
               "created_at": time.time(), "status": "queued", "verdict": None,
               "qualified": False, "mode": mode, "parent_id": parent_id, "bundle": bundle,
               "policy": {"version": "prototype-v1", "charter": CHARTERS[charter], "charter_id": charter,
                          "task_goals": overlay, "reviewer": reviewer, "harbor_probes": bool(probes)},
               "budget": {"limit_usd": budget_usd, "reserved_usd": 0.0, "actual_usd": 0.0},
               "gates": [{"id": g, "status": "pending", "attempt": None} for g in ["structure", "review", "harbor", "decision"]],
               "findings": [], "evidence": [], "interactions": [], "limitations": [], "seal": None}
        if pipeline is not None:
            from .policy import gates
            run["policy"].update(version="full-v2", pipeline=pipeline)
            run["gates"] = gates(pipeline)
            run["budget"]["calls"] = {}
        with self.connect() as con:
            con.execute("BEGIN IMMEDIATE")
            old = con.execute("SELECT request_digest,document FROM runs WHERE request_key=?", (request_key,)).fetchone()
            if old:
                if old[0] != digest(request):
                    raise Conflict("Idempotency key reused with different input")
                return json.loads(old[1])
            if allowance_id is not None:
                from decimal import Decimal, ROUND_CEILING, ROUND_FLOOR
                row=con.execute('SELECT maximum,committed FROM provider_allowances WHERE id=?',(allowance_id,)).fetchone()
                def units(value,rounding):return int((Decimal(str(value))*1_000_000).to_integral_value(rounding=rounding))
                if row is None:raise ValueError('Operator-authorized service budget exhausted or unavailable')
                committed=units(row[1],ROUND_CEILING)+units(budget_usd,ROUND_CEILING)
                if committed>units(row[0],ROUND_FLOOR):raise ValueError('Operator-authorized service budget exhausted or unavailable')
                con.execute('UPDATE provider_allowances SET committed=? WHERE id=?',(committed/1_000_000,allowance_id))
            con.execute("INSERT INTO runs VALUES(?,?,?,?,?)", (run_id, request_key, digest(request), 0, canonical(run)))
            con.execute("INSERT INTO events(run_id,kind,at,revision,payload) VALUES(?,?,?,?,?)",
                        (run_id, "run.created", time.time(), 0, canonical({"mode": mode})))
        return run

    def authorize_service(self, allowance_id, maximum):
        if not isinstance(maximum,(int,float)) or not math.isfinite(maximum) or not 0 < maximum <= 50:
            raise ValueError("Service allowance must be between $0 and $50")
        with self.connect() as con:
            con.execute("INSERT OR IGNORE INTO provider_allowances VALUES(?,?,0)",(allowance_id,maximum))
            row = con.execute("SELECT maximum FROM provider_allowances WHERE id=?",(allowance_id,)).fetchone()
            if row[0] != maximum: raise ValueError("Existing allowance has another limit; use a new explicitly authorized ID")

    def service_allowance(self, allowance_id):
        with self.connect() as con:
            row = con.execute("SELECT maximum,committed FROM provider_allowances WHERE id=?",(allowance_id,)).fetchone()
        return dict(row) if row else None

    def mutate(self, run_id: str, kind: str, fn: Callable, expected=None, key=None, command=None):
        with self.connect() as con:
            con.execute("BEGIN IMMEDIATE")
            row = con.execute("SELECT document FROM runs WHERE id=?", (run_id,)).fetchone()
            if not row:
                raise KeyError(run_id)
            run = json.loads(row[0])
            if key:
                old = con.execute("SELECT digest FROM commands WHERE run_id=? AND key=?", (run_id, key)).fetchone()
                if old:
                    if old[0] != digest(command):
                        raise Conflict("Idempotency key reused with different command")
                    return run
            if expected is not None and run["revision"] != expected:
                raise Conflict("Run changed; refresh before deciding")
            if run["seal"]:
                raise Conflict("Sealed runs are immutable; create a new run")
            before, before_status = gate_states(run), run["status"]
            fn(run)
            run["revision"] += 1
            con.execute("UPDATE runs SET revision=?,document=? WHERE id=?", (run["revision"], canonical(run), run_id))
            con.execute("INSERT INTO events(run_id,kind,at,revision,payload) VALUES(?,?,?,?,?)",
                        (run_id, kind, time.time(), run["revision"],
                         canonical(transition(before, before_status, run))))
            if key:
                con.execute("INSERT INTO commands VALUES(?,?,?)", (run_id, key, digest(command)))
        return run

    def control(self, run_id, action, expected, key):
        def apply(run):
            if action == "pause" and run["status"] in {"queued", "running", "waiting_interaction"}:
                run["status"] = "paused"
            elif action == "resume" and run["status"] == "paused":
                run["status"] = "waiting_interaction" if any(i["status"] == "open" for i in run["interactions"]) else "queued"
            elif action == "cancel" and run["status"] not in {"completed", "cancelled", "failed"}:
                run["status"] = "cancelling" if any(g["status"] == "running" for g in run["gates"]) else "cancelled"
                for interaction in run["interactions"]:
                    if interaction["status"] == "open":
                        interaction["status"] = "cancelled"
                for gate in run["gates"]:
                    if gate["status"] in {"pending", "waiting_interaction"}:
                        gate["status"] = "cancelled"
            else:
                raise Conflict("Action is not valid in the current state")
        return self.mutate(run_id, "run." + action, apply, expected, key, {"action": action, "revision": expected})

    def decide(self, run_id, interaction_id, decision, reason, context_digest, expected, key, actor="local-human"):
        if actor not in {"local-human", "agent-cua"}:
            raise ValueError("Invalid review actor")
        if self.get(run_id)["policy"].get("pipeline"):
            from .dag import decide
            return decide(self, run_id, interaction_id, decision, reason, context_digest, expected, key, actor=actor)
        if decision not in {"confirm", "dismiss", "request_evidence"} or not isinstance(reason, str) or not reason.strip():
            raise ValueError("A valid decision and non-empty reason are required")
        command = dict(interaction_id=interaction_id, decision=decision, reason=reason,
                       context_digest=context_digest, expected=expected, actor=actor)
        def apply(run):
            if run["mode"] != "hitl" or run["status"] not in {"waiting_interaction", "paused"}:
                raise Conflict("This run is not accepting human decisions")
            interaction = next((i for i in run["interactions"] if i["id"] == interaction_id), None)
            if not interaction or interaction["status"] != "open" or interaction["context_digest"] != context_digest:
                raise Conflict("Interaction is stale or no longer open")
            interaction.update(status="resolved", decision=decision, reason=reason,
                               actor=actor, resolved_at=time.time())
            if decision == "request_evidence":
                run["limitations"].append(("Agent requested more evidence: " if actor == "agent-cua" else "Human requested more evidence: ") + reason)
            for finding in run["findings"]:
                finding["disposition"] = {"confirm": "confirmed", "dismiss": "dismissed", "request_evidence": "unresolved"}[decision]
            run["gates"][-1]["status"] = "inconclusive" if decision == "request_evidence" else "succeeded"
            if run["status"] != "paused":
                seal(run)
        return self.mutate(run_id, "interaction.resolved", apply, expected, key, command)


def seal_payload(run):
    payload = {k: run[k] for k in ("schema", "id", "mode", "bundle", "policy", "gates", "findings",
                                 "evidence", "interactions", "limitations", "budget", "verdict", "qualified")}
    if "certificate" in run: payload["certificate"] = run["certificate"]
    return payload


def seal(run):
    if any(g["status"] in {"pending", "running", "waiting_interaction"} for g in run["gates"]):
        raise Conflict("Required work has not finished")
    blocking = any(f["severity"] == "blocking" and f.get("disposition") in {"proposed", "confirmed"} for f in run["findings"])
    run["verdict"] = "fail" if blocking else "inconclusive" if run["limitations"] else "pass"
    if run["policy"].get("pipeline"):
        required_complete = all(g["status"] == "succeeded" for g in run["gates"] if g.get("required"))
        verdicts = [e["result"]["assessment"]["verdict"] for e in run["evidence"] if isinstance(e["result"].get("assessment"),dict)]
        unresolved = any(f["severity"] == "blocking" and f.get("disposition") == "unresolved" for f in run["findings"])
        rejected = any(i.get("decision") == "dismiss" for i in run["interactions"])
        run["verdict"] = "fail" if blocking or rejected or "fail" in verdicts else "inconclusive" if unresolved or not required_complete or "inconclusive" in verdicts or not verdicts else "pass"
        # Qualification follows explicit versioned policy, never UI optimism.
        run["qualified"] = bool(run["verdict"] == "pass" and run["mode"] == "automated" and run["policy"]["pipeline"]["allow_automated_release"])
        run["certificate"] = {"schema":"environment-qa.certificate.v1","policy_sha256":run["policy"]["pipeline"]["sha256"],
            "task_sha256":run["bundle"]["sha256"],"evidence_digest":digest(run["evidence"]),
            "review_mode":run["mode"],"verdict":run["verdict"],"qualified":run["qualified"],
            "human_decision_count":sum(i.get("actor")=="local-human" for i in run["interactions"]),
            "identity_assurance":"local-operator-only","production_human_certification":False}
    run["status"] = "completed"
    # Prototype checks never issue production qualification certificates.
    run["seal"] = {"sha256": digest(seal_payload(run)), "at": time.time()}


def verify_seal(run):
    return bool(run.get("seal")) and run["seal"]["sha256"] == digest(seal_payload(run))

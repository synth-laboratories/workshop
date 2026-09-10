"""Durable approvals, bound to the exact work they authorised.

The in-memory ledger in `codex_executor` is fine for one process's lifetime and
useless across a restart -- which is precisely when approvals matter, because a
HITL run's whole point is that it pauses, possibly for hours, and resumes. A
decision a person made before lunch has to still be there afterwards, and a
decision that is no longer current has to still be *refused* afterwards.

Four bindings survive here, and each one refuses a different mistake:

  run/gate/attempt   a re-run is a new attempt and does not inherit consent
  input hash         the gate's inputs changed, so the approved work is not this work
  revision           the run moved on beneath the decision
  single use         one approval authorises one request, not a class of them

Expiry is stored rather than computed at read time so a decision cannot be
resurrected by a clock change or a slow queue.

The table is created here rather than in `core.py`'s schema so this module can be
added without editing a file other people are concurrently changing.
"""
from __future__ import annotations

import time

from .codex_executor import DECISION_ALIASES, ApprovalLedger

SCHEMA = """
CREATE TABLE IF NOT EXISTS approvals(
    fingerprint TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    gate_id TEXT NOT NULL,
    attempt TEXT NOT NULL,
    decision TEXT NOT NULL,
    actor TEXT NOT NULL,
    revision INTEGER,
    reason TEXT,
    created REAL NOT NULL,
    expires_at REAL,
    spent INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS approvals_by_run ON approvals(run_id, gate_id, attempt);
"""


class StoreApprovalLedger:
    """`ApprovalLedger`'s interface, backed by the run store."""

    def __init__(self, store, clock=time.time):
        self.store = store
        self.clock = clock
        with store.connect() as con:
            con.executescript(SCHEMA)

    fingerprint = ApprovalLedger.fingerprint
    subject = ApprovalLedger.subject
    PRESENTATION_FIELDS = ApprovalLedger.PRESENTATION_FIELDS

    def record(self, spec, request, decision, actor, revision, reason=None, ttl_seconds=None):
        if decision not in DECISION_ALIASES:
            raise ValueError(f"Unknown approval decision {decision!r}")
        key = self.fingerprint(spec, request)
        now = self.clock()
        with self.store.connect() as con:
            # A fresh decision replaces a previous one and is not already spent.
            con.execute("INSERT OR REPLACE INTO approvals"
                        "(fingerprint,run_id,gate_id,attempt,decision,actor,revision,reason,created,expires_at,spent)"
                        " VALUES(?,?,?,?,?,?,?,?,?,?,0)",
                        (key, spec["run_id"], spec["gate_id"], spec["attempt"], decision, actor,
                         revision, reason, now, now + ttl_seconds if ttl_seconds else None))
        return key

    def lookup(self, spec, request, revision):
        """Return (entry, reason). Claiming an approval is atomic and single-use."""
        key = self.fingerprint(spec, request)
        with self.store.connect() as con:
            con.execute("BEGIN IMMEDIATE")
            row = con.execute("SELECT run_id,gate_id,attempt,decision,actor,revision,reason,created,expires_at,spent"
                              " FROM approvals WHERE fingerprint=?", (key,)).fetchone()
            if row is None:
                return None, "no_decision"
            entry = {"run_id": row[0], "gate_id": row[1], "attempt": row[2], "decision": row[3],
                     "actor": row[4], "revision": row[5], "reason": row[6], "recorded_at": row[7],
                     "expires_at": row[8]}
            if entry["run_id"] != spec["run_id"] or entry["gate_id"] != spec["gate_id"]:
                return None, "wrong_gate"
            if entry["attempt"] != spec["attempt"]:
                return None, "stale_attempt"
            if revision is not None and entry["revision"] != revision:
                return None, "stale_revision"
            if entry["expires_at"] is not None and self.clock() > entry["expires_at"]:
                return None, "expired"
            if row[9]:
                return None, "already_spent"
            claimed = con.execute("UPDATE approvals SET spent=1 WHERE fingerprint=? AND spent=0", (key,))
            if claimed.rowcount != 1:  # another worker claimed it between read and write
                return None, "already_spent"
        return entry, "fresh"

    def pending(self, run_id):
        """Unspent decisions for a run, for a resume view."""
        with self.store.connect() as con:
            return [{"fingerprint": r[0], "gate_id": r[1], "attempt": r[2], "decision": r[3], "actor": r[4]}
                    for r in con.execute("SELECT fingerprint,gate_id,attempt,decision,actor FROM approvals"
                                         " WHERE run_id=? AND spent=0", (run_id,))]

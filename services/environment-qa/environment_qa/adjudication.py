"""Post-seal comparison review, separate from immutable detector predictions."""
import json
import time
from .core import Conflict, canonical, digest, verify_seal

DISPOSITIONS = {"matched_public", "additional_supported", "duplicate", "unsupported", "needs_evidence"}


def initialize(store):
    with store.connect() as con:
        con.execute("CREATE TABLE IF NOT EXISTS adjudications(run_id TEXT, revision INTEGER, request_key TEXT, request_digest TEXT, document TEXT, PRIMARY KEY(run_id,revision), UNIQUE(run_id,request_key))")


def get(store, run_id):
    initialize(store)
    with store.connect() as con:
        row = con.execute("SELECT document FROM adjudications WHERE run_id=? ORDER BY revision DESC LIMIT 1", (run_id,)).fetchone()
    return json.loads(row[0]) if row else None


def seed(store, run_id, proposal):
    initialize(store)
    run = store.get(run_id)
    if not verify_seal(run):
        raise ValueError("Valid sealed predictions required")
    if proposal["prediction_seal"] != run["seal"]["sha256"]:
        raise ValueError("Proposal belongs to another prediction seal")
    if {f["prediction_id"] for f in proposal["findings"]} != {f["id"] for f in run["findings"]}:
        raise ValueError("Proposal must cover every prediction")
    proposal = proposal | {"run_id": run_id, "revision": 0, "actor_kind": "assistant", "human_confirmed": False}
    with store.connect() as con:
        con.execute("INSERT INTO adjudications VALUES(?,?,?,?,?)", (run_id, 0, "initial-proposal", digest(proposal), canonical(proposal)))
    return proposal


def decide(store, run_id, body):
    actor = body.get("actor", "local-human")
    if actor not in {"local-human", "agent-cua"}:
        raise ValueError("Invalid adjudication actor")
    initialize(store)
    run = store.get(run_id)
    if not verify_seal(run):
        raise ValueError("Valid sealed predictions required")
    with store.connect() as con:
        con.execute("BEGIN IMMEDIATE")
        old = con.execute("SELECT request_digest,document FROM adjudications WHERE run_id=? AND request_key=?", (run_id, body["request_key"])).fetchone()
        if old:
            if old[0] != digest(body): raise Conflict("Idempotency key reused")
            return json.loads(old[1])
        row = con.execute("SELECT document FROM adjudications WHERE run_id=? ORDER BY revision DESC LIMIT 1", (run_id,)).fetchone()
        if not row: raise ValueError("No comparison proposal exists")
        current = json.loads(row[0])
        if body["revision"] != current["revision"] or body["prediction_seal"] != current["prediction_seal"]:
            raise Conflict("Comparison changed; refresh before reviewing")
        if body["disposition"] not in DISPOSITIONS or not isinstance(body.get("reason"), str) or not body["reason"].strip():
            raise ValueError("Valid disposition and reason required")
        item = next((f for f in current["findings"] if f["prediction_id"] == body["prediction_id"]), None)
        if not item: raise ValueError("Unknown prediction")
        gold_id = body.get("gold_id") or None
        duplicate_of = body.get("duplicate_of") or None
        if body["disposition"] == "matched_public":
            if gold_id not in {g["id"] for g in current["gold"]}: raise ValueError("Choose a public reference defect")
            if any(f is not item and f.get("gold_id") == gold_id and f["disposition"] == "matched_public" for f in current["findings"]):
                raise Conflict("Reference already matched; mark repeated predictions as duplicates")
        else: gold_id = None
        if body["disposition"] == "duplicate":
            if any(f.get("duplicate_of") == item["prediction_id"] for f in current["findings"]):
                raise ValueError("Resolve dependent duplicates first")
            target = next((f for f in current["findings"] if f["prediction_id"] == duplicate_of), None)
            if not target or target is item or target["disposition"] == "duplicate": raise ValueError("Choose a distinct, non-duplicate prediction")
        else: duplicate_of = None
        item.update(disposition=body["disposition"], reason=body["reason"], gold_id=gold_id, duplicate_of=duplicate_of,
                    actor_kind="human" if actor == "local-human" else "agent-cua",
                    actor=actor, human_confirmed=actor == "local-human", reviewed_at=time.time())
        current["revision"] += 1
        current["human_confirmed"] = all(f.get("human_confirmed") for f in current["findings"])
        # Finding review does not independently adjudicate the gold labels.
        current["primary_metrics_status"] = "pending_independent_gold_adjudication"
        con.execute("INSERT INTO adjudications VALUES(?,?,?,?,?)", (run_id, current["revision"], body["request_key"], digest(body), canonical(current)))
        return current


# Conservative normalization of model-proposed duplicate graphs.
def normalize_dispositions(dispositions,findings):
    decisions={d['finding_id']:dict(d) for d in dispositions}
    original={f['id']:f for f in findings}; notes=[]
    for id,decision in decisions.items():
        target=decision.get('duplicate_of','')
        if decision['status']!='dismissed':
            decision['duplicate_of']=''
            continue
        if not target: continue
        seen={id}
        while target not in seen and target in original:
            seen.add(target)
            next_decision=decisions.get(target,original[target])
            if next_decision.get('status',next_decision.get('disposition'))!='dismissed': break
            target=next_decision.get('duplicate_of','')
        else:
            decision.update(status='unresolved',duplicate_of='')
            notes.append('Unresolvable or cyclic duplicate link retained unresolved: '+id)
            continue
        decision['duplicate_of']=target
    return list(decisions.values()),notes

"""Follow a QA run's gate transitions live, over the house trace-stream contract.

The store already keeps an append-only `events` table with an autoincrement
sequence, written inside the same transaction that advances the run document.
So the durability rule the rest of the platform follows -- persist before a
record is visible to any consumer -- is already satisfied here by SQLite's
commit, and this module only reads.

Envelopes, the digest, the cursor block and the SSE framing are deliberately
byte-compatible with `synth_containers.event_log`, so anything that already
consumes a rollout stream can consume a QA run without a second reader. The one
difference is the cursor's source: sequence comes from the table's rowid rather
than an in-RAM high-water mark, which is what lets a follower attach to a run
that started before it did, and reattach after it dies, with no replay buffer.
"""
from __future__ import annotations

import hashlib
import json
import time
from datetime import datetime, timezone

SCHEMA_STREAM_EVENT = "synth.trace-stream-event.v1"
SCHEMA_DESCRIPTOR = "synth.qa.stream.v1"
SSE_HEADERS = {"Cache-Control": "no-cache", "X-Accel-Buffering": "no"}

# A run stops producing when it is sealed or fully cancelled. `cancelling` is
# not terminal: gates are still winding down and their transitions are exactly
# what someone watching a cancellation wants to see.
TERMINAL_STATUSES = frozenset({"cancelled"})


def _digest(kind, sequence, payload):
    blob = json.dumps({"kind": kind, "sequence": sequence, "payload": payload},
                      sort_keys=True, separators=(",", ":"), default=str)
    return hashlib.sha256(blob.encode("utf-8")).hexdigest()[:16]


def _iso(at):
    return datetime.fromtimestamp(at, timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z")


def envelope(row):
    """One stored event as a trace-stream envelope.

    Stored rows are always semantic: the table has no control records, because a
    control record marks something about the subscription rather than the run.
    `subscribed` below is synthesised per reader instead.
    """
    payload = row["payload"]
    sequence = row["seq"]
    return {"schema": SCHEMA_STREAM_EVENT, "kind": row["kind"], "ts": _iso(row["at"]),
            "control": False, "payload": payload, "digest": _digest(row["kind"], sequence, payload),
            "sequence": sequence, "event_id": str(sequence)}


def subscribed(run_id, after, high_water):
    payload = {"type": "stream.subscribed", "run_id": run_id,
               "next_sequence": high_water + 1, "resumed_from": after, "ready": True}
    return {"schema": SCHEMA_STREAM_EVENT, "kind": "stream.subscribed", "ts": _iso(time.time()),
            "control": True, "payload": payload, "digest": _digest("stream.subscribed", None, payload),
            "event_id": "stream.subscribed"}


def closed(store, run_id):
    """True once the run can produce no further events."""
    run = store.get(run_id)
    return bool(run.get("seal")) or run.get("status") in TERMINAL_STATUSES


def poll_payload(store, run_id, after=0, limit=1000):
    """A page of envelopes after `after`, with the cursor a reader resumes from."""
    if isinstance(limit, bool) or limit < 1 or limit > 10_000:
        raise ValueError("invalid_page_limit")
    rows = store.events(run_id, after)
    page = [envelope(r) for r in rows[:limit]]
    events = ([subscribed(run_id, after, page[-1]["sequence"] if page else after)] if after <= 0 else []) + page
    return {"run_id": run_id, "cursor": {
        "kind": "sequence", "after": after, "closed": closed(store, run_id),
        "high_water": page[-1]["sequence"] if page else after,
        "next": max([after, *(e["sequence"] for e in page)]),
        "has_more": len(rows) > limit}, "events": events}


def format_sse(event):
    """One SSE record. `id` is the semantic sequence, or 0 for a control record."""
    return (f"id: {event.get('sequence', 0)}\n"
            f"event: {event['kind']}\n"
            f"data: {json.dumps(event, separators=(',', ':'))}\n\n")


def follow(store, run_id, after=0, poll=0.25, idle_heartbeat=15.0, now=time.monotonic):
    """Yield envelopes as they land, then stop when the run closes.

    Polling rather than blocking on a condition variable is deliberate: the
    producer may be a different process (the worker holds an exclusive lock on
    the store), so there is no in-process log object to wait on. The cursor makes
    that safe -- a reader that misses a tick catches up rather than losing events.

    Yields `None` as a heartbeat when nothing has arrived for `idle_heartbeat`
    seconds, so a caller writing SSE can keep an idle connection open while a
    long gate runs without inventing an event that did not happen.
    """
    if after <= 0:
        yield subscribed(run_id, after, after)
    last = now()
    while True:
        rows = store.events(run_id, after)
        for row in rows:
            after = row["seq"]
            last = now()
            yield envelope(row)
        if not rows:
            if closed(store, run_id):
                return
            if now() - last >= idle_heartbeat:
                last = now()
                yield None
            time.sleep(poll)


GLYPH = {"running": "▶", "succeeded": "✔", "failed": "✘", "cancelled": "⊘",
         "inconclusive": "?", "waiting_interaction": "⏸", "pending": "·"}


def render(event, started):
    """One human line per transition, or None for events that moved nothing."""
    if event is None:
        return None
    payload = event["payload"]
    at = f"{event['sequence']:>4}  {time.time() - started:7.1f}s" if event.get("sequence") else "   -        -  "
    if event["control"]:
        return f"{at}  subscribed, resuming after {payload['resumed_from']}"
    lines = [f"{at}  {GLYPH.get(gate['to'], '-')} {gate['id']:<12} {gate['from'] or 'none'} -> {gate['to']}"
             for gate in payload.get("gates", [])]
    if "run" in payload:
        lines.append(f"{at}  run          {payload['run']['from']} -> {payload['run']['to']}")
    if payload.get("waiting_on"):
        lines.append(f"{at}  blocked on interaction {', '.join(payload['waiting_on'])}")
    if not lines and event["kind"] == "run.created":
        lines.append(f"{at}  run created ({payload.get('mode', '?')})")
    return "\n".join(lines) if lines else None


def main(argv=None):
    import argparse
    from pathlib import Path
    from .core import Store

    parser = argparse.ArgumentParser(description="Follow a QA run's gates live.")
    parser.add_argument("store", type=Path)
    parser.add_argument("run_id")
    parser.add_argument("--after", type=int, default=0, help="Resume from this sequence.")
    parser.add_argument("--json", action="store_true", help="Emit trace-stream envelopes instead of text.")
    parser.add_argument("--sse", action="store_true", help="Emit SSE records.")
    args = parser.parse_args(argv)

    store = Store(args.store)
    started = time.time()
    for event in follow(store, args.run_id, args.after):
        if args.sse:
            print(format_sse(event) if event else ": heartbeat\n", end="", flush=True)
        elif args.json:
            if event:
                print(json.dumps(event, separators=(",", ":")), flush=True)
        else:
            line = render(event, started)
            if line:
                print(line, flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

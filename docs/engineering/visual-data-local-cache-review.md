# Review — visual data reliability and local cache

Reviews: `docs/engineering/visual-data-local-cache-handoff.md`
Date: 2026-08-31
Method: code read plus measurement against a byte copy of the live desktop database
(`~/Library/Application Support/Synth Desktop/synth.sqlite3`, 475 MB, 26 optimizer runs,
8,497 events).

## Verdict

The handoff's architecture is right and its invariants are right. Its diagnosis is not.

The document attributes the delay to *how much* the hot path reads — the durable projection,
then the run record, then the whole event journal, serially, before mounting. Measured, that
whole sequence costs about **250 ms** on the largest run in the live database. It cannot
produce the multi-second "Restoring run evidence…" or the "producer stopped answering"
failure on its own.

Three defects that the handoff does not mention account for essentially all of the observed
latency and all of the observed failures. Two of them are three-line fixes. One of them means
that on this machine **25 of 26 runs cannot be read at all** — for those, a durable snapshot
cache would never be populated, because the read it caches always throws.

Building Phases 2–4 on top of the current read path would add a cache in front of a read that
is slow because it takes a write lock, and empty because it fails. Phase 1 as written would
help; the three fixes below help more, cost less, and are prerequisites for the rest.

## What the handoff gets right, and should keep

- Presentation availability must not be coupled to live transport availability. Correct, and
  still correct after the fixes below.
- The kernel projection is the sole authority; a renderer must never re-derive product truth
  from an incomplete event stream. Correct, and load-bearing.
- Aggregate visuals must not block on raw-event hydration. Correct — and, after fix D2 below,
  also *cheap*, because the aggregate is already a 5 KB row.
- The identity key `(visual_id, visual_revision, run_id, projection_revision, template_version,
  digest)`. Correct. Do not weaken it.
- "Do not merely raise the 15-second timeout." Correct, and stronger than the document knows:
  see the watchdog note in D1.
- The full invariant list (1–12) and the fault-injection matrix. Keep both as written.

## The three defects

### D1 — Every read of the durable projection takes the database *write* lock

`Database::run_transaction` opens every transaction with `TransactionBehavior::Immediate`
(`src-tauri/src/storage/database.rs:76`). `Immediate` acquires SQLite's write lock at `BEGIN`,
before a single row is read. `OptimizerService::run_view_v2`
(`src-tauri/src/optimizers/service.rs:1265`) is a read, and it runs through `run_transaction`.
So does most of the rest of the service: 99 `run_transaction` sites and 37 synchronous
`Database::transaction` sites, all `Immediate`, many of them read-only.

The database is in WAL mode, where readers are supposed to never block. That property is
discarded by asking for `Immediate`.

Measured on the live database snapshot, with one writer holding a transaction for 3 seconds:

| read | wall time |
| --- | --- |
| `BEGIN IMMEDIATE` — what `run_view_v2` does today | **3,062.9 ms** |
| `BEGIN DEFERRED` — a WAL read snapshot | **1.1 ms** |

A read that should cost a millisecond costs exactly as long as the producer holds the lock.

Two things make this worse rather than merely bad:

1. **The UI polls this read.** `subscription.ts` runs `pollDurableRevision` every 750 ms per
   subscribed run (`POLL_INTERVAL_MS = 750`), and that poll calls `runViewV2`. Every mounted
   card therefore reaches for the database write lock 1.33 times per second, against the
   producer that is trying to append to it, and against every other mounted card. The
   handoff's performance target "opening 10 output rows does not create 10 polling loops"
   understates the problem: today those ten loops are ten *write-lock* acquisitions per
   750 ms, each serialized behind the others.
2. **The watchdog is set to fire before SQLite gives up.** Connections are opened with
   `busy_timeout=30000` (`database.rs:107`); the renderer's stall watchdog is
   `STALL_TIMEOUT_MS = 15_000`. SQLite will wait patiently for up to 30 seconds for a lock
   that the renderer has already declared dead at 15. The message the user is shown —
   "subscription stalled — the producer stopped answering" — is emitted while the read is
   still queued behind a lock the producer legitimately holds. It names the wrong owner for
   the reason the handoff describes in §5, but the underlying event is lock contention, not
   producer liveness.

**Fix.** Add a read variant and route reads to it:

```rust
// storage/database.rs — reads take a WAL read snapshot, never the write lock.
pub async fn run_read<F, T>(&self, f: F) -> Result<T>
where F: FnOnce(&Connection) -> Result<T> + Send + 'static, T: Send + 'static {
    let path = self.path.clone();
    spawn_blocking(move || {
        let mut conn = connect(&path)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let result = f(&tx)?;
        tx.commit()?;   // a deferred read-only transaction commits without a write
        Ok(result)
    }).await.context("database worker join")?
}
```

Then switch `run_view_v2` and the other read-only `run_transaction` sites. Set
`busy_timeout` below the renderer's stall watchdog rather than above it, so a genuine lock
timeout surfaces as a stage-attributed database error instead of as a phantom producer death.

This is the single highest-value change in the whole effort, and it is about twenty lines.

### D2 — Every event append re-projects the entire run history, inside that write lock

`commit_validated_events` (`src-tauri/src/optimizers/service.rs:3973`) inserts the new events
and then, at line 4018:

```rust
let history = load_events_upto(conn, &run.id, run.cursor_seq)?;
```

It loads and JSON-deserializes **every event the run has ever emitted**, folds usage over all
of it (lines 4028–4051), and hands the whole history to `persist_kernel_projection`, which
calls `reduce_envelopes` — which starts from `RunKernelState::new(...)` and an empty
`DurableProducerLog` and replays from zero (`kernel/bridge.rs:146`).

All of it runs inside the `BEGIN IMMEDIATE` transaction opened by `append_event_payloads`
(`service.rs:1642`).

So per-append cost grows linearly with history, total cost over a run grows quadratically,
and all of it is spent holding the lock that D1 makes the UI wait for.

Measured, reading and parsing `load_events_upto` at increasing cursors on the largest run in
the database (`opt_eval_craftax_20d6d7486c9e`, 2,259 events, 9.36 MB of `payload_json`):

| cursor | history read + parse |
| --- | --- |
| 100 | 3.5 ms |
| 500 | 10.8 ms |
| 1,000 | 20.3 ms |
| 1,500 | 36.6 ms |
| 2,000 | 43.5 ms |
| 2,259 | 50.5 ms |

Clean linear growth at ~24 µs per event of history, *per append*. That is the read and parse
alone; `reduce_envelopes` then folds all 2,259 envelopes again, and `upsert_projection`
re-serializes the result. Integrated over the run, the read-and-parse term alone is on the
order of a minute of accumulated write-lock occupancy — measured in Python, so serde is
some multiple faster, but the shape is what matters, not the constant.

Extrapolate to the handoff's own fault-injection target of 50,000 events: the final append
reads and parses 50,000 events before it can commit, and does so while holding the write
lock. That run does not merely render slowly. It stops making progress, and every UI read
behind it times out.

**Fix.** The kernel already supports the incremental fold. `commit::commit(state, log, batch,
committed_at)` takes a prior state and a batch; `persist::load_state` already returns the
durable prior state; `RunKernelState` is already persisted whole in
`optimizer_algorithm_projections.projection_json`. Only `reduce_envelopes` throws that away by
always starting from `RunKernelState::new`.

Fold forward from the persisted state over the new envelopes only. Two hazards to handle
deliberately, neither of them blocking:

- `DurableProducerLog` is not persisted. `plan_producer_batch` needs it for replay/idempotency
  verdicts. Persist it beside the projection, or reconstruct the bounded slice needed for the
  batch — the service already computes an equivalent fact in `durable_event_ids` +
  `plan_batch` immediately before, so today it is planned twice, once redundantly.
- `envelopes_to_producer` assigns `producer_sequence = index + 1` over the batch it is given
  (`kernel/bridge.rs:109`), and reorders so the canonical terminal fact folds last
  (lines 118–130). Both are whole-history operations today. Incrementally, the sequence must
  be offset by the committed count, and the terminal-reordering rule must be expressed
  against the accumulated state rather than the batch.

Keep the full replay available as an explicit, one-shot repair path — see D3 — but off the
append path.

### D3 — On this machine, 25 of 26 runs cannot be read at all

`run_view_v2` falls back to a "one-time repair" when no kernel projection exists: replay the
history and persist a projection. That repair calls `persist_kernel_projection`, which begins
(`service.rs:4395`):

```rust
let spec_digest: String = conn.query_row(
    "SELECT spec_digest FROM optimizer_run_specs WHERE optimizer_run_id = ?1", ...)
    .optional()?
    .ok_or_else(|| anyhow!("optimizer run {} is missing its admitted spec", run.id))?;
```

The live database:

```
optimizer_runs:                    26
optimizer_algorithm_projections:    1
optimizer_run_specs:                1
```

| run | events | spec | projection |
| --- | --- | --- | --- |
| `opt_eval_craftax_20d6d7486c9e` | 2,259 | yes | yes |
| `banking77_gepa_luna_med_4d67b456` | 1,658 | **no** | **no** |
| `banking77_gepa_luna_med_7df6fcd1` | 1,575 | **no** | **no** |
| `banking77_gepa_luna_med_5bbdfb49` | 1,132 | **no** | **no** |
| `banking77_gepa_luna_med_8c1278ef` | 1,058 | **no** | **no** |
| `craftax_gepa_luna_med_c83e3d15` | 382 | **no** | **no** |
| `craftax_gepa_luna_med_3aae850e` | 356 | **no** | **no** |

Every GEPA run predates kernel admission (created 2026-08-16/17, `source = 'local'`). The spec
backfill migration (`storage/migrations.rs:2938`) covers only `source = 'legacy_campaign_migration'`,
so these are not backfilled.

For each of those runs, opening the visual does this, on every attempt:

1. `run_view_v2` opens `BEGIN IMMEDIATE` — takes the write lock (D1).
2. `load_state` returns `None`.
3. `load_events_upto` reads and parses the entire history — up to 3.6 MB.
4. `persist_kernel_projection` fails: *missing its admitted spec*.
5. The whole transaction rolls back. Nothing is repaired, so the next attempt repeats it.
6. `subscription.ts` records the failure, backs off, retries — five times.
7. The pane lands on **"Run evidence unavailable."**

That is the reported symptom, exactly, and it is not a caching problem. There is nothing to
cache: the read that a snapshot would be built from can never succeed. A visual in this state
has never rendered once, so the handoff's central invariant — *once Workshop has successfully
rendered a revision, it stays viewable* — is never armed.

**Fix.** Two parts, both needed:

- **Backfill.** Synthesize a spec row for pre-admission runs the way the legacy-campaign
  migration does, with an explicit `not_required` authorization and a `legacy-local:<run_id>`
  digest, so provenance stays honest about what was reconstructed rather than admitted.
- **Fail visibly and once.** A missing spec is a permanent, structural condition, not a
  transient transport fault. It must not be retried five times with an exponential ladder, and
  it must not be reported as a stalled subscription. Return a typed
  `projection_unavailable / missing_admitted_spec` failure, attribute it to the projection
  stage as the handoff's Phase 0 asks, and render it once. Separately, take the repair replay
  off the hot read path: it belongs in the existing outbox sweep, where it is not sitting under
  a user-visible deadline holding the write lock.

## The corrected cost model

Everything the handoff identifies as the hot path, measured on the largest healthy run:

| step | measured |
| --- | --- |
| open a fresh connection (`db.run` does this per call — there is no pool) | 0.1 ms |
| read the durable projection row (4,985 bytes) | 1.4 ms |
| one `eventsAfter` page — 500 events, fresh connection, deserialized | 13.5–16 ms |
| the full 5-page journal walk | 85 ms |
| the same walk on a warm reused connection | 65 ms |
| renderer `JSON.parse` of all 9.36 MB of payloads | 27 ms |
| Rust→IPC re-serialize of the same | 17 ms |
| **whole nominal cold path** | **≈ 250 ms** |

Two conclusions follow, and both revise the handoff.

**The volume of raw events is not the problem.** 9.36 MB and 2,259 events cost about a quarter
of a second end to end. Phase 1 — unblocking first paint from the event walk — is still worth
doing, and the two IPC round trips it removes are still real, but it buys roughly 150 ms, not
seconds. It should be sequenced after D1 and D3, not before them.

**There is no SQLite queue to instrument.** The handoff's Phase 0 asks for "SQLite queue wait
vs query/deserialize time." There is no queue and no pool: every `db.run` / `run_transaction`
call opens its own connection on the tokio blocking pool, and that open costs 0.1 ms. The
metric that matters instead is **lock-acquisition wait**: time between `BEGIN` and the first
row. That is the number that goes to three seconds, and today nothing measures it.

The absence of a connection pool is worth noting but is not urgent. Each fresh connection
starts with a cold per-connection page cache, which is the difference between the 85 ms and
65 ms rows above — about 25%, and second-order behind everything else here.

## Two smaller findings

**`mergeOptimizerEventPage` is quadratic** (`renderer/src/runtime/optimizerEventCursor.ts`).
Each page rebuilds a `Map` from every event accumulated so far, then re-sorts the whole set
into a fresh array. Measured over a full paged walk: 3 ms at 2,259 events, 19 ms at 10,000,
268 ms at 50,000. It is not what is hurting today, but it directly contradicts the handoff's
own acceptance target that first-paint latency stay "approximately constant" at 50,000 events.
Merging into a retained `Map` and materializing the sorted array once makes it linear.

**`get` is a redundant round trip.** `run_view_v2` already calls `load_run` inside its own
transaction (`service.rs:1270`), and the renderer's `projectRunViewV2` needs only a bounded
slice of that record — `usage`, `summary.terminalManifest`, `startedAt`/`createdAt`/`finishedAt`,
`objective`, `capabilities`, `algorithmId`, `id` (`runProgress/viewV2.ts`). Returning that slice
on the V2 envelope removes an entire await from the hot path at zero query cost, and delivers
the handoff's "one coherent backend read" for first paint without the new command surface. Keep
the fuller `visuals.dataSnapshot` envelope for Phase 2; this is the cheap version of it.

## Revised sequencing

The handoff's phases are kept; what changes is what goes first and what each phase is claimed
to buy.

**Phase −1 — correctness and contention (hours, no schema change beyond a backfill).**
D1: `run_read` with `Deferred`, reads switched over, `busy_timeout` below the stall watchdog.
D3: spec backfill, plus a typed non-retryable `missing_admitted_spec` failure and the repair
replay moved to the outbox. Replace the 750 ms `runViewV2` poll with a projection-revision
probe (`SELECT projection_revision`, a single indexed column read) and stop polling terminal
runs at all — `stopPolling` already exists, the interval just should not be armed. *Expected:
"Run evidence unavailable" stops occurring on legacy runs; stalls under producer load stop
occurring; visual open on a healthy run drops to roughly its 250 ms nominal.*

**Phase 0 — measure, as written, with the metric set corrected.** Lock-acquisition wait
replaces "SQLite queue wait"; per-stage attribution (`projection` / `metadata` / `evidence
page` / `notification`) as the handoff already specifies; append-transaction duration against
history length, which is the leading indicator for D2.

**Phase 1 — unblock first paint from raw events, as written**, plus the `runSummary` slice on
the V2 envelope. Worth ~150 ms and two round trips, and it is the structural precondition for
lazy evidence. Sequenced third because it is a refactor, not a defect fix.

**Phase 1.5 — incremental kernel fold (D2).** Removes the quadratic append and the growing
write-lock hold. This is what makes the handoff's 50,000-event target reachable; no amount of
caching does.

**Phases 2–4 — durable snapshot, terminal materialization, lazy evidence, poster — as
written**, with one reframing. After Phase −1 the durable snapshot is no longer a latency fix;
a cold read of the durable projection is 1.4 ms. Its value is precisely what invariant 6 says
and nothing more: **a terminal revision stays viewable across restart and while the producer
is unavailable.** That is worth building. It should be justified on availability, not on speed,
because justifying it on speed will make it look like it underdelivered.

## What to add to the acceptance tests

The handoff's matrix is good. Four additions, each aimed at a defect it did not cover:

- **Reader/writer non-interference.** With a producer appending continuously, `run_view_v2`
  p99 stays under 50 ms. Today it tracks the producer's transaction duration.
- **Append cost is flat in history length.** Append-transaction duration at sequence 100 and
  at sequence 50,000 differ by less than 2×. Today it is linear, so this fails by ~500×.
- **A run with no admitted spec.** Renders one typed, stage-attributed
  `missing_admitted_spec` failure, does not retry, does not report a stalled subscription, and
  does not hold the write lock while failing.
- **Watchdog ordering.** The renderer's stall deadline is strictly greater than the sum of the
  database busy timeout and the IPC deadline, so a lock timeout can never surface as a
  producer-liveness failure.

## Appendix — how these numbers were produced

All figures come from a byte copy of the live database
(`~/Library/Application Support/Synth Desktop/synth.sqlite3`, 475,336,704 bytes, taken
2026-08-31), read with Python `sqlite3` and Node on this machine. Absolute constants are
therefore indicative — serde is faster than Python's `json`, and the OS page cache was warm —
but every claim above rests on a *ratio* or a *shape* measured within one runtime:
3,063 ms vs 1.1 ms for the same read under two transaction behaviors; ~24 µs per event of
history, linear across six sample points; 3 ms → 268 ms across a 22× increase in event count.
Those hold regardless of the constant factor.

The lock-contention measurement holds one writer in `BEGIN IMMEDIATE` for 3 seconds and times
a concurrent reader under each transaction behavior. Both connections use
`busy_timeout=30000`, matching `database.rs:107`.

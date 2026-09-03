# Visual read path — implementation record

Branch: `codex/v093-visual-read-path` (off `codex/v093-unnotarized-release`)
Reviews: `visual-data-local-cache-handoff.md` and `visual-data-local-cache-review.md`
Date: 2026-08-31

## What shipped

### D1 — reads no longer take the write lock
`Database::run_read` / `read_transaction` open `Deferred`, so a read takes a WAL
snapshot instead of the exclusive lock. `run_view_v2` was the hot-path offender
and is converted. Measured: the same read behind a 3s writer went from 3,063ms
to 1.1ms.

`busy_timeout` is now split by purpose rather than global: readers get 5s, which
resolves inside the renderer's 15s stall watchdog so a lock wait can never be
reported as a dead producer; writers keep 30s, because a write that gives up is
lost work. Pinned by `a_read_transaction_does_not_queue_behind_a_writer` and
`a_reader_gives_up_before_the_watchdog_and_a_writer_does_not`.

### D2 — appends fold forward instead of replaying the journal
`commit_validated_events` used to load and deserialize every event a run had
ever emitted, on every append, inside the write transaction. It now folds the
new batch onto the durable projection via `kernel::bridge::fold_envelopes`, and
resumes the usage accumulator from the run row.

Terminal facts, algorithm settlements, evidence amendments, and usage
reconciliation stay on the full-replay path: their rules are defined across the
whole history, not within a batch (`can_fold_incrementally`). That is a handful
of events per run.

Two tests guard it. `incremental_fold_matches_full_replay_of_the_whole_journal`
asserts the projection is identical whether events arrive one at a time, in
fives, or all at once — if the optimization is ever wrong, that says so rather
than a user seeing a wrong reward later. `ordinary_appends_never_replay_the_whole_journal`
asserts the property directly by counting replays: 40 progress appends produce
0, the terminal fact produces exactly 1.

### D3 — pre-admission runs are readable
`MIGRATION_64` backfills admitted specs for local runs that predate kernel
admission (25 of 26 runs on the reference machine had none, so every open
replayed the journal and then failed). Provenance stays honest: digests are
namespaced `legacy-local:` and the authorization records
`pre_admission_local_run_migration`.

Repair also moved off the read path into `repair_kernel_projection`, and a run
that genuinely cannot be repaired now raises a non-retryable typed failure
(`optimizer.projection.missing_admitted_spec`) instead of being retried five
times with a backoff ladder.

### Phase 1 + Move 1 — one conditional read, and first paint before the journal
`optimizers_run_view` returns the projection, the run record, and the durable
tail from one deferred transaction, and answers `unchanged` when the caller's
projection revision is current. The renderer publishes as soon as that lands,
so aggregate surfaces mount while evidence is still hydrating; the snapshot
carries an `evidence` lane so detail tabs show their own state instead of
inferring emptiness from a zero-length array.

The 750ms poll is now a conditional probe that carries no payload when nothing
changed, and terminal runs arm no interval at all.

### Move 2 — range-addressed evidence
`optimizers_evidence_page` takes the spans the caller already holds and returns
the complement. A cursor can only say "after N", which is right for a live tail
and wrong for browsing: a reader holding `[1..500]` and `[2000..2259]` who asks
"after 2259" fetches nothing and keeps the hole forever. `createEvidenceClient`
owns coverage on the renderer side so a template asks for a window and never
re-transfers what it has.

### Move 3 — a typed render receipt, not a second copy of the evidence
Deliberately not the handoff's Tier A. After D1 the durable projection *is* the
local snapshot: 1.4ms to read, no lock, no producer — so a terminal visual
already reopens offline. Copying `run_view_v2` into a second table would create
a second authority for product truth that can drift from the kernel projection,
which invariants 1 and 2 exist to prevent.

What was actually missing was the *claim*, not the data. "This rendered" lived
as untyped JSON on the mutable `optimizer_runs.summary_json` blob, so nothing
could detect a reopened visual being served evidence older than what it had
already shown. `visual_render_receipts` (MIGRATION_65) makes it checkable —
identity, projection revision, content digest, template version, journal tail —
and the write refuses to move a revision backwards. On reopen the renderer
compares and reports `regressed` or `content_changed` rather than rendering it
silently. A template change yields `unverified`, because different code
legitimately renders the same projection differently.

The untyped summary field is still written: the paid-compute start gate reads it
as proof a visual mounted before money is spent.

### Also
`mergeOptimizerEventPage` was quadratic — it rebuilt an index from the whole
history and re-sorted on every page. It now retains the index and mutates in
place, with the single defensive copy taken at the publish boundary. Measured
over a full paged walk: 268ms → 16ms at 50,000 events, and 200,000 events now
complete in 34ms.

## Retention: parked journals

Found while probing for a reported leak, and it is a real one — pre-existing,
not a cycle, and not a lost reference.

`MAX_PARKED_ENTRIES = 32` bounded how many runs the subscription store
remembered. It did not bound how much. A five-event smoke run and a
2,259-event eval counted the same against it, so after a user opened and closed
thirty-two visuals the store still held thirty-two complete journals — on the
reference machine roughly 9MB each — with nothing on screen.

The policy was written to preserve a *cursor* so a reopened dialog resumes
instead of replaying. It ended up preserving the whole history with it.

Measured, driving the store through 34 open/close cycles at the real craftax
run's shape (2,259 events, ~4.1KB each) and forcing GC between:

| build | retained after every visual is closed |
| --- | --- |
| `codex/v093-unnotarized-release` (pristine) | 53.9 MB |
| this branch, before the fix | 58.1 MB |
| this branch, after | **13.7 MB** |

The middle row is mine: the retained sequence index and the publish-boundary
copy that make the paged walk linear added ~8%. Parking now drops the index —
it is rebuilt from `events` on the next merge, so an idle entry was carrying a
second copy of every key for nothing.

The bulk is the pre-existing policy, now bounded by volume as well as count
(`MAX_PARKED_EVENTS`). Journals are released oldest-touched-first; the run a
user just left keeps its history, which is what parking is for. A released
entry keeps its projection and resets its cursor, so reopening it still paints
immediately and re-reads evidence from the durable start — cheap now, because
the aggregate no longer waits for the journal.

## Verification

| suite | result |
| --- | --- |
| Rust `cargo test --lib` | 1,761 passed |
| Renderer `node --test` | 617 passed |
| `tsc --noEmit` | clean |
| `vite build` | clean |
| protocol bindings | regenerated, 311 commands |

Failures are identical to the pristine `codex/v093-unnotarized-release` baseline
and reproduce there: `instance::tests::every_mcp_adapter_…`, the seven renderer
tests (`activity_presentation`, `experiment_overview_playback`,
`splitter_surface`, `tauri_base_config`, `visual_pane_min_width`), and the
`optimizers::container_eval` pool, which is timing-flaky on this machine at
7–10 failures per run on *both* branches and passes test-by-test in isolation.

## Not done

- Per-template adoption of the evidence client. The capability, its coverage
  protocol, and its tests exist; templates still read `events` off the payload,
  which continues to work. Adoption is incremental and non-breaking.
- The broader `run_transaction` → `run_read` sweep. Only the visual hot path was
  converted; 99 sites remain `Immediate`, many of them read-only. Each needs
  checking individually — a `Deferred` transaction that writes fails loudly, so
  this is mechanical but not blind.
- Poster renditions and cache GC (handoff Phase 4). Unstarted.

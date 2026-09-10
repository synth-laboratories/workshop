# Workshop visual data reliability and local-cache handoff

Status: implementation handoff  
Scope: optimizer/eval-backed visuals in the desktop Workshop app  
Primary symptom: opening an existing visual spends seconds in “Restoring run evidence…”, may fail with “subscription stalled”, and only later renders.

## Executive diagnosis

The chart renderer is not the primary bottleneck. `VisualHost` refuses to mount an optimizer visual until `RunProgressSubscription` has produced a run payload. That subscription performs a live transport sequence even for terminal runs:

1. read the durable V2 projection (`runViewV2`);
2. read the compatibility run record (`get`);
3. page through persisted raw events (`eventsAfter`, 500 at a time);
4. only then publish the payload that allows the visual shell to mount.

Any hung IPC/database/producer read holds the entire visual behind a 15-second watchdog. After five failures the pane presents the producer as unavailable. The screenshot’s “journal is hydrated” and “producer stopped answering” states are therefore transport/hydration states, not evidence that React or the chart itself needs that long to render.

Workshop already has the beginning of the right architecture: CoreRuntime persists an authoritative optimizer kernel projection in SQLite (`optimizer_algorithm_projections.projection_json`) and exposes a versioned `OptimizerRunViewV2`. The renderer, however, does not have a durable read-through cache. Its subscription cache is a module-level `Map`, retains at most 32 parked runs, and disappears on app restart. A terminal visual therefore depends on a newly successful live read despite already having durable local truth.

## Why the current system is weak

### 1. Presentation availability is coupled to live transport availability

`VisualHost` renders a skeleton whenever it has an optimizer binding but no `optimizerPayload`. It turns a subscription failure into “Run evidence unavailable.” A completed, previously viewed visual can consequently become unreadable merely because the bridge or optimizer service is temporarily stalled.

This violates the desired product invariant:

> Once Workshop has successfully rendered a revision from complete local evidence, that revision remains viewable offline and after restart.

### 2. The hot path fetches more than the first paint needs

The durable V2 projection is explicitly product truth, but `load()` also waits for the legacy run record and every event page before it publishes. Aggregate cards and charts generally need the V2 projection; raw terminal/enrichment events are for replay, frame drill-down, and diagnostics. Requiring both tiers before first paint turns optional detail hydration into a blocking dependency.

### 3. Reads are serial and polling is aggressive

`load()` awaits `runViewV2`, then `get`, then all `eventsAfter` pages. Active subscriptions also poll `runViewV2` every 750 ms, in addition to producer wakeups. This creates avoidable transactions and IPC crossings and increases contention precisely when many output rows or visual surfaces are mounted.

### 4. “Cache” means renderer memory, not durable local state

The shared subscription correctly deduplicates multiple consumers inside one renderer process and retains cursors while entries are parked. But the cache:

- is lost on app restart or renderer reload;
- is evicted after 32 parked entries;
- has no schema/version key;
- cannot serve a last-known-good visual before transport recovery;
- does not make terminal views independent from subscriptions.

### 5. The failure label identifies the wrong owner

The user sees “producer stopped answering,” but the timed operation might be `runViewV2`, `get`, or `eventsAfter` over the desktop bridge and SQLite worker. These have different owners and remedies. The current error collapses backend projection load, journal paging, IPC congestion, and actual producer liveness into one “subscription” failure.

### 6. Existing rendition storage does not solve template visuals

Workshop stores SVG renditions for chart/Mermaid/systems visuals, but the Craftax optimizer visual is a React template surface backed by live structured data. Caching only a screenshot/SVG would make first paint fast but would discard replay, selection, drill-down, and accessibility. The primary cache must therefore store the structured visual projection; a poster rendition is a useful secondary cache.

## Target architecture

Use a two-tier local snapshot with stale-while-revalidate semantics.

### Tier A: visual projection snapshot (blocking data, durable)

Persist a self-contained `VisualDataSnapshot` in CoreRuntime/SQLite or CAS. It should contain exactly the structured data required to mount the visual’s default view:

```text
visual_id
visual_revision
optimizer_run_id
projection_schema_version
projection_revision
template_id
template_version
data_digest
lifecycle              # running | terminal
completeness           # complete | partial
run_view_v2             # canonical aggregate/product truth
run_summary             # only compatibility fields still required by templates
terminal_cursor
enrichment_cursor
created_at
updated_at
```

Key the snapshot by `(visual_id, visual_revision, optimizer_run_id, projection_revision, template_version)`. Store the JSON/blob by digest and keep a small indexed manifest row.

For a terminal complete run, the snapshot is immutable. For a live run, atomically replace it only after a valid newer projection is committed.

### Tier B: evidence pages (non-blocking detail data, durable)

Keep raw events in their existing journal. Add locally cached, cursor-addressed evidence pages or rely on the existing durable event table, but load them only when the user opens Replay, Agent transcript, Raw trace, or frame drill-down. Page keys must include run ID and inclusive cursor range plus a digest.

Aggregate charts must not wait for Tier B.

### Optional Tier C: poster rendition

After a successful render, persist a lightweight PNG/SVG poster keyed by the Tier A `data_digest`, template version, theme, and size class. Use it for instant visual continuity while the interactive template bundle loads. Never treat the poster as evidence authority.

## Required read behavior

On visual open:

1. Read the local `VisualDataSnapshot` in one IPC call.
2. If valid, mount the interactive visual immediately from it.
3. In the background, compare its `projection_revision` with the current durable head.
4. If a newer revision exists, fetch one new snapshot and atomically swap it in.
5. Subscribe after the cached revision/cursor only for nonterminal runs.
6. Fetch evidence pages lazily for detail tabs.
7. If refresh fails, keep the cached view visible and show a nonblocking “refresh unavailable” badge with its as-of revision/time.

On first open with no snapshot:

1. Fetch `runViewV2` and the minimal run metadata together in one backend command/transaction.
2. Publish/mount immediately after validating identity, schema, and completeness.
3. Persist Tier A.
4. Hydrate raw evidence asynchronously.

On terminal transition:

1. Commit the terminal kernel projection and manifest.
2. Build and persist the terminal Tier A snapshot in the same transaction or through the existing durable outbox.
3. Stop polling.
4. Do not require the producer or subscription to open the visual again.

## Invariants

1. CoreRuntime’s kernel projection remains the sole authority for lifecycle, aggregate result, reward, usage, and completeness.
2. A renderer cache never manufactures or reduces product truth from an incomplete event stream.
3. Snapshot identity must match visual ID/revision, run ID, projection schema, projection revision, and template version before use.
4. Revisions are monotonic. A lower revision cannot overwrite a higher cached revision.
5. A cursor gap never gets silently patched. Detail views fail closed or reload the affected page range.
6. Terminal + complete snapshots are immutable and viewable without a live producer.
7. Cached partial/live snapshots are visibly labeled with their as-of revision and refreshed in the background.
8. Failure to refresh never blanks a valid last-known-good snapshot.
9. Cache writes are atomic: temp/blob write, digest verification, then manifest pointer swap.
10. Cache invalidation is explicit on projection schema, template version, visual revision, or data digest changes.
11. Authorization/ownership is checked before returning a cached snapshot; the cache must not bypass `sessionRef` or workspace visibility rules.
12. Raw reasoning/evidence exposure rules remain unchanged; caching cannot broaden what the visual is permitted to display.

## Proposed API boundary

Prefer one read model command rather than teaching every renderer to orchestrate three transports:

```ts
type VisualDataEnvelope = {
  snapshot: VisualDataSnapshot | null;
  source: "local_snapshot" | "kernel_projection";
  stale: boolean;
  currentProjectionRevision?: number;
};

visuals.dataSnapshot(visualId, visualRevision): Promise<VisualDataEnvelope>
visuals.refreshDataSnapshot(visualId, visualRevision): Promise<VisualDataEnvelope>
visuals.evidencePage(runId, afterCursor, limit): Promise<EvidencePage>
```

The backend should resolve the visual binding, verify ownership, load/project the run, and return a coherent envelope from one database transaction. Do not expose “get + runView + events” as a required UI choreography.

## Implementation sequence

### Phase 0 — measure before changing semantics

Add spans and counters for:

- visual open → cached first paint;
- visual open → interactive first paint;
- `runViewV2`, `get`, and each `eventsAfter` page separately;
- SQLite queue wait vs query/deserialize time;
- bytes and event count loaded before first paint;
- cache hit/miss/stale/invalid reason;
- number of subscribers and polls per run;
- full replay count and reason.

The error shown to users should identify the failed stage (`projection`, `metadata`, `evidence page`, or `notification channel`) without exposing internals.

### Phase 1 — unblock first paint from raw events

- Publish after the canonical V2 projection and minimal metadata are available.
- Move event paging to a second, nonblocking state channel.
- Let the template render aggregate/metrics surfaces while Replay/Trace show their own loading state.
- Fetch projection and minimal metadata in one backend call, not serial IPC calls.

This is the highest-value and lowest-schema-risk improvement.

### Phase 2 — durable read-through snapshot

- Add the snapshot manifest/table and CAS blob.
- Build snapshots on successful canonical reads and on projection commits.
- Load snapshot first in `VisualHost` (ideally above it, in a visual data repository).
- Preserve valid cached data across transient refresh failures and restarts.
- Deduplicate refreshes process-wide by snapshot key.

### Phase 3 — terminal materialization and lazy evidence

- Use the kernel projection outbox to materialize terminal visual snapshots reliably.
- Stop subscribing/polling for terminal complete snapshots.
- Add paged detail-data loading and prefetch only on intent (tab hover/open, selected rollout).

### Phase 4 — poster and cache maintenance

- Add optional poster renditions for template visuals.
- Add bounded LRU/size policy, last-access time, schema-aware garbage collection, and integrity verification.
- Never evict the only terminal snapshot merely because a renderer entry was evicted; durable cache policy is independent of the 32-entry in-memory subscription limit.

## Acceptance tests

### Functional

- Open a completed Craftax visual, quit Workshop, disable/kill the optimizer producer, restart, and open it again. The same terminal visual renders from local snapshot.
- Open Replay after offline first paint. Aggregate visual remains visible; only evidence detail reports unavailable if its local page is missing.
- Open one run in transcript, dialog, and pane. Exactly one refresh occurs.
- Advance a live run by one projection revision. Cached view stays visible until an atomic swap to the new revision.
- Inject a lower, wrong-run, wrong-visual, wrong-template, corrupt-digest, and gapped snapshot. Each is rejected without replacing last known good.
- Change template version or projection schema. Old cache is invalidated and rebuilt.
- Verify ownership denial is identical for cached and uncached reads.

### Performance targets

- Warm terminal visual: cached structured first paint p95 < 150 ms; interactive p95 < 500 ms on the reference Mac.
- Cold terminal visual from local SQLite projection: p95 < 1 s without event replay.
- Opening 10 output rows does not create 10 polling loops for the same run.
- Default visual first paint reads zero raw event pages.
- A refresh stall never replaces a visible valid snapshot with a blank/error page.

### Restart and fault injection

- Restart between projection commit and snapshot materialization; outbox recovery produces the snapshot exactly once.
- Kill the app during a cache write; previous manifest remains valid.
- Hang each bridge method independently and verify stage-specific UI plus cached fallback.
- Force event histories larger than 500, 5,000, and 50,000 events; first-paint latency remains approximately constant.

## Likely files to change

- `apps/synth_desktop/src/renderer/src/components/VisualHost.tsx` — stop gating the entire shell on full event hydration; consume a visual data envelope.
- `apps/synth_desktop/src/renderer/src/runtime/runProgress/subscription.ts` — split projection state from evidence state; reduce polling; keep it for live deltas, not terminal restoration.
- `apps/synth_desktop/src/renderer/src/runtime/runProgress/project.ts` and `viewV2.ts` — project from the cached canonical V2 view.
- `apps/synth_desktop/src/renderer/src/runtime/desktopBridge.ts`, bridge types, and generated protocol — add snapshot/evidence-page commands.
- `apps/synth_desktop/src-tauri/src/optimizers/service.rs` — expose one coherent minimal visual read and avoid historical replay on the normal hot path.
- `apps/synth_desktop/src-tauri/src/optimizers/kernel/persist.rs` — continue using the saved kernel projection as authority.
- `apps/synth_desktop/src-tauri/src/optimizers/kernel/outbox.rs` — trigger/recover snapshot materialization.
- `apps/synth_desktop/src-tauri/src/visuals/registry.rs` and storage migrations — snapshot manifest/CAS integration and optional template poster rendition.

## Non-goals and traps

- Do not merely raise the 15-second timeout. That hides contention and makes failure slower.
- Do not put raw journal reduction back in React. It creates a second authority and makes gaps dangerous.
- Do not cache only rendered HTML or SVG. The visual must remain interactive and evidence-addressable.
- Do not key by run ID alone. Visual revision, projection revision/schema, template version, and digest are required.
- Do not poll terminal runs.
- Do not erase a valid cached view on refresh failure.
- Do not call browser `localStorage` the durable cache; it lacks the ownership, transaction, migration, and integrity guarantees already available in CoreRuntime.

## Definition of done

The work is done when a previously opened terminal visual loads quickly after a full Workshop restart with the producer unavailable; aggregate presentation never depends on raw-event hydration; live visuals update monotonically from the durable projection; detail evidence remains cursor-verified and lazy; and fault-injection tests prove that stale, corrupt, unauthorized, or gapped cache entries cannot become authority.

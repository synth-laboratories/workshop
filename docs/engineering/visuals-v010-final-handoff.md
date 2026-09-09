# v0.10.x visuals: end-to-end completion handoff

Updated 2026-09-09. This is a continuation handoff, not a release-completion receipt.

## Objective and scope

Finish the non-Eraser visuals-engine extraction and integration for Workshop
v0.10.x. The reusable engine must support diagrams and domain visuals equally:
human/AI interaction, semantic inspection, logical time, shared panes, retained
evidence, snapshots, recordings, coherent pixels, and MCP. Workshop owns actual
eval/annotated-eval, GEPA, SFT, CISPO, multi-agent, rollout, and research visuals.
Do not move those domain semantics into the generic engine. Eraser is excluded.

The user requested implementation end to end, not another architecture proposal.
Preserve the implemented work and close the remaining acceptance/release gates.

## Repository and safety

- Repository: `/Users/joshuapurtell/GitHub/workshop-v010`.
- Branch: `codex/v0.10.0`; inspected HEAD: `7a099b0c`.
- **Large mixed dirty worktree**: visuals work coexists with session/Codex,
  optimizer, styling, generated protocol, and release work. Do not reset, clean,
  stage everything, commit everything, or assume every changed file is ours.
- Original `visuals/` domain directories have moved to
  `packages/workshop-visuals`; compatibility symlinks are intentional. Git's
  deleted-old/untracked-new presentation is not permission to discard them.
- No provider calls or paid experiments were run for this acceptance.
- Do not access macOS Keychain or Workshop's Keychain-backed secret registry.
  Project-local authorized credentials/secrets proxy only if later needed.
- Do not interrupt other Workshop processes or builds. Several other release
  and acceptance instances were running; only the isolated instance below is
  owned by this task.
- Use `apply_patch` for edits. No subagents were authorized for this task.

## Read first

1. `docs/engineering/visuals-v010-migration-matrix.md`: current ledger. Read the
   September 9 delta before its older numbered gate inventory; some original
   "implement" items now have code and partial/native evidence.
2. `docs/contracts/visual_session_v1.md`: committed session, replay, source cuts,
   capture, and authority boundaries.
3. `docs/contracts/visuals_core_v1.md`: package boundaries and analytical adapter.
4. `docs/engineering/visuals-v010-completion-plan.md`: original broader plan;
   its baseline observations are not all current defects.
5. `docs/receipts/2026-09-08/visuals-native-acceptance.json` and
   `docs/receipts/2026-09-09/visuals-retention-acceptance.json`.

## Architecture already implemented

| Area | Location / responsibility |
| --- | --- |
| Portable contracts | `packages/visuals-protocol` |
| Session client, conformance runtime, query interfaces, evidence helpers | `packages/visuals-sdk` |
| React session/control hooks, logical playback, toolbar, viewport, capture adapters | `packages/visuals-react` |
| All 40 domain families, shared Workshop components, binding/read-port orchestration | `packages/workshop-visuals` |
| Native committed session authority | `apps/synth_desktop/src-tauri/src/visuals/engine.rs` |
| Bounded SQLite analytical engine | `.../visuals/query_engine.rs` |
| Workshop recorded-run adapter | `.../visuals/collection_corpus.rs` |
| App host/transport integration | renderer `components/VisualHost.tsx`, `visuals/WorkshopVisualSession.tsx`, `components/VisualsPage.tsx` |

Native reducer owns committed versions, deduplicated receipts, durable actions,
checkpoints, recording seek/playback, and tick arbitration. Human UI and MCP use
the same state. Domain effects—annotation submission, training, provider calls,
source edits—are not replayed presentation actions.

Migrations 73/74 contain session/corpus storage; 75 adds separately retained
corpus details; 76 adds digest-addressed evidence cuts. Recheck migration numbers
against concurrent release work before integrating. Legacy migration 72 storage
is compatibility-only, not the mounted session authority.

## Latest implementation to preserve

### Retained evidence and reads

- SDK `src/evidence.ts`: `retainVisualRead` pins explicit read answers before
  returning them. In replay it reads only retained answers, never the live port;
  missing or tampered answers fail explicitly.
- SDK `src/client.ts`: transport `evidenceCuts` capability and queued
  `commitEvidence`; renderer-only native `evidence.put`, read-only
  `evidence.read`. Bodies are bounded at 1.5 MB and scoped to visual revision.
- React `src/evidence.ts`: `useVisualEvidence` displays a retained cut and blocks
  readiness while source input and committed digest disagree.
- `VisualHost.tsx`'s `PinnedTemplate`: retains serializable props, deduplicates
  top-level object aliases, and excludes effect/read ports from serialization.
- Workshop `runtime/retainedPorts.ts`: wraps history, evidence, research,
  collection page/item reads and subscriptions. Latest fix serializes retained
  subscription commits and suppresses superseded/cancelled notifications.
- Workshop `chrome/useLiveEvalStream.ts`: retains complete rendered stream views.
- Checkpoints retain references, not repeated source bodies. Portable checkpoint
  JSON **does not bundle** source bodies/media for another host. Do not claim
  arbitrary cross-host offline portability from a semantic export alone.
- The 512-control/session bound also bounds dynamic retained-read identities.
  Large populations should use native lazy analytics, not ever-growing arrays.

### Recorded-run analytics

- `corpus.from_collection` validates an actual canonical optimizer-run binding
  on the visual revision. A nested lookalike descriptor in fixture data does not
  authorize access. Collections are allowlisted: candidates, rollouts,
  evaluations, metric_points, proposer_calls.
- One transaction pins the existing durable projection's metadata and detail
  rows. A projection behind its durable journal fails closed. The adapter never
  repairs/reduces the optimizer's source authority itself.
- `corpus.detail` loads only the selected detail, with a 2 MB response limit.
- `components/RecordedCollectionExplorer.tsx` implements exact metadata facets,
  bounded pages, filter history, selection, explicit source refresh, and lazy
  details. It does not invent reward/outcome semantics from generic scores.
- `analysis.swarm_trajectories.v1` uses this mode for a bound `run.id`; explicit
  trajectory-array/synthetic-fixture mode remains supported.
- New example: its `examples/recorded_run_binding.json`.
- Acceptance imports now allocate fresh run IDs. Reimporting the old legacy
  fixture ID reset a projection header while journal dedup skipped old events,
  producing an invalid empty projection. Do not call that a successful empty
  population; the new guard rejects it.

### Shared panes and managed HTML

- Visuals More actions → Open mirrored pane mounts two views of the same session.
  Both get shared controls, source cuts, recording cursor, and logical clocks.
- Native `playback.tick` deduplicates per clock interval across panes.
  Restore/record seek keeps saved family `playing` flags inert.
- `components/ManagedHtmlFrame.tsx` retains normalized payloads (including the
  legitimate empty payload), cumulative latest frame references and history
  lists. Replay stops live polling. Frame bytes remain on the native media port.
- Frame-session publication waits for `synth.visual.managed.ready`; otherwise
  initial state could arrive before the imported script installed its listener.
- Capture blocks while payload/media synchronization is pending or errored.
- Existing canvas/media/CSS-image renderers without an explicit freeze adapter
  fail closed. Native video export is not advertised.

## Verified, with qualifications

- Latest full visual JS suite: **433 passed, 1 skipped** (434 tests).
- Latest core and app TypeScript checks: pass.
- Production renderer build: pass, with existing chunk-size warnings. Last tiny
  media-pending cleanup change followed this build; rerun before release.
- Native `cargo test --lib visuals::`: **124 passed, 3 ignored** before the last
  canonical-binding restriction. The two collection-adapter tests were rerun
  afterward and both passed, including embedded-descriptor rejection.
- Latest generic native sweep: **40/40 captures**, 30 generic control/stale-action/
  recording round trips; 10 capture-only. This is not all-domain acceptance.
- Additional historical cursor capture/replay passes: annotated eval, Craftax,
  Harbor, GEPA candidate/frontier, SFT, CISPO. Historical SFT/CISPO PNGs were
  inspected and showed earlier metrics/checkpoints with honest unmeasured states.
- Recorded native eval fixture: four rollout records at `collection-v1:30:30`;
  metadata-only page, lazy detail, human selection, MCP nested filter-history and
  checkpoint restoration pass. Recording ID is in the September 9 receipt.
- Native mirrored Craftax: human starts primary, pauses secondary; both frames
  agree. Recording has 21 events, consecutive frame indices, minimum tick gap
  455 ms for a 450 ms clock: no double-speed second-pane ticking.
- Managed native HTML: reviewed networkless import, control registration,
  0→3→0 captured restore; human 0→1 also tested. Latest retained-payload variant
  passed. Browser regression independently tests cold startup and restored
  sandbox controls. This does **not** prove real CAS frame-stream behavior.
- Prior strong swarm proof: 1,000→692 successes→352 recoveries, human/MCP history
  restore, recording survives native restart, zero differing decoded pixels in
  the replay comparison. Keep its receipt separate from newer revisions.
- Canonical Mermaid source revision changes reject old-revision restore.
- Native MCP wide/compact `capture_review` and review recording pass.
  `mark_ready` correctly refuses the dirty build. No guard was bypassed.

## September 9 (later) session: what changed

Read this before the ordered remaining work below; it closes step 2's data
prerequisite, changes step 3's shape, and adds one blocker.

### Format authority registered

`synth-containers 0.4.2.dev20260903` — the exact version
`apps/synth_desktop/src-tauri/synth-containers-version.txt` pins — is now
registered at `~/.synth-desktop/dev-builds/synth-containers/0.4.2.dev20260903/current`,
built by `scripts/register-local-dev-build.sh` from a **clean** worktree of
`containers@a5743efd88bbc33724057f565b4286fb7bda742e` (the dirty
`~/GitHub/containers` checkout belongs to another lane and was not used as the
build source). Without this, `resolve_trace_cli()` finds nothing and no trace
can be imported at all.

### Acceptance trace/media data now exists

`visuals/tests/produce_trace_v5_fixture.mjs` is an explicitly labelled
deterministic test producer. It drives `synth-trace serve` — the format
authority's own detached capture supervisor — to seal three rollouts of 28
events with twelve real PNG artifacts, proves each bundle self-contained with
`synth-trace verify`, and archives it. Every event and artifact carries
`synthetic: true` and `measurement: false`. It writes no SQLite row directly and
calls no provider.

```sh
node visuals/tests/produce_trace_v5_fixture.mjs /tmp/workshop-visuals-trace-fixture-Pjo0DW --rollouts=3 --events=24 --frames=4
node visuals/tests/native_visual_trace_media.mjs /tmp/workshop-visuals-native-Pjo0DW /tmp/workshop-visuals-trace-fixture-Pjo0DW
node visuals/tests/native_visual_trace_research.mjs /tmp/workshop-visuals-native-Pjo0DW /tmp/workshop-visuals-trace-fixture-Pjo0DW
node visuals/tests/native_visual_optimizer_frames.mjs /tmp/workshop-visuals-native-Pjo0DW
node visuals/tests/native_visual_managed_media.mjs /tmp/workshop-visuals-native-Pjo0DW
node visuals/tests/native_visual_nested_selection.mjs /tmp/workshop-visuals-native-Pjo0DW
```

Per-family domain acceptance, which is what closes a ledger row. `--refresh-fixture`
is required for any `optimizer_run`-bound family:

```sh
node visuals/tests/native_visual_acceptance.mjs /tmp/workshop-visuals-native-Pjo0DW <family> --capture --exercise --domain --refresh-fixture
```

### Four defects that data exposed, now fixed

1. **`has_media` could never be true for a real capture** (`data.rs`). The
   bundle manifest types every CAS blob `application/octet-stream`; only the
   sealed trace declares an artifact's `media_type`. Ingest read the inventory
   alone, so any capture whose frames live in the CAS reported no media and fell
   out of the media filter. Ingest now reads the sealed declarations and types
   those blobs (`image/png` · `observation`).
2. **Re-import kept stale flags** (`data.rs`). The `trace_index` upsert omitted
   `has_media`/`has_evidence`, so the index and the trace record disagreed after
   a re-import.
3. **Sealed media was not resolvable offline** (`data.rs`). Frame bodies lived
   only inside the archive, so a replayed pane could name a frame it could not
   show — and the only way to render one was the live endpoint replay must never
   poll. Ingest now publishes declared media bodies into the blob CAS under
   their own digests, bounded per object and digest-checked on write.
4. **Managed-template import wrote into the shipped package**
   (`visuals/templates.rs`). `managed_templates_root()` carried its own
   `SYNTH_DESKTOP_DATA_ROOT` lookup and fell back to `visuals_root()`.
   `packages/workshop-visuals/visuals/templates/accept.managed-session.v1` was
   found there — untracked, written by the earlier managed-HTML acceptance — and
   every later template index reported it as a bundled template, which is what
   was failing `visuals::templates::tests::recursively_indexes_templates_by_manifest_id`.
   It now resolves through `crate::instance::data_root()`; the index test takes
   an isolated data root; the stray directory was moved into the instance data
   root (a copy is in this session's scratchpad).

`VisualHost.tsx` also had its async-binding effect keyed on the
`artifact.bindings` object identity, which callers rebuild inline every render,
so it aborted and restarted its own in-flight read. It is now keyed on a value
signature. That is a correctness fix on its own terms; it did **not** clear the
blocker below, so do not record it as having done so.

### Verified this session

- Visual JS suite: **433 passed, 1 skipped** (434).
- Core and app TypeScript checks: pass. Production renderer build: pass, with
  the existing chunk-size warnings.
- Native `cargo test --lib visuals::`: **125 passed, 0 failed, 3 ignored** — up
  from 124 with two failures, both from defect 4.
- Native `cargo test --lib data::`: 22 passed.
- Native sweep: **40 captures, 30 control/stale/record/replay round trips** —
  identical to the recorded baseline, so none of the above regressed it.
- Every native gate passes in one sitting: trace media, trace research,
  optimizer frames, managed HTML, managed media, nested selection, recorded
  collection, and revision refusal. Four of those mint a fresh visual id per run
  and were unrunnable before the mount fix.
- Native `cargo test --lib optimizers::frames`: 3 passed, including the new
  canonical-spelling test.
- Recorded-collection round trip passes: four records, metadata-only page, lazy
  detail, human selection, seek to zero, restored selection, both captures
  paint-verified (recording `0aea0eb7-84c4-48b5-a77a-7b0d38645615`).
- New trace/media gate passes: three bundles import native, valid and
  self-contained with `hasMedia`; re-import keeps the index truthful; all twelve
  frames resolve byte-exact and distinct from `/v1/cas`; research paging holds
  one pinned snapshot forward and back.

### Fixed: `show` was a silent no-op for every workspace visual

`/show` on a visual the running renderer had not already mounted left `inspect`
answering "visual session unavailable" indefinitely, with no error anywhere.

Root cause, in `useAppController.ts`'s `visual.show` handler: the owning
conversation was read as `payload.ownerSessionId ?? event.sessionId`. Those are
different facts, and the registry says so in its own comment — `ownerSessionId`
is "who *owns* this visual, which is not who opened it", while the journal event
carries the session that opened it, which is set even for a workspace visual
nobody owns. So every workspace visual was classified as chat-owned; the branch
below then returns early unless that conversation is the active view and the
event asked to foreground it, and the workspace library — the one surface that
renders a workspace visual, because "Visuals owns its list/preview split" — was
never told. Ownership now comes only from `payload.ownerSessionId`, so an
unowned visual takes the `presentWorkspaceVisual` path it was always meant to.

Why it hid for so long: the review-capture path (`synth:visual-review-capture`)
selects and focuses a visual independently, so any script that captured pixels
still worked. Every acceptance script that captures — the sweep among them —
passed, while `show` + `inspect` alone silently did nothing. The reproduction is
one line: create a visual with a new id, show it, and inspect it.

Verified after the fix, in the isolated instance:

- A brand-new visual mounts on `show` alone in 0.5 s.
- `native_visual_managed.mjs` passes again (it mints a fresh uuid per run):
  0→3→0 capture/restore, three verified cuts.
- `native_visual_revision_acceptance.mjs` — also a fresh uuid per run — passes,
  including the foreign-revision restore refusal.
- The sweep is unchanged at 40 captures and 30 round trips, so nothing that
  already worked regressed.

### Native trace research acceptance now obtained

With the mount fixed, `/v1/traces/open` on an imported sealed trace brings up
`trace.rollout_inspector.v1` bound by digest, and the pane registers fifteen
real controls (rollout selection, annotated/full/limit/follow/markers, agent
actor/section/query/selection/pin/limit/view, and the inspector tab).

`visuals/tests/native_visual_trace_research.mjs` is the repeatable gate. Against
the 28-event sealed fixture it asserts: the pane mounts on `show` alone; a human
section change commits; the same action replayed against the version it already
consumed is refused as stale; a nested event selection commits; a pixel capture
passes the paint barrier and carries the selection it photographed; replay to
the opening event restores the opening state exactly; restoring the captured
checkpoint brings back both the section and the nested item; and the **real**
native MCP process reads that same committed state, so human and agent share one
session rather than two. It is written to round-trip from whatever state the
durable session opens in, and passed three consecutive runs.

Receipt: `acceptance/trace-research-acceptance.json` in the instance root. The
restored capture was inspected at full size: "All events", 28 recorded items,
the frame/craftax.turn timeline, and the selected frame event's detail.

### The optimizer frame lane retained frames it never served

`optimizer_frames` and `optimizer_run_media` were both zero, so nothing
downstream of the live optimizer relay had ever run against real bytes. Feeding
it real frames exposed a fifth defect, and the reason the gap was invisible:

The lane derives its rows by joining the event journal against
`optimizer_run_media` through JSON paths. The writer preserves whichever
spelling the producer used and prefers the canonical `container_event`; the
reader recognised only the legacy `containerEvent` under `delta`. Any
canonically-spelled run therefore had its frames admitted, catalogued, and made
permanently invisible — `framesLatest`, `framesList` and `frameContent` all
returned nothing for it. Both spellings now live in one shared pair of SQL
fragments so the two sides cannot drift again. Every pre-existing test in that
module used the legacy spelling, which is exactly why nothing caught it; the new
test uses the canonical one. The fix also cleared two pre-existing failures
elsewhere in `optimizers::` and broke none.

`visuals/tests/native_visual_optimizer_frames.mjs` is the gate. It derives a
journal from the packaged eval example by attaching labelled deterministic PNGs
to its seeded trial events under a fresh run id, admits it through the ordinary
`optimizers/import_local` path, and asserts the frames are *served*: byte-exact
and distinct from `/v1/cas`, coalesced to one frame per seed, newest-first
history, and no second copy on re-import.

### Managed media acceptance

New fixture `visuals/tests/fixtures/accept.managed-media.v1` — a networkless
managed document that renders frames arriving over the native media port — and
gate `native_visual_managed_media.mjs`. It proves a live frame reaches the pane
and matches the lane's newest frame for that seed; per-seed history is retained;
an explicitly selected older frame is served as a history read and displayed;
the live lane does not silently replace that selection; two cuts of different
frames do not share an image; and restoring the live checkpoint returns the pane
to the frame it photographed. Three consecutive passes, and the two captures
were compared side by side as visibly different images.

One contract worth stating, found while building it: **a selection must carry
its own seed**. Inheriting whichever seed the live lane last delivered asks the
host for a `(seed, frame)` pair that never existed, and it answers "not found"
for a frame the run really holds.

### The 40-family ledger is closed

The table in the migration matrix is now written from durable evidence under
`acceptance/domain/` in the instance root:

- **30 families** with a paint-verified domain cut on a meaningful domain
  control, a refused stale action, and a replay that restored the opening state.
- **2 more** (`live.eval_stream.v1`, `compose.visual.v1`) through
  `native_visual_nested_selection.mjs`. Their cursors are *object* controls
  carrying an identity into the bound stream, which the generic sweep skips by
  design — so the row read "no safely toggleable control" while these were in
  fact the only place nested selection and re-resolution are exercised at all.
  The gate selects a real identity computed with the shell's own
  `envelopeIdentity`, and checks that an unresolvable identity resolves to
  nothing rather than to a neighbouring event.
- **8 static renditions** that register only the generic source pin and
  therefore hold no domain presentation state; the capture and its observation
  are the acceptance.

Acceptance for an `optimizer_run`-bound family must pass `--refresh-fixture`.
Without it the visual keeps a binding to a run id that no longer resolves in
this instance, and capture correctly fails closed with "Capture visual is
unresolved, loading, or invalid".

### Pre-existing failures in this worktree, unrelated to visuals

`optimizers::` has **27 failing tests at HEAD**, before any change from this
lane — for example `cispo::tests::production_source_does_not_name_mlx_or_tinker_urls`,
which scans for an `async fn start_local` that no longer exists in `cispo.rs`.
They belong to other lanes' in-flight work in this mixed worktree. This matters
for step 1 below: a clean release checkout must not carry them, and they must
not be mistaken for visuals regressions. Measure against this baseline, not
against zero.

### Containers-side gap, recorded rather than worked around

The detached capture finalizer never populates `EventV5.artifact_ids`, so the
rollout-inspector projection's per-event `artifacts` list is always empty for any
bundle sealed through that path, and `craftaxTraceView`'s sealed-media branch —
which reads `payload.artifacts` — cannot fire. Sealed frames are reachable by
digest but not yet *linked* to their step. That linkage belongs to
synth-containers and was deliberately not patched from this repo.

## Remaining work: execute in this order

### 1. Establish the actual release integration target

Coordinate a clean release checkout/commit containing the intended visuals
changes, without absorbing unrelated dirty work. Rebuild native and renderer
from the same source identity. The last installed isolated binary predates the
final canonical-binding restriction; final TS changes were served by Vite HMR.
Do not certify that mixed dev pairing as the final package.

This requires an integration decision, not weakening `-dirty` checks. Other
release directories/processes were observed but were not adopted or modified.

### 2. Supply compatible retained trace/media acceptance data — DONE

Superseded by the sections above. Both media authorities are now populated and
exercised: sealed Trace V5 bundles with real PNG/CAS assets import through the
normal native path and resolve offline from the blob CAS, and the live optimizer
relay admits and *serves* real frames — which is where the fifth defect was
found. The original wording follows.

The isolated instance had **zero optimizer_frames, zero optimizer_run_media,
and zero imported traces**. The recorded eval fixture refers to a sealed trace
file in an old `/private/tmp/claude-501/.../evalhome/.../trace.jsonl` path that is
absent. Its metadata is usable; its referenced binary/trace evidence is not.

Use a known sealed, self-contained Trace V5 bundle with actual PNG/CAS assets
through the normal native import/read-model path. An explicitly labeled,
deterministic test producer is acceptable engineering test data, but do not
write fake acceptance rows directly into SQLite or label synthetic evidence as
experimental measurements. Do not start provider runs merely to fill this gap.

### 3. Finish managed media and nested research acceptance — mostly DONE

Managed media, nested research and nested/composed selection are covered by the
gates described above. Still open from the original list: the in-flight
cancellation/revision-change case for historical frame reads; recording-speed UI
and Previous/Next ergonomics in both panes; detached/offscreen panes; and
capture after scrolling below the initial viewport — the last of which this
session ran into directly, since a pane taller than the viewport is photographed
only down to the fold. The original wording follows.

- Native managed runtime: multiple seeds/frames, selected historical frame,
  history list, retained payload, source advancing after snapshot, restore,
  process restart, and unavailable/offline body. Confirm replay never polls the
  live latest-frame endpoint and capture never pairs one cut with another image.
- Inspect cancellation/revision changes while historical frame reads are in
  flight, and ensure later selections cannot be overwritten by earlier replies.
- Native trace research: populated next/previous event windows, nested detail,
  retained read answer, human action → MCP inspect/capture → restore, including
  missing/ambiguous identity. Generic read-retention tests are not this proof.
- Verify recording speed UI and Previous/Next event ergonomics in both panes.
  Native MCP 75 ms cadence/stepping already passes; UI acceptance is separate.
- Verify source/extension revision changes, detached/offscreen panes, and
  capture after scrolling below the initial viewport.

### 4. Close the 40-family and consumer ledger explicitly — families DONE, consumers open

The per-family half is closed; see "The 40-family ledger is closed" above and
the table in the migration matrix. What remains under this step is the consumer
audit in the second paragraph below: chat cards, right pane, Visuals library,
Reports, optimizer workspaces, authored TSX/HTML and nested/composed visuals,
checked for duplicated generic state/playback/capture ownership. That audit has
not been done.

For each family record the meaningful domain action or explain why it is static;
inspect its resulting pixels and restored semantics. Sparse/early-history or
unbound smoke examples do not close populated domain gates. Prioritize real
annotated-eval/Craftax media, GEPA candidate/frontier details, SFT/CISPO history,
and composed/nested selections. Do not blanket-mark the table complete based on
the sweep's generic toggle.

Audit chat cards, right pane, Visuals library, Reports, optimizer workspaces,
authored TSX/HTML, and nested/composed visuals for remaining duplicated generic
state/playback/capture ownership. Move any remaining reusable mechanics to core
and domain orchestration to Workshop ports, retaining app navigation/effects.
No new optimizer/evidence reducer or parallel mounted session store.

### 5. Compatibility and release certification

Only after consumers and domain acceptance pass, consolidate obsolete generic
paths. Keep required legacy import/read adapters and symlinks explicit and small;
do not delete persisted data/schema for cosmetic cleanup. In-memory runtime is
still valid for standalone/conformance, not an additional desktop authority.

On the clean built package rerun native captures, wide/compact review, record
reviews, `mark_ready`, seal, restart/offline restore, and packaged app smoke.
Verify executable/renderer/template/source identities match the certification
receipt. Update the ledger with concrete evidence and any remaining exclusions.
Only then report end-to-end release completion.

## Isolated native environment

- Root: `/tmp/workshop-visuals-native-Pjo0DW`
- App: `/tmp/workshop-visuals-native-Pjo0DW/WorkshopVisualsAcceptance.app`
- Bundle ID: `com.synth.visualsacceptance`
- Build override: `/tmp/workshop-visuals-acceptance-config.json`
- SQLite: `synth.sqlite3` under root; private loopback connection:
  `visuals-ipc.json` under root. Scripts read the token; **do not print it**.
- Vite development renderer was served at `127.0.0.1:1420`.
- Native PNGs: `visual-pixel-captures/`; detailed receipts: `acceptance/`;
  contact sheets: `gallery/`. `/tmp` artifacts are not durable release evidence.
- CUA can select the app by its exact path. Sometimes native `show` updated the
  registry but the current dev/HMR UI did not switch; opening the fixture in the
  native Visuals library allowed the test to continue. Reproduce on a cold clean
  build; do not silently count an unmounted target as acceptance success.
- A Laguna 7333 timeout in the isolated app is unrelated to fixture tests.
  Do not fix it by sourcing credentials or touching another instance.

## Commands

Run from the repository unless otherwise stated. Inspect current process state
before restarting anything; never use broad `pkill` against Workshop.

Register the format authority first if `resolve_trace_cli()` reports none; build
from a **clean** worktree, never from a dirty Containers checkout:

```sh
git -C ~/GitHub/containers worktree add --detach <scratch>/containers-0.4.2 a5743efd88bbc33724057f565b4286fb7bda742e
<scratch>/containers-0.4.2/scripts/register-local-dev-build.sh
```

Restarting the isolated instance needs both the data root and the config
override, and the Vite dev renderer must already be answering on 1420 — the app
starts without it and then answers `renderer did not answer in time`:

```sh
(cd apps/synth_desktop && node --preserve-symlinks ../../node_modules/vite/bin/vite.js --host 127.0.0.1 --port 1420 --strictPort)
SYNTH_DESKTOP_DATA_ROOT=/tmp/workshop-visuals-native-Pjo0DW \
TAURI_CONFIG="$(</tmp/workshop-visuals-acceptance-config.json)" \
  /tmp/workshop-visuals-native-Pjo0DW/WorkshopVisualsAcceptance.app/Contents/MacOS/synth-desktop
```

Another lane cleared `apps/synth_desktop/src-tauri/target` mid-session; if
`native_visual_mcp.mjs` reports ENOENT, rebuild `synth-visuals-mcp` before
concluding anything about the instance.

```sh
node --test visuals/tests/*.test.mjs
npx tsc --noEmit -p packages/visuals-tsconfig.json
npx tsc --noEmit -p apps/synth_desktop/tsconfig.json
TAURI_CONFIG="$(</tmp/workshop-visuals-acceptance-config.json)" cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib visuals::
TAURI_CONFIG="$(</tmp/workshop-visuals-acceptance-config.json)" cargo build --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --bin synth-desktop --bin synth-visuals-mcp
git diff --check
```

Renderer build, from `apps/synth_desktop`:

```sh
node --preserve-symlinks ../../node_modules/vite/bin/vite.js build
```

Use the preserve-symlinks invocation: the existing node_modules setup otherwise
hit a Rollup native-package resolution issue. Do not delete/reinstall the shared
dependency tree as a workaround.

```sh
node visuals/tests/native_visual_sweep.mjs /tmp/workshop-visuals-native-Pjo0DW --refresh-fixture
node visuals/tests/native_visual_acceptance.mjs /tmp/workshop-visuals-native-Pjo0DW optimizer.sft.live.v1 --capture --exercise --domain --refresh-fixture
node visuals/tests/native_visual_acceptance.mjs /tmp/workshop-visuals-native-Pjo0DW analysis.swarm_trajectories.v1 --example=recorded_run_binding.json --capture --refresh-fixture
node visuals/tests/native_visual_recorded.mjs /tmp/workshop-visuals-native-Pjo0DW
node visuals/tests/native_visual_managed.mjs /tmp/workshop-visuals-native-Pjo0DW
node visuals/tests/native_visual_gallery.mjs /tmp/workshop-visuals-native-Pjo0DW
node visuals/tests/produce_trace_v5_fixture.mjs /tmp/workshop-visuals-trace-fixture-Pjo0DW --rollouts=3 --events=24 --frames=4
node visuals/tests/native_visual_trace_media.mjs /tmp/workshop-visuals-native-Pjo0DW /tmp/workshop-visuals-trace-fixture-Pjo0DW
```

`native_visual_recording_controls.mjs` takes root, test visual ID, and explicit
revision. `native_visual_revision_acceptance.mjs` covers source-revision refusal.
`native_visual_mcp.mjs` takes root, visual ID, operation, and JSON fields; include
`revision` (default is 1). It invokes the real native MCP process, not a mock.
Inspect scripts before use; refresh bumps test-owned visual revisions and makes
old control-schema receipts intentionally inapplicable to the new revision.

Key recent logs: `/tmp/workshop-visuals-final-js.log`,
`/tmp/workshop-visuals-final-core-tsc.log`, `/tmp/workshop-visuals-final-tsc.log`,
`/tmp/workshop-visuals-final-renderer.log`, `/tmp/workshop-visuals-final-native.log`,
`/tmp/workshop-visuals-final-collection.log`,
`/tmp/workshop-visuals-domain-native.log`,
`/tmp/workshop-visuals-recorded-roundtrip.log`,
`/tmp/workshop-visuals-managed-pinned2.log`,
`/tmp/workshop-visuals-managed-browser2.log`.

## Definition of done

All intended code is integrated in a clean source identity; core/app/native and
browser tests pass; every family/consumer has meaningful recorded acceptance;
retained trace/media, nested research, shared clocks and recording UI pass
human/MCP/restart checks; native pixels and ready/seal certification agree with
the shipped package; the migration ledger has no unexplained non-Eraser gaps.

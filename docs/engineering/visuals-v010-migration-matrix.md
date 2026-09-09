# v0.10 visuals migration ledger

Updated during implementation. The baseline is the existing mixed worktree on `codex/v0.10.0`; session/Codex/optimizer/app/test changes listed in the completion plan are preserved. Registration is not migration. Native and end-to-end verification remain open until recorded below.

## Implementation status

- Generic session/control protocol, structural validation, immutable client snapshots and standalone reference runtime: implemented and unit-tested.
- Native committed reducer, deduplicated receipts, durable events, checkpoints, restore, paginated replay and validated import/export: implemented. Native family-clock and recording-clock ticks are version-gated; restore pauses replay so restored play flags cannot restart family animation.
- Shared React client, toolbar, native event subscription and reconnect probe: implemented. Standalone browser proves two-client synchronization, snapshot restore, logical-step replay, viewport and opaque-iframe presentation controls. Native human/MCP actions, restart durability and exact semantic/pixel replay are verified for the 1,000-trajectory swarm; two-pane and broader domain acceptance remain open.
- User intent controls: migrated across eval, annotated eval, Craftax, optimizer/GEPA/SFT/CISPO shared controls, trace detail/events, collections and subagents. Compound gestures batch into one commit. Historical cursor restoration now loads the pinned window and avoids self-canceling its request. Annotation submission/forms remain domain-owned, not replayed effects.
- Domain code physically lives under `packages/workshop-visuals`; original `visuals/` directories are compatibility symlinks. Generic rendition/viewport, declarative dynamic-diagram rendering, session/frame hooks, and last-known-good projection selection live in core packages.
- SQLite derived corpora: immutable batched ingestion, typed full-corpus query/aggregate/sample, source pins, bounded result pages, and read-only MCP access implemented. The 1k/10k/100k scale gate passes; 100k hydration took 7.63s, query+aggregate+representative sample 1.09s in this debug run. Predicate fields are not separately indexed.
- Core package typecheck entrypoints and a no-Workshop standalone host exist. Authored TSX can opt into a transport-free session/component kit; authored HTML can opt into the namespace-limited frame protocol.
- Unified native WKWebView PNG capture is implemented through a committed-state freeze plus verified paint barrier; UI, IPC and MCP use it, including review capture. Static opaque documents and managed DOM HTML share the frame handshake. Canvas/media/CSS URL-image renderers without a dedicated freeze adapter fail closed. A semantic snapshot alone is still not a pixel certificate.
- Binding/trace projection, sealed-trial comparison, optimizer subscription/readiness and managed-HTML runtime/media orchestration now live behind Workshop-package read ports. The app constructs services and retains navigation/effect authority.
- The native sweep has 40 verified PNG cuts and 30 generic control/stale-action/record/replay round trips. All 40 viewport captures were visually inspected. This is partial acceptance, not closure of all domain gates; several fixtures show empty or early-history states.

## Catalog inventory (40 manifests)

All families are destined for `packages/workshop-visuals`. Generic mechanics belong in the core packages. Native binding authority and existing observation/readiness contracts are retained. “Open” means no end-to-end completion claim.

| ID | Renderer | Binding schemas | Observation contract | Fixtures | Migration / verification |
| --- | --- | --- | --- | --- | --- |
| analysis.annotation_workbench.v1 | template | evidence:synth.annotation-workbench.v1; trace:synth.trace.v5; rubric:synth.verifier-result.v2 | Declared | Present | Domain cut verified: `annotation.view` → "findings"; stale refused; replay restored |
| analysis.chart.v1 | chart | spec:synth.visual.chart-spec.v1; data:declared binding | None | Present | Domain cut verified: `rendition.source` → true; stale refused; replay restored |
| analysis.swarm_trajectories.v1 | template | trajectories:synth.workshop.agent-trajectory.v1 | Declared | Present | Domain cut verified: `swarm.sampling` → "diverse"; stale refused; replay restored |
| analysis.visual.v1 | template | spec:synth.visual.analysis_spec.v1 | None | Present | Static rendition: registers only the generic source pin, so it holds no domain presentation state; the capture and its observation are the acceptance |
| annotation.overlay.v1 | template | trace:synth.trace.v5; annotations:synth.visual.annotation_markers.v1 | None | Present | Domain cut verified: `overlay.index` → 1; stale refused; replay restored |
| blank.canvas.v1 | template | document:synth.visual.canvas_document.v1 | None | Present | Static rendition: registers only the generic source pin, so it holds no domain presentation state; the capture and its observation are the acceptance |
| compose.visual.v1 | template | spec:synth.visual.compose_spec.v1; stream:synth.visual.live_eval_events.v1; optimizer_run:optimizer_event.v1 | None | Present | Domain cut verified through the nested-selection gate: `compose.cursor` identity selected and refused when unresolvable; stale refused; replay restored |
| model.compare.v1 | template | comparison:synth.visual.model_compare.v1 | None | Present | Static rendition: registers only the generic source pin, so it holds no domain presentation state; the capture and its observation are the acceptance |
| posttrain.rollout_viewer.v1 | template | trajectory:synth.visual.rollout_steps.v1 | None | Present | Domain cut verified: `posttrain.index` → 1; stale refused; replay restored |
| reward.breakdown.v1 | template | reward:synth.visual.reward_breakdown.v1 | None | Present | Static rendition: registers only the generic source pin, so it holds no domain presentation state; the capture and its observation are the acceptance |
| sourced.visual.v1 | tsx | stream:synth.visual.live_eval_events.v1 | None | Present | Domain cut verified: `authored.step` → 1; stale refused; replay restored |
| trace.catalog.v1 | template | result:declared binding | None | Present | Static rendition: registers only the generic source pin, so it holds no domain presentation state; the capture and its observation are the acceptance |
| trace.rollout_inspector.v1 | template | projection:synth.trace-projection.rollout-inspector.v1 | Declared | Present | Domain cut verified: `agent.acceptance-fixture.section` → "messages"; stale refused; replay restored |
| live.intern_acceptance.v1 | template | acceptance:synth.visual.live_eval_events.v1 | None | Present | Static rendition: registers only the generic source pin, so it holds no domain presentation state; the capture and its observation are the acceptance |
| diagram.mermaid.v1 | mermaid | Canonical source | None | Present | Domain cut verified: `rendition.source` → true; stale refused; replay restored |
| diagram.systems.dynamic.v1 | systems-dynamic | Canonical source | None | Present | Domain cut verified: `dynamic.source` → true; stale refused; replay restored |
| diagram.systems.v1 | systems | Canonical source | None | Present | Domain cut verified: `rendition.source` → true; stale refused; replay restored |
| experiment.overview.v1 | template | experiment:synth.experiment.overview.v1 | None | Present | Static rendition: registers only the generic source pin, so it holds no domain presentation state; the capture and its observation are the acceptance |
| craftax.eval_matrix.v1 | template | matrix:synth.visual.craftax_matrix_slice.v1 | Declared | Present | Static rendition: registers only the generic source pin, so it holds no domain presentation state; the capture and its observation are the acceptance |
| craftax.rollout_scrub.v1 | template | rollout:synth.visual.rollout_steps.v1 | Declared | Present | Domain cut verified: `craftax.rollout.index` → 1; stale refused; replay restored |
| craftax.trace_workbench.v1 | template | optimizer_run:synth.optimizer-run-document.v1 | Declared | Present | Domain cut verified: `trace.selectedCall` → 1; stale refused; replay restored |
| live.annotated_rollouts.v1 | template | stream:synth.trace-stream-event.v1; optimizer_run:synth.optimizer-run-document.v1 | Declared | Present | Domain cut verified: `annotated.globalCursor` → 1; stale refused; replay restored |
| live.container_rollouts.v1 | template | stream:synth.rollout.event.v1 | None | Present | Domain cut verified: `container.cursor` → 0; stale refused; replay restored |
| live.craftax.v1 | template | stream:synth.trace-stream-event.v1; optimizer_run:synth.optimizer-run-document.v1 | Declared | Present | Domain cut verified: `craftax.evaluationCutoff` → 1; stale refused; replay restored |
| live.eval_stream.v1 | template | stream:synth.visual.live_eval_events.v1 | None | Present | Domain cut verified through the nested-selection gate: `eval.cursor` identity selected and refused when unresolvable; stale refused; replay restored |
| live.harbor_eval.v1 | template | stream:synth.trace-stream-event.v1; experiment:synth.experiment.overview.v1; optimizer_run:synth.optimizer-run-document.v1 | None | Present | Domain cut verified: `harbor.eventCutoff` → 1; stale refused; replay restored |
| trace.workbench.v1 | template | optimizer_run:synth.optimizer-run-document.v1 | Declared | Present | Domain cut verified: `trace.selectedCall` → 1; stale refused; replay restored |
| optimizer.run.v1 | template | optimizer_run:optimizer_run.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 5; stale refused; replay restored |
| optimizer.cispo.live.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 3; stale refused; replay restored |
| optimizer.eval.live.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 15; stale refused; replay restored |
| optimizer.gepa.candidate.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 6; stale refused; replay restored |
| optimizer.gepa.evaluations.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 3; stale refused; replay restored |
| optimizer.gepa.frontier.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 6; stale refused; replay restored |
| optimizer.gepa.live.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 6; stale refused; replay restored |
| optimizer.sft.checkpoints.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 8; stale refused; replay restored |
| optimizer.sft.dataset.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 8; stale refused; replay restored |
| optimizer.sft.examples.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 8; stale refused; replay restored |
| optimizer.sft.lineage.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 8; stale refused; replay restored |
| optimizer.sft.live.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 8; stale refused; replay restored |
| optimizer.sft.rollouts.v1 | template | optimizer_run:optimizer_event.v1 | Declared | Present | Domain cut verified: `optimizer.sequence` → 8; stale refused; replay restored |

## Other surfaces

- Managed HTML and authored TSX: existing isolation retained; opt-in session interfaces implemented. Existing custom scripts are not automatically rewritten. Managed DOM HTML capture/verification is browser-tested; native managed HTML and renderer-specific canvas/media acceptance remain open.
- Subagent panel (`synth.subagents.v1`): implementation moved into the Workshop package with injected read service, shared selection, and an agent/count semantic scene. Causal relationships are not fabricated when the source does not supply them.
- App consumers: chat cards, right pane, Visuals library, Reports, optimizer workspaces, composed/nested components. Check each before removing compatibility imports.

## Acceptance evidence

- Generated native API regeneration: passes after adding `visuals_engine` (348 commands).
- Core + app typechecks and production frontend build pass.
- Latest combined visuals/source-surface suite: 456 passed, 1 skipped, including managed-HTML capture and hosted fixture-clock/binding regressions.
- Broad native `cargo test --lib visual`: 187 passed, 4 ignored. The explicit corpus scale test was run separately earlier and passed. Seal/report storage fixtures now supply valid certification identities; production readiness checks were not weakened.
- Standalone real-browser test: `node visuals/tests/standalone.browser.mjs`, passes against the documented local demo host.
- [Native acceptance receipt](../receipts/2026-09-08/visuals-native-acceptance.json): per-family capture/interaction IDs and local artifact paths. The swarm recording retained five acknowledged events across native app restart, restored exact values, and produced zero differing pixels across 4,608,000 pixels. Its 352/1,000 cohort and 692-row parent were verified through native UI and real MCP.
- Cold-mount testing found and fixed fabricated empty anonymous payloads and initial async-binding readiness; scene contributor/control publication now converges instead of starving human actions.
- Hosted finite fixtures now expose their complete source cut instead of independent per-pane ingest timers. Multiple retained stream inputs normalize correctly; annotated eval now renders 2/2 completed rollouts with retained findings, and live Craftax renders its retained reward/steps. Both were recaptured and inspected at full size.
- Native canonical Mermaid revision change passes: revision 2 renders changed source, rejects restoring revision 1's checkpoint, and leaves revision 2 state unchanged after rejection.
- Real MCP `capture_review` passes at wide and compact viewports. Both PNGs were inspected and their reviews recorded. `mark_ready` correctly refuses `7a099b0c7306-dirty`: final ready/seal certification requires a clean committed renderer build. No production guard was bypassed and this mixed worktree was not committed.
- These checks do not close the 40 per-family domain acceptance rows.

## Remaining release gates (not completed)

### September 9 implementation and acceptance delta

- Durable evidence cuts are implemented (migration 76): template props, live
  stream views, historical/research/collection reads retain canonical digests.
  Offline replay tests verify that restored reads never invoke the live port
  and reject missing/tampered dependencies. Portable checkpoint JSON does not
  itself bundle these dependencies. Managed HTML payload/frame-list retention
  is implemented; native 0→3→0 capture/restore passes after this change
  (`7cd16a8c-5b29-4b7d-8664-f394d6a327b3.png`,
  `6ba6bef1-7230-4a52-83f7-1d6543654a11.png`,
  `b06c94e4-ab9a-4320-8017-5114b7358efb.png`). A browser regression covers
  retained empty-payload startup and waiting for runtime readiness before
  publishing sandbox session state. Real CAS frame-stream acceptance is open.
- The recorded-run lazy adapter is implemented (migration 75), with explicit
  run binding, allowlisted collections, transactional revision pinning, metadata
  paging, and separate lazy details. A fresh native imported eval journal
  produces four rollout records at `collection-v1:30:30`. Human selection and
  MCP query/detail/filter-history/restore pass. Recording
  `2df689b6-313f-41ca-999f-acf71b3e235d` restores the retained selection.
  Stale projections fail closed; acceptance imports use fresh run identities.
- Native mirrored-pane UI is implemented. Human play in the primary pane and
  pause/stop-recording in the secondary pane synchronize both Craftax views.
  Recording `2e2c82aa-e67a-4f23-8491-7337d8bfc738` has 21 events with consecutive
  frames and no duplicate clock ticks (minimum observed tick gap 455 ms for a
  450 ms clock). Shared capture `301a31ab-a610-4bd2-94b9-7d391b4076a7.png`
  passes the paint barrier. Recording-speed UI acceptance remains separate.
- Networkless managed HTML native import, namespace registration, 0→3→0
  capture/restore, and human 0→1 pass for the pre-retention implementation.
  Fixture and repeatable script are under `visuals/tests/fixtures/accept.managed-session.v1`
  and `native_visual_managed.mjs`. The step-3 PNG was visually inspected.
- The latest sweep passes 40 captures and 30 generic round trips. Additional
  historical-cut captures/replays pass for annotated eval, live Craftax,
  Harbor, GEPA candidate/frontier, SFT and CISPO. SFT and CISPO historical PNGs
  were inspected and show earlier records and honest unmeasured states.
  This does not establish CAS media acceptance or all populated family cases.
- Latest visual suite: 433 passed, one skipped, including the new managed-runtime
  retained-startup regression. Native visuals suite: 124
  passed, three ignored. Core/app typechecks and production renderer build
  pass; the renderer build was rerun after managed-media changes.
  Additional subscription-race, stale-projection, and nested-descriptor binding
  rejection tests pass.

### September 9 (later) — sealed trace import, retained media, and a mount blocker

The trace/media data prerequisite recorded below is **closed**. The isolated
instance now holds three imported sealed Trace V5 bundles (28 events each) with
twelve verified `image/png` CAS artifacts, produced by an explicitly labelled
deterministic test producer, not by a paid or live run.

- `visuals/tests/produce_trace_v5_fixture.mjs` drives the exact format authority
  Workshop pins (`synth-trace serve`, the detached capture supervisor from
  synth-containers 0.4.2.dev20260903, registered from a clean worktree of
  `containers@a5743ef`). Each bundle is proved self-contained by
  `synth-trace verify` before Workshop sees it. No SQLite row is ever written
  directly, and no provider was called.
- Three ingest defects were found by that data and fixed in `data.rs`:
  1. **Media was structurally undetectable.** The bundle manifest types every
     CAS blob `application/octet-stream` — content addressing has no opinion
     about what a body is — and only the sealed trace declares an artifact's
     `media_type`. `has_media` read the blob inventory alone, so *every* real
     capture containing frames reported `hasMedia: false` and disappeared from
     the media filter. Ingest now reads the sealed declarations and types the
     blobs from them (`image/png` · `observation`).
  2. **Re-import left stale flags.** The `trace_index` upsert omitted
     `has_media`/`has_evidence`, so a re-imported trace kept its old flags while
     its trace record was refreshed — the index and the record disagreed.
  3. **Sealed media was not resolvable offline.** Frame bodies existed only
     inside the trusted archive, so a replayed pane could name a frame it could
     not show and had no honest source but the live endpoint. Ingest now
     publishes declared media bodies into Workshop's blob CAS under their own
     digests.
  Covered by `data::tests::sealed_artifact_declarations_type_the_content_addressed_blobs`.
- A fourth defect, found by a failing native test: `managed_templates_root()`
  carried its own `SYNTH_DESKTOP_DATA_ROOT` lookup and fell back to
  `visuals_root()`, so `import_managed_template` wrote into the shipped visuals
  package. `packages/workshop-visuals/visuals/templates/accept.managed-session.v1`
  was found there (untracked, written by the earlier managed-HTML acceptance),
  and every later template index reported it as a bundled template. It now
  resolves through `crate::instance::data_root()`; the index test takes an
  isolated data root; the stray directory was moved to the instance data root.
- `VisualHost.tsx`'s async-binding effect was keyed on the `artifact.bindings`
  object identity, which callers rebuild inline on every render, so it aborted
  and restarted its own in-flight read. It is now keyed on a value signature.
  This is a correctness fix in its own right and did **not** clear the gate below.
- New repeatable gate `visuals/tests/native_visual_trace_media.mjs`: three
  bundles import native/valid/self-contained with `hasMedia`, re-import keeps the
  index truthful, all twelve frames resolve byte-exact and distinct from
  `/v1/cas`, and research paging holds one pinned snapshot forward and back.
  Receipt: `acceptance/trace-media-acceptance.json` in the instance root.

**Fixed: `show` was a silent no-op for every workspace visual.** A visual the
renderer had not already mounted never mounted on `/show`; `inspect` answered
"visual session unavailable" forever, with no error anywhere. The `visual.show`
handler read the owning conversation as `payload.ownerSessionId ?? event.sessionId`,
but those are different facts — the registry's own comment says `ownerSessionId`
is "who *owns* this visual, which is not who opened it", and the journal event
carries the opener even for a workspace visual nobody owns. Every workspace
visual was therefore treated as chat-owned, the handler returned early because
that conversation was not the active view, and the workspace library — the only
surface that renders a workspace visual — was never told. Ownership now comes
only from `payload.ownerSessionId`.

It hid because the review-capture path selects and focuses a visual on its own,
so anything that captured pixels still worked: the sweep passed throughout while
`show` + `inspect` alone did nothing. After the fix a brand-new visual mounts on
`show` alone in 0.5 s; `native_visual_managed.mjs` and
`native_visual_revision_acceptance.mjs` (both mint a fresh id per run) pass
again; the sweep is unchanged at 40 captures and 30 round trips.

**Native trace research acceptance is now obtained.** `trace.rollout_inspector.v1`
mounts bound by digest to an imported sealed trace and registers fifteen real
controls. `visuals/tests/native_visual_trace_research.mjs` asserts mount on
`show`, a human section change, refusal of a stale action, a nested event
selection, a paint-verified capture carrying that selection, replay to the
opening state, checkpoint restore of both the section and the nested item, and
the real native MCP process reading the same committed state. Three consecutive
passes; receipt at `acceptance/trace-research-acceptance.json`. The restored
capture was inspected at full size and shows 28 recorded items with the selected
frame event's detail.

### September 9 (continuation) — the frame lane, managed media, and the family ledger

- **Fifth defect, and the reason no run ever served a frame.** The frame lane
  derives its rows by joining the event journal against `optimizer_run_media`
  through JSON paths. The writer preserves whichever spelling the producer used
  and prefers the canonical `container_event`; the reader recognised only the
  legacy `containerEvent` under `delta`. Any canonically-spelled run therefore
  had its frames admitted, catalogued and made permanently invisible —
  `framesLatest`/`framesList`/`frameContent` all returned nothing. Both spellings
  now live in one shared pair of SQL fragments (`FRAME_DIGEST_SQL`,
  `FRAME_SEED_SQL`) so the two sides cannot drift again, covered by
  `optimizers::frames::tests::canonically_spelled_frames_are_served_not_just_retained`.
  Every pre-existing test in that module used the legacy spelling, which is why
  it was never caught. The fix also cleared two pre-existing failures elsewhere
  in `optimizers::` and broke none.
- **`optimizer_frames`/`optimizer_run_media` are no longer zero.**
  `visuals/tests/native_visual_optimizer_frames.mjs` derives a journal from the
  packaged eval example by attaching labelled deterministic PNGs to its seeded
  trial events under a fresh run id, admits it through the ordinary
  `optimizers/import_local` path, and then asserts the frames are *served*, not
  merely retained: every body resolves byte-exact and distinct from `/v1/cas`,
  the lane coalesces to one frame per seed, history is newest-first, and a
  re-import admits no second copy.
- **Managed media acceptance now exists.** New fixture
  `visuals/tests/fixtures/accept.managed-media.v1` (a networkless managed
  document that renders frames arriving over the native media port) and gate
  `native_visual_managed_media.mjs`. It proves: a live frame reaches the pane
  and matches the lane's newest frame for that seed; per-seed history is
  retained; an explicitly selected *older* frame is served as a history read and
  displayed; the live lane does not silently replace that selection; two cuts of
  different frames do not share an image; and restoring the live checkpoint
  returns the pane to the frame it photographed. Three consecutive passes.
  Building it surfaced a contract worth stating: a selection must carry its own
  seed. Inheriting whichever seed the live lane last delivered asks for a
  `(seed, frame)` pair that never existed, and the host answers "not found" for a
  frame the run really holds.
- **The 40-family ledger is closed in the table above**, from durable evidence
  under `acceptance/domain/` in the instance root: 30 families with a
  paint-verified domain cut on a meaningful domain control, a refused stale
  action and a restored replay; 2 more (`live.eval_stream.v1`,
  `compose.visual.v1`) through `native_visual_nested_selection.mjs`, whose object
  cursors the generic sweep skips by design; and 8 static renditions that
  register only the generic source pin and therefore hold no domain presentation
  state. Acceptance for an `optimizer_run`-bound family must pass
  `--refresh-fixture`: without it the visual keeps a binding to a run id that no
  longer resolves, and capture correctly fails closed.

**Pre-existing failures in this worktree, unrelated to visuals.** `optimizers::`
has **27 failing tests at HEAD** before any change of this lane's — for example
`cispo::tests::production_source_does_not_name_mlx_or_tinker_urls`, which scans
for an `async fn start_local` that no longer exists in `cispo.rs`. They belong to
other lanes' in-flight work in this mixed worktree. This matters for remaining
step 1: a clean release checkout must not carry them, and they must not be
mistaken for visuals regressions.

**Containers-side gap, recorded not worked around.** The detached capture
finalizer never populates `EventV5.artifact_ids`, so the rollout-inspector
projection's per-event `artifacts` list is always empty and
`craftaxTraceView`'s sealed-media branch — which reads `payload.artifacts` —
cannot fire for any bundle produced through that path. Sealed frames are
therefore reachable by digest but not yet *linked* to their step. That linkage
belongs to synth-containers, not to this repo.

Superseded native data prerequisite (kept for history; traces are now populated,
`optimizer_frames`/`optimizer_run_media` are still zero): the isolated acceptance
instance contained zero `optimizer_frames`, zero `optimizer_run_media`, and zero
imported `traces`.
The recorded eval fixture's referenced sealed trace file is absent locally.
Consequently it proves recorded metadata/detail navigation but cannot certify
CAS-backed frame replay or paginated sealed-trace research. No producer evidence
was fabricated and no provider run was launched to fill this gap.

The numbered list below is the original gate inventory. Items 1 (source
retention), 3 (two-pane family clock), and 5 (recorded optimizer collections)
now have the implementation/evidence above; do not treat their original wording
as missing code. Their broader acceptance/certification qualifications remain.

Latest incremental verification (2026-09-08 local): structured navigation and
recording-speed changes pass 462 JS tests (one skipped), 13 native engine tests
(one ignored), package/app typechecks and renderer build. A further offscreen
capture regression test passes: WebKit suspends animation frames when occluded,
so Workshop uses explicit native-snapshot paint preparation, including opaque
frames, rather than waiting for foreground RAF. Native swarm revision 4 renders
1000 → 692 successes → 352 recoveries; restart and human Back/MCP restore retain
the complete navigation intent. PNG `6c6ac1bf-e4e9-4001-a22b-f5e86cee08cd.png`
was inspected with zero paint mutations and verification true. MCP recording
`f0166d99-51eb-4919-8d6c-b345448f275d` preserves 75 ms cadence across forward/back
steps. This is incremental evidence, not closure of all family gates.

1. Finish durable source/evidence-cut pinning for live projections. Complete ready/seal certification on a clean committed renderer build; native MCP capture/review already passes, but dirty-build readiness refusal is an explicit release blocker. The shared PNG barrier is implemented and verified; arbitrary renderer canvas/media/CSS images need explicit adapters. Native video export is not advertised.
2. Verify the extracted managed-HTML runtime/media ports in native Workshop; finish any remaining consumer-specific orchestration without introducing another optimizer/evidence authority.
3. Complete native two-pane shared-clock acceptance. Recording speed is now committed session state (`intervalMs`, 16..60000), preserved by stepping, with Previous/Next event controls; native rejects ticks proposing a different committed speed. Family playback and recording tick ownership are implemented; restored family play flags remain inert. Native UI acceptance of the new ergonomics remains open.
4. Complete nested research human/MCP restore acceptance. Explicit nested schemas now cover swarm history, page maps/trails and compose/eval cursors. Eval/compose retain identity only and rehydrate event details; missing or ambiguous identities remain unresolved. Swarm history retains navigation intent only and re-queries pinned cohort counts/membership. These control-schema changes require a new visual revision for previously persisted controls, not silent coercion.
5. Implement the real-run lazy analytical adapter. Current swarm binds an explicit complete trajectory array and ingests it to SQLite; it does not lazily derive arbitrary recorded optimizer/trace collections. Event occurrence/order predicates and typed aggregate tie ordering now have shared JS/native conformance fixtures (including missing/null, boolean/number/string collisions and Unicode).
6. Finish per-family native domain acceptance with populated, compatible persisted evidence (not merely a generic control toggle). Specifically exercise annotated eval, Craftax/Harbor media, GEPA candidate/frontier and SFT/CISPO history; source/extension revision changes; two panes; missing/offline states; and capture below the initial viewport where required. Current examples must not be mistaken for paid/live experimental measurements.
7. Consolidate/deprecate legacy stores/compatibility paths only after those gates pass. Eraser remains excluded.

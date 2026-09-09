# Visuals engine: remaining non-Eraser work for v0.10.x

Status: implementation plan, not a completion claim. Prepared 2026-09-08 after inspecting the current code on `codex/v0.10.0` in `/Users/joshuapurtell/GitHub/workshop-v010`.

## Outcome and scope

Workshop will import a reusable visuals engine whose sessions, semantic actions, evidence, logical time, snapshots, recordings, and renderer capabilities are shared by humans and AI clients. Workshop owns eval, annotated eval, rollout, multi-agent, GEPA, SFT, and CISPO meaning and components. Diagrams and data-rich visuals use the same lifecycle without requiring a static diagram to pretend it is an analytical corpus.

Complete the existing non-Eraser extraction and integration for v0.10.x. Start as in-process packages plus Workshop's native storage/transport adapters; a separately deployed service is unnecessary. Preserve existing visual IDs, sources, bindings, evidence authorities, authoring, export, review, readiness, and paid-compute gates. Eraser integration and a new general-purpose diagram editor are outside this plan. Reuse existing source-edit/revision workflows.

## Verified starting point

- Four packages exist: `packages/visuals-protocol`, `packages/visuals-sdk`, `packages/visuals-react`, and `packages/workshop-visuals`.
- Protocols, reference query engine, binding/extension registries, ordered feeds, clock mappings, exploration sessions, React primitives, and renderer registration exist.
- `analysis.swarm_trajectories.v1` demonstrates 1,000 deterministic trajectories, aggregate drill-down, cohorts, sampling, logical step selection, semantic scenes, and snapshot/recording buttons.
- Native migration 72, `visuals/state.rs`, generated Tauri commands, loopback IPC, and MCP persist presentation records, snapshots, and recordings.
- The catalog contains 40 `template.json` manifests. Existing families register through a compatibility adapter. Most family implementations and shared domain components still live in `visuals/`.
- Previous verification: frontend typecheck/build, core/registry tests, native state/MCP tests, migration registration, and generated bindings passed. This is not native end-to-end proof of the complete design.

Current gaps identified in code:

- The SDK MCP adapter is in-memory; native MCP does not drive the mounted React session. Native `visual_snapshot` stores a caller-provided object rather than initiating coordinated capture.
- `VisualHost` passes snapshot/recording writes to swarm, but no presentation hydration or live command subscription. Swarm has parallel local UI and SDK state.
- `VisualExplorationSession`, `ExplorationPath`, and `VisualSnapshot` assume corpus/cohort ownership. A generic static-diagram session is missing.
- Snapshots hold digests but omit the full presentation/cursor state required for faithful restoration. Recording begins with a context-free snapshot, and the UI saves only at stop.
- `ActionDescriptor.kind` is extensible but `VisualAction.kind` is a closed union. Idempotency is declared but not implemented as a durable command guarantee. The SDK invokes its domain callback before session stale-state validation.
- React selection, presentation, viewport, and timeline are separate primitives, not subscribers to one authoritative session. Presentation identity differs between the reference store and native store.
- Compatibility definitions currently advertise semantic interaction broadly; actual capabilities need auditing rather than inference from registration.
- Mermaid/systems viewers still own duplicated pan/zoom/source behavior and depend directly on Workshop bridges. `VisualHost` still owns binding, transport, domain orchestration, observation, and pane concerns.

## Ownership after extraction

| Owner | Code and responsibilities |
| --- | --- |
| `@synth/visuals-protocol` | Versioned wire contracts, schemas, identities, commands, receipts, evidence cuts, semantic scenes, clocks, snapshots, recordings, extension manifests |
| `@synth/visuals-sdk` | Generic session/reducer, subscriptions, host ports, registry validation, binding/projection lifecycle, query reference implementation, clock/feed mechanics, restore/replay/capture coordination, MCP facade |
| `@synth/visuals-react` | Session-backed providers/hooks, semantic targets, common viewport/source/rendition surfaces, timeline, accessibility, renderer registration and generic diagram adapters |
| `@synth/workshop-visuals` | Domain schemas/projectors, optimizer/trace adapters, shared Workshop components, all first-class family implementations and registrations |
| Workshop desktop/native | Database and analytical backend, filesystem/network/process authority, IPC/Tauri/MCP transport, source/rendition services, domain commands, evidence/review/seal/sharing integrations |
| `@synth/visuals` | Thin compatibility exports, legacy IDs/manifest lookup and asset aliases; no second generic runtime |
| Workshop app shell | Pane layout, navigation, application styling, host-service construction, and mounting a registered visual |

Core packages must have no imports from Workshop, optimizer modules, app bridges, or the compatibility catalog. Domain packages receive typed service ports instead of importing app globals. Add package entrypoints and CSS/asset exports deliberately; do not create reverse-import cycles while moving code.

## Ordered implementation batches

Each batch has a completion gate. Track all 40 manifests through the migration; registration alone does not count as migration. File names marked “new” are proposed implementation locations.

### 0. Establish the migration ledger and protect current work

Inventory each manifest's renderer, bindings, source authority, time model, actions, observation contract, fixtures, and consuming app surfaces. Add `docs/engineering/visuals-v010-migration-matrix.md` (new), with per-family engine integration, domain ownership, replay/capture, and test status. Include managed TSX/HTML and non-template surfaces such as the subagent pane.

Record the current worktree diff before edits. Other session/optimizer/frontend changes are present; preserve them and reconcile overlapping files rather than treating the whole diff as ours. Recheck current migrations before allocating a new number. Use an additive migration if migration 72 may already have been applied; do not silently change its meaning.

Gate: every existing renderer/family and app consumer has an explicit destination and regression check, with no unidentified fallback path.

### 1. Finish the portable contracts and generic session

Primary files: `visuals-protocol/src/index.ts`, its JSON Schema, `visuals-sdk/src/{session,api,registry,presentation}.ts`; new session reducer and validation modules as needed.

- Introduce a generic `VisualSession` with optional analytical exploration. Keep the current exploration session as an analytical capability/facade. Static diagrams need identity, source, presentation and semantic state, not fabricated cohorts.
- Define separate visual revision, evidence cut/projection version, presentation state version, and host session epoch. Shared presentation is keyed by visual + revision + view key; use a shared default view, with explicit separate views when requested. Hover/focus remain local ephemeral state.
- Make semantic references unambiguous across visual, corpus, run, lane, entity and revision. Define custom namespaced actions through registered payload schemas and tiers: ephemeral, presentation, overlay, domain effect.
- Use one reducer/command pipeline: validate capability, payload, target, expected version and idempotency before any domain effect. Make projection planning pure. Persist deduplicated receipts, reject reuse of an idempotency key with different input, and return structured failures.
- Define source-edit and domain-effect commands separately from exploration. Recording/replay must never rerun training, provider calls, filesystem writes, or annotation submission.
- Add observable session snapshots, subscribe/unsubscribe/dispose, immutable published state, and coherent scene derivation. No direct mutable reference can bypass versioning.
- Define restoration payloads, recording events, cursor sets, renderer/projector pins and provenance. Complete runtime validation at IPC and package boundaries; cross-language fixtures must establish TS/JSON Schema/Rust agreement, not just matching version strings.
- Validate extension protocol ranges, required inputs, capabilities and supported authority. Compatibility definitions advertise only implemented capabilities. Unknown renderer/action/schema errors are explicit.

Gate: static diagram and analytical session both work headlessly; rejected/duplicate/stale commands cannot mutate state or invoke effects; every supported action serializes and validates consistently across boundaries.

### 2. Connect the live runtime to React, desktop and MCP

Primary files: SDK `api.ts`, `transport.ts`, `mcp.ts`; React `index.tsx`, `state.tsx`; `VisualHost.tsx`, bridge types/implementation; native `visuals/state.rs`, `visuals_ipc.rs`, MCP operations, `lib.rs` and Specta bindings. Add a native session/command broker and a renderer-side Workshop session adapter.

- Keep Workshop native storage as durable authority. The mounted session owner executes semantic reducers/projectors; all views subscribe to committed receipts/state. Native validates incoming envelopes and serializes commands per session; the session owner validates definition-specific semantics.
- Attach/detach with an epoch/owner lease so duplicate panes or stale webviews cannot both commit. Native commits the resulting presentation/event/receipt atomically with expected-version checks, then broadcasts; acknowledge success only after commit. On commit conflict the owner reloads committed state.
- Route both human actions and MCP actions through that path. Use native events plus cursor-based resynchronization after reconnect; do not put the IPC capability token in renderer props.
- Hydrate on mount and revision/view change. Reset stale UI state, cancel in-flight work on detach, and serialize concurrent writes. A failed save must be visible and recoverable.
- Give native MCP bounded, paginated semantic inspection, discoverable capabilities/targets, typed interaction, snapshot capture/restore, recording start/stop/list/replay, and analytical query access. Adapt existing tool names compatibly; distinguish record import from capture of live state.
- Define closed-visual behavior: read durable state without a mounted view; return a precise unavailable result for live renderer actions unless an explicit open action attaches a host. Never claim a stored scene is live or acknowledge an unexecuted action.
- Reuse existing Workshop authorization for overlay/domain effects. Read-only inspection and presentation changes do not gain run-start authority.

Gate: an MCP seek/filter/selection visibly updates the open visual; a human action is immediately inspectable through MCP; two panes agree; restart restores committed state; disconnect, duplicate delivery and stale epoch tests pass.

### 3. Complete binding, projection, evidence and time lifecycles

Primary files: SDK `bindings.ts`, `standardResolvers.ts`, `orderedFeed.ts`, `clocks.ts`; Workshop bindings/projectors; current `visuals/runtime/*`, `visuals/chrome/useLiveEvalStreams.ts`, optimizer history/evidence/collection clients.

- Extract generic resolution/cache/invalidation and incremental projection mechanics from `TemplateVisualHost`. Workshop supplies bindings to optimizer read models, traces, media and streams through typed ports.
- Preserve the existing durable read path: range-addressed evidence, conditional projections, keyset-paged collections and checkpoint-backed history. Do not introduce a duplicate authority or replay entire journals on every render.
- Pin source sets and projector versions; distinguish missing, pending, failed, partial, stale, redacted and absent-by-design. Preserve last-known-good display only with explicit freshness and evidence identity.
- Make feed scope/cursor identity explicit, including starting cursor, gaps, conflicts, closure, cancellation, backpressure, and bounded retention. Repair gaps before claiming completeness. Keep separate stream cursors.
- Put wall time, source sequence, rollout step, optimizer iteration, training step and causal relations in declared domains. Cross-lane synchronization requires evidence-backed correspondences; equal integer values do not imply simultaneity.
- Wire play/pause/step/seek/follow-live through the session. Seeking historical evidence pins its cut; live arrivals do not move a paused cursor. Keep capture/review/readiness/sealing as distinct lifecycle facets and preserve readiness allowlists.

Gate: live and historical rendering agree at the same evidence cut; delayed lanes and gaps do not lose data; terminal visuals reopen offline; static visuals have no spurious timeline; unsupported clock mappings are reported.

### 4. Implement complete snapshots, restore and durable replay

Primary files: protocol snapshot/recording types, SDK `sessionStore.ts`, `session.ts`, new capture/replay modules; native state store/migrations; existing rendition, observation, review and capture services.

- Snapshot actual presentation, selections, navigation/back history, filters, cohorts/sampling receipts when applicable, cursors, source/evidence cut, semantic scene or reproducible scene reference, projector/renderer versions, viewport/theme/fonts and rendition refs. Digests alone are insufficient.
- Use one specified canonical serialization and SHA-256 for portable integrity receipts. Keep lightweight cache keys clearly separate. Validate referenced visual/revision, ownership and resource existence; retries with the same snapshot ID must be idempotent, conflicting content must fail.
- Add a capture barrier: select one committed session/evidence state, wait for the matching render observation, capture pixels, verify the state stayed coherent, then finalize the receipt. Retry within a bound or fail explicitly if inputs change. Integrate existing PNG review capture rather than creating a rival path.
- Provide load/list/open/restore UI and MCP commands. Restore a pinned historical view or explicitly fork a presentation; never silently map old state to the newest evidence. Missing sources/extensions yield a diagnostic and any saved rendition, with no false claim of exact replay.
- Persist recording start and initial complete snapshot immediately. Append ordered validated events incrementally, checkpoint at bounded intervals, and finalize on stop. Avoid rewriting the entire history per event. Enforce contiguous sequences, immutable history/checkpoints, sealed completion, ownership and revision pins.
- Implement checkpoint restore plus deterministic event replay, play/pause/speed/step/seek, and interrupted-recording recovery. Record semantic commands and evidence-cut transitions sufficient to reproduce the view, not pointer noise. Clearly label interrupted or incomplete recordings.
- Export/import a versioned bundle containing the manifest, semantic history, pins and permitted evidence/renditions. Preserve redaction and missing-resource metadata. Shared bundles must not contain credentials or accidentally embed unauthorized evidence.
- Provide semantic recording playback and static PNG/SVG exports where supported. Advertise video output only if a real bounded encoder/export path is implemented; semantic recording must not be described as a video file.

Gate: after app restart, a saved state restores the same filters, cursor, cohort, sample and selection; replay matches checkpoint digests; a crash retains acknowledged events; captured pixels and semantic receipts refer to the same state; import/export round-trips with explicit missing dependencies.

### 5. Extract the generic renderer host and integrate diagrams

Primary files: `VisualHost.tsx`, `MermaidVisual.tsx`, `SystemsMapVisual.tsx`, `SystemsDynamicVisual.tsx`, `ChartVisual.tsx`, managed HTML/TSX hosting, React viewport/timeline/surfaces, existing native renderer modules.

- Build a generic session-backed visual surface with injected content/rendition/action/capture ports. Move repeated source display, pan/zoom/fit, timeline controls, error/loading/diagnostic surfaces and keyboard behavior into reusable components.
- Make `VisualHost` construct Workshop services, resolve a registered definition and mount the surface. Move pane navigation/application chrome out of engine orchestration. Remove generic renderer dispatch based on template IDs; retain historical decoding only at compatibility boundaries.
- Register Mermaid, systems/2D ASCII, dynamic systems, charts, managed TSX and HTML with truthful capabilities. Preserve canonical source editing, version history, failed-render source fallback, themes, layout and existing exports.
- Expose diagram entities/links where source or renderer metadata provides stable semantics. For image-only output expose source/rendition-level semantics explicitly; never invent node identities from pixels. Declared source mappings can provide richer interaction.
- Reuse the existing managed frame isolation and message restrictions. Validate protocol/version/origin/size, cancel jobs on unmount and keep authored content behind host-granted ports. Extend renderer descriptors to reflect the actual in-process/worker/frame/process boundaries.
- Use a common accessible selection/viewport model, keyboard controls, focus behavior and reduced-motion handling. Renderer libraries remain adapters; do not force a shared visual layout across every family.

Gate: diagrams support shared viewport persistence, source/revision handling, MCP inspection, capture and applicable replay through the same host; no duplicated pan/zoom implementation or hidden Workshop import remains in the extracted core.

### 6. Add the indexed analytical backend and real trajectory bindings

Primary files: SDK `query.ts`, protocol query/cohort contracts, `workshop-visuals/src/swarm.ts`, Workshop read-model adapters; a native query backend and IPC operations (new).

- Implement the async backend against existing SQLite/read-model services. Compile validated typed query ASTs with bound parameters and allowlisted schema fields. Support bounded paging, stable ordering/tie-breakers, cancellation and revision-pinned reads.
- Define one set of missing/null, array membership, numeric/string comparison, aggregation, parent intersection and sampling semantics. Run the same conformance fixtures against in-memory and indexed backends.
- Add bounded relational/event predicates needed for trajectory exploration: tool/event occurrence, within-trajectory ordering, related agent/run membership and registered derived fields. Keep domain predicates and classifier versions in Workshop. Natural-language interpretation produces an inspectable typed query, never portable arbitrary SQL.
- Compute aggregates and samples over the full source cohort, not the visible page. Preserve root/parent denominators, exclusions, completeness and exactness; label heuristic/model-derived classifications separately from observed events.
- Bind swarm to real run/trace collections with lazy trajectory/event/media detail. Remove parallel shell state in favor of session selectors. Show fixtures only in explicit fixture/demo mode; malformed or unavailable real data must not silently become demo data.
- Benchmark deterministic 1k/10k/100k-trajectory corpora with event detail. Set and record latency/memory budgets on a reference machine before tuning; bounded browser memory and canceled stale queries are mandatory.

Gate: full-corpus answers remain truthful beyond page limits; a real recorded run supports aggregate → nested cohort → explained sample → trajectory → event → back; native and reference query results agree; provenance and limitations are visible to humans and MCP.

### 7. Move Workshop-owned components and migrate every family

Move implementations into `packages/workshop-visuals` with explicit core/domain/app dependency boundaries. Keep stable manifest IDs and thin compatibility exports in `visuals/`; update dynamic imports, app imports, CSS, fixtures, native template discovery and packaged asset paths together.

Extract reusable Workshop components from `visuals/families/optimizers/_shared/optimizer.run.v1`, first-class-container `_shared/traceWorkbench*`, and domain portions of `visuals/chrome`/`visuals/runtime`. Examples include run/metric headers, evidence coverage, rollout/event viewers, annotation overlays, trace media, checkpoint/candidate selectors and run-history adapters. These stay outside core.

| Migration group | Required behavior through the engine |
| --- | --- |
| Swarm / multi-agent | Full corpus/cohort exploration, model/behavior comparisons, causal parent-child/tool/delegation relations, linked agent/trajectory/event selection, samples with provenance |
| Eval / rollout / annotated eval | Matrix and rollout drill-down, scoped step history, linked media, metric truth states, annotation targets and durable review overlays; no copied evidence authority |
| CISPO | Existing training/collection/experiment surfaces, training-step and rollout clocks, reward/policy/importance-weight facts only where supplied, cohort-to-example navigation and historical state |
| SFT | Dataset/examples/rollouts/live/checkpoints/lineage, linked sample/checkpoint selection, training history and full evidence references |
| GEPA | Live/frontier/candidate/evaluations, iteration/candidate/evaluation correspondence, parent lineage and exact evidence-cut restore |
| Craftax / Harbor / container examples | Existing lane replay, media/frame scrubbing, stream readiness and annotations preserved through injected ports |
| Remaining analysis / experiment / compatibility | Catalog, inspector, comparisons, reward breakdown, sourced/composed/blank visuals, annotation workbench and experiment overview migrated or explicitly retained as a thin registered adapter |

For every manifest: declare supported inputs/projector, presentation schema, actual actions/time domains, semantic scene, observation contract, and capture/replay level. Snapshot/replay behavior is capability-specific, but no first-class interactive family may remain outside the session pipeline. Use fixtures and existing tests to preserve behavior while moving code; do not redesign algorithms as part of extraction.

Gate: all 40 ledger rows are closed with evidence; human and MCP actions use the same state for each applicable family; existing saved visuals and assets still resolve; no family relies on a second generic lifecycle implementation.

### 8. Consolidate storage, compatibility and package quality

- Align native/reference stores on revision/view/schema identity, optimistic versioning, command receipts and recording semantics. Bound list/event queries with pagination and verify deletion/reference retention behavior without deleting existing user evidence.
- Add migration fixtures from pre-v0.10 databases and the current foundation schema. Test old visuals/revisions/seals/renditions and applied migration 72, interrupted updates and restart recovery.
- Remove superseded generic state, replay and capture paths after all consumers migrate. Keep legacy API/ID adapters small and document them. Preserve the existing read-path performance invariants and evidence receipts.
- Add independent package typecheck/build/test entrypoints, deliberate public exports, dependency-boundary checks and React peer-dependency hygiene. Verify production bundle assets and native resource inclusion after moving families.
- Document extension authoring, host adapter implementation, capability/error semantics and snapshot/recording compatibility. Include a minimal non-Workshop host exercising a static visual and an analytical visual without app imports.

Gate: the engine is independently consumable; compatibility imports route to one implementation; existing artifacts survive upgrade; production packaging contains all moved assets and no accidental duplicate runtime.

### 9. Run end-to-end acceptance and prepare v0.10.x

Automated gates must cover meaningful behavior, not source-text assertions alone:

- Session/schema conformance, stale commands, idempotency, concurrent ownership, failed persistence and reconnect/resync.
- Query backend parity, truthful denominators/sampling, historical cuts, clock mappings, out-of-order streams and evidence regression detection.
- Snapshot/render coherence, restart restoration, interrupted recordings, checkpoint seeking, version mismatch and bundle round-trip.
- Family regressions for all manifests; existing visuals suite, desktop typecheck, production frontend build, native visuals/MCP/migration/generated-contract tests.
- Browser integration plus native Workshop CUA for actual Tauri/WKWebView behavior. Browser mocks alone do not prove native MCP, media, capture or restart.

Record these release scenarios as evidence:

1. Open an existing Mermaid and a systems/ASCII visual; inspect source, change viewport, capture, close/reopen, and verify state and source revision.
2. Open real persisted eval/annotated-eval, CISPO, SFT and GEPA fixtures/runs; select evidence, seek historical state, restore snapshots, and verify annotations/lineage remain linked correctly.
3. Explore at least 1,000 trajectories: aggregate behavior, narrow a nested cohort, show root/parent prevalence, choose an explained sample, inspect an event and return exactly to the parent state.
4. Drive that same exploration through real MCP while a human sees it; submit a stale action and prove no state changed.
5. Start recording, mix human/MCP actions, interrupt/restart, recover acknowledged history, replay/seek and compare semantic state and captured renditions.
6. Repeat with missing evidence, terminal/offline sources, two open views and an extension-version mismatch; failures are explicit and recoverable.

Use deterministic fixtures and local persisted evidence by default; this plan does not require paid provider runs. Measure latency, memory, subscription cleanup and bundle impact, and resolve material regressions. Prepare release notes and a reviewed diff. Commit/package/publish only within the authority of the subsequent implementation/release request; this planning handoff performs none of those actions.

Gate: the migration ledger, semantic/capture/replay acceptance matrix and release checks are complete. A passing unit suite or a working swarm demo alone is insufficient.

## Dependency order and review boundaries

Execute `0 → 1 → 2 → 3 → 4`. Then complete diagram hosting (5) and analytical scaling (6) before the relevant family migrations (7). Finish consolidation (8) and release acceptance (9). UI/adaptor work may be developed independently once contracts are stable, but do not count a disconnected primitive as integrated capability.

Keep each batch reviewable as a separate change. Do not assign individual patch-release numbers before the release baseline is checked. This is a substantial remaining refactor, not a final polish pass.

## Definition of done

The reusable packages own the generic engine; Workshop owns domain semantics and its host authorities. All existing first-class visuals run through the shared session/lifecycle boundary. A human or AI can discover a visual, inspect truthful semantic state, perform a supported action, observe the same committed result, capture it coherently, reopen it and replay its exploration. Large trajectory analysis operates on the full corpus with explicit provenance. Existing diagrams, run evidence, source authoring, annotations, review and readiness behavior remain compatible. The documented claims match behavior demonstrated in native Workshop.

## Next-session starting instruction

Read this plan and `docs/contracts/visuals_core_v1.md`, inspect current worktree changes and applicable instructions, then implement batches 0 and 1 first. Continue through the dependency order under the user's implementation authorization, updating the migration ledger and acceptance evidence as work lands. Preserve unrelated changes. Keep Eraser excluded and domain-specific code in Workshop-owned packages. Do not stop at adding interfaces or registry entries; prove each capability through the actual host and transport before marking it complete.

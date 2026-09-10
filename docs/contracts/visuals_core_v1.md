# Visuals Core v1

Visuals Core is the reusable substrate for Workshop's evidence-backed visual experiences. It is split into four workspace packages:

- `@synth/visuals-protocol` — portable contracts and JSON Schema;
- `@synth/visuals-sdk` — extension and binding registries, query backend ports, ordered feeds, logical clocks, exploration sessions, presentation state, snapshots, recordings, HTTP transport, and MCP adapter;
- `@synth/visuals-react` — semantic scene/action context, selection and presentation providers, semantic targets, truth-state/diagnostic surfaces, timeline, viewport, and the React renderer registry;
- `@synth/workshop-visuals` — Workshop-owned domain definitions and projectors, beginning with agent-trajectory swarm analysis.

The existing `@synth/visuals` package remains the compatibility/catalog package for v0.10.x. It consumes the new packages and keeps existing template IDs and persistence readable.

## Invariants

1. A corpus has stable identity, schema, count, and revision.
2. A query is a serializable typed AST, not a component callback or portable SQL string.
3. A cohort records its query, numerator, denominator, exclusions, completeness, exactness, and parent.
4. Sampling records strategy, source cohort, seed where applicable, parameters, and member identities.
5. Aggregate marks drill into source cohorts and trajectories through semantic identity.
6. Presentation actions use optimistic state versions and fail on stale state.
7. Semantic recordings store actions and coherent checkpoints, not pointer noise.
8. MCP and React operate on the same visual session.
9. Workshop supplies domain schemas and derived behavior meaning; core does not know eval, CISPO, GEPA, SFT, or agent-trajectory semantics.
10. Renderer choice comes from a registered `rendererKind`; template IDs never act as hidden renderer dispatch.
11. Evidence digests are SHA-256. Lightweight state identities are explicitly labelled and are not evidence receipts.

## v0.10.x vertical slice

`analysis.swarm_trajectories.v1` accepts explicitly bound `synth.workshop.agent-trajectory.v1` rows. An unbound visual reports missing evidence; it does not silently substitute a fixture. The standalone demo explicitly selects a deterministic 1,000-trajectory fixture. The catalog's small native smoke fixture is also explicitly bound.

The visual supports outcome, behavior, model, and reward-band aggregation; nested cohorts; root- and parent-relative prevalence; declared sampling strategies; individual trajectory/event inspection on logical trajectory time; semantic annotation targets; durable snapshot receipts; and durable semantic recording controls.

The fixture is visibly diagnosed as fixture evidence. It performs no provider calls and uses no credentials.

## Storage and process boundaries

`VisualQueryBackend` is the analytical storage boundary. The in-memory implementation is the conformance reference and `AsyncVisualQueryBackend` is the host adapter contract; domain packages can provide SQLite or other indexed implementations without changing a visual.

Desktop migrations 73/74 provide the generic committed session and immutable analytical corpus stores. React's `VisualSessionClient`, the existing `visuals_engine` Tauri command, and MCP `visual_session` (also `visual_manage`'s session operation) operate on that same authority. Migration 72 and its legacy exploration import/list interfaces remain readable during migration; they are not the mounted session's playback authority.

React renderers register through `ReactVisualRendererRegistry`. Process-backed renderers such as Eraser/Chromium will implement the separate isolated renderer boundary; they do not receive arbitrary application authority.

## Committed playback and restoration

Family playback drivers propose pure presentation patches through `playback.tick`. The native host admits at most one committed tick per visual/revision/view/clock interval, regardless of pane count. Stale or paused proposals do not consume a tick. Playback proposals are renderer-only; MCP uses ordinary presentation actions to change discovered play, pause and cursor controls.

`restore` and `record.seek` install an inert replay cursor. Recorded `playing: true` control values do not restart family playback. An explicit presentation edit exits replay. Recording playback uses `record.play` and the renderer-only, shared `record.tick` gate, with its cursor and playing flag visible to every client. Playback restores presentation, never annotation submission, provider runs, training, or source edits.

Controls may declare bounded `oneOf` variants, nullable values, required object fields and array item schemas. A mounted control inventory that disagrees with persisted schemas is not capture-ready; a compatible visual revision must be opened. Source-revision changes remount the projection lifecycle instead of retaining another revision's last-known-good payload.

## Coherent pixel captures

Semantic `capture` is distinct from `capture.pixels`. On macOS, the latter uses the existing native WKWebView capture pipeline and stores a PNG, an imported semantic checkpoint, and an adjacent JSON receipt under the instance's `visual-pixel-captures` directory. Review capture uses this same pixel path rather than substituting an SVG export. Existing observation, ownership and certification checks still apply to reviews and seals.

The host atomically freezes presentation commits, then the renderer confirms the exact revision/state version, waits for assets, pauses compositor/SMIL/media animations, and freezes decoded image frames. Mutation observers reject changes to rendered evidence during capture. The post-snapshot verification phase must succeed before a `synth.visual-pixel-cut.v1` receipt is returned. Both sides expire abandoned barriers after 30 seconds; mutations resume on release. Capture is not a claim that a live source has become a durable historical evidence cut.

Canvas and opaque-frame renderers must register `VisualPixelFreezeAdapter` with `prepare(signal)`, `verify()` and idempotent `release()` functions. Preparation must stop all pixel-producing work; verification can be asynchronous. `StaticVisualDocument` implements this handshake in an opaque frame using a nonce-authorized bootstrap. Authored scripts, inline event handlers, network access and same-origin access remain denied in static documents. Workshop's managed HTML host installs the same capture handshake under its existing script policy; DOM changes during capture fail verification. This does not grant managed renderers canvas/media capture automatically. Oversized/tainted images, CSS URL images and unsupported active media fail with a diagnostic. No video encoder is advertised.

## Workshop projection service ports

The Workshop package owns binding resolution, bounded/deduplicated sealed-trial projection resolution, optional sibling comparisons, optimizer visual subscription orchestration, and managed HTML runtime/media orchestration. The app supplies narrow read/subscription, media and ready-receipt ports. Durable optimizer projections remain authoritative, receipt comparison precedes ready-receipt replacement, and cancelled/revised consumers cannot publish late results. Hosted finite fixtures expose a complete retained source cut; family cursors control playback, not independent per-pane arrival timers. Multiple retained stream bindings preserve each stream's event identity without inventing shared run metadata or clock mappings.

Typed analytical predicates include `any` over object-array members and `sequence` over explicit numeric logical order, with an optional bounded gap. They can express observed event and relation patterns without inventing causal edges. JavaScript and SQLite share conformance fixtures.

### Recorded-run analytics

An explicit `optimizer_run` binding selects the recorded-collection mode of
`analysis.swarm_trajectories.v1`. Workshop's `corpus.from_collection` adapter
transactionally pins the existing candidates, rollouts, evaluations,
metric_points, or proposer_calls read model. The source must belong to the
visual revision and its projection must not trail the durable journal.
Migration 75 stores immutable detail cuts separately from metadata rows.
Only bounded metadata pages cross IPC; `corpus.detail` loads the selected
record (maximum 2 MB). Full-population facets, filters, page position, selection,
and nested filter history are shared presentation state. Missing score/outcome
semantics are not inferred. Arbitrary trace schemas require an explicit domain
adapter; this is not an arbitrary-SQL or whole-journal browser interface.

Migration 76 retains bounded render/read evidence by canonical digest. See
`visual_session_v1.md` for replay, offline, and portable-checkpoint dependency
rules. The in-memory runtime remains a standalone/conformance implementation,
not an additional desktop state authority. Legacy storage endpoints and
`@synth/visuals` paths are compatibility-only for v0.10.x; new integrations use
the four extracted packages and committed session API.

# Shared visual sessions (v0.10)

The generic engine does not know eval, GEPA, SFT, CISPO, or Workshop's app bridge.
Those meanings and their shared UI live in `packages/workshop-visuals`. Core
contracts, reducers/query interfaces, and React integration live respectively in
`visuals-protocol`, `visuals-sdk`, and `visuals-react`. `visuals/` retains compatibility
paths. The native adapter owns durable SQLite state; a standalone reference host
can use the same presentation protocol without native storage.

## Authority and identity

A session is keyed by `(visualId, revision, viewKey)`. `stateVersion` is the
committed presentation version, not a visual revision or evidence cursor.
Definition pins carry the registered definition's version. Presentation controls
have stable IDs and portable structural schemas. Both human and MCP mutations
use `presentation.set` or atomic `presentation.patch`. Expected state versions
are mandatory, and native receipts deduplicate action IDs/idempotency keys.
Reusing a key with different input is an error. No presentation action starts
provider work, changes source code, or submits annotations.

For these generic controls the native reducer is the authority: there is no
renderer-owner lease or a second renderer-side commit. React caches committed
snapshots, subscribes to native change events, and probes every five seconds to
recover a missed event. Published snapshots are immutable. An unmounted session
can still be inspected and changed; its last published scene is not proof that
an active renderer has painted those values. A command removes the old scene
until the renderer publishes one for the new version.

## Native/MCP operations

The compact MCP facade exposes `visual_manage` with `operation: "session"` and
an `arguments` object containing `visual_id`, `revision`, optional `viewKey`, and:

- `inspect`: discover registered controls, committed values and last scene.
- `act`: submit a typed presentation action with ID and expected state version.
- `capture`, `restore`: save/restore semantic state, with expected state version.
- `checkpoints`, `checkpoint.read`, `checkpoint.import`: bounded summaries,
  full state export, and validated same-revision imports.
- `record.start`, `record.stop`, `recordings`, `record.read`, `record.seek`,
  `record.import`: durable semantic recording, paged reads, seek and import.
- `corpus.query`, `corpus.aggregate`, `corpus.sample`: read a registered,
  revision-pinned corpus. Upload/registration are host-only operations, not MCP.

Legacy `visual_set_presentation`, `visual_snapshot`, and `visual_recording`
are storage compatibility endpoints. They do not control the mounted session.
Use the session operations for human/AI shared state.

## Snapshots and recording

`synth.visual-checkpoint.v1` contains full presentation state and a SHA-256 digest.
Canonical encoding is tagged JSON: strings and booleans carry type tags, numbers
use big-endian IEEE754 hex bits (negative zero normalized), arrays recurse, and
object keys sort by UTF-16 code units. This is a versioned cross-language format,
not JCS. Rust and TypeScript share a golden digest test.

Recordings start with a complete checkpoint. Each acknowledged event includes
its action, resulting state, and pre-command registration context, so opening a
new detail panel during recording remains replayable. Events and action receipts
commit in the same transaction. Import validates transitions without executing
them. Restore/replay never re-executes domain effects and requires recording to
be stopped. Imports require matching identity, definition and registered controls.

Limits: state 1 MiB/512 controls; requests 2 MiB; portable UI imports 2 MB;
recording reads at most 100 events and an approximately 4 MiB page budget;
the browser replay UI seeks individual events without loading the full recording.
Checkpoint lists return summaries. Large recording exports use paginated reads.

Recording playback has a shared optional `intervalMs` (integer 16..60000,
default 300). `record.play` commits speed and playing state using the expected
state version. Ticks at a different explicitly committed speed are rejected.
Stepping seeks one event, pauses playback, and retains the selected recording's
speed. Speed is an event cadence, not a fabricated mapping to source wall time.

Workshop eval/compose cursors persist event identity, not event bodies. Detail
rehydrates from the retained evidence cut and stays unresolved for missing or ambiguous
identity. Swarm history persists bounded navigation intent and recomputes
cohorts from the pinned backend; imported counts do not become an authority.
Changed registered control schemas require a new visual revision.

Semantic checkpoints alone are not certified pixels or video. The shared PNG
capture/review path coordinates native state and renderer paint barriers.
Ready/seal certification additionally requires a clean committed build.

### Retained source cuts

Hosts advertise `evidenceCuts` to enable `useVisualEvidence` and
`retainVisualRead`. Renderer-only `evidence.put` retains a canonical SHA-256
JSON body (at most 1.5 MB) scoped to the visual revision. `evidence.read` is
read-only and verifies the stored digest. Presentation controls retain digest
references; checkpoints and recording events do not repeat the source bodies.
Workshop pins template data, complete stream views, historical projections,
research reads, and collection pages/items before displaying them. Replay uses
only retained answers, never the live read port. A missing or corrupt answer
is explicitly unavailable; editing presentation intent resumes live reads.
Media still uses the domain's immutable content-addressed media port.

These are derived caches, not new optimizer or evidence authorities. The
512-control bound also bounds retained request identities within one session;
large populations use the lazy analytical adapter instead of retained arrays.
Portable semantic checkpoint JSON alone does not bundle the referenced source
bodies or media; moving it to another host requires those retained dependencies.

The native host installs `nativeSnapshotPaint` for its WKWebView snapshot path:
prepare freezes assets and layout, the native offscreen snapshot paints the
frozen tree, and verify rejects intervening mutations. This also propagates to
opaque frame adapters. Browser hosts retain foreground animation-frame waits;
an occluded browser times out explicitly rather than claiming a settled frame.

## Analytical backend

SQLite stores immutable, visual-revision-scoped derived corpora. The existing
domain read model remains the source of authority. Uploads are idempotent,
bounded to 500 rows, and queries fail until the declared corpus is complete.
Filters are compiled into parameterized SQL; result pages are at most 1,000 rows.
Counts, aggregate denominators and sampling operate on the full matching corpus,
not on the returned page. Aggregate cardinality is bounded to 1,000 buckets.
Predicates scan within the indexed corpus identity; arbitrary field predicates
do not currently have dedicated field indexes.

Missing fields differ from null, boolean operands differ from numbers, array
members are deduplicated per row, and ordered comparisons require matching scalar
types. Numeric sampling excludes nonnumeric scores; failure sampling requires
boolean true. Swarm keeps a source-revision pin and requires explicit fixture
mode instead of silently inventing trajectories for an absent binding.

## Sandboxed authored HTML

Allowlisted authored TSX can import `useVisualState`, `useVisualSessionSnapshot`,
`SemanticTarget`, `VisualViewport`, and `VisualSessionToolbar` from
`@synth/visuals-react`. This module deliberately omits client/transport exports.

The core `useFrameSession` bridge accepts messages only from the exact iframe
window. Messages are bounded to 64 KiB, registrations to 64 controls per message,
and control IDs/actions to the `frame.` namespace. The sandbox remains
`allow-scripts` with an opaque origin and its existing networkless CSP.

Send `synth.visual.session.request.v1` with `requestId`, `operation: "register"`,
`controls` and `defaults`, or `operation: "act"` and `action`. Registration
acceptance is not a commit receipt; wait for `synth.visual.session.state.v1`
before issuing versioned actions. Responses use
`synth.visual.session.response.v1`. No transport tokens, host services, domain
effect ports, or evidence bindings cross this channel. Existing authored HTML
must opt into this protocol; legacy local JS controls are not auto-migrated.

## Verification and remaining acceptance

See `docs/engineering/visuals-v010-migration-matrix.md` for the per-family ledger.
The standalone browser host proves two-client synchronization, snapshot restore,
logical-step replay, bounded population display and shared viewport behavior.
It is not a substitute for native Workshop acceptance across the domain families.

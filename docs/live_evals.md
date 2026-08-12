# Live evals as a first-class Workshop capability

**Master plan:** `execution_platform_master_plan.md`

> Shared nouns, execution identities, and independent environment/policy/harness
> lifecycles are proposed in `execution_ontology.md`. Shared provider discovery,
> resource references, and stream delivery semantics are proposed in
> `execution_stream_contracts.md`. This note owns the Container/eval profile, task event
> vocabularies, Trace V5 reconciliation, and live visual behavior.

> Private dogfood templates, persistent local extension source, recipe catalogs, and
> policy × task × execution-profile matrices are proposed in
> `private_eval_workspace_extensions.md`.

**Status:** Product and interaction design proposal
**Working mode:** Real-stream local prototypes first; engineering implementation after review
**Initial examples:** Harbor / GameBench, Craftax Rust with a ReAct policy, and Banking77

## Summary

Workshop should support live evaluations as a first-class workflow rather than as an
after-the-fact visualization task. An agent should be able to discover or register an
eval source, create and open the appropriate live visual, verify that the visual is
connected, start the eval, and leave the visual attached through terminal state and
durable trace sealing.

Before asking an engineer to implement that product surface, we will build and iterate
local HTML reference prototypes. These are not mocked-data demos. They must subscribe to
real container or eval event streams and render real rollout state, images, rewards,
achievements, policy activity, usage, cost, and failures. They are prototypes only in the
sense that a Workshop agent is not creating or managing them through the product yet.

The output of this phase is a tested interaction recipe, a canonical event contract,
three representative live examples, and an engineering-ready implementation brief for
Workshop templates, skills, MCP tools, lifecycle handling, and Trace V5 correlation.

## Why this needs first-class support

Today the pieces exist but are not yet one dependable workflow:

- Evals already writes append-only `evals.event-stream.v1` events and can expose them as
  SSE.
- Registered containers may expose rollout events, live SSE, frames, and terminal
  evidence.
- Workshop has `live.container_rollouts.v1`, `live.eval_stream.v1`, and
  `live.harbor_eval.v1` templates.
- The Synth Visuals MCP can create, bind, show, and update visuals.
- The container and eval-driver paths can register containers and execute rollouts.
- Completed evidence can be imported and inspected as Trace V5.

The missing product contract is the orchestration and identity joining these pieces.
Opening a visual after an eval finishes is not a live-eval workflow. Neither is opening a
visual against a guessed endpoint, starting execution before the viewer connects, or
showing engine steps produced by a fixed action list as evidence of an LLM policy.

There is also a concrete mismatch in the current Craftax gate harness: it binds a slot
named `live` to a container-level `/events` URL, while the current live rollout template
requires a slot named `stream` and Craftax Rust exposes per-rollout SSE at
`/rollouts/{rollout_id}/stream`. The first-class design must remove this kind of
caller-specific endpoint construction.

## Product promise

Given a registered eval or container source, Workshop can:

1. Choose a compatible live visual recipe.
2. Allocate stable run and rollout identities before execution begins.
3. Bind the visual to a normalized, replayable live stream.
4. Open the visual and prove that it is ready.
5. Start the requested eval only after readiness.
6. Display honest intermediate state throughout execution.
7. Correlate policy, environment, reward, frame, and usage events.
8. End every authoritative rollout with durable evidence and, where required, a sealed
   Trace V5 identity.
9. Reopen the completed visual without depending on a still-running container.

## Design principles

### Real streams, even during prototyping

Local reference prototypes must use real producers and real transport:

- local HTML/CSS/JS served from a loopback origin;
- `EventSource` connected to the actual SSE endpoint or a thin loopback normalization
  bridge;
- real frame URLs emitted by the environment;
- real rewards, achievements, progress, policy calls, tokens, and cost;
- real disconnect, reconnect, failure, and terminal behavior.

Recorded streams may be used for deterministic visual regression and replay after a real
run has been captured. Synthetic fixtures are useful for component tests, but cannot be
used as evidence that the live workflow works.

### Producer authority

The eval harness or policy runner is authoritative for policy activity. The environment
is authoritative for observations, actions accepted by the engine, frames, rewards,
achievements, and terminal state. The visual is a projection; it must not reconstruct,
guess, or invent evidence.

For a ReAct policy, the product may display policy identity, observation summaries,
chosen actions, tool/model-call metadata, latency, tokens, cost, and an explicitly
emitted concise rationale. It must not request or expose private chain-of-thought.

### Connect before execution

The required lifecycle is:

1. Discover, register, and probe the source.
2. Preflight policy credentials, provider, model, limits, and spend controls when a model
   is involved.
3. Allocate a run ID and rollout IDs without stepping the environment.
4. Start the SSE source or run-level bridge.
5. Create, bind, and show the visual.
6. Wait for an explicit connected/ready acknowledgement.
7. Start the eval.
8. Keep the visual attached through all terminal events.
9. Seal and attach durable evidence.

If readiness cannot be established, the paid or authoritative run should not silently
continue. A caller may explicitly choose a headless run, but it is a different mode and
must be reported as such.

### Stable identity and fail-closed correlation

Every event needed for evidence must carry enough identity to prevent accidental
cross-binding:

- schema version;
- run ID;
- rollout ID;
- task instance and seed;
- lane ID when applicable;
- monotonically increasing event sequence;
- event kind and occurrence time;
- environment step;
- policy call index when applicable;
- frame step and content digest when applicable;
- trace digest after sealing.

A frame, model event, reward, or trace whose identity does not match the selected rollout
must not be presented as if it does. Missing correlation is an explicit incomplete or
failed evidence state, not a warning hidden in logs.

## Canonical live contract

The long-term public contract should remain `evals.event-stream.v1`, with a documented
rollout vocabulary rather than a parallel Workshop-only event schema. Container-native
events such as `synth.rollout.event.v1` should be normalized at the boundary.

The minimum normalized envelope is:

```json
{
  "schema_version": "evals.event-stream.v1",
  "run_id": "run_...",
  "rollout_id": "rollout_...",
  "lane": "seed-17",
  "sequence": 42,
  "occurred_at": "2026-08-12T12:34:56.789Z",
  "kind": "rollout.snapshot",
  "payload": {}
}
```

The vocabulary must cover at least:

| Event family | Required information |
| --- | --- |
| Lifecycle | allocated, queued, starting, running, terminal, cancelled, failed |
| Progress | true completed and total steps; never elapsed-time imitation |
| Environment | observation summary, accepted action, vitals, inventory |
| Frame | URL, step, media type, dimensions when known, content digest |
| Reward | step delta, cumulative reward, named components when available |
| Achievement | newly unlocked and cumulative achievements |
| Policy | policy kind, provider, model, effort, call index, chosen action |
| Usage | calls, input/output/total tokens, latency, metered cost and provenance |
| Evidence | trace status, Trace V5 digest, visibility and completeness |
| Operations | warning, retry, reconnect, heartbeat, malformed event, stream error |

The stream should support replay followed by live tailing. Reconnection needs a stable
cursor, ideally SSE `id` plus `Last-Event-ID`, and consumers must de-duplicate by stable
event identity.

## Reference visual recipe

We will determine the final recipe by iterating on local HTML against real streams. The
initial hypothesis is one responsive dashboard with drill-down rather than a collection
of unrelated charts.

### Above the fold

- Task, source, run identity, and live/terminal status
- Policy/provider/model/effort when applicable
- Rollouts completed out of total
- Aggregate reward, achievements, calls, tokens, and precise cost
- Connection health and time of the most recent real event

### Selected rollout

- Latest gameplay or environment image with follow-live behavior
- Step scrubber that can leave and return to live mode
- Observation summary and selected action
- Current policy call, model latency, tokens, and cost
- Vitals, inventory, and task-specific state
- Exact rollout ID, seed, step, and evidence status

### Plots

- Cumulative reward by environment step
- Reward deltas or named reward components when meaningful
- Achievement unlock timeline and cumulative achievement count
- Calls, tokens, latency, or spend over time when a policy is involved

Plot semantics must follow the evidence. We will not connect unordered arms, invent
confidence intervals, turn one rollout into a frequency claim, or conflate reward,
achievement count, and pass rate.

### Multi-rollout view

- One compact lane card per rollout
- Honest states: allocated, waiting, running, finished, failed, cancelled, incomplete
- True step progress
- Latest action/event, reward, achievements, and spend
- Selection controls that update the detailed pane without losing the overall run
- Failures kept visible instead of disappearing from aggregates

### Activity and evidence

- Concise recent semantic events rather than raw JSON
- Filterable policy, environment, reward, achievement, usage, and failure events
- At terminal time, a link from every lane to the matching sealed trace
- Selecting a plotted point or event opens the matching rollout and trace step

## Prototype program

The prototypes live outside the product runtime and are served locally. They should be
small enough to change quickly, but they must use the same stream contract we intend to
ship. Each iteration produces screenshots, a short observation log, and any proposed
contract change.

### Prototype 1: Harbor / GameBench

Purpose: establish the generic orchestration and concurrent-lane model.

Use a real Harbor/GameBench run exposed through the standard eval results service:
append-only `events.jsonl` as authority and
`/api/results/stream?run_key=...` as SSE. Exercise job allocation, queueing, rollout
progress, retries, terminal verdicts, and a mix of successful and failed lanes.

Questions to settle:

- Is the relationship between job, task, lane, and rollout legible?
- Can the view remain useful when a source has no gameplay frames?
- Are operational failures distinct from benchmark failures?
- Does replay plus live tailing survive reconnection without duplication?

### Prototype 2: Craftax Rust with a ReAct policy

Purpose: establish the richest interactive rollout view.

The first real-stream reference lives in
[`prototypes/live-evals/craftax`](../prototypes/live-evals/craftax). It already accepts
native Craftax rollout SSE and normalized eval streams, orders merged events by observed
time with deterministic sequence tie-breaking, and supports follow-live, historical
scrubbing, and timestamp-paced replay. Its transport-smoke helper creates and steps a
real telemetry rollout but is deliberately not described as policy evidence.

Use the registered Craftax Rust engine and a genuine ReAct policy. The runner must emit
the real model/provider/effort, model calls, actions chosen from observations, tokens,
cost, frames, rewards, achievements, and terminal evidence. A fixed action sequence,
uniform policy, or direct engine stepping is transport evidence only and cannot pass this
prototype.

The prototype should exercise multiple concurrent rollouts, changing gameplay images,
reward and achievement plots, lane selection, policy activity, spend limits, and the
transition from live SSE to sealed Trace V5.

Questions to settle:

- How much policy activity is useful without overwhelming the game state?
- What is the right default frame size and refresh cadence?
- Should reward and achievements share a time axis but remain separate plots?
- How does a point selection open the exact correlated trace step?

### Prototype 3: Banking77

Purpose: prove that the recipe generalizes beyond game environments.

Use a real Banking77 eval stream. Replace gameplay frames with the task-appropriate
evidence: input/classification preview, predicted and reference intent when visible,
per-example correctness, aggregate progress, confusion/error slices, latency, and cost.
Preserve visibility rules and do not reveal withheld labels before the producer makes
them eligible.

GEPA may launch or consume these Banking77 evals, but the optimizer remains a parent
workflow with its own candidate, generation, selection, frontier, reflection, and budget
events. It must link to the exact child eval run and rollout identities rather than
flattening optimizer progress into `evals.event-stream.v1`. The GEPA-specific live design
is in [`live_optimizers_gepa.md`](live_optimizers_gepa.md).

Questions to settle:

- Which parts of the generic shell survive without images or achievements?
- Does the selected-lane detail work for examples as well as game rollouts?
- Which task-specific panels belong in template extensions rather than the base recipe?
- Can a GEPA candidate link to and drill into this eval stream without confusing
  candidate progress with benchmark rollout state?

## Prototype exit criteria

The recipe is ready for engineering review when all three real-stream prototypes can
demonstrate:

- the viewer connected before execution began;
- more than one visible intermediate update per active lane;
- correct allocation of events to concurrent lanes;
- reconnect/replay without duplicated points or counters;
- visible failure and incomplete states;
- precise units, denominators, seeds, and cost provenance;
- a useful narrow-pane layout and a larger inspection layout;
- task-specific content without forking the entire lifecycle model;
- durable terminal evidence and exact trace correlation where Trace V5 is required.

The Craftax prototype additionally requires changing real frames, live policy actions,
incremental reward and achievement plots, and a trace link that resolves to the selected
rollout and step.

## Proposed Workshop product surface

This section is a target for engineering review, not a frozen API.

### Templates

Keep a small template family sharing one stream reducer and lifecycle model:

- `live.eval_stream.v2`: generic tasks and result streams;
- `live.container_rollouts.v2`: container rollouts and task-specific state;
- `live.react_rollouts.v1`: policy activity plus environment state and frames;
- `live.harbor_eval.v2`: Harbor job/task orchestration projected through the same contract.

Task-specific panels should be declared capabilities or optional slots, not separate
copies of connection, replay, lane, usage, and failure logic. Completed views should
reuse `trace.rollout_inspector.v1`, `craftax.rollout_scrub.v1`, and analytical templates
where those interactions are stronger than the live shell.

### Skills

Update the Synth Visuals and Synth Containers skills with one canonical live-eval recipe:

- discover the source and capabilities;
- distinguish an engine acceptance run from a real policy evaluation;
- create and show the visual before starting execution;
- wait for visual readiness;
- start through the owning benchmark harness;
- keep the stream attached through terminal state;
- bind sealed Trace V5 evidence afterward;
- report exact run, rollout, model, seed, limit, cost, and trace identities.

Add a focused `use-synth-live-evals` skill only if the combined workflow remains too
large or ambiguous after the two existing skills are tightened. The skill should teach
orchestration, not contain policy logic or task-specific endpoint guesses.

### MCP and host APIs

The product needs an orchestration operation above today’s independent container and
visual calls. The exact names are open, but the capabilities should include:

- prepare a live run and allocate stable rollout identities;
- return one normalized run-level stream binding rather than requiring the agent to
  assemble per-rollout URLs;
- create/show a compatible visual from source capabilities;
- report viewer connection and readiness;
- start, cancel, and inspect the run through the owning harness;
- query live state and cost without scraping the visual;
- attach terminal Trace V5 identities;
- produce a correlation receipt.

A possible compact MCP shape is:

```text
live_eval_manage(operation, arguments)

operations:
  prepare       -> run identity, lanes, normalized stream binding
  open_visual   -> visual identity and selected recipe
  await_ready   -> connected/replaying/live or explicit failure
  start         -> execution receipt and enforced limits
  get           -> authoritative state, usage, cost, evidence coverage
  cancel        -> bounded cancellation result
  finalize      -> terminal results, trace identities, correlation receipt
```

This could be exposed by a dedicated MCP or composed behind the existing container and
visual MCP servers. The important constraint is that the agent receives a workflow-level
operation and cannot accidentally start a paid eval before the visual is ready.

The existing `visual_manage` operations remain useful for inspection and customization.
The existing `container_run_rollouts` fixed-action operation remains useful for bounded
engine acceptance, but must not be presented as a ReAct or model evaluation path.

### Host responsibilities

Workshop should own:

- loopback and origin policy for live stream and frame access;
- normalized SSE proxying or multiplexing when a source has per-rollout streams;
- replay cursor and reconnect behavior;
- visual readiness acknowledgement;
- lifecycle supervision and cancellation;
- stable visual binding persistence;
- Trace V5 attachment and correlation checks;
- body, header, event, and retained-history limits;
- explicit malformed-event and unavailable-field behavior.

Workshop should not own benchmark policy implementations, reward calculation, graders,
or task-specific truth that belongs in Evals, GameBench, Harbor, or the registered
container.

## Trace V5 live transport refactor review

The Craftax prototype exposed an important distinction: a live eval projection is not
automatically a live Trace V5 capture. The completed Craftax run streamed native eval
events and imported them into Trace V5 only after terminal. It therefore had no raw
capture spool and could not produce genuine `trace.raw` or `trace.visual` partials.

We should keep two correlated planes rather than calling either one the other:

| Plane | Authority | Payload | Consumer use |
| --- | --- | --- | --- |
| Rollout presentation | Harness/container | `evals.event-stream.v1` or normalized `synth.rollout.event.v1` | frames, progress, rewards, achievements, policy and usage UI |
| Trace evidence | Containers capture supervisor | ordered `synth.capture.raw.v1` envelopes | durable evidence, derived projections, final Trace V5 reconciliation |

Containers already provides most of the evidence transport semantics. Its collector
supports authenticated cursor polling at `GET /v1/events?after_ordinal=N&limit=M`, SSE
at `GET /v1/events/stream` with `Last-Event-ID`, and status at
`GET /v1/live-manifest`. Pages use `synth.trace-live-page.v1`; raw records use
`synth.capture.raw.v1`; identity is `(capture_id, ordinal)`; completion is proven only
when the spool is closed and the consumer cursor has reached `high_water_ordinal`.
Evals already has `ContainerTraceSource` to convert those raw pages into `trace.raw`,
throttled `trace.visual`, and terminal `trace.reconciled` events.

The missing work is integration and contract hardening, not a new trace format:

1. **Discovery and correlation.** A rollout response needs a typed stream descriptor
   naming `run_id`, `rollout_id`, `trace_id`, `capture_id`, schemas, supported
   transports, cursor semantics, and authenticated endpoint handles. Today rollout SSE,
   the capture collector, and the eventual bundle are discoverable through different
   mechanisms.
2. **One semantic contract over several transports.** Poll, SSE, and an optional
   WebSocket transport must carry identical raw envelopes. Delivery is at least once;
   consumers de-duplicate by `(capture_id, ordinal)` and reject gaps, regressions, or a
   changed capture ID. Poll uses `after_ordinal`; SSE uses `Last-Event-ID`; WebSocket
   should start with the same cursor and support bounded acknowledgement/backpressure.
   The current reference rollout SSE is snapshot polling rather than durable replay:
   `Last-Event-ID` only seeds a new connection-local counter, while WebSocket restarts
   its counter at zero. Neither can recover missed transitions. The generic
   `/rollouts/{id}/events` response has no cursor/page contract. These surfaces must be
   backed by one retained event log before they can claim resumable delivery.
3. **Explicit stream completion.** SSE and WebSocket need a typed status/control frame
   containing `closed`, `high_water_ordinal`, and manifest generation before a normal
   close. EOF alone is not evidence of a complete capture.
4. **Live artifact retrieval.** Artifact envelopes carry evidence identity, but the
   collector currently has no authenticated read endpoint for their bytes. Add a
   digest-addressed, size-limited artifact route so Workshop can cache gameplay frames
   before ephemeral rollout state is removed. Large media stays out of event frames.
5. **Trusted host brokering.** Collector capabilities and bearer tokens must terminate
   in Workshop's Rust host, never in a visual WebView. The host validates raw envelope
   digests and ordering, applies origin/loopback rules, stores the stream, and emits a
   redacted presentation projection over the existing `runtime:event` boundary.
   Evals' current `ContainerTraceSource` reads a local `capture_root` path, so its page
   reducer and projection logic are reusable but its source must be split behind a
   transport-neutral page-client interface with filesystem, authenticated HTTP poll,
   and optionally SSE implementations.
6. **Durable partial state.** Workshop needs an append-only live-capture store keyed by
   capture ID and ordinal. It must resume after restart, distinguish open/incomplete
   from corrupt, and avoid inserting an open capture into the trusted sealed-trace
   catalog.
7. **Seal and reconcile.** When the collector closes, the owner seals the bundle.
   Workshop verifies exactly one matching capture, imports through the existing trusted
   Trace V5 path, records the final digest, and atomically links the provisional live
   record to the sealed trace. A derived `trace.visual` is replaceable; raw envelopes
   and the final digest are the evidence authority.
   A post-hoc native import with no live reconciliation receipt must be labeled
   `trace.sealed` or "sealed import", not `trace.reconciled`. The current Craftax
   bridge needs that naming correction.
8. **Contract publication.** The base Containers OpenAPI currently documents generic
   pull `/rollouts/{id}/events` and `/trace`, while the implementation also has rollout
   SSE/WebSocket and the capture collector has its own live API. The refactor should
   publish typed capability and stream-descriptor schemas and update discovery docs so
   consumers do not infer routes.

A proposed descriptor shape for review is:

```json
{
  "schema_version": "synth.live-stream-descriptor.v1",
  "run_id": "run_...",
  "rollout_id": "rollout_...",
  "channels": [
    {
      "name": "trace.raw",
      "payload_schema": "synth.capture.raw.v1",
      "capture_id": "cap_...",
      "cursor": {"kind": "ordinal", "initial": -1},
      "transports": {
        "poll": {"url": "/v1/events"},
        "sse": {"url": "/v1/events/stream"},
        "websocket": null
      },
      "status_url": "/v1/live-manifest",
      "auth": {"mode": "host_capability"}
    }
  ]
}
```

The URLs above are illustrative. A remote or containerized deployment may need a host
relay or opaque handle rather than exposing the collector address. Workshop should
prefer SSE for one-way live tailing, polling for recovery and deterministic tests, and
WebSocket only where duplex acknowledgement or control materially helps.

The minimum end-to-end proof is one real capture where Workshop connects before the
first paid model call, displays raw-derived partials while the spool is open, survives a
disconnect by replaying from an ordinal, retains frame artifacts after rollout cleanup,
observes the explicit high-water close, and replaces the provisional capture with one
verified sealed Trace V5 digest without changing rollout identity.

### Proposed Containers standard: Trace Streaming Profile

Containers should consolidate the schemas, normative lifecycle, transports, reference
reducer, fixtures, and conformance runner under one **Trace Streaming Profile**. This is
not a Trace V6 and does not replace the sealed `synth.trace.v5` document:

| Layer | Proposed authority |
| --- | --- |
| Durable capture facts | Existing `synth.capture.raw.v1` |
| Cursor page/status | Existing `synth.trace-live-page.v1` and `synth.trace-live-status.v1` |
| Semantic live lifecycle | New `synth.trace-stream-event.v1` deterministic projection |
| Final trace | Existing `synth.trace.v5` |
| Live-to-final proof | Existing `synth.trace-live-reconciliation.v1` |

The useful lesson from OpenResponses is its use of semantic events and explicit object
state machines: an item is added before deltas, terminal objects cannot be updated, and
the same event objects are used over SSE and WebSocket. Trace streaming should adopt
that discipline while deliberately retaining resumable event IDs and replacing a bare
`[DONE]` sentinel with a verifiable high-water close.

The semantic projection should be append-only. Data events are deterministic from raw
envelopes; close/seal control events are emitted by the capture/finalization authority
and carry their proof. Neither may mutate or replace the raw evidence. A compact
vocabulary is enough:

- `trace.opened`, optional `trace.sealing`, then exactly one of `trace.completed`,
  `trace.failed`, or `trace.interrupted`; a completed trace carries its final digest;
- `session.opened` and `session.closed`;
- `span.opened`, zero or more typed `span.data` events, then `span.closed`;
- immutable `event.recorded` occurrence facts;
- `artifact.declared`, followed by `artifact.available`, `artifact.truncated`, or
  `artifact.missing`;
- `capture.high_water` for resumable progress and `capture.closed` for the immutable
  final ordinal.

Every stream event needs a monotonic `sequence_number`, `trace_id`, `capture_id`, stable
subject identity, occurrence time, typed payload, raw-envelope references, and its own
content digest. `span.data` is a discriminated union, not an arbitrary JSON Patch. The
initial standard should include data types for model response frames, usage, tool I/O,
environment observation/action/transition, reward, achievement, artifact reference,
error, and safe reasoning summary. Raw reasoning remains private/opaque unless capture
policy explicitly permits it.

The lifecycle reducer enforces these invariants:

1. `trace.opened` is first and trace terminal is unique; `trace.completed` is legal only
   after `capture.closed` and successful seal verification.
2. Parent sessions/spans open before children; children close before parents.
3. Subject IDs are never reused for another kind or parent.
4. No data or child may be added to a closed subject.
5. An error does not imply closure; a matching terminal transition is still required.
6. Every artifact terminal state proves availability, truncation, or absence explicitly.
7. Stream terminal is not capture completeness. Completeness requires `closed=true` and
   consumer cursor equal to `high_water_ordinal`.
8. “Sealed” requires a verified final digest. “Reconciled” additionally requires the
   live reconciliation receipt covering every raw ordinal.

### Trace streaming acceptance suite

Following the OpenResponses model, Containers should ship a runnable conformance CLI
and machine-readable receipt. Schema validation is only the first layer.

#### Gate A: schema and lifecycle

| ID | Acceptance test |
| --- | --- |
| TS-A01 | Discovery returns one versioned stream descriptor with stable trace/capture/run/rollout identities and declared transports. |
| TS-A02 | The first semantic event is `trace.opened`; exactly one legal trace terminal occurs. |
| TS-A03 | Nested session/span fixture obeys parent-before-child and child-before-parent closure. |
| TS-A04 | Model call, tool execution, environment step, reward, achievement, usage, error, and artifact fixtures validate their discriminated payload schemas. |
| TS-A05 | Unknown namespaced data kinds survive relay and storage without being mistaken for core kinds. |
| TS-A06 | Duplicate subject open, orphan child, post-close data, double close, and terminal regression fail conformance. |
| TS-A07 | Missing values remain unavailable; they are never normalized to zero, empty success, or completed. |
| TS-A08 | Raw secret fixtures are rejected/redacted before publication, including headers, nested credentials, URLs, and artifact metadata. |

#### Gate B: evidence and ordering

| ID | Acceptance test |
| --- | --- |
| TS-B01 | Every `synth.capture.raw.v1` envelope verifies ID, capture ID, ordinal, and content digest. |
| TS-B02 | Ordinals are contiguous from the advertised start through high water; gaps and regressions fail closed. |
| TS-B03 | Repeated delivery is accepted only when the duplicate ordinal has identical envelope ID and digest. |
| TS-B04 | Semantic events deterministically reproduce from the same raw prefix byte-for-byte. |
| TS-B05 | Prefix projections are monotonic: extending a raw prefix cannot rewrite already emitted semantic events. |
| TS-B06 | Active partial-file reads expose only complete newline-terminated envelopes and survive segment rotation. |

#### Gate C: transport equivalence and recovery

| ID | Acceptance test |
| --- | --- |
| TS-C01 | Poll, SSE, and any advertised WebSocket transport yield the same ordered raw envelope IDs/digests. |
| TS-C02 | Poll resumes with `after_ordinal`; SSE resumes with `Last-Event-ID`; WebSocket resumes from the same cursor model. |
| TS-C03 | Forced disconnect after every possible ordinal resumes without loss; duplicates are bounded and de-duplicable. |
| TS-C04 | Heartbeats never advance evidence cursors or create semantic events. |
| TS-C05 | A slow consumer either receives bounded backpressure or a typed recoverable disconnect; the producer remains healthy. |
| TS-C06 | EOF, socket close, and HTTP timeout never masquerade as trace completion. |
| TS-C07 | Wrong capture ID, stale capability, invalid cursor, oversized page, and unauthorized artifact reads are rejected. |
| TS-C08 | Transport-advertisement claims are truthful: absent transports are null/omitted and advertised routes pass their suite. |

#### Gate D: artifacts, close, seal, and reconciliation

| ID | Acceptance test |
| --- | --- |
| TS-D01 | A declared artifact is fetched by authenticated digest, matches media type/size/digest, and remains available after rollout cleanup. |
| TS-D02 | `closed=true` is observed with a stable high water and no later raw ordinal. |
| TS-D03 | The sealed trace capture ID and high-water ordinal exactly match the live capture. |
| TS-D04 | Reconciliation contains exactly one retained/merged/redacted/dropped disposition for every raw ordinal. |
| TS-D05 | Every reconciliation target resolves in the sealed trace and verifies its entity digest. |
| TS-D06 | Disconnect during sealing resumes to the same final trace digest. |
| TS-D07 | Failed finalization remains partial/failed and is never published as a trusted sealed trace. |
| TS-D08 | Post-hoc native import passes sealing tests but explicitly fails the stronger live-reconciliation claim. |

#### Gate E: Workshop consumer

| ID | Acceptance test |
| --- | --- |
| TS-E01 | Workshop attaches and acknowledges readiness before the first paid model call. |
| TS-E02 | Raw partials persist across Desktop restart and replay without duplicated UI facts. |
| TS-E03 | A visual shows open/running/closed spans and real typed data at the selected temporal cutoff. |
| TS-E04 | Cross-run, cross-rollout, cross-capture, and cross-frame correlation mismatches fail visibly. |
| TS-E05 | Collector capabilities never enter renderer state, logs, saved visuals, or exported receipts. |
| TS-E06 | At seal, one transaction links the provisional capture to the trusted imported trace without changing visual identity. |
| TS-E07 | Scrubbing before seal shows unsealed evidence; live edge after reconciliation shows the verified digest. |
| TS-E08 | Craftax, Harbor/GameBench, and Banking77 fixtures exercise the same core reducer with task-specific data kinds only. |

The conformance runner should emit a signed or digest-addressed
`synth.trace-stream-conformance.v1` receipt containing implementation version,
advertised capabilities, test IDs, pass/fail/skip, fixture digests, transport transcript
digests, and final reconciliation digest. It should support an in-process reference
server for CI and a black-box `--base-url` mode for any container. A smaller browser
suite may test public schema and SSE behavior, but credential, filesystem corruption,
backpressure, restart, and reconciliation tests remain CLI-only.

Recommended ownership in the Containers repository:

```text
docs/specs/trace-streaming-profile-v1.md     normative BCP 14 contract
schemas/trace-stream/*.schema.json           public discriminated schemas
src/synth_containers/tracing/streaming/       reducer, clients, transports
tests/conformance/trace_stream/               fixtures and black-box suite
```

Trace V5 model schemas should be re-exported from that public schema catalog so there is
one standards surface, while Python dataclasses remain an implementation. Workshop and
Evals consume the published schemas/test kit; neither should fork the trace vocabulary.

## Engineering implementation sequence

After the prototypes and recipe are reviewed:

1. Freeze the normalized rollout vocabulary and compatibility rules for
   `evals.event-stream.v1`.
2. Add producer adapters for Harbor/GameBench, Craftax Rust ReAct, and Banking77 without
   inventing a second evidence authority.
3. Implement the host’s replayable run-level SSE binding, readiness handshake, limits,
   and correlation enforcement.
4. Extract a shared live stream reducer and lifecycle shell for the registered templates.
5. Implement the approved template panels and task capability extensions.
6. Add the workflow-level MCP operations and update the relevant skills.
7. Implement the live-to-Trace-V5 transition and exact point-to-trace navigation.
8. Validate with a bounded smoke run for each example, then multi-rollout and failure
   cases.
9. Run independent CUA and capture a release receipt.

## Verification matrix

| Area | Required proof |
| --- | --- |
| Contract | Schema validation, unknown-event tolerance, malformed required fields fail visibly |
| Ordering | Visual ready before the first environment step or paid model call |
| Identity | No cross-run, cross-rollout, cross-seed, cross-frame, or cross-trace binding |
| Replay | Initial history followed by live tail with no duplication |
| Recovery | Disconnect/reconnect, source restart, visual reopen, and cancellation |
| Concurrency | Multiple active lanes with independent progress and terminal state |
| Truthfulness | Missing is distinct from zero; failed is distinct from scored |
| Policy | Real model calls and chosen actions; no fixed-action substitution |
| Media | Frame URL policy, step correlation, digest, broken-image state |
| Metrics | Reward, achievements, usage, latency, tokens, and precise metered cost |
| Evidence | Terminal result and Trace V5 completeness/correlation where required |
| Layout | Narrow Desktop pane, expanded view, keyboard access, reduced motion |

## CUA acceptance scenario

An independent tester should be able to ask Workshop to run a supported live eval and
observe this sequence without manual endpoint construction:

1. Workshop identifies the registered source and policy harness.
2. A visual opens in waiting state.
3. The visual changes to connected before execution starts.
4. Multiple rollout lanes appear and advance from real events.
5. Task-appropriate detail changes live: frames and actions for Craftax, job state for
   Harbor, example outcomes for Banking77.
6. Reward, achievements, usage, and cost update when the producer emits them.
7. Failures remain visible and aggregates stay honest.
8. Terminal lanes acquire durable result and trace identities.
9. Reopening the visual shows a stable replay without requiring the original service.

For a paid ReAct run, the receipt must also name the provider, model, effort, call and
step limits, authorized spend ceiling, actual metered cost, rollout IDs, seeds, and trace
digests.

## Decisions to make during our prototype iterations

- One adaptable live template versus a small shared family with task extensions
- Run-level multiplexed SSE versus multiple visual stream bindings
- Minimum event cadence and frame cadence
- Default selected lane and automatic lane switching behavior
- Follow-live and historical scrub interaction
- Reward/achievement plot layout at narrow widths
- Safe and useful policy detail without private reasoning
- Whether readiness is an MCP response, host event, or persisted visual state
- How long live history remains in memory before durable storage takes over
- The exact boundary between terminal stream replay and Trace V5 projection

These are product decisions to settle with real-stream prototypes. They should not be
left for an implementation engineer to infer from today’s templates.

## Deliverables for engineering review

- This design note updated with decisions rather than open alternatives
- Three local reference prototypes connected to real streams
- Captured real event streams for deterministic replay and regression
- Screenshots or short recordings at narrow and expanded widths
- Final normalized event examples and capability matrix
- Approved component hierarchy and interaction recipe
- Proposed MCP request/response schemas
- Skill wording and an end-to-end agent example
- Acceptance matrix and CUA receipt template
- Known compatibility and migration requirements for existing live templates

Engineering begins after those deliverables are reviewed. The prototypes establish what
the product should do; the engineer then reviews the contracts, challenges unsafe or
expensive assumptions, and implements the supported Workshop path with production
lifecycle, security, persistence, and test coverage.

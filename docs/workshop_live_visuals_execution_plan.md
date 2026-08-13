# Workshop live visuals execution plan

**Status:** Workshop implementation brief derived from
`execution_platform_master_plan.md`

## Decision

Workshop should be the durable projection and interaction layer, not another eval,
container, or optimizer runtime. It consumes provider discovery, ordered events,
resources, relationships, and receipts from Containers and Optimizers; it owns local
mirroring, replay, readiness, visual instances, safe template execution, and the user
experience.

The team working in Containers should not build Workshop reducers or UI. The team
working in Optimizers should not build a second Workshop transport. Workshop should not
guess provider URLs, recompute rewards, flatten optimizer child evaluations, or embed
benchmark-specific execution logic.

## Contract with the other teams

### Containers must deliver

- provider discovery with declared operations, stream descriptors, transports, auth,
  limits, schemas, and capabilities;
- stable evaluation-run, attempt, rollout, episode, service-generation, span, and
  artifact identities;
- a cursor-addressed durable event stream with replay plus live tail;
- explicit stream-ready/control records, gaps, terminal manifest, and cancellation;
- typed resource references for frames, traces, logs, snapshots, and native results;
- declared reward/evaluator authority and no missing-to-zero coercion;
- Harbor and later compatibility-profile metadata without Workshop-specific payloads.

### Optimizers must deliver

- the same provider/event/resource primitives, with optimizer-run and candidate IDs;
- candidate lineage, proposal/training/checkpoint/selection events;
- child evaluation relationships pointing to exact Containers evaluation runs;
- separate proposer, evaluator, held-out, and training usage;
- cursor-addressed state slices for expensive projections;
- algorithm-profile schemas for GEPA, GELO, SFT, and later algorithms;
- terminal outputs and selection provenance without copying child rollout truth.

### Workshop will deliver

- registration and capability negotiation for both provider families;
- persist-before-publish event ingestion and a durable local mirror;
- incremental reads after the last accepted cursor, replay/live overlap de-duplication,
  gap handling, and terminal reconciliation;
- connection states: loading, replaying, ready, live, stale, reconnecting, terminal,
  and failed;
- a readiness acknowledgement used by connect-before-execution workflows;
- typed relationships and navigation among optimizer, eval, attempt, rollout, trace,
  artifact, and visual records;
- versioned shared reducers/state slices and optional provider-profile reducers;
- bundled visual templates plus explicitly trusted local workspace extensions;
- artifact/frame resolution through the host under installed-app origin and credential
  rules;
- exact through-time replay, follow-live, comparison, accessibility, and export.

## What already exists

- durable visual records and revisions;
- bundled template registry and dynamic shell loading;
- visual MCP list/create/bind/show operations;
- `live_sse`, fixture, Trace V5, CAS, run-reference, and optimizer-run bindings;
- shared live ingestion, de-duplication, reducer, chrome, and timeline primitives;
- live Craftax, Harbor, generic container, dig-bench, and optimizer templates;
- a CAS-backed live spool for persisted raw envelopes;
- a local Craftax HTML reference viewer;
- optimizer bridge calls and renderer event notifications.

These are useful scaffolding, not completion proof. The current templates mostly consume
fixtures or direct SSE, while Desktop still lacks one host-owned durable stream session
that all templates can read by cursor.

## Immediate correctness fixes

1. Remove reward inference from generic status. `completed` must not imply reward `1`,
   and `game_over` must not imply reward `0`; only an authoritative reward/evaluator
   event may set reward.
2. Change optimizer visual refresh from `eventsAfter(run, 0)` on every notification to
   incremental reads after the last accepted cursor, with a bounded snapshot recovery
   path.
3. Publish optimizer update notifications only after the corresponding run, event, and
   cursor transaction commits.
4. Replace boolean `live`/`ready` with the explicit connection-state machine.
5. Validate the complete event envelope, producer generation, sequence, run binding,
   payload schema, and resource references before reducer admission.
6. Stop templates from opening arbitrary direct loopback URLs. The host should resolve a
   declared provider stream into a capability-scoped local stream session.
7. Make frame loading use host-resolved resource references and surface missing,
   forbidden, corrupt, or unsupported resources distinctly.
8. Make malformed partial trace lifecycles fail closed and reconcile the reduced trace
   to the sealed Trace V5 digest.

## Build order

### W1 — provider-neutral live mirror

Add a host service with operations conceptually equivalent to:

```text
stream_attach(provider, stream_descriptor, binding)
  -> stream_session_id, current_cursor, connection_state

stream_events_after(stream_session_id, cursor, limit)
  -> validated events, next_cursor, gap?, terminal?

stream_state_at(stream_session_id, cursor, reducer_profile)
  -> versioned state slice

stream_detach(stream_session_id)
```

The service resolves auth through the credential broker, consumes the provider's
declared poll/SSE/WebSocket transport, persists raw validated envelopes and cursor in one
transaction, then emits a lightweight renderer notification. Templates receive a stream
session/run binding, never credentials or a guessed source URL.

**Acceptance:** attach before execution, receive the first event without reload,
disconnect/reconnect, replay/live overlap without duplicates, detect a sequence gap,
restart Workshop, and reproduce the same terminal projection.

### W2 — shared execution projection

Implement one reducer package for provider-neutral nouns:

- evaluation run, attempts, rollouts, queue and concurrency;
- service generations and restart boundaries;
- partial trace span open/delta/close;
- reward, grade, metric, usage, cost, achievement, and artifact evidence;
- terminal manifest and evidence coverage;
- relationships to optimizer candidates and parent runs.

Reducers are pure, versioned, deterministic, cursor-addressable, and preserve unknown.
Provider/profile reducers may add typed state but cannot overwrite shared authoritative
fields.

**Acceptance:** fixture, captured replay, and live-tail reduction converge at every
cursor; future events never leak into historical scrub; malformed input cannot crash a
visual or manufacture data.

### W3 — generic live visual family

Build shared components once:

- run header and provenance drawer;
- task × policy × seed matrix and queue;
- rollout selector and service/restart timeline;
- trace tree with policy, tool, environment, evaluator, and artifact spans;
- frame/artifact viewer;
- reward/metric/achievement/usage plots;
- evidence and terminal-integrity panel;
- through-time scrubber with explicit return-to-live.

Compose these into a small supported family:

- `live.eval_stream.v2` — generic evaluation and queue;
- `live.container_rollouts.v2` — task/rollout/environment evidence;
- `live.react_rollouts.v1` — policy trace beside environment state and frames;
- `live.harbor_eval.v2` — Harbor-native artifacts and verifier evidence;
- `optimizer.run.v2` — provider-neutral optimizer plus child evals.

Craftax, OpenEnv, and Prime panels should be optional typed-profile slots inside this
family, not copies of connection, replay, trace, and plotting code.

### W4 — connect-before-execution orchestration

Add workflow-level host/MCP operations:

```text
prepare -> open_visual -> await_ready -> start -> watch/get -> cancel -> finalize
```

`await_ready` means the renderer has loaded the visual, replayed through cursor N, and
subscribed for N+1. Starting paid work before that acknowledgement is rejected. Start
still requires the appropriate compute/spend authorization; opening, replaying, and
inspecting do not.

**Acceptance:** a bounded real Craftax policy rollout shows policy and environment
partials before terminal, and the final receipt correlates every run, stream, visual,
trace, artifact, model, seed, limit, usage, and cost identity.

### W5 — optimizer views

Once Optimizers publishes the shared envelope and relationships:

- make the generic optimizer view incremental and durable;
- add GEPA candidate lanes/frontier/selection using its typed profile;
- add SFT training/checkpoint/sample/validation panels;
- navigate from any candidate score to the exact child Containers rollout and trace;
- distinguish selection data from measurement-only held-out data;
- preserve separate usage authorities.

Do not wait for every optimizer algorithm before finishing W1–W4. GEPA is the first
profile; later profiles reuse the host and generic components.

### W6 — local workspace extensions

Implement explicit registration, trust grants, multi-root catalog, safe build cache,
last-known-good rollback, revision pinning, and missing-renderer states. Keep private
eval templates and recipes in their owning private checkout. This work should follow the
stable shared reducer/template APIs so private packages do not freeze prototype shapes.

## Recommended first three pull requests

1. **Correctness and incremental optimizer ingestion** — remove synthesized reward,
   add cursor tracking, committed notifications, connection states, and focused tests.
2. **Provider-neutral stream session and reducer** — host-owned attach/replay/live,
   validation, persistence, readiness, resources, and deterministic state-at-cursor.
3. **Craftax real-stream vertical slice** — rebuild the reference visual from shared
   components, attach before one real rollout, show real policy partials/frames/rewards/
   achievements, then prove terminal replay.

After those, Harbor and GEPA become two consumers of the same substrate rather than two
new streaming implementations.

## Coordination rule

Freeze only the narrow cross-repository contract early: identities, envelope, cursor,
resource reference, relationship, authority, and terminal receipt. Let Containers own
producer adapters and Optimizers own algorithm profiles. Let Workshop iterate rapidly
on projections and visual composition against captured real streams, while running a
small live compatibility test against each provider in CI.

If an upstream field is not yet stable, carry its native payload/reference and display
it in an evidence inspector; do not prematurely add it to the shared schema or silently
drop it.

## Workshop definition of done

- one code path handles replay plus live for Containers and Optimizers;
- the visual is ready before paid execution begins;
- real partial trace, environment, frame, reward, and usage evidence appears live;
- missing data stays unknown and fake fields cannot appear;
- cursors, reconnect, producer restart, and terminal reconciliation are visible;
- historical scrub is deterministic and terminal replay works with producers offline;
- optimizer candidates link to exact child eval evidence;
- Harbor, Craftax, OpenEnv, and Prime views compose shared primitives;
- private recipes/templates require no public Workshop code change; and
- accessibility, resource security, retention, and installed-app behavior pass CUA.


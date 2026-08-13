# Shared execution-provider discovery and streaming contracts

**Master plan:** `execution_platform_master_plan.md`

**Status:** Tentative design for review; not an implementation contract

**Scope:** Containers and Optimizers discovery, resources, streams, artifacts, usage,
and their Workshop projections

**Related:** `execution_ontology.md`, `live_evals.md`, `live_optimizers_gepa.md`

## Decision in one sentence

Define one small, transport-neutral provider and stream substrate, then compose it into
discriminated Container and Optimizer profiles with algorithm- and environment-specific
payload schemas.

The neutral substrate does not make `synth-containers` neutral. Containers remains an
opinionated task/runtime façade that folds supported native formats into a consistent,
runnable product surface. Harbor is the only committed first-class external fold in
this proposal. Other framework compatibility layers are aspirational until promoted
through an owned adapter and conformance suite. GameBench is benchmark dataset/suite
content consumed by Evals through Containers, not a format to fold. When published
through the supported compatibility surface, it is packaged as Harbor tasks.

Do not make an optimizer inherit container lifecycle semantics, do not flatten child
evaluation traces into optimizer events, and do not use an untyped `metadata` object as
the primary extension mechanism.

## Repository review

### Containers

Containers already has a useful discovery nucleus:

- `RuntimeMetadata` identifies a runtime and contains a `RuntimeCapabilitySurface`;
- the capability surface declares runtime kind, profiles, rollout modes, statefulness,
  fidelity, checkpoints, pause/resume, state, traces, reward, verification, artifacts,
  annotations, tool runtime, token emission, multi-actor, inference, and branching;
- `RouteHints` identifies task, rollout, state, event, trace, artifact, and annotation
  routes;
- `TaskInfo`, `TaskCatalog`, execution contracts, and `ResourceRef` provide domain data;
- trace capture exposes cursor-oriented poll/SSE machinery, while rollout observation is
  closer to snapshot polling and is not yet the same durable replay contract.

The gap is not a lack of fields. Capabilities and route hints do not fully describe the
operation, request/response schemas, cursor rules, transport equivalence, retention, or
authorization. A client still has to know implementation conventions. Every declared
capability must also round-trip through discovery; hand-maintained serializers make it
easy for a field such as annotation support to be declared but omitted on the wire.

The implementation target is not a generic adapter marketplace. It is one high-quality
first-class Harbor fold: resolve Harbor tasks/datasets, package their environments,
preserve agent/verifier separation and native evidence, expose the opinionated Synth
runtime/rollout surface, and prove the mapping end to end. Any Archipelago, OpenEnv, or
other compatibility work remains experimental until explicitly promoted.

### Public Optimizers

The Rust optimizer platform has the strongest candidate for shared execution concepts:

- `optimizer_run.v1`, `optimizer_event.v1`, and `optimizer_state_slice.v1`;
- monotonic `sequence_number` and cursor-addressed state slices;
- provider-neutral execution bindings and input/output/visual resource references;
- shared slices for summary, timeline, usage, logs, artifacts, and execution;
- algorithm slices for GEPA, Go-Ex/GELO, and SFT;
- operations, leases, jobs, checkpoints, rollouts, evidence, usage, and state-machine
  records in the platform store.

The hosted client supports SSE plus bounded NDJSON backfill and state-slice reads. It
treats OHCO as catalog-visible but not currently submittable. GELO supports SFT today and
declares RLVR/OPSD as future plugin lanes.

The remaining schema risks are:

- missing sequence values are normalized to zero in some Python and Workshop adapters;
- absent usage values default to zero, conflating unavailable with observed zero;
- several generations of event records are dual-read or dual-written;
- algorithm capability booleans do not name the schemas or operation semantics they
  support.

### Optimizers Beta

`optimizers-beta` is the private execution service behind the Synth backend. It provides
generic lifecycle SSE, durable Go-Ex event/state routes, algorithm artifact routes,
standalone GEPA and SFT paths, and MAPO execution. Its useful domain additions are:

- hosted/backend ownership and billing boundaries;
- GELO search board, themes, candidates, checkpoints, proposer/plugin work, and optional
  local resource leases;
- SFT checkpoints, training curves, dataset/compute state, and evaluations;
- MAPO generations, candidate registries, rollout evidence, branch checkpoints, review
  rows, and held-out comparisons;
- future GELO plugin lanes such as RLVR.

The service also shows why the common contract must sit above an individual process
route layout. Generic lifecycle history, durable algorithm spools, projected state, and
artifact files currently have different persistence and replay mechanics. MAPO is
primarily artifact-oriented and uses `synth_mapo.v1` plus `ohco.review_row.v1`, rather
than cleanly participating in the canonical optimizer event vocabulary. Those are
adapters to converge, not shapes to copy into Workshop.

### Workshop

Workshop already normalizes local OSS GEPA and hosted Go-Ex payloads into
`optimizer_event.v1`, stores runs/events/slices, and has shared optimizer visual
templates. This is the right product projection boundary. It should consume canonical
provider contracts and retain adapters for older producers; it should not become the
upstream schema authority.

The current normalization also proves the need for strict ingestion: an event without a
valid sequence can become sequence zero. Under the proposed contract, a missing cursor
is an invalid durable event, never a default.

## Shared type system

Use JSON Schema composition and discriminators, or language-native composition, rather
than object-oriented inheritance. The wire shape is one base plus one typed `profile`.

```text
synth.execution-provider-info.v1
  identity + contracts + resources + operations + streams + schemas + limits
                              |
                 discriminator: provider.kind
                    /                         \
synth.container-provider-info.v1     synth.optimizer-provider-info.v1
  runtime/tasks/rollouts/traces         algorithms/runs/slices/jobs/evals
```

The common types should live in a small public schema package. Containers and public
Optimizers depend on or generate bindings from it. `optimizers-beta` implements the
Optimizer profile through the backend boundary. Workshop consumes the package and must
not maintain a fork.

## Master provider-info schema

Tentative wire example:

```json
{
  "schema_version": "synth.execution-provider-info.v1",
  "provider": {
    "id": "local.craftax-rust",
    "kind": "container",
    "name": "Craftax Rust",
    "description": "Interactive Craftax ReAct runtime",
    "version": "0.4.0",
    "deployment": "local"
  },
  "contracts": [
    {"id": "synth.container-provider", "version": "1.0.0"},
    {"id": "synth.stream", "version": "1.0.0"}
  ],
  "resource_kinds": ["task", "rollout", "trace_capture", "artifact"],
  "operations": {
    "rollouts.create": {
      "method": "POST",
      "path": "/rollouts",
      "request_schema": "synth.rollout-create.v1",
      "response_schema": "synth.rollout.v1",
      "idempotency": "keyed"
    }
  },
  "streams": {
    "rollout.events": {
      "schema": "synth.rollout-event.v1",
      "cursor": "sequence",
      "ordering_scope": "rollout",
      "replay": {"supported": true, "retention": "run"},
      "transports": [
        {"kind": "poll", "operation": "rollout.events.list"},
        {"kind": "sse", "operation": "rollout.events.stream"}
      ]
    }
  },
  "schemas": {
    "catalog": "/.well-known/synth/schemas",
    "digests": {"synth.rollout-event.v1": "sha256:..."}
  },
  "visual_templates": [
    {
      "id": "eval.craftax.live.v1",
      "version": "1.0.0",
      "binding_schema": "evals.live-binding.v1",
      "resource_kinds": ["eval_run", "rollout"],
      "required_streams": ["rollout.events"],
      "package_digest": "sha256:..."
    }
  ],
  "accounting": {
    "usage_schema": "synth.usage.v1",
    "currency": "USD",
    "budget_enforcement": true
  },
  "limits": {"max_page_items": 500, "max_event_bytes": 1048576},
  "authorization": {"modes": ["host_capability"], "renderer_access": false},
  "profile": {"kind": "container", "runtime_kind": "interactive_environment"}
}
```

### Base fields

| Field | Rule |
| --- | --- |
| `schema_version` | Exact master discovery schema. |
| `provider` | Stable identity, discriminating kind, implementation version, and local/hosted/hybrid deployment. |
| `contracts` | Supported protocol IDs and semantic versions; negotiation is explicit. |
| `resource_kinds` | Resource nouns the provider can author, read, or reference. |
| `operations` | Typed descriptors. Paths are data, not conventions guessed from booleans. |
| `streams` | Named stream contracts with schema, ordering scope, replay, retention, and equivalent transports. |
| `schemas` | Resolvable catalog plus immutable schema digests. |
| `visual_templates` | Compatible signed template identities and binding requirements, never executable UI supplied ad hoc by a provider. |
| `accounting` | Usage schema, currency when cost is supported, and whether the provider can enforce a budget. |
| `limits` | Safety and pagination bounds; omitted means unknown, never unlimited. |
| `authorization` | Supported authorization modes and trust boundary, never credentials. |
| `profile` | Discriminated Container or Optimizer contract. |

Health and current capacity are separate resources. Provider discovery is cacheable
contract data; queue depth, leases, accelerators, and availability are time-varying.

Visual-template declarations are compatibility claims. Local sidecars and hosted
providers do not send executable UI at run time. Workshop resolves the declared ID,
version, and digest through its signed template catalog and retains the exact revision
needed for historical replay.

### Typed operations

An operation descriptor names method, path, request schema, response or stream schema,
idempotency, authorization mode, and limits. This replaces the ambiguous combination of
`stream_events: true` and an optional `events_url`.

```json
{
  "method": "GET",
  "path": "/v1/runs/{run_id}/events",
  "request_schema": "synth.optimizer-events-query.v1",
  "response_schema": "synth.optimizer-event-page.v1",
  "stream_schema": "synth.optimizer-event.v2",
  "idempotency": "safe",
  "authorization": "host_capability",
  "limits_ref": "optimizer_events"
}
```

## Shared resource reference

Both domains should converge on:

```json
{
  "kind": "eval_run",
  "id": "eval_123",
  "role": "candidate_evaluation",
  "schema": "evals.run.v1",
  "digest": "sha256:...",
  "media_type": "application/json",
  "provider_ref": "containers.harbor",
  "attributes": {}
}
```

Only `kind` and `id` are universally required. Digest and media type are required when
the reference claims immutable artifact integrity. `attributes` is for non-semantic,
namespaced additions; essential fields require a versioned schema revision.

This replaces parallel `ResourceRef` and `OptimizerResourceRef` shapes over time. During
migration, both deserialize into the common type and preserve their original payload.

## Shared stream envelope

The common layer standardizes delivery facts, not domain meaning:

```json
{
  "schema_version": "synth.stream-event.v1",
  "event_id": "evt_01J...",
  "stream": {
    "kind": "optimizer.events",
    "id": "opt_gepa_123",
    "sequence": 42
  },
  "occurred_at": "2026-08-12T15:03:07.125Z",
  "observed_at": "2026-08-12T15:03:07.148Z",
  "producer": {"kind": "optimizer", "id": "local.gepa"},
  "subject": {"kind": "optimizer_run", "id": "opt_gepa_123"},
  "type": "optimizer.candidate.evaluation.completed",
  "phase": "closed",
  "data_schema": "synth.optimizer.candidate-evaluation.v1",
  "data": {},
  "resource_refs": [],
  "usage_delta": null,
  "error": null
}
```

Required invariants:

1. `(stream.kind, stream.id, sequence)` is the durable cursor identity.
2. Sequence is present, at or after the stream's declared integer origin, and strictly
   monotonic for new events. A valid zero-origin stream may begin at zero; a missing or
   invalid sequence is never synthesized as zero.
3. Re-delivery at one sequence is valid only when `event_id` and canonical digest match.
4. `occurred_at` is producer time; `observed_at` is relay time. Neither orders events in
   place of sequence.
5. `type` is namespaced and open; `data_schema` identifies the payload.
6. `phase` is optional shared lifecycle (`opened`, `updated`, `closed`), not a replacement
   for domain status.
7. Missing usage, reward, counts, or duration remain `null`/absent, not zero.
8. Heartbeats are transport control records and do not consume domain sequence numbers.
9. Poll, SSE, and WebSocket are equivalent views of the same committed event log.
10. A producer publishes only after the event is durable enough to satisfy its
    advertised replay/retention claim.

The base envelope is intentionally smaller than Trace V5. Raw trace captures retain
their evidence-specific ordinal, digest, and reconciliation protocol. An optimizer
update can reference a trace without pretending to be a trace span.

## Container implementation profile

`synth.container-provider-info.v1` composes the master info with:

```json
{
  "kind": "container",
  "runtime_kind": "interactive_environment",
  "profiles": ["synth.runtime.http.v1", "synth.trace-streaming.v1"],
  "task_catalog": {"operation": "tasks.list", "schema": "synth.task-catalog.v1"},
  "rollout": {
    "modes": ["batch", "interactive"],
    "multi_actor": false,
    "proxied_inference": true,
    "event_stream": "rollout.events"
  },
  "state": {
    "tier": "checkpointable",
    "read": true,
    "pause": true,
    "resume": true,
    "checkpoint": true,
    "restore": true,
    "branch": true
  },
  "evaluation": {"reward": true, "verifier": true, "annotations": true},
  "traces": {
    "live": "trace.capture.raw",
    "sealed_schema": "synth.trace.v5",
    "reconciliation": true
  },
  "fidelity": {"noun": "native", "protocol": "native", "profile": "native"}
}
```

Container payload families are `synth.rollout-event.v1`,
`synth.trace-stream-event.v1`, lossless `synth.capture.raw.v1`, artifact declarations,
and task namespaces such as `craftax.*`, `harbor.*`, and `banking77.*`.

Container template declarations bind to task/profile capabilities, for example
`eval.craftax.live.v1`, `eval.harbor.live.v1`, or `eval.banking77.live.v1`; they are not
hard-coded route hints and do not grant the container renderer access.

The rollout stream links to, but does not duplicate, the trace capture. A Craftax frame
event carries step/rollout identity and an artifact reference; the corresponding Trace
V5 environment-step entity resolves by correlation after sealing.

## Optimizer implementation profile

`synth.optimizer-provider-info.v1` composes the master info with:

```json
{
  "kind": "optimizer",
  "algorithms": [
    {
      "id": "gepa",
      "version": "...",
      "status": "available",
      "run_schema": "synth.optimizer-run.v1",
      "event_stream": "optimizer.events",
      "state_slices": [
        "run.summary",
        "run.timeline",
        "run.usage",
        "gepa.candidates",
        "gepa.frontier",
        "gepa.reflections"
      ],
      "visual_templates": [
        {
          "id": "optimizer.gepa.live.v1",
          "version": "1.0.0",
          "binding_schema": "optimizer.live-binding.v1",
          "required_slices": ["gepa.candidates", "gepa.frontier"]
        }
      ]
    }
  ],
  "execution_modes": ["managed_sidecar", "hosted"],
  "run_controls": ["cancel", "pause", "resume"],
  "jobs": {"leases": true, "retries": true},
  "child_evaluations": {
    "supported": true,
    "run_schema": "evals.run.v1",
    "event_stream_schema": "evals.event-stream.v1"
  }
}
```

The canonical optimizer records become the domain implementation of shared concepts:

- `optimizer_run.v1` evolves compatibly into `synth.optimizer-run.v1`;
- `optimizer_event.v1` maps into the shared fields plus optimizer payloads;
- `optimizer_state_slice.v1` remains a cursor-addressed projection, not an event;
- execution bindings and resource refs migrate to shared types;
- optimizer relationships remain explicit graph edges.

An optimizer event references, but does not inline, child evaluation streams:

```json
{
  "type": "optimizer.candidate.evaluation.started",
  "data_schema": "synth.optimizer.candidate-evaluation.v1",
  "data": {
    "candidate_id": "cand_7",
    "evaluation_id": "eval_44",
    "purpose": "selection"
  },
  "resource_refs": [
    {"kind": "eval_run", "id": "eval_44", "role": "selection_evidence"}
  ]
}
```

```text
optimizer run / candidate
          -> evaluation relationship
eval run / examples
          -> rollout relationship
container rollout
          -> trace capture + artifacts
```

## Algorithm-specific profiles

Algorithm profiles extend Optimizer info through versioned slices and payload schemas,
not new top-level fields.

| Algorithm | Canonical live data |
| --- | --- |
| GEPA | Candidate lifecycle/lineage, evaluation batches, scores, frontier membership, reflections, prompt/program artifacts, selection versus held-out measurement. |
| GELO / `go-ex` | Board cells, themes, hypotheses, candidate/plugin recipes, checkpoint frontier, evidence, proposer/plugin jobs, promotions, and local-slot leases. |
| SFT | Dataset snapshot, training steps/epochs, loss and learning-rate metrics, checkpoint lifecycle, checkpoint evaluations, compute/worker state, inference endpoint, and promotion. |
| MAPO | Generation lifecycle, multi-agent candidate policies, rollout groups, branch checkpoints, train/selection/held-out scores, review rows, champion promotion, and failure artifacts. |
| RLVR | Policy/checkpoint version, rollout batch, verifier and reward signals, advantage/return metrics, update steps, evaluation, usage, and promotion. Exact payloads wait for a real producer. |
| OHCO | Catalog/artifact schemas may be referenced, but no core live vocabulary should be invented before an authoritative producer is reviewed. Preserve namespaced unknowns meanwhile. |

GELO is the product name; `go-ex` remains the current algorithm ID unless a migration
assigns a canonical replacement and aliases old records. MAPO should select one stable
algorithm ID and emit canonical optimizer events around existing artifact writes.

## Dynamic provider state

Discovery must not contain mutable health. Providers expose a separate observation:

```json
{
  "schema_version": "synth.execution-provider-state.v1",
  "provider_ref": "optimizer.cloud",
  "observed_at": "2026-08-12T15:03:07.148Z",
  "status": "ready",
  "capacity": {"available": null, "queued": 3},
  "bindings": [
    {"kind": "local_slot", "id": "slot_mac_01", "status": "leased"}
  ],
  "problems": []
}
```

Optimizer run status, container rollout status, provider health, and resource-lease
status remain separate. A lost local slot must not rewrite a hosted optimizer run as
local or terminal.

## Transport protocol

Every advertised durable stream implements:

- bounded poll/backfill with `after_sequence`, `limit`, `high_water`, and `terminal`;
- SSE with the sequence as `id` and resume through `Last-Event-ID`;
- optional WebSocket using the same subscribe cursor and envelope;
- capability-scoped authorization owned by the host/backend;
- explicit retention and a typed error for an expired cursor;
- readiness acknowledgement containing accepted identity, replayed cursor, and the next
  subscribed sequence;
- reconnect by backfill first, then live tail, with overlap de-duplicated by identity and
  digest.

SSE is the default one-way live transport. WebSocket is appropriate when a producer
needs bidirectional flow control or controls on the same connection. Poll is a required
recovery and compatibility path, not a lesser event format.

## Migration plan

1. Publish common schemas and conformance fixtures in a neutral public package.
2. Add provider-info projection over existing Containers metadata and implement the
   first-class Harbor fold without breaking old discovery routes or Harbor-native
   evidence.
3. Make invalid/missing sequences fail ingestion. Stop defaulting missing usage,
   metrics, and reward to zero.
4. Give each advertised stream one durable source of truth and make poll/SSE/WS read it.
5. Adopt the common resource ref in Containers and public Optimizers with dual read.
6. Map GEPA, GELO, and SFT to canonical optimizer payloads and state slices.
7. Add canonical MAPO events around its existing generation/artifact writes.
8. Add RLVR/OHCO profiles only with real captured producer streams and fixtures.
9. Make Workshop persist-before-publish, consume after its last cursor, and link
   optimizer -> eval -> rollout -> trace identities.
10. Deprecate boolean/route-hint negotiation after consumers use typed operations and
    stream descriptors.

## Cross-domain acceptance tests

| ID | Requirement |
| --- | --- |
| EP-01 | Discovery validates as exactly one discriminated profile; every operation references known schemas and limits. |
| EP-02 | Generated Python, Rust, and TypeScript bindings round-trip every capability, including false, null, and unknown namespaced fields. |
| EP-03 | Missing cursor, usage, reward, cost, duration, and counts remain invalid or unavailable as specified; none silently becomes zero. |
| EP-04 | Poll, SSE, and advertised WebSocket yield the same ordered event IDs and canonical digests. |
| EP-05 | Disconnect/reconnect at every sequence produces no loss and only identical bounded duplicates. |
| EP-06 | Persist-before-publish survives producer, relay, and Workshop restart at every commit boundary. |
| EP-07 | Expired cursor, wrong stream/run identity, unauthorized reference, oversized event/page, and schema mismatch fail with typed errors. |
| EP-08 | Unknown namespaced events and payload schemas survive relay, storage, export, and replay without being treated as known semantics. |
| EP-09 | One optimizer candidate links to exact child eval runs; each eval links to exact container rollouts/traces without flattening cursor domains. |
| EP-10 | Provider loss, execution-binding loss, child-eval failure, and optimizer terminal status remain separately observable. |
| EP-11 | A state slice at cursor N equals deterministic reduction through N or declares its independent authoritative source. |
| EP-12 | Real GEPA, GELO, SFT, MAPO, Craftax, Harbor-packaged GameBench, and Banking77 captures pass common and profile suites; this validates GameBench data, not a GameBench format adapter. |

## Open design questions

- Which base stream/resource types need a neutral package, while Containers retains
  product authority for its opinionated runtime surface and first-class Harbor fold?
- Do operations expose URI templates directly or stable IDs plus a resolved link doc?
- Is `synth.stream-event.v1` a literal outer envelope, or a normative trait flattened
  into `optimizer_event.v2`, `rollout_event.v1`, and `trace_stream_event.v1`? Flattening
  is likely simpler for existing consumers.
- What retention minimum can ephemeral providers promise before Workshop becomes the
  durable replay authority?
- Which controls require request/accepted/completed events rather than a synchronous
  operation response?

## Tentative recommendation

Start with flattened domain envelopes generated from one shared schema definition. Keep
existing names as aliases during dual read, and target:

```text
synth.execution-provider-info.v1
synth.execution-provider-state.v1
synth.resource-ref.v1
synth.stream-event.v1                 shared required fields/semantics
synth.container-provider-info.v1
synth.rollout-event.v1
synth.trace-stream-event.v1
synth.optimizer-provider-info.v1
synth.optimizer-run.v1
synth.optimizer-event.v2              adds shared stream fields strictly
synth.optimizer-state-slice.v1
```

This gives Workshop one reliable way to discover providers, resume streams, verify
identity, account for resources, and link evidence while preserving the different
lifecycles and vocabularies that make Containers and Optimizers useful.

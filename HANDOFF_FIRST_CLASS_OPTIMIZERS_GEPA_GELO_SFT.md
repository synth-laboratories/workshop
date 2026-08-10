# Handoff: First-Class Optimizers — GEPA Visuals → Hosted GELO + Local Slot → Hosted SFT

**Date:** 2026-08-09  
**Repos:** `workshop/`, `optimizers/`, `optimizers-beta/`, `backend/`  
**Audience:** engineers implementing the shared optimizer platform, Desktop projections, hosted adapters, and dogfood  
**Status:** execution scope; implement in the order below  
**Primary acceptance:** installed Workshop app, real cloud run, real local slot, real event replay, CUA evidence

Related:

- `workshop/HANDOFF_RUST_CORE_VISUALS_AND_INTERN.md`
- `workshop/apps/synth_desktop/HANDOFF_INTERN_LOCAL_SLOT.md`
- `workshop/visuals/README.md`
- `optimizers/rust/crates/synth_optimizer_platform/src/observability.rs`
- `optimizers-beta/src/main.rs`
- `optimizers-beta/src/run_projection.rs`
- `optimizers-beta/crates/synth_go_ex/src/plugins/sft.rs`
- `backend/app/api/v1/routes_optimizers.py`

---

## 0. Decision

> Optimizers become a first-class product noun beside Traces, Containers, and Visuals. Synth Cloud owns hosted optimizer execution and the canonical hosted run. Workshop's Rust `CoreRuntime` owns the durable local index, event cursor, relationships, and offline projection. TypeScript renders those projections. A local slot is an execution binding, not a second optimizer authority.

The locked implementation order is:

```text
shared optimizer noun + contracts
  → visualize GEPA first
  → hosted GELO through a real local slot
  → prove SFT-compatible affordances without advertising standalone SFT
  → implement hosted standalone SFT with Tinker
  → run Banking77 end to end through Workshop
  → verify the installed app with CUA
```

Do not begin by adding a separate SFT dashboard, a renderer-owned event store, or an SFT-specific cloud runtime. GEPA, GELO, and future SFT must exercise the same run identity, event envelope, cursor/replay, artifact references, Desktop mirror, and visual host.

---

## 1. Product noun and UX

An optimizer is not merely a job row or a visual. It is a durable run with an algorithm, objective, execution bindings, events, candidates/checkpoints, evaluations, usage, artifacts, and linked visuals.

### Product surfaces

1. **Chat loop:** an agent starts or discovers an optimizer run; the transcript receives one optimizer card and one linked visual card. Opening the visual expands the existing right `VisualHost` pane.
2. **Optimizers home:** searchable list of local and cloud runs with status, algorithm, objective, cost, recency, and source. Selecting a run opens its inspector/visual without changing its identity.
3. **Visuals vault:** retains the linked visual revision and can reopen it after the run is complete or the app restarts.
4. **Relationships:** optimizer inspectors link to their input/output Traces, bound Containers/local slot, Visuals, datasets, and model/prompt artifacts.

```text
┌──────────── Workshop ────────────┬──────────── Optimizer visual ────────────┐
│ Chat                             │ GEPA / GELO / SFT                         │
│                                  │ status · objective · cost · elapsed       │
│ Agent started optimizer opt_123  │                                           │
│ [Optimizer: GEPA · running]      │ shared timeline / historical scrub        │
│ [Visual: candidate frontier ↗]   │ shared events / artifacts / usage          │
│                                  │                                           │
│                                  │ algorithm-specific overlay                 │
└──────────────────────────────────┴───────────────────────────────────────────┘

Optimizers home
┌──────────────────────────────────────────────────────────────────────────────┐
│ Search   Status   Algorithm   Source                                         │
│ ● GEPA  banking77 prompt       cloud   running   $2.14                       │
│ ● GELO  craftax themes         cloud   paused    $8.30   slot: local-mac-01  │
│ ✓ SFT   banking77 qwen3-4b     cloud   complete  $12.72  Tinker              │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Identity rule

One hosted optimizer has one cloud `optimizer_run_id`. Workshop assigns no competing run ID; it stores the cloud ID plus local relationships and replay state. A visual has its own `visual_id` and binds to the optimizer run. Chat, Optimizers home, Visuals vault, and the right pane all resolve those same IDs.

---

## 2. Ownership and system layers

```text
┌──────────────────────────── TYPESCRIPT / REACT ─────────────────────────────┐
│ Chat cards · Optimizers home · inspector · VisualHost · CUA/a11y projection │
│ Renders state only; no cloud secrets, SQLite, scheduling, or raw slot lease  │
└───────────────────────────────────┬──────────────────────────────────────────┘
                                    │ Tauri commands + runtime:event
┌───────────────────────────────────▼──────────────────────────────────────────┐
│ WORKSHOP RUST CoreRuntime                                                    │
│ OptimizerService · local mirror/index · journal · cursor/replay · relations  │
│ Visual Registry · Container/slot registry · Cloud client · MCP adapter        │
└───────────────┬───────────────────────────┬───────────────────────────────────┘
                │                           │ outbound slot connection/lease
                ▼                           ▼
┌────────────────────────────┐   ┌─────────────────────────────────────────────┐
│ SYNTH CLOUD                │   │ LOCAL SLOT                                  │
│ canonical hosted run       │   │ bounded execution/eval/inference capability │
│ event log + projections    │   │ health + lease + capability manifest        │
│ scheduling + billing       │   │ never a mailbox or optimizer scheduler      │
└───────────────┬────────────┘   └─────────────────────────────────────────────┘
                │
       ┌────────┴─────────┐
       ▼                  ▼
 optimizers/         optimizers-beta/
 GEPA runtime        GELO runtime
                            │ future standalone SFT adapter
                            ▼
                          Tinker
```

### Authority table

| Concern | Authority |
| --- | --- |
| Hosted lifecycle, scheduling, billing, canonical events | Synth Cloud |
| Algorithm execution and algorithm-native artifacts | `optimizers/` or `optimizers-beta/` worker |
| Tinker training job/checkpoints | Tinker, normalized by Synth Cloud |
| Local execution capability and lease health | Workshop Rust slot service |
| Local durable mirror, cursor, chat relationship, offline summary | Workshop Rust `CoreRuntime` |
| Visual records/revisions/CAS | Workshop Rust Visual Registry |
| Rendering and interaction | TypeScript/React |

---

## 3. Shared optimizer contract

### 3.1 Optimizer record

Add a versioned provider-neutral record shared by Cloud and Desktop:

```text
optimizer_run.v1
  id
  algorithm_id                 # extensible string, e.g. gepa, go-ex, sft
  algorithm_version
  status                       # queued|starting|running|pausing|paused|...
  source                       # cloud|local
  objective
  project_ref / session_ref
  created_at / started_at / finished_at
  cursor_seq
  capabilities
  execution_bindings[]         # container, local_slot, provider
  input_refs[]                 # traces, datasets, prompts, configs
  output_refs[]                # prompt, adapter, checkpoint, report
  visual_refs[]
  summary
  usage
  error
```

For hosted runs, Workshop stores this as a mirror with its last durable cursor and local relationships. Reconciliation must be idempotent by `(optimizer_run_id, sequence_number)`.

### 3.2 Extensibility correction

The existing Rust platform contract uses a closed `OptimizerAlgorithm { Gepa, GoEx }` enum and closed item/slice enums. That is sufficient for the current two algorithms but not a long-lived first-class noun.

Before adding more templates:

- make `algorithm_id` forward-compatible, preferably a validated string/newtype with constants for known algorithms;
- keep known algorithm helpers, but do not reject an event solely because a newer algorithm ID is unknown;
- allow namespaced projection slices such as `gepa.frontier`, `go-ex.themes`, and `sft.training_curves`;
- add generic item kinds for checkpoint, metric, evaluation, dataset, artifact, and provider operation;
- preserve unknown `raw` fields for inspection without making TS depend on them.

If changing the current serialized enum would break deployed consumers, introduce additive `algorithm_id`/`slice_id` fields in `optimizer_event.v1`, dual-read during migration, and only remove the old enum in a versioned `v2` contract.

### 3.3 Capabilities

Do not infer controls from algorithm names. Each run publishes capabilities:

```json
{
  "cancel": true,
  "pause": true,
  "resume": true,
  "streamEvents": true,
  "stateSlices": true,
  "candidates": true,
  "checkpoints": false,
  "checkpointEvaluations": false,
  "inferenceEndpoint": false,
  "localSlotBinding": true
}
```

The UI hides or disables controls from this manifest. MCP and Rust enforce the same capabilities server-side.

### 3.4 Event envelope

All algorithms use one replayable envelope:

```text
optimizer_event.v1
  event_id
  sequence_number
  occurred_at
  optimizer_run_id
  algorithm_id
  type
  level
  item?                        # typed resource snapshot
  delta?                       # optional incremental detail
  snapshot?                    # absolute fields needed for historical replay
  usage_delta?
  artifact_refs[]
  error?
  raw?                         # provider/algorithm payload
```

Requirements:

- sequence numbers are stable and monotonic per run;
- reconnect uses `after_seq`, with bounded backfill followed by live SSE;
- terminal events and output artifact registration are durable before acknowledgement;
- events contain immutable checkpoint/candidate/artifact IDs;
- secret values and signed provider URLs never enter events;
- zero-valued metrics are facts, not missing values;
- the Desktop journal deduplicates replay/live overlap.

### 3.5 State slices

Common slices:

```text
run.summary
run.timeline
run.usage
run.logs
run.artifacts
run.execution
```

Algorithm overlays:

```text
gepa.candidates     gepa.frontier       gepa.reflections
go-ex.board         go-ex.themes        go-ex.data_engine
sft.training_curves sft.checkpoints     sft.checkpoint_evaluations
sft.dataset         sft.compute         sft.examples
```

One cursor request plus a batch slice read must reconstruct the current visual without replaying the entire event history.

---

## 4. Rust Desktop implementation

Add an `OptimizerService` to the existing Rust `CoreRuntime`; do not create another daemon.

### Storage

Recommended tables:

```text
optimizer_runs
optimizer_event_cursors
optimizer_relationships
optimizer_cached_slices
```

The global journal remains the UI event source. Cloud payloads are normalized and committed with cursor advancement in one transaction. Large state/artifacts remain CAS or cloud references; do not duplicate entire training datasets in SQLite.

### Commands/bridge

The renderer should receive a typed `window.synthOptimizers` bridge backed by Tauri commands:

```text
list / get / refresh
create
cancel / pause / resume
get_state / get_state_batch
subscribe / reconcile
open_visual
```

No React component calls Cloud directly. API keys remain in Rust-owned secure configuration and are never serialized to the renderer or visual bindings.

### MCP

Expose the same service to agents, initially:

```text
optimizer_list_algorithms
optimizer_list_runs
optimizer_get_run
optimizer_create_run
optimizer_watch_run
optimizer_get_state
optimizer_cancel_run
optimizer_open_visual
```

MCP mutations and UI mutations must converge on the same Rust service and journal path.

### Relationships

Use typed edges rather than metadata-only links:

```text
optimizer --uses--> trace | dataset | prompt | container | local_slot
optimizer --produces--> prompt | adapter | checkpoint | trace | report
optimizer --visualized_by--> visual
optimizer --started_from--> chat/session
```

---

## 5. Shared visual architecture

Create one shared template family with algorithm overlays:

```text
visuals/templates/
  optimizer.run.v1/
    template.json
    shell.tsx
    components/
      RunHeader.tsx
      GlobalTimeline.tsx
      UsageCards.tsx
      EventLog.tsx
      ArtifactList.tsx
      CandidateRail.tsx
    overlays/
      gepa.tsx
      go-ex.tsx
      sft.tsx                 # fixture-only until hosted SFT is enabled
    examples/
      gepa_events.json
      goex_events.json
      sft_events.json
```

The template receives an `optimizer_run` binding plus optional immutable artifact/trace bindings. Add `optimizer_run` to the canonical binding kinds rather than treating optimizer IDs as an untyped live URL.

### Shared visual behavior

- follow the newest event until the operator scrubs;
- global historical slider reprojects all panels at the selected sequence/time;
- algorithm-specific panels consume namespaced slices but use shared chrome;
- reconnect backfills from the last sequence and resumes live updates;
- completion freezes a reproducible visual revision bound to output digests;
- chat and Optimizers home open the same `VisualHost` and `visual_id`;
- the Visuals vault retains the visual after restart;
- charts have accessible textual projections and CUA-addressable controls.

---

## 6. Layer 1 — Visualize GEPA first

GEPA is the reference implementation for the shared optimizer noun and visual shell. Do not start with SFT.

### GEPA overlay

```text
header: objective · status · iteration · best score · cost
timeline: proposal / rollout / reflection / promotion markers
candidate lineage: parent → mutation → evaluation
Pareto frontier: quality vs cost/rollouts
candidate rail: score, deltas, rank, status
rollout/eval evidence: linked Trace V5 samples
reflection panel: compact algorithm rationale
usage: tokens, dollars, rollouts, wall time
```

### Implementation sequence

1. Produce a bounded GEPA event fixture using the current `optimizer_event.v1` envelope.
2. Implement `optimizer.run.v1` shared chrome and the GEPA overlay.
3. Bind the fixture through the Rust Visual Registry.
4. Consume a real local GEPA run and prove live-follow plus historical scrub.
5. Consume a hosted GEPA run through Cloud SSE/backfill.
6. Close/reopen Workshop and prove the local optimizer mirror and visual restore.

### Exit gate

- no GEPA-specific network calls in TS;
- one optimizer run is visible in Chat, Optimizers home, and its linked visual;
- live and replayed events yield identical state at the same cursor;
- candidate/frontier evidence links to real traces/artifacts;
- CUA can open, scrub, compare candidates, and return to latest.

---

## 7. Layer 2 — Hosted GELO through a real local slot

After the GEPA visual proves the shared path, connect hosted GELO from `optimizers-beta` and bind its execution to a Workshop local slot.

### Slot rule

The slot is a bounded capability leased by Cloud. It is not a mailbox, run registry, or scheduler. The Desktop establishes an authenticated outbound connection/tunnel so Cloud never assumes it can dial arbitrary localhost addresses.

```text
Workshop SlotService
  register capabilities
  → outbound authenticated connection
  → Cloud grants short lease for optimizer_run_id
  → GELO dispatches allowed eval/container operations
  → slot returns typed results/artifact refs
  → Cloud appends canonical optimizer events
```

Required slot facts:

```text
slot_id · machine_id · status · capabilities · lease_id
optimizer_run_id · expires_at · last_heartbeat_at
container_refs · allowed_operations · concurrency · resource limits
```

Fail closed on expired lease, run mismatch, missing capability, invalid generation, or disconnected tunnel. Do not put cloud or Tinker credentials in slot events.

### GELO overlay

Reuse shared chrome and add:

```text
phase/tick board
themes and saturation
checkpoint/state map
candidate and treatment frontier
data miner queue + near-miss evidence
full rollout queue
paired acceptance/heldout results
plugin lane status, including existing goex.sft.v1 artifacts when present
slot health/lease status as execution metadata
```

`optimizers-beta` already exposes durable event artifacts, state projections, hosted artifact publication, and live streams. Normalize those into the shared contract; do not make the visual parse raw `events.jsonl` shapes directly.

### Exit gate

- a real Cloud GELO run is created from Workshop;
- the run leases the selected real local slot and records the binding;
- slot disconnect/reconnect is visible and recoverable without duplicate work;
- GELO state batch plus cursor reconstructs the visual;
- completion artifacts and traces are linked and reopenable;
- the app can restart mid-run and resume from the durable cursor.

---

## 8. Layer 3 — Add SFT affordances, not yet a product

At this layer, standalone SFT is not listed as generally available and `/v1/fine_tuning/jobs` need not be live. The purpose is to prove that the shared platform can accept a training algorithm without another architecture.

### Required compatibility work

- forward-compatible `algorithm_id` and namespaced state slices;
- generic checkpoint, metric, evaluation, dataset, provider-operation, and artifact items;
- run capabilities for checkpoints, checkpoint evaluations, and inference endpoint;
- `sft` overlay registration in the visual template catalog;
- synthetic SFT event/state fixtures covering reconnect and replay;
- provider-neutral `SftBackend` contract;
- reserved OpenAI-compatible request/response DTO tests;
- feature availability reports `sft: unavailable|private_beta|available` rather than failing as an unknown algorithm.

### Existing GELO SFT lane

Keep `goex.sft.v1` as a GELO plugin lane. It currently materializes selected traces, datasets, a Tinker training job/result, sampler configuration, and a policy bundle. It must not be mislabeled as a standalone SFT optimizer.

Later, both the GELO plugin and standalone SFT may reuse a shared Tinker training adapter, but neither should call into the other's orchestration state machine.

### Fixture exit gate

A synthetic SFT run can render, scrub, compare checkpoints, reconnect, complete, and persist in the Visual Registry using only the shared optimizer APIs. No production SFT route or billing claim is required.

---

## 9. Layer 4 — Hosted standalone SFT, Tinker first

Once the compatibility fixture is green, register `algorithm_id = "sft"` in hosted `optimizers-beta` and back it with Tinker.

### Provider-neutral boundary

```text
SftBackend
  submit
  poll_or_stream_events
  pause / resume / cancel
  list_checkpoints
  materialize_checkpoint
  create_inference_target
  dispose

implementations
  TinkerSftBackend       # first
  ModalSftBackend        # later, same contract
```

Provider-native IDs and payloads stay in a provider namespace. Synth assigns stable optimizer, checkpoint, evaluation, and artifact IDs. Provider secrets and signed URLs are never exposed to TS, MCP, or persisted visual props.

### OpenAI-compatible façade

After the canonical optimizer implementation works, expose an OpenAI-like compatibility layer:

```text
POST /v1/files
POST /v1/fine_tuning/jobs
GET  /v1/fine_tuning/jobs
GET  /v1/fine_tuning/jobs/{id}
POST /v1/fine_tuning/jobs/{id}/cancel
POST /v1/fine_tuning/jobs/{id}/pause
POST /v1/fine_tuning/jobs/{id}/resume
GET  /v1/fine_tuning/jobs/{id}/events
GET  /v1/fine_tuning/jobs/{id}/checkpoints
```

This is an adapter over the canonical optimizer run, not a second job database. Synth extensions such as backend choice, checkpoint evaluation policy, slot binding, and dataset roles live under a namespaced request extension such as `extra_body.synth`.

### SFT events

```text
optimizer.run.created
sft.dataset.validation_started
sft.dataset.validated
sft.training.queued
sft.training.started
sft.step.metrics
sft.epoch.completed
sft.checkpoint.created
sft.checkpoint_eval.queued
sft.checkpoint_eval.started
sft.checkpoint_eval.case_completed
sft.checkpoint_eval.completed
sft.checkpoint.promoted
sft.training.paused | resumed | completed | failed
sft.heldout_eval.started | completed
sft.model.materializing | materialized
optimizer.artifact.created
optimizer.run.completed
```

Step events may be coalesced for storage/stream efficiency, but checkpoint and lifecycle events must never be dropped. Every checkpoint is immutable and evaluation results bind to its digest, dataset digest, evaluator version, and seed/config.

### Scientific split policy

Use three explicit roles:

| Split | Purpose | May affect checkpoint selection? |
| --- | --- | --- |
| `train` | gradient updates | yes |
| `selection` | recurring checkpoint evaluation | yes |
| `heldout` | final/precommitted measurement | no |

If a dataset is evaluated repeatedly and used to pick a checkpoint, call it `selection`, not `heldout`. Default hosted policy:

```text
selection: evaluate every configured checkpoint
heldout: evaluate final + promoted checkpoint only
```

Sparse precommitted heldout audits may be supported, but the UI must label them and they must not feed automatic promotion. Hosted mode should reject any configuration equivalent to `allow_heldout_training_examples=true`.

### SFT overlay

```text
header: base model + adapter · backend · status · step/epoch · cost
curves: train/validation loss · learning rate · throughput
checkpoint rail: immutable checkpoints, selection scores, promotion
eval matrix: aggregate + per-category metrics and uncertainty
examples: baseline vs selected checkpoint outputs and failures
dataset: split counts, digest, filtering/rejection summary
compute: provider, GPU, utilization, tokens/sec, spend
lineage: base model → adapter/checkpoint → inference artifact
events/artifacts: shared panels
```

This should recover the useful old Synth SFT feel—live status, dense metrics, checkpoint messages, files, and a visible improvement story—inside the new Rust/cloud authority model.

### Hosted SFT exit gate

- OpenAI SDK compatibility tests pass against the Synth base URL;
- a real Tinker job streams normalized metrics and checkpoint events;
- pause/resume/cancel semantics are honest and capability-gated;
- checkpoint evaluations are durable, reproducible, and linked to immutable artifacts;
- terminal model identity represents base model plus adapter/checkpoint, not only an opaque renamed model;
- the same run is visible through the canonical optimizer API and compatibility API.

---

## 10. Layer 5 — Banking77 end-to-end through Workshop and CUA

The final acceptance is a real installed-app dogfood, not a fixture and not only browser Playwright.

### Intended topology

```text
Workshop installed app
  → Rust OptimizerService
  → Synth Cloud hosted SFT optimizer
  → Tinker training
  → checkpoint eval dispatch
  → leased Workshop local slot
  → Banking77 evaluator/inference workload
  → canonical Cloud events + artifacts
  → Rust cursor/replay + Visual Registry
  → chat card + right SFT visual + Optimizers home
```

### Banking77 run

- use a pinned Banking77 dataset revision and record train/selection/heldout digests;
- use a supported small instruct base model and a bounded LoRA configuration;
- run a base-model evaluation before training;
- train on Tinker and produce multiple immutable checkpoints;
- evaluate checkpoints on the selection set through the leased local slot;
- promote using a predeclared selection metric, preferably macro-F1 with accuracy displayed;
- run heldout only for the final/promoted model and the frozen baseline;
- persist per-category metrics, confusion evidence, representative failures, usage, and model lineage;
- bind the completed visual to all immutable source/output references.

### CUA acceptance script

1. Launch the installed Workshop `.app` with the private API key available only to the Rust process.
2. Open Containers/local slots and verify the intended slot is healthy and unleased.
3. In chat, ask the agent to start the pinned Banking77 SFT recipe using the selected local slot.
4. Confirm an optimizer card and linked visual card appear without manual refresh.
5. Open the right pane and verify live status, curves, slot binding, and checkpoint rail.
6. Scrub to an earlier checkpoint; verify all panels, times, metrics, and examples rewind consistently.
7. Return to latest and compare baseline with the promoted checkpoint.
8. Close and reopen Workshop during the run; verify cursor replay and live continuation without duplicates.
9. After completion, open the same run from Optimizers home and the same visual from the Visuals vault.
10. Verify the final heldout result is labeled measurement-only and distinct from checkpoint-selection metrics.
11. Open linked artifacts/traces and confirm digests, dataset roles, provider, slot, cost, and model lineage.

### Required evidence packet

```text
banking77_sft_acceptance/
  run_record.json
  events.ndjson
  state_slices/
  dataset_manifest.json
  checkpoint_manifest.json
  evaluation_manifest.json
  model_lineage.json
  usage.json
  visual_record.json
  restart_reconciliation.json
  screenshots/
  cua_report.md
```

Secrets, signed URLs, and raw credentials must be absent from the evidence packet.

### Final release gate

- real hosted run, real Tinker compute, real slot lease, and real Banking77 metrics;
- installed-app CUA completes the entire script;
- app restart produces no missing or duplicated events;
- Cloud and Desktop agree on terminal status, cursor, selected checkpoint, usage, and artifact IDs;
- SQLite integrity and foreign-key checks pass;
- no retired Python product runtime process is required;
- the final visual remains usable offline from its sealed summary/artifact references.

---

## 11. Cloud API and projection requirements

Retain one canonical optimizer API family:

```text
POST /api/v1/optimizers/runs
GET  /api/v1/optimizers/runs
GET  /api/v1/optimizers/runs/{id}
GET  /api/v1/optimizers/runs/{id}/events?after_seq=N
GET  /api/v1/optimizers/runs/{id}/state
GET  /api/v1/optimizers/runs/{id}/state/batch?slices=...
GET  /api/v1/optimizers/runs/{id}/state/{slice}
POST /api/v1/optimizers/runs/{id}/commands
GET  /api/v1/optimizers/runs/{id}/artifacts
```

Cloud projection must be sufficient to serve run lists and terminal summaries when an algorithm worker is gone. The hot worker may accelerate live reads, but correctness and bounded replay cannot depend on that process remaining alive.

The current algorithm-specific Go-Ex stream may remain temporarily, but the product visual should migrate to the canonical optimizer event stream. Algorithm-native streams become debug/compatibility surfaces.

---

## 12. Testing layers

### Contract tests

- Rust/JSON golden fixtures for record, events, capabilities, state slices, and relationships;
- unknown future algorithm and slice IDs deserialize without data loss;
- replay/live overlap deduplicates correctly;
- zero metrics and terminal failures survive normalization;
- no secret-like fields appear in public events or visual bindings.

### Algorithm tests

- GEPA event adapter and projection fixtures;
- GELO state/event normalization and slot interruption;
- SFT synthetic fixture before provider implementation;
- Tinker adapter checkpoint/event translation;
- selection/heldout policy enforcement.

### Cloud tests

- durable event append and cursor reads;
- worker restart and terminal projection;
- command idempotency;
- artifact publication and digest verification;
- slot lease fencing, expiry, reconnect, and run binding;
- OpenAI-compatible façade maps to one canonical run.

### Workshop tests

- Rust mirror transaction + cursor advancement;
- restart reconciliation;
- optimizer list/search/filter;
- chat and visual relationship projection;
- MCP and UI mutations use the same service;
- Visual Registry retains the linked visual;
- Playwright/a11y covers fixture and deterministic fake-cloud modes;
- CUA covers installed-app real-cloud acceptance.

---

## 13. Multi-repo implementation map

| Layer | Primary locations |
| --- | --- |
| Shared optimizer contract | `optimizers/rust/crates/synth_optimizer_platform/src/observability.rs` plus shared schema fixtures |
| GEPA adapter/events | `optimizers/` Rust runtime and hosted client surfaces |
| GELO events/projections | `optimizers-beta/src/main.rs`, `src/run_projection.rs` |
| Existing GELO SFT plugin | `optimizers-beta/crates/synth_go_ex/src/plugins/sft.rs` |
| Hosted optimizer API/projection | `backend/app/api/v1/routes_optimizers.py` and optimizer projection/storage services |
| Workshop Rust authority | `workshop/apps/synth_desktop/src-tauri/src/` (`CoreRuntime`, new optimizer module, storage, IPC) |
| Workshop renderer | `workshop/apps/synth_desktop/src/renderer/src/` (`OptimizersPage`, cards, bridge, `VisualHost`) |
| Visual template | `workshop/visuals/templates/optimizer.run.v1/` |
| Visual Registry/bindings | `workshop/apps/synth_desktop/src-tauri/src/visuals/`, `workshop/visuals/runtime/` |
| Local slot | Workshop Rust container/slot service plus authenticated Cloud lease/tunnel surface |
| Installed-app acceptance | `workshop/apps/synth_desktop/tests/` plus retained CUA evidence packet |

---

## 14. Explicit non-goals and traps

- Do not ship standalone SFT before GEPA proves the shared visual/event path.
- Do not conflate `goex.sft.v1` with standalone `algorithm_id=sft`.
- Do not let TS call Cloud, Tinker, Modal, SQLite, or slot tunnels directly.
- Do not make a local slot a second scheduler, mailbox, or optimizer registry.
- Do not make SSE the only durable record; it is transport over the event log.
- Do not let visuals parse arbitrary algorithm-native files when canonical projections exist.
- Do not use true heldout results for checkpoint selection while continuing to call them heldout.
- Do not store API keys in TOML, events, SQLite payloads, evidence packets, or visual bindings.
- Do not represent a LoRA only as a renamed opaque model; preserve base + adapter lineage.
- Do not build separate GEPA, GELO, and SFT pages with unrelated data models.

---

## 15. Execution checklist

### Layer 0 — noun and shared infrastructure

- [ ] Version the optimizer record and extensible event/state contracts.
- [ ] Add capabilities and typed relationships.
- [ ] Add Workshop Rust `OptimizerService`, tables, IPC, reconciliation, and MCP.
- [ ] Add Optimizers home and chat cards.
- [ ] Add `optimizer_run` visual binding and shared `optimizer.run.v1` shell.

### Layer 1 — GEPA visual

- [ ] Fixture → local live run → hosted run.
- [ ] Candidate lineage, Pareto frontier, evidence, usage, timeline.
- [ ] Historical scrub, reconnect, restart, vault persistence, CUA/a11y.

### Layer 2 — hosted GELO + slot

- [ ] Normalize hosted GELO into the shared stream/slices.
- [ ] Implement real slot capability registration, outbound connection, and fenced lease.
- [ ] Render GELO overlay and execution binding.
- [ ] Prove interruption/reconnect and terminal artifact publication.

### Layer 3 — SFT affordances

- [ ] Add generic checkpoint/eval/training affordances.
- [ ] Add SFT fixture, overlay, provider trait, and compatibility DTO tests.
- [ ] Keep feature unavailable/private and keep GELO's SFT plugin identity honest.

### Layer 4 — hosted Tinker SFT

- [ ] Register standalone `sft` algorithm.
- [ ] Stream Tinker training/checkpoint events.
- [ ] Orchestrate selection and heldout evaluations.
- [ ] Add OpenAI-compatible façade over the canonical run.
- [ ] Publish immutable adapter/checkpoint/inference artifacts and usage.

### Layer 5 — Banking77 CUA dogfood

- [ ] Pin data/model/config and establish a healthy local slot.
- [ ] Start the real hosted run from Workshop chat.
- [ ] Verify live visual, scrub, compare, restart, Optimizers home, and Visuals vault.
- [ ] Seal the evidence packet and pass all release gates.

---

## 16. Definition of done

The program is complete when an operator can treat Optimizers exactly as they treat Traces, Containers, and Visuals: discover one, start one, watch it live, inspect its evidence, follow its relationships, reopen it after restart, and hand it to an agent—while Cloud remains authoritative for hosted execution and Workshop remains authoritative for the local product experience.

The final proof is a real Banking77 Tinker SFT run started from the installed Workshop app, evaluated through a leased local slot, visualized live and historically, restored after restart, and verified end to end with CUA.

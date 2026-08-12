# Live GEPA optimizer progress as a first-class Workshop capability

**Master plan:** `execution_platform_master_plan.md`

> Shared nouns, service bindings, and child-evaluation relationships are proposed in
> `execution_ontology.md`. Shared provider discovery, resource references, and stream
> delivery semantics are proposed in `execution_stream_contracts.md`. This note owns the
> Optimizer profile, algorithm projections, child-eval relationships, sidecar lifecycle,
> and visuals.

**Status:** Product and interaction design proposal

**Working mode:** Dedicated GEPA real-stream reference prototype first; shared optimizer
implementation after review

**Initial task:** Banking77 prompt optimization

## Summary

Workshop already treats an optimizer as a first-class product noun and already has most
of the durable substrate: `optimizer_run.v1`, `optimizer_event.v1`, cursor-addressed
events, projected state slices, relationships, an Optimizers page, a Rust-owned service,
an optimizer MCP, and `optimizer.run.v1` with a GEPA overlay.

What it does not yet prove is a dependable live-observation workflow. A local GEPA recipe
is tailed into the Rust store every 750 ms, but the background ingestion path does not
publish its returned `optimizer.run.updated` event to the Tauri event bus. The renderer
reloads optimizer data only when it receives such an app event. Opening the visual before
the worker starts is therefore close to the right lifecycle, but it is not yet a verified
live connection with readiness, incremental delivery, or connection health.

The first reference prototype should be dedicated to GEPA so that candidate lineage,
evaluation batches, Pareto selection, reflections, held-out measurement, and budget use
can be made legible. Optimizer execution should move behind a modular, versioned sidecar
managed by Workshop in the same architectural style as the inference sidecar. Each
algorithm should have a concomitant, first-class visual template family installed and
versioned with its supported sidecar capability. Templates share Workshop lifecycle and
visual primitives, but they do not force GEPA, GELO, and SFT into one generic dashboard.

## Relationship between Workshop and Optimizers

The intended boundary is:

```text
Workshop product and control plane
  owns local index, cursor/replay, relationships, visual records, readiness,
  lifecycle supervision, permissions, and the user-facing projection
                              |
                              | starts, mirrors, or reconciles
                              v
Optimizer sidecar or hosted provider
  local sidecar or Synth Cloud owns search, proposal, evaluation scheduling, selection
  semantics, algorithm-native artifacts, and authoritative optimizer events
                              |
                              | launches or references
                              v
Eval/container providers
  own examples, rollouts, predictions, rewards/scores, traces, and task truth
```

Optimizers is therefore not merely an optional Workshop visual and not a parallel app.
It is a first-class Workshop domain whose local execution is supplied by a modular
sidecar and whose hosted execution may be supplied by Synth Cloud. Workshop manages,
mirrors, and projects the run; it does not embed the GEPA algorithm in CoreRuntime.

One hosted optimizer keeps the cloud `optimizer_run_id`. A local optimizer also has one
stable `optimizer_run_id`. Candidate evaluations keep their own `eval_run_id`, rollout,
example, trace, and artifact identities. A visual has a separate `visual_id` bound to the
optimizer run.

## Modular sidecar decision

Optimizer execution is a first-class, modular sidecar analogous to the inference
sidecar. Workshop should ship an `OptimizerManager` alongside `LagunaManager`. The
manager owns discovery, installation, signature and digest verification, version
selection, process lifecycle, health, authenticated loopback connection, compatibility,
and recovery. The sidecar owns optimizer compute and authoritative execution events.
The existing Rust `OptimizerService` remains the durable Workshop projection, run index,
relationship store, and visual integration.

The sidecar is supported first-class without being always-on. Users can install, start,
stop, update, roll back, and remove individual sidecar versions. Stopping or uninstalling
a sidecar version does not delete optimizer runs, mirrored events, visuals, or artifacts;
data deletion is a separate explicit operation.

Each installed sidecar advertises a signed capability manifest containing at least:

- sidecar distribution version and immutable digest;
- supported optimizer algorithms and algorithm-version ranges;
- supported contract and recipe-schema versions;
- supported visual template IDs, versions, binding schemas, and default recipes;
- platform and accelerator requirements;
- health, lifecycle, event replay, cancellation, and artifact capabilities;
- migration and backward-compatibility declarations.

Its runtime responsibilities are narrow:

- advertise a versioned GEPA worker capability;
- accept an allowlisted, bounded recipe or lease from Workshop;
- emit canonical optimizer events and durable artifacts;
- expose health and cancellation;
- keep credentials out of bindings, events, logs, and images;
- never own Workshop visuals, chat relationships, the local run index, or the canonical
  hosted run.

Workshop records three independent versions on every run:

| Version | Meaning |
| --- | --- |
| `sidecar_version` | Installed optimizer service distribution plus immutable digest |
| `algorithm_version` | GEPA or other algorithm implementation version |
| `recipe_version` | Pinned recipe, task, dataset, and configuration contract |

Version selection is per new run. An active run remains pinned to the sidecar instance
and versions with which it started. Workshop must not silently migrate an active run.
Removal is blocked while that version owns an active run; rollback starts a previous
installed version for subsequent runs.

The resulting user modes are:

| User mode | Execution | Workshop role |
| --- | --- | --- |
| Sidecar not installed | No local optimizer compute is available | Optimizer UI, imports, history, and cloud remain available |
| Sidecar installed and stopped | No local compute is running | Can inspect history or select a version to start |
| Desktop-managed sidecar | Workshop starts the selected native sidecar version | Owns management, mirror, relationships, and visual |
| Compose-managed sidecar | User opts into `optimizer-gepa`; Workshop discovers it | Owns compatibility checks, mirror, relationships, and visual |
| Synth Cloud | Cloud schedules and runs GEPA | Mirrors cloud identity/events and renders them |

Compose should expose the same sidecar contract under an opt-in `optimizer-gepa` profile;
it is a packaging and lifecycle option, not a different optimizer architecture. Do not
make the default Workshop Compose stack depend on it. The current `docker-compose.yml`
is also a development/compatibility stack and includes the legacy Python runtime; it
should not become the source of truth for Desktop process architecture.

## Algorithm-owned visual template families

First-class optimizer support includes first-class visuals. Every supported algorithm
advertises one or more compatible visual recipes in an algorithm capability manifest.
For local execution, that manifest is included with the sidecar. For hosted execution,
it comes from the authenticated Synth backend and signed Workshop template catalog.
Workshop installs the template package into the Visual Registry and chooses a default
template from the run's algorithm and capabilities. A user does not need to install a
local sidecar merely to visualize a hosted optimizer.

The architecture is:

```text
shared Workshop optimizer visual primitives
  run identity, status, connection, cursor, historical scrub, usage, budgets,
  lifecycle controls, artifacts, failures, and child-evidence navigation
                              |
                              v
algorithm template families
  GEPA: candidates, lineage, evaluation progress, frontier, reflections, prompt diffs
  GELO: search board, themes, experiments, local-slot activity, promotion
  SFT: training curves, checkpoints, evaluations, compute, dataset, promotion
```

The initial GEPA family should be:

| Template | Purpose |
| --- | --- |
| `optimizer.gepa.live.v1` | Default live run progress, active candidates, budget, and activity |
| `optimizer.gepa.frontier.v1` | Pareto frontier, incumbent history, cost/quality trade-offs, and selection |
| `optimizer.gepa.candidate.v1` | Candidate lineage, materialized prompt/program diff, scores, and evidence |
| `optimizer.gepa.evaluations.v1` | Candidate-by-example evaluation progress and child eval/trace drill-down |

These templates may compose shared components and the same cursor-addressed reducer, but
they remain independently registered Visual Registry templates. `optimizer.run.v1` may
remain as a generic fallback for unknown or unavailable algorithm packages; it should not
be the primary experience for an installed, supported algorithm.

Every visual record persists:

- exact template ID and template-package version;
- template digest and compatible sidecar/algorithm contract ranges;
- optimizer run ID and selected cursor;
- required state slices and child evidence bindings;
- the template revision or durable package reference needed for terminal replay.

Uninstalling an optimizer sidecar must not make completed visuals unreadable. Workshop
retains the exact signed template revision used by existing visual records, or an
equivalent durable renderer bundle, after compute removal. Removing retained visual
packages is a separate operation that must disclose which historical visuals would lose
their native renderer.

Sidecars and hosted services do not send arbitrary executable UI code at run time.
Template packages are signed, installed through Workshop's visual/template registry,
compatibility-checked, and subject to the same origin, binding, size, and capability
policy as bundled Workshop visuals.

## Hosted provider and GELO compatibility

Local sidecars and hosted algorithms must converge at the Workshop optimizer subsystem,
not at the process boundary. They share durable run, event, state-slice, artifact,
relationship, and visual contracts while retaining different execution ownership.

```text
local algorithm
  Workshop -> managed optimizer sidecar -> canonical local optimizer events
                                      \
                                       -> OptimizerService mirror -> Visual Registry
                                      /
hosted algorithm
  Workshop -> Synth backend -> optimizers-beta -> canonical hosted optimizer events
```

`optimizers-beta` remains private and backend-authorized. Workshop must call the Synth
backend, never the private optimizer service directly. The backend owns user/org/project
authorization, billing, public run projection, artifact access, and the replay-plus-live
event boundary. Workshop stores the hosted `optimizer_run_id` without minting a competing
local identity.

The provider-neutral run record adds an explicit execution descriptor:

```json
{
  "id": "goex_123",
  "algorithm_id": "go-ex",
  "algorithm_version": "...",
  "source": "cloud",
  "execution_bindings": [
    {
      "kind": "synth_cloud",
      "id": "goex_123",
      "status": "running"
    },
    {
      "kind": "local_slot",
      "id": "slot_mac_01",
      "status": "leased"
    }
  ]
}
```

The second binding is optional. A hosted GELO run may lease a Workshop-local slot or
container for rollout/evaluation capability while the optimizer remains cloud-owned.
The visual must show optimizer-provider health and slot/lease health independently. A
lost local slot is an execution-binding failure on a hosted run, not evidence that the
run became local or that its durable cloud history disappeared.

The shared provider contract should be:

| Capability | Local sidecar | Hosted GELO through Synth backend |
| --- | --- | --- |
| Discover algorithms/templates | Sidecar capability manifest | Backend algorithm catalog |
| Create run | Authenticated loopback command | Authenticated backend `POST /optimizers/runs` |
| Stable identity | Local `optimizer_run_id` | Cloud `optimizer_run_id` |
| Replay events | Cursor backfill from sidecar/Workshop mirror | Backend events after cursor |
| Live events | Sidecar SSE or WebSocket into Workshop | Backend SSE or WebSocket into Workshop |
| State slices | Sidecar projections mirrored by Workshop | Backend state batch/artifacts mirrored by Workshop |
| Artifacts | Local durable references/CAS | Backend-authorized durable artifact references |
| Cancel/control | Sidecar capability | Backend capability |
| Runtime install/start/stop | Supported | Not applicable to user; provider-managed |
| Historical visuals | Workshop mirror and retained templates | Workshop mirror and retained templates |

Workshop then exposes one renderer-facing path regardless of provider:

```text
provider replay/live stream
  -> validate and normalize to optimizer_event.v1
  -> persist event, cursor, run, and slices transactionally
  -> publish committed cursor
  -> renderer reads events after its last accepted cursor
  -> algorithm templates render the selected durable cursor
```

This is intentionally not a browser connection to Cloud SSE. Rust owns cloud auth,
reconnection, normalization, persistence, and de-duplication. The visual binds only to
the Workshop optimizer run projection.

### Negotiated live transports

The provider contract supports both SSE and WebSocket transports. Transport is a
capability negotiated per sidecar or hosted provider, not encoded into an algorithm
template. Cursor-addressed bounded backfill remains mandatory regardless of the live
transport so that reconnection never depends on an uninterrupted socket.

```json
{
  "events": {
    "schema": "optimizer_event.v1",
    "backfill": {
      "transport": "http",
      "cursor": "sequence_number"
    },
    "live_transports": ["sse", "websocket"],
    "preferred_live_transport": "sse",
    "supports_resume": true,
    "supports_heartbeat": true
  }
}
```

SSE is the preferred default for ordered server-to-Workshop optimizer events because it
is simple, works naturally with replay followed by live tailing, and has `id` plus
`Last-Event-ID` semantics. WebSocket is first-class when a provider needs bidirectional
session behavior, high-frequency updates, multiplexed runs, slot/tunnel coordination, or
explicit delivery acknowledgements.

The behavioral contract is the same for both:

1. Workshop requests durable events after its last committed sequence.
2. Workshop validates run identity, algorithm identity, schema, and monotonic sequence.
3. Workshop persists events, cursor, projections, and usage before publishing progress.
4. The live transport starts at the next sequence and overlaps safely with backfill.
5. Duplicate delivery is ignored by `(optimizer_run_id, sequence_number)` or stable
   `event_id`.
6. Gaps, out-of-order events, malformed events, heartbeat loss, and reconnect are visible
   operational states.
7. After reconnect, Workshop backfills from its last committed sequence before accepting
   the connection as live again.

For SSE, the provider emits stable event IDs and supports `Last-Event-ID` or an equivalent
`after_seq` query. For WebSocket, the opening handshake includes run ID, contract version,
and `after_seq`; the provider responds with an accepted cursor and heartbeat policy.
WebSocket messages use the same `optimizer_event.v1` envelope rather than a second event
schema.

Control operations such as create, cancel, pause, resume, artifact access, and state
reads remain authenticated request/response APIs by default. A WebSocket may carry
control messages only when the capability manifest explicitly advertises them and
Workshop enforces the same authorization, idempotency, and receipt semantics.

The renderer and visual templates never receive provider credentials, remote URLs, or
raw sockets. One Rust-owned provider adapter handles HTTP backfill plus SSE or WebSocket,
then publishes committed cursor updates through the normal Workshop bridge. This keeps
local sidecars, hosted GELO, and future transports interchangeable at the visual layer.

The initial GELO template family should include:

| Template | Purpose |
| --- | --- |
| `optimizer.gelo.live.v1` | Round/tick phase, active lanes, budgets, provider and slot health |
| `optimizer.gelo.themes.v1` | Theme lifecycle, evidence checkpoints, saturation, and hill-climb progress |
| `optimizer.gelo.frontier.v1` | Full-scope frontier, champion versus `best_base`, and actual cost/wall time |
| `optimizer.gelo.promotion.v1` | Paired-seed promotion evidence, margin, majority rule, and retained achievements |
| `optimizer.gelo.data-engine.v1` | Checkpoint mining, tentative/final themes, plugins, and produced training artifacts |

User-facing copy calls the product GELO; contracts retain `algorithm_id: "go-ex"`.
Templates must preserve GELO's distinct rulers: theme frontier, global frontier,
`champion`, and `best_base` are not interchangeable. Held-out results remain
measurement-only and must not appear as search guidance.

The current hosted path already provides useful foundations: the Synth backend exposes
run submission, bounded event backfill, SSE, state batches, and artifact projection; the
Desktop mirrors cloud runs and normalizes hosted Go-Ex events. The remaining gaps are:

1. Desktop's cloud client currently consumes bounded NDJSON backfill rather than keeping
   a durable hosted SSE or WebSocket subscription.
2. Hosted events need the same commit-then-publish path as local sidecar events.
3. The current GELO overlay shows only a small phase board and theme list; it does not yet
   represent GELO's lanes, rulers, promotion evidence, plugin artifacts, or slot health.
4. Algorithm, template, and live-transport capability negotiation is not yet
   provider-neutral.
5. Viewer readiness must cover the remote subscription cursor before starting a hosted
   paid run, just as it covers the local sidecar subscription.

## Hosted standalone SFT and checkpoint-rollout visuals

The SFT design has two intentionally distinct product identities:

1. `goex.sft.v1` is a plugin lane inside a GELO run. It selects GELO traces/checkpoints,
   materializes an SFT dataset and Tinker job/result, creates a sampler and policy bundle,
   and returns an SFT candidate to GELO. It remains `algorithm_id: "go-ex"` and must not
   be presented as a standalone SFT optimizer.
2. Hosted standalone SFT is a future first-class `algorithm_id: "sft"` run in
   `optimizers-beta`, backed initially by Tinker through a provider-neutral `SftBackend`.
   It owns training lifecycle, immutable checkpoints, checkpoint evaluation campaigns,
   promotion, model materialization, and its dedicated SFT visual family.

Both may reuse provider adapters and SFT visual primitives, but they do not share an
orchestration state machine. A GELO SFT plugin can open a scoped child SFT visual bound to
`optimizer_run_id + plugin_work_id`; a standalone SFT visual binds directly to its SFT
optimizer run.

### OpenAI-compatible baseline and Synth extension

The compatibility baseline includes a fine-tuning job record, lifecycle status and
errors, base and output model identity, training and validation files, hyperparameters,
trained tokens, timestamped message/metric events, and immutable checkpoints with step
number and training/validation metrics. The canonical Synth optimizer contract is a
superset: it additionally models checkpoint evaluation campaigns, live rollout lanes,
selection versus held-out roles, trace evidence, promotion decisions, provider compute,
cost, and model lineage.

An OpenAI-compatible API remains an adapter over the canonical SFT optimizer run, not a
second database or event authority:

```text
FineTuningJob                 <- run.summary + sft.training
FineTuningJobEvent            <- lifecycle + message/metric optimizer events
FineTuningJobCheckpoint       <- sft.checkpoints
GET job events/checkpoints    <- cursor replay over the canonical run

Synth-only extension
  checkpoint evaluation campaigns
  checkpoint rollout streams and traces
  promotion evidence
  dataset role and leakage controls
  compute, usage, cost, and execution bindings
```

### Identity required for live checkpoint rollouts

Every relevant event must carry or resolve these stable identities:

| Identity | Purpose |
| --- | --- |
| `optimizer_run_id` | One canonical standalone SFT run or parent GELO run |
| `training_job_id` | Provider-neutral training operation |
| `provider_job_ref` | Opaque Tinker/provider identity retained only in trusted state |
| `checkpoint_id` | Stable Synth checkpoint identity |
| `checkpoint_digest` | Immutable checkpoint content/version proof |
| `evaluation_id` | One checkpoint evaluation campaign |
| `baseline_checkpoint_id` | Exact parent/baseline used for comparison |
| `split_role` | `train`, `selection`, or `heldout` |
| `example_id` and `seed` | Exact evaluation case identity |
| `rollout_id` | Exact checkpoint rollout identity |
| `trace_id` and digest | Durable step-level evidence |
| `dataset_digest` | Exact split contents used by training or evaluation |
| `evaluator_version` | Grader/harness identity |
| `policy_ref` | Base, adapter, sampler, and inference configuration |

A metric without checkpoint, split role, denominator, and evaluator identity is not
sufficient promotion evidence. A rollout without checkpoint and evaluation identity
must not appear in the checkpoint viewer.

### Canonical SFT event vocabulary

Keep the shared `optimizer_event.v1` envelope and standardize the following SFT events.
All events use one monotonic sequence per optimizer run, including training and rollout
events, so historical scrub reconstructs a causally consistent screen.

```text
optimizer.run.created

sft.dataset.validation.started
sft.dataset.validation.case_completed
sft.dataset.validated | rejected

sft.training.queued | started | paused | resumed
sft.training.metrics
sft.training.epoch_completed
sft.training.completed | failed | cancelled

sft.checkpoint.materializing
sft.checkpoint.created
sft.checkpoint.ready

sft.checkpoint_evaluation.allocated
sft.checkpoint_evaluation.queued
sft.checkpoint_evaluation.started
sft.checkpoint_rollout.allocated
sft.checkpoint_rollout.started
sft.checkpoint_rollout.progress
sft.checkpoint_rollout.completed | failed | cancelled
sft.checkpoint_evaluation.completed | failed | cancelled

sft.checkpoint.promotion_evaluated
sft.checkpoint.promoted | rejected
sft.heldout_evaluation.started | completed | failed
sft.model.materializing | materialized | failed

optimizer.artifact.created
optimizer.run.completed | failed | cancelled
```

Use `sft.training.metrics`, not parallel event names per provider. A metrics event contains
one aligned observation:

```json
{
  "schema_version": "optimizer_event.v1",
  "event_id": "sft_123:418",
  "sequence_number": 418,
  "occurred_at": "2026-08-12T15:04:22.183Z",
  "optimizer_run_id": "sft_123",
  "algorithm_id": "sft",
  "type": "sft.training.metrics",
  "item": {
    "kind": "training_job",
    "id": "train_123",
    "status": "running"
  },
  "delta": {
    "global_step": 1200,
    "epoch": 1.75,
    "train_loss": 0.431,
    "validation_loss": 0.508,
    "train_token_accuracy": 0.891,
    "validation_token_accuracy": 0.862,
    "learning_rate": 0.000012,
    "tokens_per_second": 18420,
    "trained_tokens": 9812345,
    "gradient_norm": 0.83
  },
  "usage_delta": {
    "trained_tokens": 16384,
    "wall_time_ms": 902,
    "cost_usd": 0.0412
  }
}
```

Missing metrics stay missing. Producers must not emit synthetic zeroes for unavailable
validation loss, throughput, cost, or utilization.

A checkpoint-rollout completion binds all evaluation evidence:

```json
{
  "schema_version": "optimizer_event.v1",
  "event_id": "sft_123:527",
  "sequence_number": 527,
  "occurred_at": "2026-08-12T15:09:03.441Z",
  "optimizer_run_id": "sft_123",
  "algorithm_id": "sft",
  "type": "sft.checkpoint_rollout.completed",
  "item": {
    "kind": "rollout",
    "id": "rollout_ckpt1200_seed501",
    "status": "completed"
  },
  "delta": {
    "training_job_id": "train_123",
    "checkpoint_id": "ckpt_1200",
    "checkpoint_digest": "sha256:...",
    "baseline_checkpoint_id": "base_model",
    "evaluation_id": "eval_ckpt1200_selection",
    "split_role": "selection",
    "example_id": "banking77_501",
    "seed": 501,
    "score": 1.0,
    "metric": "correct",
    "reward": 1.0,
    "latency_ms": 284,
    "trace_id": "trace_...",
    "trace_digest": "sha256:..."
  },
  "usage_delta": {
    "rollouts": 1,
    "prompt_tokens": 84,
    "completion_tokens": 3,
    "cost_usd": 0.0004
  },
  "artifact_refs": []
}
```

High-frequency environment steps and frames stay in the correlated child
`evals.event-stream.v1` or Trace V5 evidence. The optimizer stream carries rollout
lifecycle, bounded progress, aggregates, and exact links. Workshop may multiplex the
selected rollout's child stream into the viewer, but it must not copy every frame or tool
event into the optimizer event log.

### State slices for a stable, beautiful viewer

The current parallel-array training-curve projection is not sufficient because missing
values can misalign steps and metrics. Replace it with cursor-addressed records:

| Slice | Required contents |
| --- | --- |
| `sft.training.v2` | status, phase, progress denominators, aligned metric observations, ETA provenance |
| `sft.checkpoints.v2` | immutable checkpoints, step, digest, provider readiness, metrics, eval and promotion status |
| `sft.checkpoint_evaluations.v2` | campaigns, checkpoint/baseline, role, progress, aggregates, uncertainty, failures |
| `sft.checkpoint_rollouts.v1` | bounded per-rollout lane state, case identity, score/reward, trace/frame availability |
| `sft.dataset.v2` | split roles, counts, digests, schema, validation and rejection summary, leakage checks |
| `sft.compute.v1` | backend, hardware, queue/running status, utilization provenance, throughput, spend |
| `sft.examples.v2` | baseline/checkpoint paired examples with visibility and redaction status |
| `sft.lineage.v1` | base model, adapter, checkpoint, sampler/inference artifact, promotion ancestry |
| `sft.promotion.v1` | declared rule, eligible checkpoints, evidence coverage, decision and reason |

Metric observations are objects, not separate arrays:

```json
{
  "cursor_seq": 527,
  "observations": [
    {
      "global_step": 1200,
      "occurred_at": "2026-08-12T15:04:22.183Z",
      "train_loss": 0.431,
      "validation_loss": 0.508,
      "learning_rate": 0.000012,
      "tokens_per_second": 18420
    }
  ]
}
```

Every slice is an absolute projection at a declared optimizer cursor. A template can
load a batch of slices at cursor N and then append events N+1 onward without mixing
timestamps or showing a checkpoint before its creation event.

### First-class SFT template family

The desired visual quality requires a family rather than one long generic overlay:

| Template | Primary interaction |
| --- | --- |
| `optimizer.sft.live.v1` | Job status, aligned training curves, latest checkpoint, live eval campaigns, compute and cost |
| `optimizer.sft.checkpoints.v1` | Checkpoint rail/table, immutable metrics, selection scores, promotion, lineage |
| `optimizer.sft.rollouts.v1` | Live checkpoint evaluation lanes with checkpoint/baseline pairing and trace drill-down |
| `optimizer.sft.examples.v1` | Paired baseline versus checkpoint outputs, scores, categories, failures, redaction |
| `optimizer.sft.dataset.v1` | Split roles, validation, filters/rejections, digests, schema, leakage safeguards |
| `optimizer.sft.lineage.v1` | Base model to adapter/checkpoint to sampler/deployable model and artifact graph |

`optimizer.sft.live.v1` should match the useful clarity of a mature fine-tuning viewer:

- persistent job identity, status, model, method, files/dataset, hyperparameters, tokens,
  cost, timestamps, controls, errors, and event activity;
- directly labeled train/validation loss and token-accuracy curves with aligned steps;
- a checkpoint rail synchronized with the plots and historical cursor;
- visible provider/compute and connection health without exposing secrets;
- durable result files, checkpoints, and model identity.

Synth's distinctive extension is the checkpoint-rollout layer:

- evaluation campaigns appear as soon as a checkpoint is ready;
- each checkpoint shows true completed/total rollout progress;
- baseline and checkpoint rollouts are paired by example/seed when the policy requires it;
- active lanes show task-appropriate state: classification preview for Banking77,
  frames/actions/reward for Craftax, or generic trace activity otherwise;
- failures and incomplete evidence remain in the denominator;
- aggregate score, category slices, confusion evidence, latency, usage, and cost update
  only from completed authoritative cases;
- selecting a case opens its exact child eval/Trace V5 evidence;
- promotion status is visually separate from checkpoint creation and provider readiness.

Scrubbing to cursor N rewinds curves, checkpoints, campaigns, rollout lanes, examples,
usage, and promotion together. New live events remain buffered until the user returns to
latest. This is why one optimizer-run sequence and cursor-addressed absolute slices are
required even when training metrics and rollouts arrive from different providers.

### Streaming, coalescing, and retention

SSE and WebSocket carry the same SFT vocabulary. Training metrics may be coalesced for
high-rate providers, but the retained observation must include min/max/last or an
equivalent honest aggregation interval; it cannot silently drop spikes. The following
events are never coalesced away:

- lifecycle and error transitions;
- checkpoint materialized/created/ready events;
- evaluation campaign allocation and terminal events;
- rollout terminal events;
- promotion decisions;
- held-out evaluation terminal events;
- model and artifact materialization.

Frames, large examples, confusion matrices, result files, and model artifacts are stored
as authorized durable references with size, digest, media type, and visibility metadata.
Events carry references and concise summaries, not large payloads or signed provider
URLs.

### Current implementation gap, stated precisely

The current contracts demonstrate the intended shape but do not yet satisfy this viewer:

1. The GELO `goex.sft.v1` plugin currently materializes selected traces, a dataset, a
   Tinker job/result, sampler configuration, policy bundle, and rolloutable candidate. It
   is artifact-oriented and does not yet emit the complete live training/checkpoint/eval
   vocabulary above.
2. The existing Workshop local SFT smoke emits teacher-rollout completion, dataset
   validation, sparse training-step messages, terminal evaluation summaries, and one
   checkpoint artifact. It does not prove multiple checkpoints with concurrent live
   checkpoint rollout campaigns.
3. Workshop's current `sft.training_curves.v1` uses parallel arrays and the visual draws
   unlabeled point clouds with hard-coded scaling. It cannot safely represent sparse or
   provider-varying metrics.
4. The current checkpoint evaluation projection stores completed evaluations but not
   evaluation campaign lifecycle, progress denominators, rollout lanes, paired baseline
   identity, or evidence coverage.
5. `algorithm_id: "go-ex"` currently projects only GELO board/themes/data-engine slices;
   scoped SFT plugin state needs namespaced `go-ex.plugins.sft.*` slices and child visuals.
6. Hosted standalone `algorithm_id: "sft"` still needs to be registered in
   `optimizers-beta` with a streaming Tinker adapter and the provider-neutral contracts.

### SFT implementation and acceptance order

1. Freeze the SFT identities, vocabulary, aligned metric observation, and v2 state-slice
   schemas.
2. Extend the Tinker adapter to stream normalized training metrics, checkpoint lifecycle,
   provider readiness, and durable job errors.
3. Implement checkpoint evaluation campaigns that allocate stable rollout identities
   before execution and correlate child eval/Trace V5 evidence.
4. Implement the Workshop SSE/WebSocket mirror, commit-before-publish behavior, and batch
   projection recovery for SFT.
5. Build `optimizer.sft.live.v1` and `optimizer.sft.checkpoints.v1` against a captured real
   multi-checkpoint hosted run.
6. Build `optimizer.sft.rollouts.v1` against real concurrent checkpoint evaluation
   rollouts, including one failure and one reconnect.
7. Add examples, dataset, and lineage templates over the same cursor and evidence graph.
8. Add scoped GELO SFT-plugin projections and reuse the SFT components without presenting
   the plugin lane as standalone SFT.
9. Prove pause/resume/cancel, provider loss, slot loss, replay after restart, promotion,
   held-out isolation, terminal model identity, and retained visuals through independent
   CUA.

The SFT viewer is ready only when a real hosted multi-checkpoint job can be opened before
training, stream aligned metrics and checkpoint rollout progress, scrub historically,
drill into exact rollout traces, select/promote from declared selection evidence, keep
held-out measurement isolated, and reopen entirely from durable Workshop state after the
training provider and eval slots are gone.

## Product promise

Given an approved GEPA recipe or existing run, Workshop can:

1. Resolve an installed compatible sidecar version or hosted provider and fixed limits.
2. Allocate or discover the stable optimizer run identity without starting paid work.
3. Create and open the visual against the durable run binding.
4. Prove the viewer is subscribed at a known cursor.
5. Start or attach to the selected GEPA sidecar only after readiness.
6. Show real proposal, evaluation, selection, frontier, reflection, usage, and failure
   events as they occur.
7. Link every candidate evaluation to its exact eval runs, examples, rollouts, and traces.
8. Preserve historical scrub while new live events continue to arrive.
9. Reopen the terminal visual entirely from durable replay and artifacts.

Headless execution may remain an explicit mode, but it must not be reported as a
connect-before-execution live run.

## Contract boundary

Keep `optimizer_event.v1` as the optimizer-level contract. Do not recast GEPA as an eval
stream. Keep `evals.event-stream.v1` as the child evaluation contract. Join them with
stable references.

The minimum optimizer envelope remains:

```json
{
  "schema_version": "optimizer_event.v1",
  "event_id": "opt_123:42",
  "sequence_number": 42,
  "occurred_at": "2026-08-12T12:34:56.789Z",
  "optimizer_run_id": "opt_123",
  "algorithm_id": "gepa",
  "type": "gepa.candidate.evaluation.progress",
  "item": {
    "kind": "candidate",
    "id": "cand_7",
    "status": "evaluating"
  },
  "delta": {
    "generation": 2,
    "completed_examples": 12,
    "total_examples": 20,
    "eval_run_ids": ["eval_train_cand_7"]
  },
  "usage_delta": {
    "rollouts": 1,
    "prompt_tokens": 812,
    "completion_tokens": 41,
    "cost_usd": 0.0031
  },
  "artifact_refs": [],
  "raw": {}
}
```

GEPA needs an explicit, documented vocabulary rather than substring matching on event
names:

| Family | Required information |
| --- | --- |
| Lifecycle | allocated, waiting-for-viewer, queued, starting, running, terminal |
| Phase | seeding, proposing, reflecting, evaluating-train, selecting, measuring-held-out |
| Candidate | immutable ID, parent IDs, generation, proposal status, materialized-value digest |
| Evaluation | candidate ID, split role, eval run IDs, completed/total examples, score status |
| Score | metric name, value, denominator, uncertainty only when actually computed |
| Selection | accepted/rejected, reason code, selection inputs, incumbent relationship |
| Frontier | absolute snapshot with candidate IDs and named dimensions |
| Reflection | safe concise artifact or summary; no private chain-of-thought |
| Usage | proposer versus evaluator calls, tokens, rollouts, latency, metered cost provenance |
| Budget | declared ceilings, consumed values, remaining values, stop reason |
| Evidence | prompt/candidate artifact, result manifest, child traces, visibility/completeness |
| Operations | heartbeat, reconnect, retry, warning, malformed event, worker loss |

Events must distinguish selection data from measurement-only held-out data. A held-out
score must never appear early enough to influence candidate selection unless the recipe
explicitly declares that role.

## Dedicated GEPA reference visual

The first prototype becomes the interaction reference for
`optimizer.gepa.live.v1`. It should optimize for answering four questions quickly:

1. What is GEPA doing now?
2. Is it making measurable progress against the declared objective?
3. Which candidate became better, why was it selected, and what did it cost?
4. Can I inspect the exact evaluation evidence without losing the optimizer context?

### Persistent run strip

- run ID, objective, source, recipe, algorithm version, and connection state;
- current phase and generation;
- candidates evaluated out of the true planned or currently known total;
- incumbent score and uplift from the declared baseline;
- rollouts, proposer/evaluator calls, tokens, precise cost, and budget ceilings;
- last real event time and current durable cursor.

Unknown totals remain unknown. The visual must not animate elapsed time as optimizer
progress.

### Main progress plot

Plot candidate score by completed evaluation work, with one point per actual scored
candidate. Encode candidate status by shape and frontier membership by a stable accent.
Show train/selection and held-out measurement as separate series or facets. Never connect
unrelated candidates as if they were samples from one continuous process; lineage edges
may be shown separately when parent relationships are known.

The plot should make the incumbent step function visible: a new point does not become the
incumbent merely because it was evaluated.

### Candidate work lanes

Each active or terminal candidate shows:

- candidate and parent IDs;
- proposed, queued, evaluating, scored, accepted, rejected, failed, or incomplete state;
- true examples/rollouts completed out of total;
- current train/selection score and eligible held-out measurement;
- cumulative cost and retry/failure state;
- child eval run and trace coverage.

Parallel candidates remain independent lanes. Failed candidates remain visible and are
not dropped from denominators.

### Selected candidate

- materialized prompt/program values or an explicit unavailable state;
- diff from the selected parent or baseline;
- metric values with names, split roles, denominators, and measurement timing;
- selection reason code and frontier membership;
- reflection/proposer artifact when safe and explicitly emitted;
- exact child eval, example, rollout, trace, and output artifact links.

### Historical scrub and follow-live

Scrubbing freezes the projection at an optimizer cursor while ingestion continues. The
control shows how many newer events are buffered and can return to the latest cursor.
Selection is cursor-aware: a candidate or score that did not yet exist at that cursor
must not leak backward into the historical view.

## Connect-before-execution lifecycle

The GEPA lifecycle should be:

1. `prepare`: validate recipe/provider, secrets by name, dataset/program capabilities,
   limits, and spend ceiling; allocate `optimizer_run_id` in `waiting_for_viewer`.
2. `open_visual`: create or resolve the visual and bind only the optimizer run ID.
3. `await_ready`: renderer confirms it loaded the run, replayed through cursor N, and
   subscribed for N+1.
4. `start`: `OptimizerManager` starts or attaches to the selected sidecar version, or
   Workshop instructs Cloud to begin.
5. `watch`: durable backfill plus live tail, de-duplicated by run and sequence.
6. `finalize`: persist terminal status, outputs, usage, child evidence coverage, and a
   correlation receipt before acknowledging completion.

The current `start_recipe(open_visual=true)` ordering opens the visual before spawning
the child, which is a useful foundation to preserve while moving execution into the
sidecar. It needs a real readiness acknowledgement between those steps rather than
treating `visuals.show` as proof that the renderer has subscribed.

## Live transport and Desktop projection

For the installed Desktop app, prefer the existing Rust service and Tauri event path over
exposing a new loopback SSE endpoint solely for the renderer:

```text
Optimizer sidecar stream / Cloud event API
  -> Rust normalize + validate
  -> one SQLite transaction: events + cursor + slices + run
  -> publish optimizer.run.updated(run_id, cursor)
  -> renderer requests events after its last cursor
  -> shared reducer projects the selected cursor
```

For the standalone reference prototypes, expose the same normalized feed through thin
loopback replay-plus-SSE and replay-plus-WebSocket bridges. This lets plain HTML
prototypes exercise readiness, resume cursors, reconnect, malformed events, and
replay/live overlap without making either transport the Desktop's permanent internal
transport.

Do not reload the full event history after every update. The visual should request
`events_after(last_cursor)`, append after validation and de-duplication, and occasionally
replace its state from a cursor-addressed projection to verify convergence.

## Immediate implementation gaps

1. Background recipe ingestion stores `optimizer.run.updated` in the journal but does not
   publish that event to the renderer. Give the worker a host-owned notification path or
   a runtime subscription that emits only after the transaction commits.
2. `VisualHost` reloads `eventsAfter(run, 0)` on every app event. Switch to incremental
   reads from the last accepted cursor, with a bounded snapshot/recovery path.
3. `VisualChrome.live` currently derives from run status plus the local follow-live
   toggle. Add explicit connection states: loading, replaying, subscribed, stale,
   reconnecting, terminal, and failed.
4. Add visual readiness and subscription cursor acknowledgement before paid execution.
5. Replace GEPA event-name substring matching with a documented normalized vocabulary and
   schema tests.
6. Add candidate-to-child-eval relationships and trace coverage to the contract and
   projections.
7. Separate proposer usage, candidate evaluation usage, and held-out measurement usage.
8. Preserve missing metrics as missing rather than displaying `$0.00`, `0` rollouts, or a
   zero score before the producer has reported them.

## Prototype program

### Prototype A: captured real GEPA replay

Run the bounded Banking77 recipe once with explicit compute approval. Capture its native
event feed, normalized `optimizer_event.v1` feed, fixed recipe manifest, and artifact
digests. Use that capture for deterministic replay and visual regression. Fixtures may
exercise edge cases, but only the captured run establishes producer compatibility.

### Prototype B: replay followed by live tail

Open the visual before starting a second approved bounded run. Verify readiness at cursor
zero, candidate/evaluation updates during execution, reconnect with `Last-Event-ID`, no
duplicates, honest failure visibility, and terminal artifact attachment.

### Prototype C: concurrent candidate evaluation

Use a bounded recipe that actually evaluates more than one candidate concurrently. Prove
independent lane progress, candidate/eval identity isolation, frontier updates, and
correct budget aggregation.

## Prototype exit criteria

- visual readiness precedes the first proposer or evaluator model call;
- the viewer receives more than one real intermediate update;
- every event matches the selected optimizer run and monotonic cursor;
- every scored candidate links to the exact eval run(s) that produced the score;
- replay/live overlap and reconnect create no duplicate candidates, points, or usage;
- selection and measurement-only held-out results are visibly distinct;
- missing, failed, rejected, and incomplete are distinct from zero or scored;
- parallel candidate lanes remain independently legible;
- declared ceilings and actual proposer/evaluator usage are both visible;
- historical scrub never leaks later candidates or values backward;
- terminal replay works without the original GEPA process or worker;
- narrow pane and expanded layouts pass keyboard, contrast, text expansion, and reduced
  motion checks.

## Proposed host and MCP shape

Extend the existing optimizer service rather than add a separate live-optimizer MCP:

```text
optimizer_manage(operation, arguments)

operations:
  prepare        -> run identity, fixed recipe/provider, bounds, initial cursor
  open_visual    -> visual identity
  await_ready    -> replayed cursor and subscribed state
  start          -> execution binding and enforced-limit receipt
  watch_run      -> events after cursor, current cursor, terminal flag
  get_state      -> cursor-addressed shared or GEPA slice
  cancel         -> bounded cancellation result
  finalize       -> outputs, usage, child evidence coverage, correlation receipt
```

Keep explicit user approval before `start`. Import, reconcile, inspect, replay, and open
visual remain non-compute operations.

## Engineering sequence

1. Define the signed sidecar capability, lifecycle, health, and version-selection
   contracts.
2. Define the signed algorithm visual-package manifest, registry installation, retention,
   and compatibility contracts.
3. Add an `OptimizerManager` parallel to `LagunaManager`; move local GEPA recipe execution
   behind the managed sidecar boundary.
4. Fix committed-event notification and incremental renderer consumption.
5. Add readiness state without changing optimizer identity.
6. Freeze the GEPA normalized vocabulary and candidate/eval relationship schema.
7. Build `optimizer.gepa.live.v1` against a captured real stream, using shared Workshop
   optimizer visual primitives.
8. Add the frontier, candidate, and evaluations templates when the live prototype proves
   their distinct interaction value.
9. Run a connect-before-execution bounded Banking77 GEPA smoke after approval.
10. Package the same sidecar under the optional Compose profile and verify native/Compose
    contract parity.
11. Add the provider-neutral HTTP-backfill plus SSE/WebSocket live adapter and capability
    negotiation path.
12. Build the GELO template family against a captured real hosted run, then prove one
    connect-before-execution hosted run and an optional local-slot lease.
13. Validate restart/replay, sidecar or remote-stream loss, sidecar and template version
    rollback, retained visual replay, cost ceilings, narrow layout, and independent CUA.

## Decisions established by this proposal

- Optimizers is a first-class Workshop domain whose local execution is supplied by a
  modular, versioned sidecar.
- Workshop manages the sidecar as a first-class runtime but does not require it to be
  installed or always running.
- The Rust `OptimizerService` remains the Workshop authority for the durable product
  projection; `OptimizerManager` owns sidecar installation, versions, and lifecycle.
- The Compose GEPA service is an opt-in packaging of the same sidecar contract.
- Each algorithm has a concomitant signed visual template family registered first-class
  in Workshop; the shared layer supplies primitives and lifecycle rather than one generic
  optimizer dashboard.
- Local sidecars and hosted providers share the Workshop run/event/slice/artifact/visual
  projection contract; runtime lifecycle operations apply only to local sidecars.
- Providers may offer SSE, WebSocket, or both for live delivery; durable cursor backfill
  and one `optimizer_event.v1` envelope are required underneath every transport.
- Hosted GELO is accessed through the Synth backend, retains its cloud run identity, and
  can be visualized without installing a local optimizer sidecar.
- `goex.sft.v1` remains a nested GELO plugin lane; it must not be presented as the future
  standalone SFT optimizer.
- Hosted standalone SFT registers `algorithm_id: "sft"` and uses aligned metric records,
  stable checkpoint/evaluation/rollout identities, split-role enforcement, and correlated
  child eval streams to support live checkpoint-rollout inspection.
- The standalone SFT visual family starts with `optimizer.sft.live.v1` and preserves the
  same durable evidence for checkpoint comparison, rollout drill-down, promotion, held-out
  measurement, and historical replay.
- Sidecar removal preserves the exact template revisions required by historical visuals.
- `optimizer_event.v1` remains the optimizer contract; child evals remain
  `evals.event-stream.v1` and are joined by identity.
- The first prototype establishes `optimizer.gepa.live.v1`; additional GEPA views remain
  separate registered templates over the same durable run and reducer.
- Viewer readiness is required before paid live runs; headless runs are explicit.

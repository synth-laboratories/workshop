# Execution ontology for Containers, Optimizers, Evals, and Workshop

**Status:** Tentative architecture for engineering review; not yet a wire contract

**Master plan:** `execution_platform_master_plan.md`

**Related:** `execution_stream_contracts.md`, `live_evals.md`,
`live_optimizers_gepa.md`

## Recommendation

Keep `Task` and `TaskInstance` near the center of the existing Containers model, but
stop using `container`, `runtime`, `execution`, `outcome`, or `rollout` as catch-all
nouns. The shared model should distinguish five planes:

1. **Content** — benchmark, dataset, world, task, rubric, harness, policy, and their
   immutable revisions.
2. **Deployment** — providers, logical services, service instances, bindings, leases,
   and physical containers or processes.
3. **Execution** — eval runs, attempts, rollouts, environment episodes, policy sessions,
   optimizer runs, and candidate evaluations.
4. **State and evidence** — workspaces, snapshots, checkpoints, traces, artifacts,
   events, usage, rewards, grades, and integrity reviews.
5. **Projection** — Workshop mirrors, relationships, state slices, visuals, and indexes.

The most consequential design choice is:

> A container is a packaging and isolation mechanism, not the semantic owner of a
> task, world, policy, rollout, reward, or trace.

`synth-containers` itself is nevertheless an opinionated executable product surface,
not merely a neutral schema library. It should fold supported task formats into one
coherent task/runtime/rollout interface, ship the operational defaults, and make the
supported compatibility path easy to build, run, inspect, stream, and score.

The only committed first-class external fold is **Harbor**. A Harbor task or dataset
should be straightforward to package and expose as a Synth container without rewriting
its instruction, environment, tests, solution, native result, or verifier evidence.
Archipelago and other framework mappings are useful research inputs. OpenEnv and Prime
Intellect Verifiers are explicit compatibility targets, but remain aspirational until
they have an owned adapter,
conformance suite, fixtures, documentation, and support commitment.

GameBench is benchmark content—a benchmark suite/dataset with task-specific engines,
policies, seed suites, and scorers—not a task format or container compatibility layer.
GameBench tasks are executed by Evals through Containers and may be packaged in Harbor
format through the first-class Harbor fold. The selected Evals lane does not change
GameBench's identity or make it a provider protocol.

Containers is the common execution substrate beneath Evals: both Harbor evaluations and
Evals-private orchestration should resolve tasks, launch services, stream evidence, and
control attempts through Containers contracts. Private Evals runner names and APIs stay
inside the Evals repository and must not leak into Workshop, Containers, Optimizers, or
other public contracts.

A single physical container may initially host the environment, policy harness, and
relay. They must still advertise separate logical service identities and lifecycles.
That lets a later deployment split them across processes, containers, local sidecars,
or hosted providers without changing the task or evidence model.

## Why the current ontology is insufficient

Containers already has useful primitives: `Runtime`, `Actor`, `Action`, `Observation`,
`State`, `Execution`, `Outcome`, `TaskInstance`, capability fidelity, checkpoints,
resume, fork, traces, and typed task discovery. Evals already has a stronger lifecycle:
`run -> score -> save evidence -> index`, with rig failures distinct from agent
failures. Optimizers already separates an optimizer run from child evaluations and has
cursor-addressed event/state records.

The gaps are semantic:

- `Runtime` can mean an environment engine, Codex session, MCP world, harness, or whole
  provider process.
- `Execution` groups rollout, session, episode, and eval run without defining their
  containment or identity rules.
- `Outcome` groups reward, score, grade, verifier result, and pass/fail even though they
  have different authorities and aggregation rules.
- `TaskContract.container_profile` makes deployment appear intrinsic to the task.
- `verifier_source_policy = "container"` conflates packaging with scoring authority.
- generic rollout SSE/WS can repeatedly publish state snapshots without proving a
  durable semantic partial-event log.
- missing reward, score, or usage must not become observed zero.
- reconnect, restart, restore, retry, branch, replay, and rescore are different actions.

## What the reviewed systems require

| System | Native content and execution | Evaluation | Ontology pressure |
| --- | --- | --- | --- |
| APEX Agents / Archipelago | world snapshot, task, criteria, MCP apps, populated environment, agent trajectory | separate grading over before/after snapshot diff and selected artifacts | first-class world, workspace snapshot, tool gateway, and post-run grading |
| TaxCalcBench | tax-year edition, case, source documents, structured tax return | deterministic strict/lenient return and line comparisons | hierarchical metrics and edition rules without requiring an interactive world |
| Harvey LAB | practice area, workflow, scenario, documents, deliverables, closed workspace | independent LLM judge; all-pass result plus criterion diagnostics | deliverable and criterion-to-artifact scope are first class |
| Crosby RedlineBench | negotiation scenario, party, turn, branch/input group, contract revision, playbook | validity gate, rubric panel, behavioral metrics, turn-weighted aggregation | role, turn, branch, document lineage, panel grades, and grouped aggregation |
| Craftax Rust | task, world/rules/readout profiles, seed, engine/RNG state, policy loop | environment reward plus declared terminal evaluation | true checkpoints, step events, frames, rewards, achievements, and policy-call correlation |
| Harbor + Terminal-Bench | dataset, task, instruction, environment, agent, trial, job | verifier/test script after the agent exits | task, agent sandbox, trial, job, and verifier execution are distinct |
| Harbor + TBLite | calibrated Harbor task dataset | same task verifiers, iteration-oriented aggregate | calibration/selection is a dataset revision, not a runtime kind |
| Evals + GameBench code-policy DEO | GameBench benchmark/dataset task, recipe, baseline/candidate policies, seed suites, child game rollouts | candidate scores, held-out scores, baseline delta, improvement gate | dataset content remains independent of the private Evals or Harbor lane; both execute through Containers, while optimizer, child rollout, and verifier identities stay separate |
| PostTrainBench | base model, rules, budget, training workspace, datasets, scripts, checkpoints, `final_model` | downstream benchmarks plus independent integrity judges | workspace history, model lineage, integrity review, and promotion verdict |

Primary references:

- [Archipelago](https://github.com/Mercor-Intelligence/archipelago) separates an
  independently runnable environment/MCP gateway, agent runner, and grading. It
  populates an initial world snapshot, runs the agent, seals a final snapshot, and then
  grades the diff and artifacts.
- [Harbor core concepts](https://www.harborframework.com/docs/core-concepts) defines a
  task as an instruction, environment, and test script; a trial is one agent execution
  on one task, and a job is a collection of trials. Its
  [task format](https://www.harborframework.com/docs/tasks/task-difference) separates
  environment, solution, and tests.
- [Terminal-Bench](https://github.com/harbor-framework/terminal-bench) is a versioned
  Harbor dataset, while
  [OpenThoughts-TBLite](https://github.com/open-thoughts/OpenThoughts-TBLite) is a
  calibrated 100-task development dataset on the same Harbor surface.
- [Harvey LAB](https://github.com/harveyai/harvey-labs/blob/main/docs/architecture.md)
  separates run, evaluate, and report. Evaluators read only criterion-relevant
  deliverables; all-pass is the verdict and criterion pass rate is diagnostic.
- [Crosby RedlineBench](https://github.com/crosbylegal/redline-bench) has 140 Harbor
  tasks across three multi-turn negotiation scenarios, multiple attorney rubric branches,
  panel judgments, document mechanics, and turn-aware aggregation.
- [TaxCalcBench](https://github.com/column-tax/tax-calc-bench) versions by tax year and
  reports strict/lenient whole-return and per-line metrics. TY25 uses realistic PDFs and
  federal plus state cases.
- [PostTrainBench](https://posttrainbench.com/) gives an agent a base model, one H100,
  and ten hours to create `final_model`, then separately performs functional evaluation
  and integrity review.

## Tentative master ontology

Use composition and discriminated records, not class inheritance on the wire:

```text
BenchmarkDefinition@revision
  contains DatasetDefinition@revision
    selects TaskDefinition@revision
      binds WorldDefinition@revision? + EvaluationPlan@revision

EvaluationRun
  contains Attempt*
    binds TaskInstance + PolicyRevision + HarnessRevision
    owns WorkspaceInstance? + EnvironmentEpisode? + PolicySession+
    produces Trace + Artifact* + UsageRecord*
    is assessed by EvaluationExecution(s)
      producing CriterionResult* + Metric* + Score* + Verdict

OptimizerRun
  owns CandidateRevision*
  launches CandidateEvaluation*
    referencing EvaluationRun/Attempt/Rollout evidence
  produces SelectionDecision* + Checkpoint* + PromotedCandidate?
```

### Content plane

Definitions are immutable, revisioned, and preferably content-addressed.

| Noun | Definition |
| --- | --- |
| `BenchmarkDefinition` | Named measurement product and aggregation methodology, such as TaxCalcBench TY25. |
| `DatasetDefinition` | Versioned task collection, split, or calibrated selection. TBLite belongs here. |
| `WorldDefinition` | Reusable initial scenario: app topology, files/data, roles, resources, rules, and population recipe. |
| `TaskDefinition` | Objective over a world/input set: instruction, capabilities, outputs, limits, evaluators, and release policy. |
| `TaskInstance` | Fully resolved task with seed, case, role, turn, branch, split, inputs, and pinned revisions. |
| `RoleDefinition` | Actor identity, permissions, private context, and viewpoint. |
| `TurnDefinition` | Stage in an ordered or branching scenario with predecessor-state requirements. |
| `HarnessDefinition` | Revisioned code/configuration mediating policy-to-environment interaction. |
| `PolicyDefinition` | Agent/policy program plus model/provider/effort/tool configuration. |
| `EvaluationPlan` | Gates, evaluators, criteria, metrics, aggregation, authority, and integrity rules. |
| `OptimizerRecipe` | Algorithm, search space, dataset/task bindings, budgets, selection, and held-out plan. |

`ScenarioDefinition` can be a profile of `WorldDefinition` for ordered, role-aware
workflows. It must not remain free-form task metadata: Crosby's input groups and rubric
branches require a turn graph.

### Deployment and service plane

| Noun | Definition |
| --- | --- |
| `ExecutionProvider` | Discoverable control plane offering typed resources, operations, and streams. |
| `DeploymentUnit` | Physical process, container, VM, pod, or hosted allocation. |
| `ServiceDefinition` | Revisioned logical service contract and implementation digest. |
| `ServiceInstance` | One running generation of a service definition. |
| `ServiceBinding` | Time-bounded association between an execution and service instance, including role/generation. |
| `Lease` | Exclusive or capacity-scoped right to use a service/resource. |
| `Endpoint` | Resolved transport address and authorization mode; never semantic identity. |

Logical service roles:

| Service | Owns | Does not own |
| --- | --- | --- |
| `EnvironmentService` | mutable world/workspace state, reset/step/tools, environment snapshots/events | model choice or benchmark aggregation |
| `PolicyService` | model calls, policy session, action proposals, policy trace/usage | authoritative environment state or benchmark grade |
| `HarnessService` | interaction loop, routing, limits, retries, correlation | environment/model/evaluator truth |
| `EvaluatorService` | one judge/scorer execution and evidence | mutation of the evaluated workspace |
| `ArtifactService` | immutable blobs, manifests, digests, retention, retrieval | semantic interpretation |
| `EventRelayService` | durable append, cursor replay, subscriptions, transport adaptation | invention of domain facts |
| `OptimizerService` | proposal/search/selection state and optimizer events | child rollout/evaluator truth |

Initially these can share one physical container. They still use distinct service
instance IDs, generations, health, and producer identity.

### Execution plane

| Noun | Definition and identity rule |
| --- | --- |
| `EvaluationRun` | Requested evaluation over a pinned benchmark/dataset plan; stable across scheduling/reconnection. |
| `Attempt` | One adjudicable try of one task instance by one policy/harness binding. Retry normally creates another attempt. |
| `Rollout` | One policy-environment interaction trajectory within an attempt; artifact tasks may have none. |
| `EnvironmentEpisode` | Continuous mutation authority over one environment/workspace lineage. |
| `PolicySession` | Continuous policy/harness conversational state; restart creates a new segment. |
| `EvaluationExecution` | One evaluator/judge invocation over sealed evidence; rejudge creates another. |
| `OptimizerRun` | Search/training process under a pinned recipe and budget. |
| `CandidateRevision` | Immutable prompt, policy, model checkpoint, dataset recipe, or code candidate with lineage. |
| `CandidateEvaluation` | Optimizer-owned reference to an evaluation request and exact child evidence. |

Containment is represented by typed relationships, not inferred IDs or paths:

```text
evaluation_run contains attempt
attempt uses task_instance
attempt binds policy_revision
attempt contains rollout
rollout contains environment_episode
rollout contains policy_session_segment
optimizer_run proposes candidate_revision
candidate_evaluation evaluates candidate_revision
candidate_evaluation references evaluation_run
evaluation_execution assesses evidence_set
```

### State and evidence plane

| Noun | Definition |
| --- | --- |
| `WorkspaceInstance` | Mutable materialization of world/task filesystem or application state. |
| `WorkspaceSnapshot` | Immutable content-addressed capture for evidence and possibly restore. |
| `Checkpoint` | Restore-oriented capture with declared fidelity/compatibility; not automatically admissible evidence. |
| `Artifact` | Immutable output with media type, digest, producer, role, visibility, and lineage. |
| `Deliverable` | Task-expected artifact addressable by evaluators and criteria. |
| `DocumentRevision` | Artifact profile with predecessor, tracked edits/comments, actor, and turn. |
| `Trace` | Ordered actions, observations, calls, tools, and spans. |
| `EventStream` | Durable ordered facts for live observation/replay; not itself the transport. |
| `EvidenceSet` | Immutable manifest of exact traces, snapshots, artifacts, and metadata supplied to an evaluator. |
| `UsageRecord` | Metered tokens, calls, compute, time, or cost with authority and nullable values. |

Snapshot and checkpoint remain distinct. A grading snapshot may diff files but not
restore a live game. A true Craftax checkpoint restores engine/RNG/event-cursor state.
A PostTrainBench workspace snapshot is important evidence but may omit process state.
A policy-session checkpoint does not restore environment state.

### Evaluation and outcome plane

Replace abstract `Outcome` as the main public result with typed signals:

| Noun | Authority and use |
| --- | --- |
| `RewardSignal` | Scalar/vector feedback at step, episode, or sample scope; environment/proxy/reward-model authored. |
| `Achievement` | Named environment fact unlocked at a step; never an invented skill metric. |
| `CriterionResult` | One rubric/test result with judge, evidence scope, rationale, and weight. |
| `MetricObservation` | Named measure with unit, scope, aggregation, and provenance. |
| `Score` | Evaluator-authored normalized/domain-valued measure with explicit metric definition. |
| `GateResult` | Fail-closed prerequisite, validity, or integrity check. |
| `BenchmarkVerdict` | Functional task/attempt status and score aggregation. |
| `IntegrityReview` | Independent contamination, policy, identity, or evidence-completeness assessment. |
| `PromotionVerdict` | Eligibility decision combining functional, integrity, release, and evidence policy. |

Craftax step reward is a `RewardSignal`; Harbor `reward.txt` is evaluator output, not
proof the environment authored reward. Harvey criterion pass rate is diagnostic while
all-pass is the verdict. RedlineBench combines validity, criteria, behavioral metrics,
and aggregates. PostTrainBench can have a high functional score but an ineligible
promotion verdict. GameBench DEO has rollout scores, candidate aggregates, baseline
deltas, held-out scores, and a final improvement gate.

## Environment, policy, and harness split

An attempt binds all logical roles independently:

```json
{
  "attempt_id": "att_01",
  "task_instance_id": "taski_01",
  "bindings": [
    {"role": "environment", "service_instance_id": "svc_env_07", "generation": 3,
     "world_revision": "sha256:...", "workspace_id": "ws_01"},
    {"role": "policy", "service_instance_id": "svc_policy_12", "generation": 8,
     "policy_revision": "sha256:...", "policy_session_id": "ps_01"},
    {"role": "harness", "service_instance_id": "svc_harness_05", "generation": 11,
     "harness_revision": "sha256:..."}
  ]
}
```

The relay or host—not a replaceable leaf producer—owns the stable attempt stream and
global sequence. Producer events also carry service instance, producer epoch, and local
sequence.

### Restart vocabulary

| Operation | Identity effect | Rule |
| --- | --- | --- |
| `reconnect` | no service/execution change | backfill cursor, then live tail |
| `service_restart` | new service generation | continue only if role recovery permits |
| `session_resume` | new policy-session segment | same attempt only when policy and evaluator rules permit |
| `environment_restore` | new episode segment from checkpoint | same attempt only for declared true restore; record discontinuity |
| `retry` | new attempt | repeats after policy, agent, or infrastructure failure |
| `branch` | new rollout/attempt lineage | new identity with parent relationship; parent is immutable |
| `replay` | no mutation authority | consumes recorded evidence only |
| `rescore` / `rejudge` | new evaluation execution | original evidence/result remains immutable |
| `world_revision` | new content revision | never mutates an active world/workspace in place |
| `harness_revision` | new content revision | active attempt stays pinned; normally requires a new attempt |

Policy/harness restart can preserve the environment, but seals the old policy session
segment and states whether conversation state was restored, rebuilt from trace, or cold.
A cold restart cannot masquerade as seamless. Harness development may reuse a preserved
development workspace only through a new non-adjudicable attempt.

Environment restart continues the same attempt only if a compatible true checkpoint
restores all task-relevant state: workspace/world, RNG, clocks, counters, cursor, and
hidden state. A filesystem-only snapshot normally creates a branch or retry.

Evaluator restart never needs live mutation access. It reads an immutable `EvidenceSet`.
A changed judge model, prompt, rubric, or code creates a new revision/execution.

Recovery is declared per logical service, not as global booleans:

```json
{
  "role": "environment",
  "recovery": {
    "reconnect": "native",
    "restart": "checkpoint_restore",
    "checkpoint_fidelity": "true_environment_snapshot",
    "preserves": ["workspace", "rng", "step", "reward", "event_cursor"],
    "invalidates": ["open_network_connections"]
  }
}
```

Fidelity stays `native`, `derived`, `approximate`, or `unsupported`, and tests must prove
every `native`/`derived` claim.

## Streaming model

Use the shared cursor/replay rules in `execution_stream_contracts.md`, but keep typed,
linked semantic streams:

| Stream | Authority and examples |
| --- | --- |
| `evaluation.events` | orchestration, phase, attempt lifecycle, terminal verdict |
| `attempt.events` | bindings, progress, retry, failure |
| `environment.events` | reset, observation, action, state change, reward, achievement, frame |
| `policy.events` | request, partial reasoning/output, tool intent, action, usage |
| `workspace.events` | file/app-object delta and snapshot sealing |
| `trace.events` | span/item open, deltas, close, trace seal |
| `evaluation.result.events` | gates, criteria, metrics, scores, verdicts |
| `optimizer.events` | proposal, batch, selection, checkpoint, promotion |
| `operations.events` | health, restart, lease loss, backpressure, stream gaps |

Poll, SSE, and WebSocket are transports over the same durable authority and must yield
the same ordered IDs/digests. SSE is the default live tail, poll is mandatory backfill,
and WS is optional for bidirectional controls or flow control.

Trace partials use explicit open/sustain/close semantics:

```text
trace.started
span.started
item.started
item.delta*
item.completed | item.failed
span.completed | span.failed
trace.sealed
```

Images, documents, checkpoints, and weights are digest-addressed resource references,
not unbounded event payloads. Workspace changes are semantic app-object deltas when
available and filesystem deltas otherwise.

Required envelope shape:

```json
{
  "schema_version": "synth.stream-event.v1",
  "stream_id": "stream_att_01",
  "sequence": 42,
  "event_id": "evt_...",
  "occurred_at": "2026-08-12T12:34:56.789Z",
  "committed_at": "2026-08-12T12:34:56.801Z",
  "kind": "policy.item.delta",
  "subject": {"kind": "policy_session", "id": "ps_01"},
  "correlation": {"evaluation_run_id": "eval_01", "attempt_id": "att_01",
    "rollout_id": "ro_01", "task_instance_id": "taski_01"},
  "producer": {"service_instance_id": "svc_policy_12", "producer_epoch": 8,
    "producer_sequence": 19},
  "payload_schema": "synth.policy-text-delta.v1",
  "payload": {"text": "..."}
}
```

Rules:

- stable sequence is scoped to stream and assigned after durable commit;
- producer sequence is diagnostic and resets with a new epoch;
- missing sequence/reward/score/usage/cost never defaults to zero;
- reconnect backfills then tails live with bounded overlap;
- gaps, expired cursors, malformed payloads, identity mismatch, and dropped data are
  typed facts;
- terminal execution does not imply trace sealing, snapshotting, grading, or integrity
  review is complete;
- each stream declares retention, high-water behavior, limits, and authorization.

## Benchmark profiles

Profiles constrain shared nouns; they do not fork the base schema.

### `interactive_game.v1` — Craftax Rust

Requires world/task, environment episode, policy session, rollout, true checkpoint with
RNG/step/reward/cursor fidelity, action/observation/state/reward/achievement/frame events,
policy-call correlation, replay proof, and branch-from-checkpoint proof.

### `sandbox_artifact_task.v1` — Harbor, Terminal-Bench, TBLite

Requires dataset/task revision, instruction, environment, agent/harness revision,
workspace, terminal/tool trace, verifier execution, artifacts, job/attempt relationships,
and retry/pass@k aggregation. Solution and tests retain separate visibility.

### `professional_deliverable.v1` — Harvey LAB and APEX

Requires source documents/world, deliverables, closed workspace, media extraction,
criterion evidence scope, immutable final snapshot, evaluator revision, criterion results,
and aggregate verdict. APEX adds MCP/app topology, population, snapshot diff, and visual
artifact extraction.

### `negotiation_document.v1` — Crosby RedlineBench

Requires scenario, roles, turn graph, predecessor document, input group, private
grounding, tracked changes/comments, validity gate, rubric revision, judge panel,
behavioral metrics, and branch/group/turn/side/scenario aggregation.

### `structured_calculation.v1` — TaxCalcBench

Requires edition/tax year, jurisdiction, case/source documents, tool/web configuration,
structured output schema, expected values/tolerances, line metrics, and strict/lenient
return verdicts.

### `autonomous_optimization_workspace.v1` — PostTrainBench

Requires base model identity, rules/budget, long-lived workspace deltas, training jobs,
datasets, checkpoints, model lineage, final submission, downstream functional eval,
integrity reviews, and promotion verdict.

### `code_policy_optimization.v1` — GameBench DEO

Requires optimizer recipe, baseline/candidate lineage, train/scout/held-out suites,
candidate-to-child-rollout links, score distributions, selection, baseline delta,
held-out result, and improvement gate while retaining GameBench as benchmark authority
and Evals as orchestration authority. Both private Evals execution and Harbor packaging
use Containers as their runtime substrate.

## Repository responsibilities

### Containers

Own the opinionated executable façade over tasks and runtimes: public
environment/policy/harness contracts, task/world profiles, discovery, operational
defaults, checkpoints, episode/rollout semantics, and canonical domain/trace events.
It folds supported native formats into this surface while retaining their authoritative
artifacts and semantics.

**Committed compatibility scope:** Harbor is the sole first-class external fold. The
Harbor layer owns lossless task/dataset resolution, environment packaging, agent and
verifier lifecycle mapping, native artifact retention, typed capability discovery, and
end-to-end conformance. Other framework adapters remain experimental or aspirational
until separately promoted. GameBench is not in this adapter list because it is a
benchmark dataset/suite, not a format.

1. Add world, workspace, service, binding, environment-episode, and policy-session
   records.
2. Replace `container_profile` with execution profiles plus a deployment descriptor;
   keep a compatibility alias.
3. Replace container-derived verifier authority with explicit evaluator bindings.
4. Split capability/recovery by service role.
5. Make semantic event append authoritative and poll/SSE/WS views over it.
6. Stop defaulting absent reward to zero.
7. Promote the Harbor fold from compatibility metadata to a typed, documented,
   conformance-tested first-class profile. Keep Archipelago/OpenEnv mappings explicitly
   experimental until a later support decision.

### Optimizers

Own recipes/runs, candidate lineage, search/selection, optimizer checkpoints, child-eval
relationships, and optimizer-native events/slices. Adopt shared resource refs, bindings,
envelope fields, and nullable usage. Keep GEPA, GELO, SFT, MAPO, RLVR, and OHCO payloads
typed. Local and hosted providers share a profile but retain ownership/billing boundaries.

### Evals

Own benchmark orchestration, private runner details, task adapters, evaluation plans,
attempts, trusted scorers/judges, evidence manifests, result index, lane policy, and
release classification. Private runner identity and APIs are repository-internal. Both
private and Harbor lanes execute through Containers contracts. Map canonical
task/attempt records to shared identities, preserve
`run -> score -> evidence -> index`, distinguish rig/agent/evaluator/benchmark failures,
and migrate `evals.event-stream.v1` through a compatibility profile.

### Workshop

Own durable product projection and supervision, not upstream truth: provider registration,
managed services, mirrored cursors/events, relationships, visuals, permissions, retained
renderers, and indexes. Supervise logical roles independently even when co-deployed;
persist before publish; show binding health separately; render one reducer per typed
stream; bind visuals to exact evidence; preserve history when compute is removed.

## Acceptance program

### Schema, identity, and streaming

| ID | Requirement |
| --- | --- |
| ONT-01 | Every record validates with one discriminated profile and exact schema version. |
| ONT-02 | Changing world, task, policy, harness, evaluator, or recipe content changes its immutable revision. |
| ONT-03 | Container/process/endpoint replacement never silently changes semantic identity. |
| ONT-04 | Every score, reward, criterion, usage value, artifact, and trace names producer authority and subject. |
| ONT-05 | Null/unavailable values round-trip across Python, Rust, TypeScript, storage, export, and replay. |
| ONT-10 | Poll, SSE, and advertised WS produce identical ordered event IDs/digests. |
| ONT-11 | Disconnect/reconnect at every boundary yields no loss and only identical bounded duplicates. |
| ONT-12 | Producer restart changes producer epoch while the durable stream stays monotonic. |
| ONT-13 | Open/delta/close trace items reduce deterministically to the sealed trace. |
| ONT-14 | Oversize, expired cursor, gap, wrong identity, malformed payload, and unauthorized resource fail typed. |
| ONT-15 | Large artifacts stream by digest reference and remain available under declared retention. |

### Independent restarts

| ID | Requirement |
| --- | --- |
| ONT-20 | Restart policy/harness while preserving Craftax environment; new policy segment is explicit and no step is lost/duplicated. |
| ONT-21 | Restore Craftax from a true checkpoint; RNG, state, reward, achievements, step, and cursor match uninterrupted control. |
| ONT-22 | Audit-only snapshot cannot claim seamless environment resume; retry/branch is required. |
| ONT-23 | Revise/restart evaluator and rejudge immutable evidence without mutating original result. |
| ONT-24 | Revise harness against preserved development workspace through a new pinned, non-adjudicable attempt. |
| ONT-25 | Lose a local lease used by a hosted optimizer; optimizer history survives and binding failure is separate. |

### Reference fixtures

| ID | Requirement |
| --- | --- |
| ONT-30 | APEX fixture: population, MCP binding, initial/final snapshots, diff, artifact selection, separate grading. |
| ONT-31 | TaxCalcBench: edition, PDFs, structured return, line metrics, strict/lenient verdicts. |
| ONT-32 | Harvey: deliverable-scoped criteria, all-pass, diagnostics, evaluator restart. |
| ONT-33 | RedlineBench: role/turn/branch, document lineage, validity, panel, behavior, aggregation. |
| ONT-34 | Craftax: real policy partials, actions, frames, rewards, achievements, checkpoints, replay, Trace V5. |
| ONT-35 | Terminal-Bench/TBLite: same Harbor profile, distinct dataset revisions and aggregates. |
| ONT-36 | Evals/GameBench DEO through Containers: candidate lineage, child rollouts, held-out evaluation, delta, final gate. |
| ONT-37 | PostTrainBench: workspace changes, model lineage, final eval, integrity, promotion. |

## Migration sequence

1. Freeze the glossary and publish schemas plus generated Python/Rust/TypeScript
   bindings in a neutral public package.
2. Add the first-class Harbor fold over existing Containers metadata, task, rollout,
   trace, checkpoint, verifier, and capability routes while retaining Harbor-native
   evidence.
3. Add logical service-role discovery/bindings without changing physical deployment.
4. Make a durable event log authoritative and serve poll/SSE/WS from it.
5. Adopt shared refs and relationships in Optimizers and Evals; keep private Evals
   runner types behind the repository boundary.
6. Teach Workshop to supervise and display logical services independently.
7. Implement the eight real-stream fixtures with restart fault injection. Non-Harbor
   fixtures exercise the ontology and provider APIs; they do not imply first-class
   Containers format compatibility.
8. Then deprecate catch-all execution/outcome APIs, route guessing, boolean-only
   capabilities, and deployment-derived identities.

## Decisions for engineering review

1. Which small contracts belong in a neutral shared package, while Containers remains
   the opinionated task/runtime product and owns the first-class Harbor fold?
2. Is `Attempt` the universal adjudicable unit, with Harbor `Trial` as an alias?
3. Is `ScenarioDefinition` a world profile or a sibling linked by `uses_world`?
4. Which service owns the stable cross-producer sequence locally: embedded relay,
   Evals host, or Workshop?
5. Which policy/harness restarts, if any, are valid inside a scored attempt? Default:
   none unless the evaluation plan declares recovery.
6. Which snapshot profiles allow workspace resume versus grading/audit only?
7. Does Trace V5 become the sealed reduction of canonical trace events, or remain an
   independent capture with digest links during migration?
8. How long must ephemeral providers retain replay before Evals/Workshop is recovery
   authority?

## Tentative conclusion

The shared structure is centered on `TaskDefinition` and `TaskInstance`, but the
operational center is an `Attempt` binding independently revisioned world, environment,
policy, harness, and evaluator roles. `WorldDefinition` describes reusable initial
reality; `WorkspaceInstance` is its mutable materialization; `Rollout`,
`EnvironmentEpisode`, and `PolicySession` are different continuity domains;
`EvaluationExecution` scores immutable evidence; and `Container` is a deployment choice.

This covers step-wise games, terminal sandboxes, document workflows, multi-turn legal
negotiations, structured calculations, post-training workspaces, and optimizer-driven
candidate evaluations without erasing native semantics. It also gives Workshop the
identities needed to stream partial traces, show real live state, restart honestly,
replay history, and keep reward, score, integrity, and promotion separate.

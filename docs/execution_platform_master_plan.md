# Execution platform master plan

**Status:** Consolidated proposal for engineering review; compatibility targets are
not support commitments until their promotion gates pass

**Detailed designs:** `execution_ontology.md`, `execution_stream_contracts.md`,
`live_evals.md`, `live_optimizers_gepa.md`,
`private_eval_workspace_extensions.md`, and
`workshop_live_visuals_execution_plan.md`

## Product goal

Build one opinionated execution substrate in **Containers** that can run native Synth
tasks, fold Harbor tasks with very little adapter work, and progressively wrap other
environment systems without erasing their native semantics. Use that substrate from
Evals and Optimizers, and make Workshop the live control room for queues, rollouts,
policy activity, environment state, rewards, grades, artifacts, and optimizer progress.

The first complete user experience should be:

1. select a revisioned task or dataset, policy, harness, evaluator plan, and budget;
2. connect a durable event consumer before execution starts;
3. launch through Containers, regardless of whether the implementation is native,
   Harbor, OpenEnv, or Prime Verifiers;
4. watch real partial evidence arrive while the run is active;
5. restart supported logical services without confusing retry, resume, or replay;
6. inspect the sealed native and normalized evidence after completion; and
7. reproduce or compare the run from a receipt with pinned inputs and digests.

This is not a universal lowest-common-denominator schema. Containers owns a coherent,
opinionated surface and preserves framework-specific evidence through typed profiles and
native artifact references.

## Support levels

| Level | Meaning | Initial members |
| --- | --- | --- |
| Native | Designed directly against Containers contracts and conformance tests | Synth-native environments and task profiles |
| First-class fold | Owned adapter, fixtures, docs, CI conformance, operational defaults, and support commitment | **Harbor only** |
| Compatibility target | Real wrapper and eval are planned; support is promoted only after equivalence and operations gates pass | **OpenEnv**, **Prime Intellect Verifiers** |
| Research input | Informs ontology and fixtures but has no compatibility promise | APEX/Archipelago, Terminal-Bench-derived systems, professional-work benchmarks, PostTrainBench, and other reviewed systems |

GameBench and CardBench are benchmark content, not wire formats. They may be run via a
native or Harbor-packaged task, but neither becomes a Containers compatibility layer.
Private Evals runner names, contracts, and repository details stay inside the Evals
repository. Public Workshop and Containers surfaces refer only to a private Evals
provider, profile, recipe, or workspace extension.

“Compatible” means the native package can be wrapped without rewriting its task logic,
the normalized run is semantically faithful, native evidence remains available, and the
published acceptance suite passes. “First-class” additionally means the team owns the
adapter lifecycle, version policy, fixtures, documentation, CI, and user support.

## Architecture

The shared ontology is organized into five planes:

```text
content       benchmark -> dataset -> task instance -> evaluation plan
deployment    provider -> deployment unit -> logical service instance -> binding
execution     evaluation run -> attempt -> rollout -> episode/session/evaluation
evidence      event log + trace + artifacts + usage + rewards/grades + snapshots
projection    Workshop mirror + state slices + visuals + indexes
```

The core rule is that a container is a deployment unit, not the semantic owner of a
task, environment, policy, rollout, reward, or trace. One physical container may host
several services at first, but each advertises its own identity, generation, lifecycle,
health, capabilities, and stream producer:

- `EnvironmentService` owns mutable world or workspace state and environment-native
  reset, step, tool, or snapshot operations.
- `PolicyService` owns policy calls, sessions, usage, and policy-native trace evidence.
- `HarnessService` owns the interaction loop, routing, retry policy, limits, and
  cross-service correlation.
- `EvaluatorService` owns a declared judge or scorer invocation and its evidence.
- `ArtifactService` owns immutable blobs, manifests, digests, and retrieval.
- `EventRelayService` owns durable append, cursor replay, subscriptions, and transport
  adaptation; it never invents domain facts.
- `OptimizerService` owns candidate proposal, search/training state, selection, and
  optimizer events, while child rollout and evaluator truth remain external references.

An `EvaluationRun` contains adjudicable `Attempt`s. An attempt may contain one or more
`Rollout`s, environment episodes, policy-session segments, and evaluator executions.
Retry creates a new attempt; reconnect does not. A service restart creates a new
service generation and normally a new episode or session segment. Resume requires a
declared checkpoint capability and provenance chain. Replay is read-only.

## Shared provider and stream contract

All providers expose versioned discovery that describes resources, typed operations,
streams, transport choices, authentication, limits, checkpoint semantics, and
compatibility profiles. The shared event envelope carries, at minimum:

```text
schema_version, event_id, event_type, occurred_at
provider_id, producer_id, producer_generation, sequence
evaluation_run_id, attempt_id?, rollout_id?, span_id?, parent_span_id?
operation, phase(open|delta|close|snapshot|terminal)
payload, resource_refs[], native_refs[], integrity
```

The semantic source is a durable ordered log. SSE, WebSocket, and polling are equivalent
delivery adapters over cursors, not separate truth models. Consumers can subscribe
before launch, reconnect from a cursor, deduplicate by event ID, detect generation
changes and gaps, and reconcile to a sealed terminal manifest. Unknown reward, score,
usage, state, or terminal status remains unknown; it is never coerced to zero.

Trace events use explicit open/delta/close lifecycles for policy calls, tool calls,
environment steps, evaluator executions, artifacts, and optimizer work. Large images,
workspace snapshots, token payloads, and native result files travel as digest-addressed
resources rather than being repeatedly embedded in events.

## Containers implementation profiles

### Native profile

Native implementations expose the shared task, service, lifecycle, evidence, and stream
contracts directly. A native implementation may offer stronger capabilities—true
checkpoint/restore, deterministic branching, frame streams, or incremental evaluator
evidence—through discovered profile extensions.

### Harbor first-class fold

The Harbor adapter preserves instruction, environment, solution, tests, verifier output,
and native result artifacts. It maps task to `TaskDefinition`, trial to `Attempt`, job to
`EvaluationRun`, agent execution to harness/policy activity, and verifier execution to an
explicit `EvaluationExecution`. Containers supplies launch, leases, supervision, stream
relay, artifact addressing, cancellation, and normalized receipts.

Promotion criteria include pinned public fixtures, native-versus-wrapped result
equivalence, failure taxonomy, cancellation and cleanup, live evidence before terminal,
cursor recovery, and documentation for authoring and running a Harbor task.

### OpenEnv compatibility profile

[OpenEnv](https://github.com/huggingface/OpenEnv) defines a typed client/server
environment around `Action`, `Observation`, and `State`, with `reset`, `step`, and
`state` operations. Its environment server is the authoritative owner of state
transition and of the reward and `done` fields returned with an observation/response.

The wrapper should run an unmodified OpenEnv server or published image behind a thin
Containers gateway:

| OpenEnv | Containers |
| --- | --- |
| environment package/image | `ServiceDefinition` + `DeploymentUnit` |
| server process | `EnvironmentService` generation |
| client connection/session | service binding + `EnvironmentEpisode` |
| `reset()` | episode open/reset event |
| `step(Action)` | correlated action, observation, reward, and terminal events |
| `state()` | typed state slice/snapshot, not automatically a checkpoint |
| observation/response `reward` / `done` | environment-authored reward sample / terminal signal |
| native messages and logs | native artifacts and trace payload references |

The adapter must discover or configure action, observation, and state schemas; preserve
native serialization; translate the persistent WebSocket lifecycle into producer
generations and cursor-backed events; and advertise actual concurrency and restart
capabilities. `state()` must not be labeled resumable unless the environment separately
proves checkpoint/restore.

First proof:

1. wrap the official `echo_env` example and its image without modifying environment
   logic;
2. run a small policy evaluation entirely through Containers;
3. prove reset/step/state, reward/done, concurrent-client behavior, cancellation,
   real-time trace delivery, terminal sealing, and native artifact retrieval; and
4. compare the same fixed actions through the native OpenEnv client and Containers.

Because Echo is intentionally trivial, promotion also requires one meaningful official
environment, preferably Chess or another trajectory-reward environment, to prove
multi-step reward and terminal semantics. The official OpenEnv builder/validator remains
the packaging authority; Containers wraps rather than forks it.

### Prime Intellect Verifiers compatibility profile

[Prime Intellect Verifiers](https://github.com/PrimeIntellect-ai/verifiers) is not merely
a step environment. Its current model separates a `Taskset` (data, prompt shaping,
setup/update/reward hooks, and toolsets), a `Harness` (program, endpoint proxy, model
controls, sandbox, and runtime hooks), and an `Env` that wires evaluation or training.
That separation must survive normalization.

| Prime Verifiers | Containers |
| --- | --- |
| installable environment package + config | revisioned compatibility package/profile |
| `Taskset` and dataset row | `DatasetDefinition` / `TaskInstance` |
| Prime `Harness` | `HarnessDefinition` / `HarnessService` |
| sandbox and tools | `EnvironmentService` capabilities and bindings |
| model endpoint/control | `PolicyService` binding |
| one generated example/trajectory | `Rollout` |
| rubric/reward function | declared evaluator or taskset reward authority |
| diagnostic metric | metric; never silently promoted to reward |
| evaluation result files | native artifacts plus normalized evidence |

The adapter loads the package through its supported `load_environment(config)` entry
point, pins package and configuration digests, preserves native state and result files,
and translates dataset/example IDs without reindexing them. Rubric outputs retain name,
weight, authority, inputs, and diagnostics. Weight-zero metrics remain metrics. Sandbox,
tool, and policy calls emit correlated partial traces even when Prime produces its
aggregate only at completion.

First proof uses the official `primeintellect/gsm8k` environment on a fixed, small
subset and controlled policy endpoint. Run the same pinned package/config/examples both
with native `prime eval run` and through Containers; require per-example identity,
reward, failure classification, and aggregate equivalence. Local acceptance uses the
local run path, never invokes `prime eval push`, and asserts that the test cannot publish
results implicitly.

The wrapper may later use Prime's own Harbor or OpenEnv integration internally, but each
receipt records the effective adapter chain. A compatibility chain does not turn every
transitive format into a first-class Containers fold.

## Evals and private workspace extensions

Public and private Evals execution should resolve tasks and policies, then launch and
observe them through Containers. Private orchestration remains private. Workshop gains
a generic, explicitly trusted local workspace-extension mechanism whose source can live
persistently in a private checkout:

```text
<private-evals-checkout>/.synth/workshop/
  extension.json
  templates/   recipes/   reducers/   schemas/   skills/   fixtures/   tests/
```

The extension declares typed provider operations and resource/stream permissions, never
credentials or arbitrary host commands. Workshop stores registration and compiled-cache
metadata in its data root, while source and private vocabulary stay in the owning
repository. Visual instances pin extension/template digests so historical runs remain
auditable.

This enables private GameBench and CardBench recipes to compare several policies,
display the rollout queue and live traces, and retain reusable local templates without
hardcoding either benchmark or private repository behavior into Workshop.

## Workshop live experience

The initial reference visual is a time-ordered control room, not a synthetic dashboard:

- evaluation header: pinned task/dataset, policy, harness, budget, status, elapsed time;
- queue/matrix: task × policy × seed, lease state, attempt state, score and cost;
- selected rollout: live frame/artifact, environment state, policy/tool trace, rewards,
  achievements, usage, and terminal evidence;
- through-time scrubber: replay every state slice from the durable event log and return
  to follow-live without losing cursor position;
- plots: reward and metric series with missing values visibly unknown, achievement
  markers, latency/tokens/cost, and optimizer candidate/baseline deltas;
- provenance: provider and producer generations, adapter chain, content digests, native
  artifact references, gaps, reconnects, retries, and restarts.

Templates consume shared state slices and optional typed profiles. They never invent
environment fields. For Craftax, “dexterity” or any other field absent from real engine
evidence must not appear. Frames are artifact resources with verified content type and
digest; a broken image is an evidence failure, not a placeholder success.

## Optimizers

Optimizers use the same event, resource, identity, and receipt substrate. An
`OptimizerRun` owns candidate revisions, proposal/search/training events, checkpoints,
and selection decisions. Each candidate evaluation references ordinary Containers
evaluation runs and their exact task, policy, cost, trace, reward, and grade evidence.

GEPA, GELO, SFT, and future MAPO/RLVR/OHCO profiles extend this base vocabulary rather
than redefining transport. Algorithm-specific templates can show Pareto fronts,
candidate lineage, minibatches, checkpoint rollouts, or held-out promotion while a
generic run viewer continues to understand progress, status, resources, and children.
No optimizer may summarize a child run without preserving the child evidence reference.

## Delivery roadmap

### Phase 0 — freeze semantics and receipts

- approve ontology names, identity/containment rules, reward and grade authority, and
  restart vocabulary;
- publish capability/discovery, event envelope, trace lifecycle, resource reference,
  terminal manifest, and receipt schemas;
- define compatibility manifests and adapter-chain provenance;
- create schema fixtures and language bindings before implementation branches diverge.

**Gate:** schema round trips, unknown-field compatibility, no missing-to-zero coercion,
and one shared conformance runner usable by Containers, Optimizers, and Workshop.

### Phase 1 — durable execution and streaming foundation

- implement append-only event storage, producer generations, cursor replay,
  deduplication, gap reporting, retention, and terminal sealing;
- expose equivalent SSE, WebSocket, and polling adapters;
- split logical Environment, Policy, Harness, Evaluator, Artifact, and EventRelay service
  identities even when they share a process;
- add leases, cancellation, health, bounded backpressure, and redaction.

**Gate:** connect-before-run, disconnect/replay, transport equivalence, concurrent
subscribers, service crash, relay crash, cancellation, artifact integrity, and sealed
reconciliation tests.

### Phase 2 — Harbor first-class fold

- ship the owned Harbor adapter, authoring path, pinned fixtures, failure mapping, and
  native-artifact preservation;
- validate public Harbor and Terminal-Bench-style tasks through Containers;
- use the same path from Evals rather than a parallel execution implementation.

**Gate:** native-versus-wrapped task/verifier equivalence plus live evidence and clean
resource teardown. Harbor is the only external fold promised at this milestone.

### Phase 3 — OpenEnv compatibility spike and promotion

- wrap official Echo using the standard OpenEnv package/image and client semantics;
- run a real policy eval and native-vs-wrapped fixed-action comparison;
- add a nontrivial official multi-step environment to validate reward and terminal
  behavior;
- document limitations such as absent checkpoint/restore.

**Gate:** OE-01 through OE-12 below. Promote from target to supported compatibility only
after CI owns pinned fixtures and upstream-version policy.

### Phase 4 — Prime Verifiers compatibility spike and promotion

- wrap `primeintellect/gsm8k` with pinned package/config and fixed examples;
- preserve Taskset/Harness/Rubric distinctions and native results;
- compare native `prime eval run` to Containers under the same policy endpoint;
- ensure local tests cannot auto-upload results.

**Gate:** PV-01 through PV-12 below. Promotion requires CI fixtures and an upstream
version policy; it does not automatically become a first-class fold.

### Phase 5 — Evals dogfood and persistent extensions

- route private Evals orchestration through the same Containers provider contracts;
- implement registration, trust, compilation, pinning, and last-known-good behavior for
  local Workshop extensions;
- author private GameBench and CardBench code-policy recipes and visuals in the private
  checkout;
- prove several policies, seeds, retries, and evaluator authorities in one live matrix.

**Gate:** no private vocabulary leaks into public repositories; extension removal does
not delete run evidence; historical visuals remain pinned and explain missing renderers.

### Phase 6 — first-class live visuals

- graduate the Craftax reference viewer from local HTML to a versioned Workshop
  template consuming real events and artifacts;
- add generic task matrix, trace, evidence, comparison, and through-time components;
- add Harbor artifact/verifier and OpenEnv/Prime profile views without changing the
  shared event substrate;
- expose template creation and validation through a supported skill and MCP surface.

**Gate:** live data is visible before completion, replay is bit-for-bit stable after
sealing, malformed partials fail closed, and screenshots/frames/resources resolve under
installed-app security rules.

### Phase 7 — optimizer integration

- project GEPA/GELO/SFT events into the shared stream and state-slice model;
- link candidate evaluations to Containers runs instead of duplicating rollout truth;
- add checkpoint, candidate-lineage, held-out, promotion, cost, and failure views;
- extend to MAPO/RLVR/OHCO only from real captures and owned algorithm profiles.

**Gate:** concurrent candidate evaluations, reconnect, checkpoint rollout, selection
provenance, and exact child-evidence navigation all work live and in replay.

### Phase 8 — hardening and broader profiles

- run the ontology fixtures for interactive games, sandbox artifact tasks, professional
  deliverables, negotiations, structured calculation, and autonomous optimization;
- add compatibility adapters only when a real owner, fixture, conformance suite, and
  support policy exist;
- publish performance, security, retention, and migration budgets.

## Reference vertical slices

| Slice | Purpose | Exit evidence |
| --- | --- | --- |
| Craftax Rust + GameBench code-policy task | richest live environment/frame/achievement/trace case | real frames, real engine state, reward series, achievements, policy spans, replay, ten-rollout matrix |
| Harbor public task | first-class external fold | native verifier equivalence, artifacts, failures, live stream, receipt |
| OpenEnv Echo then meaningful official env | typed reset/step/state compatibility | native action/result equivalence, live episodes, reward/done, restart limits |
| Prime `gsm8k` | dataset/harness/rubric compatibility | same fixed examples, per-example rewards and aggregate, no implicit upload |
| Private CardBench code-policy recipe | private extension generality | persistent private recipe/template, multi-policy queue, live comparison, no public leakage |
| GEPA candidate optimization | optimizer-to-eval linkage | candidate lineage, child rollouts, costs, held-out decision, replay |

## Compatibility acceptance matrix

Every adapter must pass shared tests plus profile-specific tests.

### Shared CA-01…15

1. Discover exact package/profile versions, schemas, operations, and limitations.
2. Pin task, config, policy, harness, evaluator, seed, and adapter-chain digests.
3. Preserve native IDs or provide a reversible ID map.
4. Match native inputs, outputs, terminal state, rewards, grades, and failure classes.
5. Keep native logs/results/artifacts retrievable with content digests.
6. Emit real partial evidence before terminal completion.
7. Maintain monotonic producer sequence within each generation.
8. Replay after disconnect from the last acknowledged cursor without duplicates.
9. Surface gaps and corrupt resources; never synthesize success or zero.
10. Correlate policy calls, tools, environment transitions, and evaluator work.
11. Distinguish reconnect, service restart, retry, resume, branch, and replay.
12. Cancel promptly and clean leases, processes, networks, and temporary storage.
13. Enforce auth, redaction, path isolation, and declared network/side-effect policy.
14. Seal a terminal manifest that reconciles event log, artifacts, outcome, and usage.
15. Reproduce the normalized result from the receipt or explain every nondeterministic
    input and observed divergence.

### OpenEnv OE-01…12

- unmodified official package/image builds and validates with upstream tooling;
- native client and Containers produce equivalent reset, fixed steps, state, reward,
  and done values;
- action/observation/state schemas and serialization remain round-trippable;
- persistent connection loss produces a visible generation boundary and safe recovery;
- concurrent sessions respect the environment's advertised isolation/capacity;
- server-side reward remains authoritative and is not recomputed by Workshop;
- state snapshots are not offered as resumable checkpoints without proof;
- frames/resources, if present, retain type, ordering, and digest;
- cancellation and server failure yield distinct terminal reasons;
- Echo passes transport smoke tests;
- the selected meaningful environment passes multi-step reward/terminal tests; and
- native and wrapped receipts document any permitted nondeterminism.

### Prime PV-01…12

- install and load a pinned package/config through the supported entry point;
- preserve dataset/example IDs, prompt shaping, and task state;
- preserve Harness program, endpoint, sandbox, tool, and model-control behavior;
- preserve rubric names, weights, authority, diagnostics, and aggregate rules;
- weight-zero metrics remain metrics and missing scores remain unknown;
- fixed examples and policy endpoint match native per-example results;
- normalized aggregate matches native evaluation within declared tolerance;
- partial policy/tool/sandbox traces arrive before aggregate completion;
- failures distinguish package, setup, policy, sandbox, rubric, and infrastructure;
- native result artifacts remain retrievable;
- local conformance performs no implicit upload or unrelated external mutation; and
- adapter chain/provenance is explicit if Prime delegates through Harbor or OpenEnv.

## Repository deliverables

| Repository | Owns |
| --- | --- |
| Containers | schemas/bindings, provider discovery, service supervision, durable relay, native runtime, Harbor fold, compatibility SDK, OpenEnv and Prime adapters/tests |
| Evals | public/private evaluation recipes, task resolution, benchmark-specific profiles, evaluator plans, private extension source, native result comparison |
| Optimizers | optimizer provider/profile, candidate lineage, child-eval references, algorithm state slices and templates |
| Workshop | provider registration, credential broker, durable mirrors, extension trust/cache, visual instances, live/replay UI, generic templates and MCP/skill authoring surfaces |

No repository may make an event authoritative merely because it consumed or displayed
it. Authority always follows the declared producer and evaluation plan.

## Engineering handoff package

Before implementation begins, the handoff should contain:

- approved ontology and JSON schemas with golden fixtures;
- explicit v1 non-goals and compatibility/support language;
- adapter design records for Harbor, OpenEnv, and Prime;
- pinned upstream example versions/digests and native baseline receipts;
- captured live streams and terminal manifests for every vertical slice;
- acceptance runner with CA, OE, and PV cases;
- local Workshop visual mocks consuming those real captured streams;
- security review for credentials, extension code, artifacts, sandbox/network policy, and
  upload defaults; and
- migration/rollback plan for existing Containers traces and Workshop Trace V5 data.

## Decisions still required

1. Which meaningful OpenEnv example follows Echo for the promotion gate.
2. Whether adapter processes run as in-container gateways or supervised sibling
   sidecars by default; the wire semantics remain the same.
3. Canonical storage and retention limits for high-volume frames and token deltas.
4. Whether evaluator rescore creates a child evaluation execution only or a derived
   evaluation-run receipt as well.
5. Minimum deterministic tolerance and seed disclosure for native/wrapped comparisons.
6. The upstream version window each compatibility adapter promises after promotion.
7. Which capabilities a trusted local Workshop extension may execute without a second
   confirmation.

## Source notes

- OpenEnv's [core API](https://huggingface.github.io/OpenEnv/reference/core.html),
  [environment anatomy](https://huggingface.github.io/OpenEnv/guides/environment-anatomy.html),
  [builder workflow](https://huggingface.co/docs/openenv/main/getting_started/environment-builder),
  and [reward guide](https://huggingface.github.io/OpenEnv/guides/rewards.html) support
  the typed server, lifecycle, packaging, and server-side reward mapping above. Its
  [Inspect evaluation tutorial](https://github.com/huggingface/openenv/blob/main/docs/source/tutorials/evaluation-inspect.md)
  provides the Echo reference path.
- Prime Intellect's [environment model](https://docs.primeintellect.ai/verifiers/environments),
  [evaluation tutorial](https://docs.primeintellect.ai/tutorials-environments/evaluating),
  and [Verifiers reference](https://docs.primeintellect.ai/verifiers/reference) support
  the Taskset/Harness/Env split, package loading, rubric model, and native-evaluation
  comparison proposed here.

# Private eval workspace extensions for Workshop dogfooding

**Master plan:** `execution_platform_master_plan.md`

**Status:** Tentative product design; engineering review required

**Related:** `execution_ontology.md`, `execution_stream_contracts.md`, `live_evals.md`

## Goal

Let a team use Workshop as the live control room for private evaluations without
hardcoding private repositories, runner names, benchmarks, schemas, or commands into
Workshop.

The dogfood experience should support:

- persistent, agent-editable private visual templates and reducers;
- private evaluation recipes over multiple policies, task datasets, and execution lanes;
- both Harbor and private Evals orchestration running through Containers;
- live rollout queue, environment state, policy activity, partial traces, rewards,
  scores, usage, artifacts, and failures;
- exact post-run replay and comparison;
- GameBench and CardBench code-policy optimization as useful first examples.

This is a general **local workspace extension** feature. Evals is its first serious
consumer, not a special case in Workshop.

## Ownership boundary

```text
private Evals checkout
  owns private runner names, recipes, benchmark adapters, private templates/reducers,
  task resolution, scoring, and native result artifacts
                              |
                              | registers a local workspace extension and provider
                              v
Containers + Evals provider contracts
  own task/runtime operations, attempts, rollouts, streams, traces, resources,
  capability discovery, and launch/control authorization
                              |
                              | generic discovery, commands, streams, and resource refs
                              v
Workshop
  owns extension registration, trust/permissions, durable mirrors, visual instances,
  relationships, local compiled cache, supervision, and the user experience
```

Workshop never imports a private Evals Python module, assumes a private directory
layout, constructs a private command, or interprets a private runner type. The private
checkout may use any internal vocabulary it needs; the boundary exposes only shared
provider, execution, event, artifact, and visual-extension contracts.

## Persistent local folder

The source of truth should live with the private repository that understands it, for
example:

```text
<private-evals-checkout>/.synth/workshop/
  extension.json
  templates/
    gamebench.code_policy.live.v1/
      template.json
      shell.tsx
      reducer.ts
      components/
      examples/
        fixture_binding.json
  recipes/
    gamebench-code-policy-deo.json
    cardbench-code-policy-deo.json
  schemas/
    code-policy-run.v1.json
  skills/
    run-private-live-evals/
      SKILL.md
  fixtures/
    captured-streams/
  tests/
```

This folder is persistent because it is ordinary source in the private checkout. An
agent can edit it, test it, and commit it under the private repository's normal review
policy. Private runner details therefore remain inside Evals.

Workshop registers the extension by explicit user action and stores only:

- extension ID and source path;
- manifest and content digests;
- granted capabilities;
- selected development/trusted mode;
- compiled-cache path and last successful build;
- template and recipe catalog projections.

Workshop-generated visual **instances** remain durable Workshop records. They reference
the extension/template revision and bindings; they do not copy private task data or
runner source into the Workshop repository.

Installed builds should compile into an application-data cache, not into the Workshop
source tree. Suggested logical locations are:

```text
Workshop data root/
  workspace-extensions/<extension-id>/registration.json
  extension-cache/<extension-id>/<content-digest>/renderer-bundle/
  visuals/<visual-id>/binding.json
```

The exact OS path follows the existing per-instance Workshop data-root rules. No feature
should assume one user's home directory.

## Extension manifest

Tentative example:

```json
{
  "schemaVersion": "synth.workspace-extension.v1",
  "id": "local.private-evals",
  "name": "Private evaluation workspace",
  "version": "0.1.0",
  "visibility": "private",
  "requires": {
    "workshop": ">=0.2.0",
    "contracts": [
      "synth.execution-provider-info.v1",
      "synth.stream-event.v1",
      "synth.visual-template.v1"
    ]
  },
  "templates": [
    {
      "id": "gamebench.code_policy.live.v1",
      "path": "templates/gamebench.code_policy.live.v1",
      "bindingSchema": "synth.code-policy-live-binding.v1"
    }
  ],
  "recipes": [
    {
      "id": "gamebench-code-policy-deo",
      "path": "recipes/gamebench-code-policy-deo.json"
    },
    {
      "id": "cardbench-code-policy-deo",
      "path": "recipes/cardbench-code-policy-deo.json"
    }
  ],
  "permissions": {
    "providerDiscovery": true,
    "providerCommands": ["evaluations.create", "evaluations.cancel"],
    "streams": ["evaluation.events", "rollout.events", "trace.events"],
    "resources": ["artifact", "trace", "workspace_snapshot"],
    "networkOrigins": ["http://127.0.0.1:*"],
    "writeRoots": ["."]
  }
}
```

The manifest contains declarative operation IDs, never shell commands or credentials.
The registered Evals provider resolves those operations. Credentials remain in the host
credential broker and are passed through capability-scoped launch authorization.

## Built-in templates versus workspace templates

Workshop should merge catalogs from two sources:

1. bundled, supported templates shipped with Workshop;
2. explicitly registered local workspace extensions.

Rules:

- IDs are globally unique; an extension cannot shadow a bundled template.
- local template IDs are namespaced or checked for collision at registration.
- every visual records extension ID, extension version, template version, and content
  digest.
- a running visual stays pinned to its loaded revision until explicitly reloaded.
- a broken new build leaves the last successful bundle available.
- uninstalling or unregistering an extension does not delete visual records or mirrored
  run evidence.
- opening a historical visual without its renderer shows a typed
  `renderer_unavailable` state and offers re-registration; it does not silently choose
  a different template.

The current single `SYNTH_VISUALS_ROOT` replacement model is insufficient. Workshop
needs a multi-root catalog with bundled and registered sources, precedence rules, source
identity, digests, and an application-data build cache. The current behavior of saving
generated TSX into `visuals/instances` is appropriate for repository development but
should not be the production persistence contract.

## Trust and code execution

Local templates are executable renderer code, so registration is a trust decision.

Two modes are useful:

| Mode | Behavior |
| --- | --- |
| `development` | Watches the registered source folder, rebuilds on change, surfaces compile errors, and permits agent edits under an explicit write grant. |
| `pinned` | Loads only a reviewed content digest; source changes have no effect until the user reviews and activates a new revision. |

Required protections:

- explicit registration and per-capability consent;
- path containment and symlink-escape checks for reads/writes;
- no renderer access to arbitrary filesystem or credentials;
- allowlisted network origins through the host, not browser-global access;
- bounded bundles, events, artifacts, and build output;
- isolated compilation and a constrained renderer boundary;
- content digest, source revision, and audit event on every activation;
- no executable template code delivered inside an evaluation event.

An agent may write only inside roots granted to the extension. A template should not be
able to launch an evaluation; it requests a typed provider operation through Workshop.

## Recipe contract

A recipe is declarative orchestration input, not a private launch script:

```json
{
  "schemaVersion": "synth.eval-recipe.v1",
  "id": "gamebench-code-policy-deo",
  "benchmark": {
    "datasetRef": "provider://private-evals/datasets/gamebench-code-policy",
    "taskSelector": {"include": ["craftax-singleplayer"]}
  },
  "matrix": {
    "policies": [
      {"policyRef": "policy://baseline"},
      {"policyRef": "policy://candidate-a"}
    ],
    "executionProfiles": ["private", "harbor"],
    "seeds": [11, 17, 23]
  },
  "budget": {"currency": "USD", "max": 5.0},
  "evidence": {
    "events": "required",
    "partialTrace": "required",
    "sealedTrace": "required",
    "artifacts": "required"
  },
  "visual": {
    "templateId": "gamebench.code_policy.live.v1",
    "openBeforeLaunch": true
  }
}
```

The example names `private` only as an opaque provider-advertised execution profile.
Workshop does not know the private implementation behind it. Harbor remains the named
public compatibility profile. The Evals provider validates dataset/task/policy refs and
expands the matrix.

Recipe validation must return a launch plan before spend:

- resolved benchmark/dataset/task revisions;
- policy and harness revisions;
- number of attempts and expected concurrency;
- required Containers capabilities;
- estimated or bounded spend;
- proposed visual and stream bindings;
- unavailable cells and named refusals.

## Live launch lifecycle

```text
1. Agent/user opens the private Evals workspace in Workshop.
2. Workshop loads the registered extension and discovers the Evals provider.
3. User selects a recipe, tasks, policies, execution profiles, seeds, and budget.
4. Provider returns a resolved, fail-closed launch plan.
5. Workshop creates the EvaluationRun record and visual instance.
6. Visual subscribes and acknowledges replay cursor/readiness.
7. Workshop invokes evaluations.create with the approved plan and idempotency key.
8. Evals launches every attempt through Containers.
9. Workshop mirrors committed events and updates the visual live.
10. Attempts finish; evaluators score sealed evidence; traces and artifacts seal.
11. Workshop pins terminal cursors/digests and keeps the exact visual revision for replay.
```

Opening and connecting the visual before step 7 is important. It makes live visibility
an acceptance condition rather than a best-effort attachment after spending begins.

## Canonical bindings for a private eval visual

The template consumes shared resources, not private file paths:

```json
{
  "evaluationRun": {"kind": "resource", "ref": "eval-run://eval_01"},
  "events": {
    "kind": "event_stream",
    "stream": "evaluation.events",
    "after": 0
  },
  "rollouts": {
    "kind": "relationship_query",
    "from": "eval-run://eval_01",
    "relationship": "contains_rollout"
  },
  "traces": {
    "kind": "relationship_query",
    "from": "eval-run://eval_01",
    "relationship": "has_trace"
  },
  "artifacts": {
    "kind": "relationship_query",
    "from": "eval-run://eval_01",
    "relationship": "produced_artifact"
  }
}
```

Provider-specific payloads survive under namespaced schemas, but the template's base
layout reduces the canonical envelope and relationships. This lets the same queue and
trace components work for Harbor and private attempts.

## Reference live visual

For GameBench or CardBench code-policy optimization, the first template should have:

### Matrix and queue

- cells by benchmark task × policy/candidate × execution profile × seed;
- queued, allocating, starting, running, evaluating, terminal, cancelled, and failed;
- true concurrency/capacity, queue wait, retry/branch lineage, and named blockers;
- budget reserved, metered spend, remaining budget, and stop control.

### Candidate comparison

- baseline and candidate revisions;
- per-seed reward/score distribution, not only a mean;
- delta from baseline with train/scout/held-out scopes kept separate;
- selection/promotion decision and evaluator authority;
- code/policy diff as an artifact, never embedded untrusted HTML.

### Selected rollout

- environment frame/state where supported;
- action, observation, reward components, achievements, and step progress;
- policy/model/provider/effort, partial output, calls, tokens, latency, and cost;
- follow-live and historical scrub using one time-ordered reducer.

### Trace and evidence

- partial Trace V5 open/delta/close spans during execution;
- exact rollout/attempt/policy-session/service-generation correlation;
- transition from partial capture to sealed trace digest;
- evaluator gates, criteria, result, native artifacts, and evidence completeness;
- stream health, producer restart, gaps, and reconnect status.

### Cross-lane comparison

- same task/policy/seed cells paired across execution profiles;
- environment, policy, harness, and evaluator revisions shown before claiming parity;
- native evidence accessible through authorized resource references;
- infrastructure failure kept separate from policy failure and benchmark failure.

## Agent workflow

A general Workshop skill can guide the agent without knowing private Evals internals:

1. discover registered workspace extensions and execution providers;
2. resolve a recipe and inspect its launch plan;
3. create or select a compatible template;
4. open the visual and wait for stream readiness;
5. request launch under an explicit budget;
6. monitor queue, rollouts, traces, scoring, and terminal evidence;
7. save useful template/reducer changes into the extension's granted source root;
8. run extension tests and present the diff for review.

The private extension may ship an additional skill containing repository-specific task
selection and debugging knowledge. That skill lives inside the private Evals checkout,
not Workshop.

## What exists versus what must be built

### Existing pieces

- versioned visual templates with manifests, shells, bindings, fixtures, and tests;
- durable visual records/revisions in Workshop;
- visual MCP create/save/show operations;
- a configurable visual root for local development;
- Evals append-only event files, results index/service, and SSE projection;
- Containers task, rollout, trace, checkpoint, capability, and HTTP surfaces;
- live eval and rollout reference templates.

### Missing product support

1. multi-root template catalog with registered extension identity and precedence;
2. persistent extension registration and per-workspace enablement;
3. safe local TSX/reducer build, cache, reload, and last-good rollback;
4. explicit agent write grants for extension source roots;
5. declarative recipe catalog and generic provider launch operations;
6. provider readiness handshake before launch;
7. durable Workshop mirror for replay-plus-live event streams;
8. typed relationship queries from eval run to attempts, rollouts, traces, and artifacts;
9. one partial-trace reducer that deterministically seals to Trace V5;
10. policy × task × execution-profile matrix controls and comparison visual;
11. extension export/import that excludes credentials and private run data by default;
12. compatibility migration for existing locally saved visual instances.

## Acceptance tests

| ID | Requirement |
| --- | --- |
| PWX-01 | Register a local extension explicitly; bundled and local catalogs merge without shadowing or ID collision. |
| PWX-02 | Restart Workshop and recover registration, visual instances, template digests, and last-good compiled bundle. |
| PWX-03 | Agent write outside the granted extension root or through a symlink escape is rejected. |
| PWX-04 | A malformed template/reducer fails build while the prior activated revision keeps rendering. |
| PWX-05 | No private runner name, module, path, command, or credential appears in Workshop source, bindings, logs, or exported visual records. |
| PWX-06 | Resolve a GameBench recipe to a task × policy × execution-profile × seed plan before launch and spend. |
| PWX-07 | Open visual, acknowledge stream readiness, then launch; first committed event is visible without reload. |
| PWX-08 | Harbor and private attempts both execute through Containers and reduce into the same canonical visual contract. |
| PWX-09 | Kill/restart policy, environment, relay, evaluator, and Workshop at fault-injection boundaries; recovery follows declared semantics without hidden loss. |
| PWX-10 | Partial trace items appear live and reduce to the exact sealed Trace V5 digest. |
| PWX-11 | Queue counts, concurrency, rewards/scores, usage/cost, and failures reconcile with authoritative terminal artifacts. |
| PWX-12 | A paired cross-profile comparison refuses equivalence when task, policy, harness, environment, or evaluator revisions differ. |
| PWX-13 | CardBench can add a private recipe/template package without any Workshop code change. |
| PWX-14 | Unregistering the extension leaves run data and visual records intact and shows a typed missing-renderer state. |

## Recommended first slice

Build the generic extension seam and one private package together:

1. multi-root catalog plus extension registration;
2. trusted development-mode build/reload for one local source root;
3. generic eval recipe resolve/create/cancel operations;
4. committed replay-plus-live stream mirror;
5. `gamebench.code_policy.live.v1` with matrix, queue, selected rollout, partial trace,
   rewards/scores, cost, and terminal evidence;
6. paired Harbor/private execution over a small Craftax code-policy matrix;
7. restart/reconnect fault injection and a saved terminal replay;
8. repeat with CardBench without changing Workshop.

That slice proves the product extension mechanism, the Containers execution boundary,
and the dogfood workflow at the same time. A second private benchmark should require
only new extension content and provider-owned recipes—not a Workshop feature branch.

# Container compat — env, policy, world, task

**Status:** First workstream for the Aug 12 Containers + Optimizers update. Working note, not a replacement for the ontology or master plan. **C7 and C8 signed off 2026-08-12** — floor may start.

**Parent:** `aug_12_update.md` (acceptance A1, A2, A5, A7; capstone **A8** dig.bench).  
**Handoff (Optimizers + Workshop visuals):** `HANDOFF_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md` + `PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`. C7 + C8 enough. No second stream, no outer `synth.stream-event.v1`, no signed `child_eval_ref`, no sibling `harness_ref`. `/reward` v1 = scalar + `node_results[]`.  
**Authority:** `execution_ontology.md`, `execution_platform_master_plan.md`.  
**Code today:** `containers/src/synth_containers/compat/{harbor,openenv,archipelago,base}.py`. There is no Prime adapter.

This note exists because the wrong first move is a lowest-common-denominator `/reset` `/step` `/reward` that claims Harbor, OpenEnv, Prime Verifiers, and Archipelago are the same container. They are not. Compat is a receipt with an adapter chain. It is not a shared gym.

---

## 0. What we are doing first

Pin **Environment / Policy / World / Task / EvaluationPlan** as independent nouns. **Policy = harness + config** (typically model and other model info; optional code). Harness is a facet of policy, not a sibling service. Worlds are allowed to be code-heavy (Compose, long-lived workspaces). Tasks stay light. Reward and its computation DAG become a first-class Containers responsibility, with a score endpoint that can grade a sealed rollout or a provided result. Optional affordances (snapshots, restore, fork, pause, …) are advertised with fidelity and matched at bind time; unsupported is a valid answer.

Then fold **Harbor** as the only first-class external format. OpenEnv and Prime stay wrappers. Archipelago stays research (but its Compose-world + post-seal grade is the world/eval split we want).

ASCII atlas of the machine, per-benchmark affordances, and format bridges: **§Map** (after §4.10). Reward production: **§3 `/reward`**.

Do this before optimizer multiplex (A3/A4) and before Prime/Chess. A1 (Craftax 10×) and A2 (Harbor GameBench live) are the proofs that the split is real.

Drop the false OpenEnv checkpoint claim **now** (`compat/openenv.py` defaults `checkpointable=True` and advertises `TRUE_ENVIRONMENT_SNAPSHOT`). That bug is the LCD failure already in tree.

---

## 1. Nouns Containers must keep distinct

A container is a **deployment unit**. It is not an environment, a task, a policy, or a score.

```text
  World         heavy initial scenario: compose, image, engine, workspace
  Task          light objective + eval plan pin over a world revision
  Environment   running mutation of a world (sandbox / engine / MCP)
  Policy        harness + config (+ optional code)
                  harness   the loop: ReAct, IsolatedPolicyProcess, Apex runner,
                            τ² Orchestrator, Harbor agent
                  config    model, effort, tools, api_family, credential_mode
                  code      prompt modules, heuristic_policy.py, LoRA (optional)
  EvaluationPlan  reward DAG: nodes, authorities, combiner, gates
  Evaluator     one execution of a DAG node over sealed evidence
  Snapshot      grading/diff capture                    often NOT restore
  Checkpoint    restore-capable capture                 rare, proven
  RewardSignal  env-authored step/episode sample
  Score/metric  evaluator/rubric output                 never silently = reward
  Gate          fail-closed admissibility               not a score
```

There is no `HarnessService`. Updating the loop is a policy-facet affordance (`update_harness` on PolicyService). Bouncing the loop is `restart_policy`. Harbor’s fused “agent” is this shape, not a special case.

Required on create-rollout / create-eval:

```text
world_ref              WorldDefinition revision (compose/workspace/engine)
environment_ref        service + generation bound to that world
policy_ref             PolicyDefinition: harness + config (+ code)
user_policy_ref        optional; τ² user simulator (another Policy, same shape)
task_ref               light TaskDefinition (instruction, limits, eval plan)
evaluation_plan_ref    reward DAG revision (scripts, rubrics, combiner)
task_instance_id       content-addressed pin (seed/split/role/extras)
stream                 named transport; consumer cursor = sequence
                       create-rollout **echoes** the stream descriptor
                       (id, poll/SSE URLs, cursor.kind=sequence, /reward URL)
```

Logical service IDs exist even when they share one process. That is how Workshop binds a visual to the env stream, Laguna to policy, and an optimizer to child evals without rewriting the task.

Restart vocabulary stays typed: reconnect ≠ restore ≠ retry ≠ branch ≠ replay ≠ rescore. Filesystem leftover is not a checkpoint. Rescore is a new `EvaluationExecution`; the original evidence and result stay immutable.

---

## 2. Worlds are heavy; tasks are light

A world can be a Compose graph, a training workspace, a game engine, or a Harbor image. A task must not carry that weight. Many tasks share one world.

```text
  WorldDefinition  (heavy, reusable)
    Archipelago     compose: MCP apps + populate snapshot
    PostTrainBench  long-lived workspace: base model, GPU, datasets, ckpts
    Craftax rust    engine binary + world/rules/readout profiles
    Harbor env      environment/Dockerfile (+ optional verifier image)
    OpenEnv         published env image / server
         |
         |  instantiate
         v
  EnvironmentService / WorkspaceInstance
         |
         |  many light pins
         v
  TaskDefinition   instruction, limits, deliverables, evaluation_plan_ref
  TaskInstance     seed / split / role / case  (still light)
```

| World (code-intensive) | Task (lightweight) |
| --- | --- |
| Archipelago Compose + MCP topology + population recipe | “Grade this snapshot against these criteria” |
| PostTrainBench workspace (H100, 10h, datasets, `final_model` path) | “Produce `final_model` under this budget” |
| Craftax rust gold HTTP | seed 0–9, `world=craftax_default`, `rules=symbolic_survival` |
| Harbor `environment/` image | `instruction.md` + `tests/` pin (TB3 dataset row) |
| GameBench engine | one DEO hillclimb / one interactive rollout |

Do not stuff Compose YAML, Dockerfiles, or training stacks into the task record. The task names a `world_ref` and an `evaluation_plan_ref`. Seeds and splits are instance knobs, not new worlds.

---

## 3. Reward DAG is a first-class Containers responsibility

Scoring is not “the float that came back with `/step`.” Containers owns **EvaluatorService** and an **EvaluationPlan**: a declared DAG of nodes, each with authority, inputs, and whether it may emit reward, score, metric, or only a gate.

```text
                    EvaluationPlan@revision
                              |
          +-------------------+-------------------+
          |                   |                   |
     [gates]            [script nodes]      [rubric nodes]
   fail-closed          trusted code         LLM/panel judge
   (contract,             score.py             rubric.md
    leak scan,            eval.sh              criterion*
    A/A control)          tests/test.sh
          |                   |                   |
          +--------+----------+--------+----------+
                   |                   |
            [env reward]         [heldout / integrity]
            Craftax step          disjoint seeds,
            OpenEnv step          promotion judges
                   |                   |
                   +--------+----------+
                            |
                     combiner (declared)
                     product | all-pass | statistical bound | env-sum
                            |
                     Reward? Score? Gate? Promotion?
                     (typed; missing stays missing)
```

Rules:

- Each node names **authority** (environment, trusted_scorer, rubric_verifier, integrity_judge). Harbor `reward.txt` is a script-node output, not an env `RewardSignal`.
- Contract / interface pass is a **gate**, not a reward. REB: contract failure → 0 and terminal; contract pass **emits no reward**. Efficacy lives on a later node.
- Metrics with weight 0 stay metrics. Integrity can fail a high functional score (PostTrainBench).
- Combiner is pinned on the plan (`script_verdict × rubric_verdict`, statistical bound, env-sum, …). Do not invent a float in the adapter.
- Missing ≠ 0. Scorer refusal is a null score with a reason (`native_harbor_trusted_scorer_failed`), not `0.0`.
- Rescore / rejudge = new execution of the same plan (or a new plan revision) over the same evidence.

### `/reward` — produce the reward for a rollout

This is the product endpoint. Not a float on `/step`. Not a second schema from `/score`.

```text
  RewardSignal     env/proxy authored, lives in the durable log (Craftax step, OpenEnv step)
  /reward          EvaluatorService runs EvaluationPlan over evidence → attempt reward
  Score / metric   node outputs that are not the combiner's reward field
  Gate             fail-closed; may omit reward entirely
```

`POST /score` is the same resource (`/reward`). Do not ship both. Consumers (Workshop, GEPA, SFT checkpoint-eval, visuals) call `/reward`.

#### Routes

```text
POST /reward                      compute (or idempotently return) an EvaluationExecution
GET  /reward?rollout_id=          latest terminal execution for that rollout
GET  /rollouts/:id/reward         same
GET  /evaluations/:execution_id   the execution record (nodes, reasons, evidence pin)
GET  /evaluations/:id/events      poll/sse while a long node (Harbor verifier) is running
```

`POST` produces. `GET` reads. A rollout with no execution yet: GET returns `status=absent`, `reward=null` — not `0.0`.

#### Request

```text
POST /reward
  rollout_id              XOR evidence
  evidence                caller-provided EvidenceSet (snapshot pair, artifacts, trace refs)
  evaluation_plan_ref     default = the plan bound on the attempt
  mode                    terminal (default) | provisional
  node_ids                optional subgraph (gates only, heldout only, …)
  idempotency_key         optional; same key + same body → same execution_id
  rescore                 default false
```

Rules:

- Exactly one of `rollout_id` or `evidence`. Evidence path is Archipelago / PostTrainBench / optimizer-owned candidate. Rollout path is the default for live evals.
- Plan must be the bound plan, or an allowed rescore plan (new `EvaluationPlan` revision). Arbitrary plan injection is refused.
- `rescore=false` + same plan digest + same evidence digest → return the existing execution (do not mint a second float). `rescore=true` → new `execution_id`; previous record stays immutable.
- `mode=terminal` requires the rollout to be sealed / agent-exited / env-terminated as the plan demands. Else `409 incomplete` with `missing_evidence[]`.
- `mode=provisional` is allowed only if the plan advertises `live_reward` (typically an env-sum node over RewardSignals so far). Harbor TB3: `live_reward=unsupported`. Craftax: provisional = sum of RewardSignals in the log up to cursor; not a second engine.
- Do not mutate the env to score. EvaluatorService has no workspace write.

#### Response

Not a bare float. Align with Trace V5 `RewardRecordV1` (`value` nullable, `missing_behavior=omit`).

```text
EvaluationExecution
  execution_id
  rollout_id?                 absent on provided-evidence path
  evaluation_plan_ref
  evidence_set_ref            digest of what was graded
  status                      pending | running | scored | gated | refused | incomplete
  reward                      float | null     ← THE combiner field
  reward_definition_id        pin to RewardDefinitionV1
  components                  named vector (optional)
  node_results[]
      node_id
      kind                    gate | env_reward | script | rubric | heldout | integrity | aggregate
      authority               environment | trusted_scorer | rubric_verifier | integrity_judge | plan
      status                  scored | gated | refused | skipped | running
      value                   float | null
      metric?                 if this node is not allowed to be the reward
      reason?                 native_harbor_trusted_scorer_failed, …
      evidence_refs[]
  combiner                    as pinned on the plan (not invented here)
  reasons[]                   why reward is null
  stream?                     { poll, sse } when status is pending/running
```

`status=scored` is the only case where `reward` may be a number, and only if the combiner actually produced one. `gated` / `refused` / `incomplete` → `reward=null`. A gate that the plan declares as “fail → 0” is an explicit combiner rule, not a missing-to-zero coerce. REB contract fail is that case and must be named on the node (`gate.contract` value 0, authority trusted_scorer), not smuggled as a default.

HTTP:

```text
  200   execution complete (scored | gated | refused)
  202   accepted, running (Harbor verifier, NL-assertion judge); poll execution events
  404   unknown rollout_id
  409   incomplete evidence, plan mismatch, live rollout + terminal mode,
        provisional requested but live_reward=unsupported
  422   malformed body / both rollout_id and evidence / neither
  403   caller is the agent workspace trying to run the scorer
```

#### What `/reward` actually runs

The bound `EvaluationPlan` is a DAG. `/reward` walks it. Each node has inputs, authority, and whether it may emit `reward`, `score`, `metric`, or only `gate`.

```text
  env_reward       READ RewardSignals already in the durable log.
                   Combiner on the node is declared (sum, last, discounted, …).
                   Does not call /step. Does not invent 0 for a missing step.
                   Craftax A1: this node IS the attempt reward (env-sum).

  script           Run the trusted scorer image / eval.sh / tests/test.sh
                   against sealed evidence. Workspace copy is not used.
                   Harbor reward.txt | reward.json is this node's output.
                   Parse named fields; only the plan's combiner field is reward.

  rubric           Judge Policy (not the SUT) over declared artifacts.
                   InternBench: script_verdict × rubric_verdict.

  heldout          May create child rollouts (new identities), wait, then
                   aggregate their /reward results. Parent does not reuse
                   child env-sum as its own reward.

  integrity        Independent judge. Can null a high functional score
                   (PostTrainBench PromotionVerdict).

  gate             Fail-closed. On fail: status=gated, reward=null
                   UNLESS the plan explicitly maps that gate to a number.

  aggregate        Combiner over named node outputs. Product, all-pass,
                   statistical bound, env-sum, weighted — pinned, not guessed.
                   Required input missing → reward=null, not identity 1.0.
```

tau3 today: `reward *= breakdown.get(basis, 1.0)` treats a missing basis as 1. That is the product-combiner version of missing→0. A required `reward_basis` that did not run leaves `reward=null`.

Long nodes (Harbor `test.sh`, NL assertions): execution starts, `202`, events on `/evaluations/:id/events`. Completeness is `execution.status`, not SSE `[DONE]`.

#### Live RewardSignal vs `/reward`

```text
  /step or engine NEV     emits RewardSignal into the log     (env authority)
  visual / GEPA child     may display running env-sum         (provisional, labeled)
  POST /reward            seals the attempt reward            (plan authority)

  Do not:
    wait for /reward to create Craftax step rewards
    treat OpenEnv step.reward as the EvaluationExecution
    treat Harbor reward.txt as an env RewardSignal
    write reward onto create-rollout because “we will score later”
```

A1 visual can plot RewardSignals live. The number in the leaderboard / optimizer is `GET /reward`. If `/reward` has not been produced, the cell is empty.

#### Idempotency, rescore, children

```text
  first POST after seal          produce; pin execution_id on the rollout
  GET                            that pin
  POST same plan+evidence        return the pin (unless rescore=true)
  rescore                        new execution; old remains; GET default = latest
                                 terminal scored/gated/refused (not incomplete)
  child rollouts                 each has its own /reward
  parent /reward                 reads child execution pins; 409 if a required
                                 child is absent
```

Optimizer (GEPA, SFT campaign) owns search aggregation. `/reward` does not return a Pareto front.

#### Who is allowed to run it

The plan’s scorer identity (image digest, `score_execution_identity=root_linux`, judge policy_ref) is bound at attempt start. The SUT policy must not POST `/reward` against a workspace-local `eval.sh`. Harbor: verifier image ≠ agent image. Nested DEO: child Craftax `/reward` is env-sum; parent Harbor `/reward` is the hillclimb DAG.

#### Per-benchmark (what `/reward` does)

```text
  Craftax A1
    evidence     sealed NEV + RewardSignals (rollout_id)
    plan         one env_reward node, combiner=sum
    terminal     episode terminated/truncated
    provisional  native (sum so far)
    missing step → that delta absent; do not fill 0
    true_checkpoint unused by /reward (restore is not scoring)

  Craftax DEO child
    same as A1 per seed rollout
    parent Harbor /reward  script+heldout DAG over child execution pins
                           + baseline delta + improvement GATE
    do not flatten heldout into engine step reward

  Harbor TB3 / TBLite
    evidence     leftover workspace + tests/ (grading_snapshot)
    plan         script node (test.sh → reward.txt|json)
    terminal     agent exited; submission captured
    provisional  unsupported
    202 while verifier runs
    reward.txt is script output, not env

  APEX / Archipelago
    evidence     provided: initial snapshot + final snapshot + trajectory
                 (often no Containers-owned rollout)
    plan         rubric/criteria over diff + selected artifacts
    terminal     snapshots sealed
    provisional  unsupported
    no env RewardSignal; inventing 0 is the LCD bug

  τ / τ² / tau3
    evidence     trajectory + env DB + gold actions
    plan         reward_basis nodes (db, actions, nl_assertions, communicate)
                 combiner = declared product (not get(, 1.0))
    NL node      judge Policy, not the user simulator
    user-sim cost is usage, not reward
    provisional  unsupported unless a basis is purely env-DB and advertised

  GEPA Banking77
    evidence     rollout_id (classify trace)
    plan         task metric node (not env-sum)
    /reward      per child eval; optimizer aggregates

  OpenEnv Echo
    evidence     step RewardSignals in the wrap's log
    plan         env_reward (env authority preserved)
    do not re-label as Harbor script
    state() is not evidence for /reward

  Prime GSM8K
    plan         taskset reward authority AND/OR rubric metrics
    metric ≠ reward; weight-0 stays metric
    if Prime nests Harbor/OpenEnv, inner /reward is inner;
    Prime combiner is a parent node. Chain on the receipt.

  dig.bench A8
    evidence     sealed relay log (rollout_id); status in the log
    plan         one env_reward node, authority=environment
                 completed → 1.0; game_over → 0.0
    terminal     state.done / status in {completed, game_over}
    provisional  unsupported (levels/lives are stats, not reward)
    GET before POST → null
    POST does not call step / start_session
    token never in evidence

  PostTrainBench
    evidence     final_model + workspace audit snapshot
    plan         downstream benches (scores) + integrity (gate)
    high score + failed integrity → reward null or ineligible
                 PromotionVerdict, not a quiet 0
```

#### Affordances on EvaluatorService

```text
  POST /reward            native (this is the product)
  live_reward             native only if a node can run on a prefix of the log
  provided_evidence       native if the plan accepts an EvidenceSet without rollout_id
  rescore                 native (new execution, immutable prior)
  separate_verifier_image native when the scorer is not the env
  subgraph                prefer (node_ids)
```

Recipe `require live_reward` refuses Harbor TB3. Recipe `unused` env_reward is GEPA Banking77.

#### What we will not do

```text
  embed a float on create-rollout and call it scored
  default missing / refused / gated to 0.0 or 1.0
  let the agent image run the scorer
  POST /reward that steps the env
  one LCD /reward that is secretly /step.reward
  treat ATIF / site HTML as the execution record
  mix child Craftax env-sum into the Harbor trial reward field
```

Trace V5: `/reward` writes `RewardRecordV1` + optional `RewardAggregationV1` onto the evidence bundle (`tracing/evidence_ops.evaluate` already appends a typed result to a sealed trace). The HTTP resource is the EvaluationExecution; the bundle is the seal.

---

## 4. Optional affordances (snapshots, restarts, and friends)

Some algorithms need snapshots, forks, or true restore. Some worlds cannot provide them, and that is fine. The protocol is **honest discovery + bind-time matching**, not a boolean that defaults to true.

Containers already has the vocabulary (`PrimitiveProtocol`, `CheckpointSemantics`, `ResumeSemantics`, `CapabilityLevel`). It is being used as LCD flags. OpenEnv advertises `TRUE_ENVIRONMENT_SNAPSHOT` by default. Stream-contract `state: { checkpoint: true, resume: true }` is the same trap. Stop that.

### 4.1 Snapshot ≠ restart ≠ checkpoint

```text
  reconnect          same process, cursor backfill          almost always
  pause / resume     hold mutation, continue same episode   optional
  grading snapshot   immutable files/diff for POST /reward   Archipelago, Harbor leftover
  audit snapshot     evidence only, not restore             PostTrainBench workspace
  state() slice      current observation                    OpenEnv; NOT a snapshot
  true checkpoint    restore env+RNG+cursor+hidden          Craftax rust
  fork / branch      new episode from checkpoint            MAPO, some RL
  policy-session     conversation restore, env may differ   Codex
  retry              new attempt                            Harbor default
  replay             consume recording, no mutation
  rescore            new EvaluationExecution, same evidence
```

A filesystem tarball is usually a **grading** or **audit** snapshot. It does not restore a live game. A Craftax checkpoint does. A Harbor leftover sandbox is neither unless the task says so.

### 4.2 Advertise per role, with fidelity, default unsupported

Recovery is per logical service, not a global `checkpoint_support: true`. Ontology already has this shape:

```json
{
  "role": "environment",
  "affordances": {
    "reconnect":           { "level": "native",      "semantics": "cursor" },
    "pause":               { "level": "unsupported" },
    "grading_snapshot":    { "level": "native",      "preserves": ["workspace_files"] },
    "audit_snapshot":      { "level": "derived" },
    "true_checkpoint":     { "level": "unsupported" },
    "restore":             { "level": "unsupported" },
    "fork":                { "level": "unsupported" },
    "concurrent_episodes": { "level": "native",      "capacity": 4 },
    "live_frames":         { "level": "unsupported" },
    "step":                { "level": "unsupported" },
    "blocking_trial":      { "level": "native" }
  }
}
```

Levels: `native` | `derived` | `approximate` | `unsupported`. Default is **unsupported**. `native` and `derived` require a proof test. `approximate` is not good enough for a scored attempt unless the recipe explicitly accepts it.

Same block on policy (session resume, harness facet, config). Evaluator is always rescore-on-immutable-evidence; never needs live mutation.

World and task can **narrow** the env: a Craftax engine may be checkpointable, but this task forbids restore inside a scored attempt. Task wins.

### 4.3 Consumers declare required vs unused

```text
  OptimizerRecipe / Harness / EvaluationPlan

    require     fail closed at bind if env/world/task cannot meet it
    prefer      use if native/derived; otherwise continue without
    unused      do not call; advertising it is irrelevant
```

```text
  MAPO / branch search     require true_checkpoint + fork
  Craftax interactive A1   prefer true_checkpoint (replay/branch);
                           unused grading_snapshot
  Harbor / TB3             unused true_checkpoint;
                           prefer grading_snapshot for verifier
  Archipelago              require grading_snapshot (before/after);
                           unused restore
  PostTrainBench           require audit_snapshot of workspace;
                           unused process restore
  GEPA Banking77           unused env snapshots (stateless classify)
  SFT training ckpt        optimizer-side, not an env affordance
```

Bind:

```text
  advertised.level  >=  required.level   →  start
  advertised.unsupported && required     →  refuse, name the affordance
  advertised.approximate && required native → refuse
  unused                                 →  never call the route
```

Do not silently retry instead of restore. Do not snapshot-diff SSE and call it a checkpoint. Do not start MAPO on Harbor TB3 and invent forks.

### 4.4 Other optional things on the same protocol

Not a second surface. Same fidelity enum, same bind rule.

| Affordance | When it matters | Fine to be unsupported |
| --- | --- | --- |
| `true_checkpoint` / `restore` / `fork` | branchy search, long-horizon RL | Harbor trials, Banking77, Echo |
| `grading_snapshot` | Archipelago, Harvey deliverables, `POST /reward` on files | live Craftax (env reward is enough) |
| `pause` | interactive / human-in-loop | batch jobs |
| `concurrent_episodes` | 10-lane Craftax, two GEPA child evals | single-lease Harbor image |
| `step` vs `blocking_trial` | OpenEnv, Craftax | Harbor agent-then-verifier |
| `live_frames` | Craftax visual | text tasks |
| `token_trace` | RLVR / logprob algos | most evals |
| `separate_verifier_image` | REB root_linux scorer, Harbor verifier env | in-process env reward |
| `poll` / `sse` / `ws` | A5 | (poll required for authoritative runs) |
| `multi_actor` | Overcooked, negotiation | single-policy tasks |
| `update_policy` / `bind_policy_config` | keep world, swap agent (§4.6) | fused Harbor agent image |
| `restart_deployment` | fallback when hot-swap is unsupported | — |
| `world_start` / `world_stop` | Compose / engine lifecycle | always-on toy Echo |

Discovery already has route hints (`/checkpoints`, `/resume`, `/fork`). A route without a `native`/`derived` claim is not an affordance. Absence of a route is `unsupported`, not an error.

### 4.5 What is already wrong

`RuntimeCapabilitySurface` mixes real semantics (`CheckpointSemantics.GRADING_SNAPSHOT`) with booleans (`checkpoint_support`, `true_environment_snapshot`) that OpenEnv sets true together. Stream contracts flatten to `state.checkpoint: true`. That is how “optional” becomes a lie.

Fix: semantics enum + fidelity per affordance; booleans only as derived views of `level != unsupported`. Algorithms read the enum, not the boolean.

### 4.6 Policy swap, config registry, world CRUD

Yes. Same protocol. These are lifecycle affordances on **logical services**, not a second API family.

The point of splitting Environment / Policy is so a Craftax world can stay up while the policy changes. Policy **is** harness + config. If the policy process is separate from the env, `update_policy` / `update_harness` / `bind_policy_config` are native. If they are one fused image with the env, those are unsupported and the fallback is `restart_deployment`. Do not pretend a full bounce is a hot swap. Do not invent a third service for the loop.

**Identity rule:** never mutate an in-flight scored attempt. A policy update creates a new `PolicyRevision` and a new `PolicySession`. The environment generation may stay. The next rollout binds the new refs. An attempt already running keeps the old pin.

```text
  PolicyDefinition          registered, immutable once used in a scored run
    harness                 loop impl + bounds (ReAct, Apex agent_id, Orchestrator, …)
    config                  model, effort, tools, api_family, credential_mode
    code                    program / prompt / adapter digest   (optional)
         |
         |  bind
         v
  PolicyService instance    running policy (harness process + bound config)
         |
         |  create-rollout
         v
  Attempt  binds environment_ref + policy_ref
```

Three different swaps **inside** policy, plus bounce. Do not collapse them, and do not promote harness to a peer of env:

| Affordance | What changes | Keeps | Example |
| --- | --- | --- | --- |
| `bind_policy_config` | model / effort / tools | world, env, harness, code | Luna med → Sol med on same Craftax engine |
| `update_harness` | loop impl / bounds | world, env, config, code | ReAct compact_every; Apex `agent_id`; τ² duplex |
| `update_policy_code` | program, prompt, LoRA adapter | world, env, harness, maybe config | DEO `heuristic_policy.py`; GEPA candidate |
| `update_policy` | any mix in one call | world if processes are split | convenience; still new PolicyRevision |
| `restart_policy` | bounce PolicyService (the loop dies with it) | env process | hung Luna/ReAct; IsolatedPolicyProcess; Apex runner |
| `restart_deployment` | bounce the whole container | nothing live | Harbor fused agent+env |

There is no `restart_harness`. The harness lives in the policy process.

```text
  advertised update_policy=native
      →  PUT /policy  { config_id | harness | code }
      →  new PolicyRevision, env generation unchanged
      →  subsequent rollouts use it

  advertised update_policy=unsupported
      →  caller must restart_policy or restart_deployment
      →  if recipe required hot-swap, bind refuses
```

**Policy config registry** is the right product shape. The container (or the provider) holds named configs:

```text
POST /policy-configs     { id, model, effort, tools, ... }   → policy_config_id
GET  /policy-configs
POST /rollouts           { ..., policy_ref: policy_config_id }
```

Workshop / GEPA / SFT then “register Luna med” and “register Sol med” and run rollouts against the same world without rebuilding the Craftax image. That is A1/A2/A3. A config is not a secret; credentials stay named, not inlined.

**World CRUD** is the same idea on the heavy side:

| Affordance | Meaning |
| --- | --- |
| `world_install` | materialize a WorldDefinition (pull image, compose, engine) |
| `world_start` / `world_stop` | process lifecycle (Archipelago compose, Craftax gold HTTP) |
| `world_lease` | capacity-scoped right to create episodes |
| `world_reset` | new episode from the world pin (not a task rewrite) |
| `task_bind` | light task + eval plan onto a running world |

Adding a world is not adding a task. Starting a world is not starting a rollout. Harbor will often only support install+start as one shot (`unsupported` for independent `world_start`). Compose-shaped worlds should support start/stop.

**Worth considering on the same list** (still require/prefer/unused, still default unsupported):

| Affordance | Why |
| --- | --- |
| `update_harness` | policy-facet: swap ReAct / Apex `agent_id` / τ² orchestrator; still `PUT /policy` |
| `bind_inference` | point PolicyService at Laguna vs cloud vs OpenRouter |
| `load_adapter` | SFT checkpoint → sampler on the same env (narrower than `update_policy_code`) |
| `scale_leases` | 1 → 10 concurrent Craftax episodes |
| `mount_tools` / `mcp_bind` | Archipelago `POST /apps`; not a policy/harness update |
| `compact_policy_session` | Codex compact; new session segment |
| `freeze_world` / `thaw_world` | pause mutation while scoring or swapping policy |
| `replace_eval_plan` | new rubric revision; does not mutate old scores (use `POST /reward`) |

Do not add `update_task` that rewrites instruction mid-attempt. That is a new `TaskInstance`. Do not add `update_reward` that patches a float; that is a new `EvaluationExecution`.

GEPA Banking77 wants `bind_policy_config` + `update_policy_code` (prompt modules) and usually unused env snapshot. Craftax 10× wants `bind_policy_config` (Luna vs Sol on the same ReAct harness) and `prefer true_checkpoint`. Craftax **code-policy DEO** wants `update_policy_code` + `restart_policy` **native** on the playing PolicyService (engine stays). APEX wants `update_harness` + `restart_policy` **native** on the runner-as-policy while Compose/MCP stays; `POST /apps` is `mcp_bind`, not a policy update. τ² / tau3 wants Orchestrator as the **agent policy’s harness**, plus a **second** Policy (user simulator = its own harness + config). SFT checkpoint-eval wants `load_adapter` while the Craftax world stays up. Harbor TB3 wants none of the hot-swaps; `restart_deployment` per trial is honest — and Harbor’s fused agent is already policy = harness + config. The Harbor **author** (Codex) is a different Policy from the Craftax **player**. Do not collapse them.

### 4.7 Craftax worked example

Target split (today evals talks to gold HTTP and runs ReAct in-process; Containers should look like this):

```text
  WorldDefinition     craftax_default + rules/symbolic_survival + readout/text
  EnvironmentService  rust gold HTTP          (engine, NEV log, frames, RNG)
  PolicyService       harness = ReAct (plan 5–20, compact_every 16)
                      config  = Luna/Sol/Laguna (model, effort, inference)
  EventRelay          durable log             (poll required; SSE/WS adapters)
  EvaluationPlan      env-sum RewardSignal    (engine payout table)
```

Advertised (honest):

```text
  environment
    step, live_frames, true_checkpoint, fork     native   (if restore proof exists)
    poll                                         native   (engine: GET /event_log + nev_cursor)
                                                 consumer: sequence (relay materializes)
    sse, websocket                               derived  (relay over the same log)
    partial_trace                                native   (NEV kinds verbatim, open while live)
    grading_snapshot                             unsupported
    update_rules_in_place                        unsupported   (rules are world revision)

  policy  (interactive Luna/Sol recipe)
    bind_policy_config, bind_inference           native   (config half)
    update_harness                               native if ReAct is out-of-proc with env;
                                                 else restart_policy
    update_policy_code                           prefer   (prompt/adapter; not the rust bin)
    restart_policy                               native   (ReAct+sidecar died; engine lives)
    partial_trace                                native   (span open / token deltas / close)
    sse                                          native
    websocket                                    prefer   (steer / interrupt)

  policy  (code-policy DEO recipe — same world, different Policy)
    harness                                      IsolatedPolicyProcess
    code                                         heuristic_policy.py
    update_policy_code, restart_policy           native
    bind_policy_config                           unused   (no chat model on the player)
    bind_inference                               unused
    update_harness                               unused   (the process IS the harness)

  world
    world_start / world_stop / world_lease       native
    scale_leases                                 native   (10 concurrent seeds)
    task_bind                                    native   (seed is the instance)
```

Engine has **no push plane** today: whole `event_log`, client-side cursor. Relay must not invent a second schema. SSE/WS are delivery of that log. Partial traces are the NEV + policy spans **while the rollout is open**, not a dump at `eval.run.terminal`.

#### Adding a world / task

```text
  POST /worlds     { id: craftax_default, image, rules: symbolic_survival, readout: text }
  POST /worlds/:id/start
  POST /tasks      { id: craftax.seed, world_ref, evaluation_plan: env_reward_sum }
  POST /policy-configs   luna_med = { model: gpt-5.6-luna, effort: medium }
  POST /policy-configs   sol_med  = { model: gpt-5.6-sol,  effort: medium }

  visual connect (SSE or WS) → stream.subscribed → ready →
  POST /rollouts  × 10
      task_instance  seed 0..9
      policy_ref     { harness: react_v1, config: luna_med }
      stream         { transport: sse }     # consumer cursor is sequence; nev_cursor stays internal
```

A new task is another light pin (held-out seeds 400–409, or DEO code-policy on the **same** engine). A new world is a new rules/readout/engine pin, not ten more seeds.

#### Update env rules

Rules live on the **world**, not the task. `symbolic_survival` → `symbolic_survival.v2` is a new `WorldDefinition` revision.

```text
  in-flight episodes     keep old world_revision
  update_rules_in_place  unsupported on Craftax gold
  next episodes          world_stop? no — start a second world generation
                         OR world_reset only if the engine can load rules
                            without dropping RNG of other leases
```

Do not PATCH rules on a live seed-3 zombie fight. That would invalidate the reward table mid-attempt.

#### Update policy harness

`compact_every` 16 → 8, `plan_max` 20 → 12: new `PolicyRevision` (harness facet). Config (Luna med) stays.

```text
  update_harness=native     load new ReAct loop; env process untouched
  update_harness=unsupported
      restart_policy        bounce the policy process (loop lives there)
      restart_deployment    only if the loop is compiled into the env image (it should not be)
```

In-flight lanes finish on the old compact_every. Prefix-cache identity changes with the new harness; that is expected and must show up in usage provenance. Same route family as config: `PUT /policy { harness: … }`, not `PUT /harness`.

#### Update policy config

Luna med → Sol med, same world, same seeds:

```text
  POST /policy-configs  sol_med
  POST /rollouts        seed 0..9, policy_ref=sol_med
  engine stays up
```

No rust rebuild. No world restart. Visual can already be open (second run, flip). If you need both in parallel, `scale_leases` ≥ 20 or two world leases. If `scale_leases` is exhausted, create-rollout **refuses** (busy/queued) — do not silently share a world lease.

#### Restarts (typed)

```text
  reconnect           visual dropped → poll after sequence → SSE/WS tail
                      (relay may translate to engine nev_cursor internally)
  restore             true Craftax checkpoint (engine+RNG+event_cursor)
  fork                new episode from checkpoint (MAPO-style); parent immutable
  restart_policy      policy process died (ReAct+Luna, IsolatedPolicyProcess, Apex runner);
                      freeze_world optional; env lives
  restart_deployment  gold HTTP crashed; restore from checkpoint or retry
  retry               new attempt, new episode, usually new seed instance
```

Cadence markers in NEV are **not** checkpoints. Only restore if `true_checkpoint=native` was proven.

#### Code-policy DEO: restart the player (first-class HTTP)

This is the example that makes the env/policy split real. GameBench `craftax-singleplayer/code_policy_opt` (evals dock + Harbor bundle) has the LLM **author** a `heuristic_policy.py`, then **play** it against Craftax. Those are two PolicyServices. The author wants to bounce the player without bouncing the world.

Today that intent is a script trick:

```text
  Codex (Harbor author)
    writes  agent_candidates/craftax/<id>/heuristic_policy.py
    shells  python workspace/run_craftax_gamebench_hillclimb_task.py run
              → run_hillclimb.py → run_policy_sweep.py
              → IsolatedPolicyProcess(path)   # JSONL IPC sandbox child
              → CraftaxEngine() / RustReplSession.reset(seed)
```

`IsolatedPolicyProcess` (`gamebench/tasks/shared/codepolicy/policy_subprocess.py`) is already a PolicyService: copy bytes into a sandbox, spawn a child, observation/action JSONL, `close()` + new process for new bytes. The rust REPL / gold HTTP can stay. The hillclimb wrapper, `importlib` `load_policy_module` cache, and “run the sweep script again” are **not** the protocol.

Containers must expose that split as endpoints on the **Craftax** container (the child env that plays). No shell, no importlib cache, no restart of gold HTTP as a stand-in.

```text
  POST /policy-revisions
      { source: bytes | uri, entry: "choose_actions" }
      → PolicyRevision { id, digest, isolation_receipt }

  PUT  /policy
      { policy_revision_id }
      → bind as current player; new PolicySession
      → EnvironmentService generation unchanged
      → in-flight scored attempts keep the old pin

  POST /policy/restart
      { policy_revision_id? }     # default: last bound revision
      → terminate IsolatedPolicyProcess (SIGTERM / docker rm)
      → spawn a new child from the same digest
      → engine, other leases, durable log stay

  GET  /policy
      → current PolicyRevision + PolicySession + isolation_receipt

  POST /rollouts
      { policy_ref, task_instance: seed, stream }
      → next episode on the still-running world
```

`PUT /policy` with new bytes **is** `update_policy_code` (and implies a player bounce). `POST /policy/restart` with the same digest is the hung/crashed-player case. Do not collapse either into `restart_deployment`.

Advertised on recipe `craftax.code_policy`:

```text
  update_policy_code     native
  restart_policy         native
  step, live_frames      native on the env (same as A1)
  true_checkpoint        same claim as the engine, not a new one
  bind_policy_config     unused     (player is a program, not Luna)
```

Fidelity rule: `IsolatedPolicyProcess` (or an equivalent out-of-proc player) is what makes `native` true. In-process `load_policy_module` + `_POLICY_CACHE` is **not** native — same path can serve a stale module after a rewrite, and you cannot unload it without bouncing the worker. If the only implementation is in-proc import, advertise `update_policy_code=unsupported` and `restart_policy=native` only if the worker is the PolicyService (env still a different process). A fused “one Python that is engine+policy” must not claim native hot-swap; that is `restart_deployment`.

Harbor Codex does **not** get these routes on the trial. Codex calls the **child** Craftax container:

```text
  Harbor trial          Codex authors files; no Craftax step
  child environment     craftax.code_policy container
  Codex → PUT /policy + POST /rollouts on the child
        not python run_hillclimb.py
```

The hillclimb script becomes a **compat client** of those endpoints (or goes away). Held-out scoring is still a separate `EvaluationExecution` / `POST /reward` over sealed child rollouts, not a second engine restart.

#### SSE / WebSocket / poll

One durable log per rollout. Request names the transport; response says what bound.

```text
  poll     required     GET .../events?after=<sequence>
                        (engine native is event_log + nev_cursor; relay
                         materializes sequence. Workshop never speaks nev_cursor.)
  sse      default live Last-Event-ID = sequence
  ws       optional     same envelopes; use for step/steer/interrupt
                        (interactive Craftax, not required for A1)
```

A1 visual: require `sse` (or `ws`) **plus** poll backfill. If the engine only has poll, the **relay** derives SSE from the log. Heartbeats do not advance **sequence**. Disconnect does not mint a new counter (today’s snapshot-diff bug). `telemetry.transport=auto` is **refused** on authoritative / visual-attached runs.

Create-rollout **echoes** the stream descriptor (not only which transport bound):

```text
  stream.id
  transports.poll.url
  transports.sse.url            null if not bound
  transports.websocket.url      null if not advertised
  cursor.kind                   sequence          (consumer-facing)
  cursor.producer_kind?         nev_cursor | ordinal   (internal; consumers ignore)
  reward.url                    /reward?rollout_id=…
  auth.mode
  retention                     run | TTL
```

Ready is a **non-advancing** control record on the bound stream, not HTTP 200:

```text
  type: stream.subscribed
  stream.id
  rollout_id | run_id
  next_sequence
  ready: true
```

Then `POST /rollouts` / first mutating event. First **semantic** event remains `trace.opened`.

If someone asks `ws` and the relay has no WS, **refuse** (authoritative run) or the recipe must have said `prefer`. Do not silently fall back to poll-only while a visual is attached.

#### Partial trace streaming

Not “wait until terminal then dump.” OpenResponses discipline on the relay projection:

```text
  trace.opened
    env.episode.opened
      span.step.opened        NEV action_applied / state_transition / ...
        data                  frame digest, reward delta, vitals
      span.step.closed
    policy.session.opened
      span.llm.opened         ReAct plan call
        data                  token deltas (if token_trace advertised)
      span.llm.closed         usage
    ...
    capture.high_water
    capture.closed
  trace.sealing → Trace V5
```

Env partials = NEV kinds + frames **as they happen** (evals already does this via cursor-poll). Policy partials = each LLM call as a span, not one blob at the end. Visual joins them by rollout_id. A zombie death on seed 5 is a closed step with a reward delta **before** the rollout terminals.

`partial_trace=unsupported` would mean Harbor-style “nothing until verifier.” Craftax must advertise `native`. Missing a step reward stays missing; do not fill 0.

Workshop: connect visual → `stream.subscribed` → replay cursor 0..N → subscribe N+1 → **then** first paid Luna call. Scrub rewinds env frames and policy spans together. After seal, same visual from V5; engine may be gone. Artifact **retention** is advertised (`run` vs TTL); silent frame 404 after `world_stop` is a fail.

### 4.8 Harbor GameBench + Codex + proxy

This is A2, not A1. Codex is the **coding agent** in a Harbor trial. The proxy is host-side inference. GameBench Craftax is **content inside the task** (DEO writes a policy, then child games score it). Do not advertise Craftax engine affordances on the Harbor trial.

```text
  World              Harbor env image (code_policy_opt workspace + tests)
  Agent              Codex in the sandbox     Policy (harness + config)
  Proxy              host, UID-isolated       credentials never in the box
                     Codex → REB_INFERENCE_PROXY → Laguna / OpenRouter / cloud
  Evaluator          separate execution after agent exits
                     tests/test.sh → reward.txt | reward.json
  Nested (child)     DEO hillclimb may start craftax_gold
                     that is a child EnvironmentService, not this trial's env
```

Evals already does this (`codex_harbor_runner`, task proxy, lifecycle JSONL `planned → launched → trace_started → submission_captured → verifier_*`).

#### What we can expect (honest)

| Affordance | Level | Notes |
| --- | --- | --- |
| `blocking_trial` | native | Agent runs, then verifier. No gym `step`. |
| `bind_policy_config` | native **before** trial | Proxy + Codex config: Laguna vs Luna vs OpenRouter. Mid-trial swap unsupported. |
| `bind_inference` / `proxied_inference` | native | The proxy **is** this. Agent sees `REB_INFERENCE_PROXY` only. |
| `credential_isolation` | native | Keys stay on the host. Sandbox cannot redirect the upstream. |
| `usage` / implied cost | native | Proxy request log; `harbor.token-cost-summary.v1`. |
| `separate_verifier` | native | Distinct `EvaluationExecution`. `reward.txt` is a **script node**, not env reward. |
| `POST /reward` | native | Score sealed workspace / `rollout_id` after submission. |
| `retry` | native | New trial, new sandbox. |
| `restart_deployment` | native | Bounce image+Codex+proxy together. |
| `world_install` | native | Build/pull Harbor image. Start is usually fused with the trial. |
| `world_start` independent | unsupported | Typical Harbor: one shot per trial. |
| `grading_snapshot` | derived | Leftover workspace + artifacts for verifier / ATIF. |
| `true_checkpoint` / `restore` / `fork` | **unsupported** | Docker leftover ≠ Craftax RNG restore. |
| `step` / `live_frames` | **unsupported** on the trial | Codex is editing files, not playing Craftax in this stream. |
| `update_policy` / `update_harness` / `update_rules` | **unsupported** on the trial | New Harbor trial. Child Craftax player: `update_policy_code` + `restart_policy` **native** (§4.7). Harbor agent already *is* harness+config. |
| `poll` | native | Lifecycle JSONL + `codex_stdout.jsonl` tail + proxy spans. |
| `sse` | derived | Relay over that log. Visual can be honest: phase, tools, tokens, then verifier. |
| `websocket` (env) | **unsupported** | No interactive env session. |
| `websocket` (policy→proxy) | native | Codex Responses WS to the proxy (`OPENAI_RESPONSES_WEBSOCKET_URL`). Token stream, not env stream. |
| `partial_trace` | **derived** | Codex tool/stdout JSONL + proxy spans, open while the trial is live. Not NEV frames. |
| `token_trace` | derived | If the proxy records it. |
| `compact_policy_session` | prefer | Codex compact inside the trial; new session segment. |
| `concurrent_episodes` | weak | Parallel = **multiple Harbor trials** (docker leases), not 10 seeds on one engine. Matrices often `max_parallel = 1`. |
| ATIF | projection | After/during capture; not the durable log. |

#### Nested Craftax (do not flatten)

If DEO actually launches `craftax_gold` to score the written policy, that child run may have A1 affordances (`step`, frames, env reward) **and** the code-policy player endpoints (`PUT /policy`, `POST /policy/restart`). Advertise them on the **child** `environment_ref` / `policy_ref`, linked from the Harbor attempt. The Harbor visual shows agent/verifier; a nested visual may show the hillclimb. Mixing them into one `live_frames=native` on the Harbor fold is the LCD lie.

The LLM’s “just restart the code policy that’s playing” is a child `POST /policy/restart` (or `PUT /policy` with a new revision). It is **not** `restart_deployment` on the Harbor trial, and it is **not** a shell-out to `run_craftax_gamebench_hillclimb_task.py`. If the child cannot advertise `update_policy_code=native` + `restart_policy=native`, the Harbor task is incomplete — do not paper over it with a workspace script.

#### Streaming you will actually see

```text
  poll/SSE  attempt.events     planned, launched, trace_started, submission_captured
  poll/SSE  policy.events      Codex stdout JSONL, tool calls, proxy request spans
  poll/SSE  evaluation.events  verifier_started → reward.txt parsed → combiner
  WS        policy→proxy only  Responses token deltas (optional)
  not       environment.events Craftax NEV / frames           (unless a child run)
```

Connect `live.harbor_eval.v1` before start. Progress is “Codex is editing / proxy call N / verifier running,” not a fake map. Missing verifier score stays missing.

#### Policy config vs interactive Craftax

Same product move, different bind:

```text
  A1  POST /policy-configs luna_med  →  next env steps on gold HTTP
  A2  POST /policy-configs luna_med  →  next Harbor trial's proxy/Codex model
```

You do **not** hot-swap Luna→Sol inside a running Codex sandbox. You start trial B with `sol_med`. That is still two policies on one GameBench Harbor **task**, which is A2.

### 4.9 APEX / Archipelago: policy harness updates

Archipelago already has the env/policy split. The runner **is** the policy (harness + config), not a third service. Do not flatten it into one “agent container” with the MCP world.

```text
  World / EnvironmentService   env container: MCP gateway, populate, snapshot
                               POST /apps          mcp_bind (hot-swap tool topology)
                               POST /data/populate world materialize
                               POST /data/snapshot grading_snapshot (not restore)
                               /mcp/               the world the policy talks to

  PolicyService                agents runner
                               harness = agent_id (react_toolbelt / loop) + max_steps,
                                         timeout, tool_call_timeout
                               config  = model, credentials, system_prompt
                               the process that connects to /mcp/

  EvaluatorService             grading container: snapshot diff + criteria
                               after seal; POST /reward provided-evidence path
```

Their `AgentDefn` is one JSON blob (`agent_id` + `max_steps` + `system_prompt` + model keys). That blob **is** a PolicyDefinition. Containers still names the facets (harness vs config vs code) so `bind_policy_config` does not pretend to be a loop swap.

| Change | Affordance | Keeps |
| --- | --- | --- |
| `react_toolbelt_agent` → `loop_agent`; `max_steps` 100 → 50; timeout | `update_harness` (`PUT /policy`) | MCP world, populated snapshot, config |
| Runner process died / cannot hot-load new `agent_id` | `restart_policy` | env container |
| Claude → Luna | `bind_policy_config` | harness, world |
| New system prompt / toolbelt instructions | `update_policy_code` | harness, world |
| New MCP server set (`POST /apps`) | `mcp_bind` | policy |
| New populate / new task files | `task_bind` / `world_reset` | policy image |
| New criteria | `replace_eval_plan` + `POST /reward` | everything live |
| Bounce env+runner+grader together | `restart_deployment` | nothing live |

```text
  PUT  /policy  { harness: { agent_id, max_steps, timeout, … } }
      → new PolicyRevision; env generation unchanged
      → in-flight attempts keep the old pin

  PUT  /policy  { config: luna_med }
      → same, config facet only

  POST /policy/restart
      → bounce the runner process (harness dies with it)
      → Compose / MCP gateway / other leases stay

  GET  /policy
```

No `/harness` routes. **Native iff the runner is a process that can connect to an already-running env.** Archipelago documents that mode: env is independently runnable; an agent connects to `/mcp/`. That is the product.

The other Archipelago mode — runner **spawns** a sandbox per trajectory (populate → `/apps` → run → snapshot → kill) — is a fused trial. Then:

```text
  update_harness on the *next* trajectory    ok (new PolicyRevision, new sandbox)
  update_harness while keeping this world    unsupported
  mcp_bind on a world the runner just killed unsupported (there is no long-lived world)
```

Do not advertise `update_harness=native` for “keep the world” if the runner always spawn+destroy. That is the same lie as claiming Craftax `restart_policy` when engine+policy are one Python.

In-flight rule is unchanged: never mutate a scored attempt. Prefix-cache / tool-schema identity changes with the new harness facet and must show up in usage provenance.

Live stream is weak (populate → run → seal → grade). Poll the runner trajectory + env health. `step` / `live_frames` / `true_checkpoint` stay **unsupported**. `grading_snapshot` is **native**. Visual is “policy step N / tool call / snapshot sealed / grader running,” not a fake gym.

### 4.10 τ-bench, τ²-bench, tau3

Content, not a wire format. Three names, two Sierra products, one Synth container:

| Name | What it is |
| --- | --- |
| **τ-bench** (v1) | Sierra 2024. Single-control. User talks; only the agent has tools. Retail / airline. Vendored in evals reportbench `taubench_vendor`. |
| **τ²-bench** (v2) | Sierra 2025. Dual-control Dec-POMDP. Agent **and** user have tools on a shared env. Airline / retail / telecom / banking. Orchestrator is first-class (half-duplex vs full-duplex). |
| **tau3** | Synth HTTP wrapper over τ² (`evals/containers/nonsensitive/arbitrary/tau3`, `tau3_repo_root = tau2-bench`). Not a third Sierra bench. |

There is no τ³-bench. Do not invent one.

#### Split (τ² is the gold mapping)

```text
  World                 domain DB + tools + domain policy document
  EnvironmentService    shared tool world (agent tools + user tools)
  Policy agent          SUT
                          harness = Orchestrator (half_duplex | full_duplex,
                                    max_steps, max_errors, timeout) + LLMAgent loop
                          config  = agent model / effort / inference
  Policy user           UserSimulator          ← second Policy, same shape
                          harness = user-sim loop + user tools
                          config  = user model / temperature
  EvaluationPlan        reward_basis (db / actions / nl_assertions / communicate)
  TaskInstance          domain + task_id + split + seed
```

v1 is the same nouns with `user_tools = empty` (user is talk-only). Dual-control is a world/task property, not a new runtime kind. The Orchestrator is the **agent policy’s harness**, not a third service. It may *call* the user policy; it does not own it.

#### What is a harness update (agent policy facet)

```text
  PUT /policy  { harness: { orchestrator: half_duplex | full_duplex,
                            max_steps, max_errors, timeout } }
  POST /policy/restart
```

Keeps: domain world, agent config, user policy. In-flight conversations finish on the old PolicyRevision.

Not harness:

| Change | Affordance |
| --- | --- |
| Agent model / effort / system prompt | `bind_policy_config` / `update_policy_code` on **agent** policy |
| User-simulator model / temperature | `bind_policy_config` on **user** policy (`role=user`) |
| Domain policy document / tool schema | world revision |
| New ticket / `task_id` | `task_bind` (light) |
| `reward_basis` / NL-assertion judge | `replace_eval_plan` |
| Message-history replay | **not** `true_checkpoint` |

Create-rollout must name both policies:

```text
  POST /rollouts
      policy_ref       agent (harness + config)
      user_policy_ref  user simulator     (another Policy)
      task_instance    domain + task_id + seed
```

Usage and cost split: `agent_cost` vs user-sim cost vs NL-judge cost. Do not roll them into one `policy` blob.

#### Honesty vs today’s tau3 container

`service_app.py` fuses the split on every `/rollout`:

```text
  policy.config.model / system_prompt / max_steps / max_errors / timeout
  env.config.user_model / user_temperature / domain / task_id
  new Environment() + LLMAgent + UserSimulator + Orchestrator.run()
```

That is one process constructing env + two policies per request. Logical IDs can still exist, but:

```text
  update_harness                 unsupported as a live swap (in-proc with env)
  restart_policy                 unsupported as “keep env” unless Orchestrator is out-of-proc
  bind_policy_config (agent)     next rollout only (native-enough if world is rebuilt anyway)
  true_checkpoint                unsupported
  restore                        approximate at best: resume_message_history is replay,
                                 not env+RNG restore
                                 (tau3.toml: checkpoint_restore_semantics=request_snapshot_rerun,
                                  true_partial_environment_restore=false)
```

To make harness updates first-class, the container must keep the domain env process up and expose `PUT /policy { harness }` the way Craftax exposes `PUT /policy { code }`. Wrapping τ²’s `Orchestrator` as the agent policy’s harness is the native implementation — it is already a separate object in tau2-bench. The trick is stuffing `max_steps` into `policy.config` as if it were a model field.

v1 `ToolCallingAgent` vs `ChatReActAgent` vs `FewShotToolCallingAgent` is also a harness-facet swap, not an env swap. Same `PUT /policy`. Same “native only if out-of-proc from the retail DB.”

#### Scoring

τ / τ² reward is **not** a Harbor `reward.txt` and **not** an Archipelago snapshot diff. It is a declared `reward_basis` combiner over env DB check, gold actions, optional NL assertions, optional communicate checks. Missing basis stays missing. The NL-assertion model is an EvaluatorService (or a judge PolicyService), not the user simulator.

tau3 already multiplies bases (`reward *= breakdown[basis]`). That combiner belongs on `EvaluationPlan`, not buried in the rollout handler.

---

### 4.11 dig.bench (A8 capstone) — content, hosted env

[dig.bench](https://digbench.ai) is a scientific-discovery benchmark: 70 text games (21 public), 7 difficulty tiers. Humans and models share the same JSON state (observation, level, lives, steps remaining, status, legal actions). Win = beat the game within the step budget. It is **content**, like Craftax / GameBench / τ-bench. It is **not** a Harbor fold, not OpenEnv, not a third wire format.

The environment is **their hosted game server**, not a local engine and not a Docker sandbox:

```text
  Agent REST     https://api.digbench.ai/api/agent
  Client         pypi: digbench          list_games / start_session / step / get_session
  MCP            pypi: digbench-mcp      same API as tools (DIGBENCH_API_TOKEN)
  Human UI       digbench.ai play        actions, creative mode, obs, stats, history
```

Their leaderboard already splits **policy = harness + config**:

```text
  Basic harness     next-action loop, rolling context          (ReAct-shaped)
  Agentic harness   tools + filesystem (Codex, Claude Code, …) (MCP-shaped)
```

A8 is the **final Workshop acceptance** for the Aug 12 update: same pins, stream, `/reward`, connect-before-start, persist-before-publish as A1–A7, on this third family. If A8 needs a new envelope, a Harbor wrap of their HTTP, or invented frames, the earlier contracts failed.

#### Split

```text
  World                 a game id (public P-1 … P-21; private tiers exist — do not
                        scrape them). Heavy thing is their server + rules, not a
                        Dockerfile we own.
  EnvironmentService    Containers **relay**: session on their API. Durable log is
                        ours. Their session is occupancy, not our world process.
  Policy basic          harness = next-action / ReAct over legal `state.actions`
                        config  = Luna / Sol / …
  Policy agentic        harness = coding-agent loop (Codex, …)
                        config  = model / effort
                        mcp_bind = digbench-mcp tools (list_games, start_session,
                                   step, get_session, list_sessions, get_openapi)
  Task                  beat this game within the step budget (light)
  EvaluationPlan        env terminal status → reward
                          completed  → 1
                          game_over  → 0
                          not done   → reward=null (do not POST terminal)
  TaskInstance          game id + session identity + model_name they record
```

`start_session` is `world_start` / episode open (mutating, paid/token). `step` is native env step. `get_session` is **reconnect** after a crash, not `true_checkpoint` (we do not restore their RNG; they resume the session they already have). Creative mode (experiment without counting toward the step budget) is a **game affordance** if/when the API exposes it — advertise `native` only if the relay can actually enter it; else `unsupported`. Do not fake a creative-mode toggle in the visual.

#### What is not a fold

```text
  Harbor-wrapping dig.bench     LCD lie: a sandbox around a remote HTTP game
  OpenEnv-wrapping start/step   not gym reset/step/state(); do not relabel
  Inventing live_frames         text-only by design; frames = unsupported
  Flattening MCP into Harbor    agentic tools ARE the env API, not a trial
  GEPA/SFT on dig.bench         out of this cut (A3/A4 stay Banking77 / hosted SFT)
```

`digbench-mcp` is `mcp_bind` on the **environment** (their tools are how you mutate the game), consumed by the agentic policy. Same noun as Archipelago `POST /apps`, different world: here the MCP *is* the env, not a topology bolted onto Compose. Basic harness must leave `mcp_bind` unused (it calls REST `step` through the relay). Do not force both policies through MCP.

#### Affordances (honest)

```text
  N  step  poll  bind_policy_config (next session)  bind_inference
     world_start (= start_session)  reconnect (get_session)
     credential_isolation (DIGBENCH_API_TOKEN)
     mcp_bind (agentic path only)
     POST /reward (env status node)
  D  sse  (relay over our log of their steps)
  ?  ws   only if we advertise it; their API is request/response
  U  live_frames  true_checkpoint  restore  fork
     grading_snapshot  blocking_trial  Harbor reward.txt
     update_rules_in_place
  .  update_policy_code on their server
```

Recipe `require live_frames` **refuses** (this is the Craftax-shaped lie). Recipe `require true_checkpoint` **refuses**. Recipe `require mcp_bind` refuses the **basic** policy; it is valid on the agentic policy.

#### Stream + visual

Log kinds (not Craftax NEV, not Harbor trial). **Seven kinds; no second `state` event:**

```text
  session.opened          start_session result (game, session_id)
                          optional: raw JSON payload
  observation             text / JSON obs (verbatim); optional raw JSON payload
  legal_actions           state.actions
  stats                   level, lives, steps remaining, creative_mode?
  action                  chosen action + step_index
  invalid_action          their invalid_action semantics (evidence, not a crash)
  status                  running | completed | game_over
```

Agentic MCP tool calls ride this **same** eval/trace stream as policy spans (open/data/close), like Harbor tools/stdout. Not a nested log. Not an optimizer stream.

Template: **`live.digbench.v1`**. Same core reducer as Craftax / `live.harbor_eval.v1`; only kinds differ. Layout may follow their human UI (actions, obs, stats, history) — do not draw a dungeon. Slot `stream`. Bind the declared id. Connect-before-start: **C1-08** — `stream.subscribed` **before** `start_session` (that call is the first mutating/token event).

Persist-before-publish: their session can expire; our log + Trace V5 must reopen the run. Token, `Authorization`, and `DIGBENCH_API_TOKEN` never appear in envelopes.

#### `/reward`

```text
  evidence     sealed log (rollout_id); env status is in the log
  plan         one env_reward node, authority=environment
               combiner = identity on terminal status
                 completed → 1.0
                 game_over → 0.0
  terminal     state.done / status in {completed, game_over}
  provisional  unsupported unless we later advertise a live “levels beaten”
               fraction — do not invent it. Lives/level are stats, not reward.
  GET before POST → reward=null
  POST does not call step
```

Win rate on their leaderboard is an **optimizer/eval aggregate** over many `/reward`s, not this endpoint. A8 is **one** pinned public game on the receipt (P-1, or first `list_games` entry if P-1 is gone), not a 70-game scrape.

#### Two policies on one world (A8)

```text
  POST /policy-configs   basic_luna     harness=react_legal_actions
  POST /policy-configs   agentic_codex  harness=codex + mcp_bind digbench-mcp
  POST /rollouts         same world_ref (game), distinct policy_ref, distinct
                         session/rollout/log/usage
```

Flipping the open visual does not stall the other (C7-O02). Mid-session `bind_policy_config` refused (their session is already bound to `model_name`). Next game = new rollout.

#### Containers vs Workshop

```text
  Containers   relay, pin, log, /reward, C8 mock + --paid live
  Workshop     live.digbench.v1, connect-before-start, reopen, two-policy flip
  Optimizers   unused this cut (no GEPA-on-digbench)
```

Do not start A8 in Desktop until C8 headless passes. Do not treat a Playwright click-through of digbench.ai itself as A8 — the run has to go through Containers + Workshop.

---

## Map — machine, affordances, benchmarks, bridges

Legend: `N` native · `D` derived · `A` approximate · `U` unsupported · `.` unused / n/a. Default is `U`. A route without a claim is `U`. Never mutate an in-flight scored attempt.

### M.1 The container (deployment unit)

```text
                         Workshop / optimizer / visual
                                    |
                    bind recipe: require | prefer | unused
                    advertised.level >= required  else REFUSE
                                    |
                                    v
+-----------------------------------------------------------------------+
|  CONTAINER  = deployment unit, not an env / task / score              |
|                                                                       |
|  WorldDefinition@rev                                                  |
|    world_install  world_start/stop  world_lease  world_reset          |
|    scale_leases   freeze_world / thaw_world                           |
|         |                                                             |
|         v                                                             |
|  +---------------------------+  +----------------------------------+  |
|  | EnvironmentService        |  | PolicyService                    |  |
|  |                           |  |   = harness + config (+ code)    |  |
|  | step  blocking_trial      |  |                                  |  |
|  | live_frames               |  | harness  ReAct / Apex runner /   |  |
|  | true_checkpoint           |  |          IsolatedPolicyProcess / |  |
|  | restore / fork            |  |          Orchestrator / Codex    |  |
|  | grading_snapshot          |  | config   model, effort, tools,   |  |
|  | pause  concurrent_eps     |  |          inference, credentials  |  |
|  | mcp_bind / tools          |  | code     prompt / heuristic /    |  |
|  | task_bind  update_rules   |  |          adapter (optional)      |  |
|  |                           |  |                                  |  |
|  |                           |  | bind_policy_config               |  |
|  |                           |  | update_harness   (facet)         |  |
|  |                           |  | update_policy_code               |  |
|  |                           |  | update_policy                    |  |
|  |                           |  | restart_policy   (loop dies too) |  |
|  |                           |  | bind_inference  load_adapter     |  |
|  |                           |  | compact_session  token_trace     |  |
|  |                           |  | (+ user_policy, same shape)      |  |
|  +-------------+-------------+  +----------------+-----------------+  |
|                |                                 |                    |
|                +----------------+----------------+                    |
|                                 |                                     |
|                                 v                                     |
|                       Attempt / Rollout                               |
|                       pins: world_ref  environment_ref  policy_ref    |
|                             user_policy_ref?  task_ref                |
|                             evaluation_plan_ref  task_instance  stream|
|                                 |                                     |
|                      +----------+-----------+                         |
|                      v                      v                         |
|             EventRelay               EvaluatorService                 |
|             one durable log          EvaluationPlan DAG               |
|             poll  (required)         gates / scripts / rubrics        |
|             sse   (default live)     env-reward / heldout / integrity |
|             ws    (optional)         POST /reward (rollout_id | evidence) |
|             cursor; missing != 0     GET /reward → null if absent         |
|             partial_trace open..     separate_verifier_image          |
+-------------------------------------+---------------------------------+
                                      |
                                      v
                            capture.closed --> Trace V5
                            ATIF / site / visuals = projections, not the log

  restart_deployment  = bounce the whole box (nothing live)
  reconnect           = visual dropped; poll after cursor; not a new attempt
  retry               = new attempt; world may stay
  no HarnessService   = harness is inside PolicyService
```

### M.2 Dynamics (what stays)

```text
                    KEEP                         CHANGE
  ------------------+----------------------------+----------------------
  bind_policy_config     world, env, harness, code   model / effort / tools
  update_harness         world, env, config, code    loop / agent_id / orchestrator
  update_policy_code     world, env, harness, maybe  program / prompt / adapter
                         config
  restart_policy         env process                 PolicyService (harness dies too)
  mcp_bind               policy                      tool topology on env
  task_bind              world process               light instruction / seed pin
  world_reset            world definition            episode (new RNG/lease)
  replace_eval_plan      live env/policy             next EvaluationExecution
  load_adapter           world, env, harness         sampler weights (config/code)
  freeze/thaw            processes                   mutation paused
  ------------------+----------------------------+----------------------
  restart_deployment     nothing live                everything
  retry                  maybe world                 new Attempt
  restore                proven checkpoint           env+RNG+cursor
  fork                   parent immutable            new episode from ckpt
  reconnect              attempt + log               transport only

  in-flight scored attempt: NEVER mutated.
  new PolicyRevision / EvaluationExecution instead.
  no restart_harness — bounce policy.
```

### M.3 Affordance index (who owns the route)

```text
  ENV       step  blocking_trial  live_frames  pause  concurrent_episodes
            true_checkpoint  restore  fork  grading_snapshot
            mcp_bind  task_bind  world_*  scale_leases  freeze/thaw
            update_rules_in_place

  POLICY    bind_policy_config  update_harness  update_policy_code
            update_policy  restart_policy  bind_inference  load_adapter
            compact_policy_session  token_trace

  EVAL      POST /reward  GET /reward  replace_eval_plan  separate_verifier_image

  RELAY     poll  sse  ws  partial_trace

  BOX       restart_deployment  retry  reconnect
```

### M.4 Benchmark by benchmark

How to read a row: left = logical split, right = honest advertise. `U` is valid. Claiming `N` without a process split is the lie.

#### Craftax interactive (A1) — native content

```text
  World     rust gold HTTP + rules/readout pin
  Env       engine, NEV, frames, RNG
  Policy    harness = ReAct; config = Luna / Sol / Laguna
  Eval      env-sum RewardSignal (engine payout table)
  Stream    poll=N (engine event_log+nev_cursor; consumer sequence via relay)
            sse=D  ws=prefer
  Bridge    none (GameBench is content, not a wire format)

  N  step  live_frames  partial_trace  bind_policy_config  bind_inference
     update_harness (ReAct facet)  world_start/stop/lease  scale_leases
     task_bind  poll  restart_policy
  D  sse  ws(relay)
  ?  true_checkpoint/restore/fork   only if rust restore is proven
  U  grading_snapshot  blocking_trial  mcp_bind  update_rules_in_place
     user_policy_ref
  .  update_policy_code on the rust bin (prefer prompt/adapter only)

  Luna med -> Sol med:  POST /policy-configs  then POST /rollouts
                        engine stays.  That is A1.
```

#### Craftax code-policy DEO — native content, often nested under Harbor

```text
  World     same Craftax engine
  Env       same EnvironmentService
  Policy    harness = IsolatedPolicyProcess; code = heuristic_policy.py   PLAYER
  Eval      child env-sum -> candidate vs baseline -> heldout GATE
  Author    Harbor Codex (different Policy: harness+config)               AUTHOR
  Bridge    Harbor trial wraps the author; child is this container

  N  update_policy_code  restart_policy  PUT /policy  POST /policy/restart
     step  live_frames  (on the child env)
  U  bind_policy_config on the player (it is a program, not Luna)
     Harbor-trial step / live_frames / true_checkpoint

  Codex -> PUT /policy + POST /rollouts on the CHILD
        not python run_hillclimb.py
```

#### Harbor + Terminal-Bench 3 / TBLite — first-class fold

```text
  World     environment/Dockerfile (+ optional verifier image)
  Env       docker sandbox the agent mutates
  Policy    harness+config fused as the Harbor "agent" (Codex / Nova …)
  Eval      tests/test.sh -> reward.txt|json   SCRIPT NODE after exit
  Stream    poll=N (lifecycle JSONL)  sse=D  ws(env)=U  ws(policy->proxy)=N
  Bridge    HARBOR FOLD  (see M.5)

  N  blocking_trial  bind_policy_config (BEFORE trial)  bind_inference
     proxied_inference  credential_isolation  separate_verifier
     POST /reward  retry  restart_deployment  world_install  poll
  D  grading_snapshot (leftover workspace)  sse  partial_trace  token_trace
  U  step  live_frames  true_checkpoint  restore  fork
     update_policy / update_harness / update_rules  (mid-trial)
     world_start independent  concurrent_episodes (parallel = more trials)
  .  mcp_bind

  TB3 / TBLite = Harbor DATASETS, not runtimes.
  TBLite calibration = dataset revision.
```

#### Harbor GameBench (A2) — Harbor fold + nested native child

```text
  Outer     Harbor trial as above (author Codex, verifier script)
  Inner     Craftax interactive and/or code-policy DEO
  Visual    live.harbor_eval.v1 = outer; nested visual = child
  Do not    advertise child live_frames on the Harbor fold
```

#### APEX / Archipelago — research (informs the split; no fold promise)

```text
  World     Compose: MCP apps + populate snapshot
  Env       gateway  POST /apps  POST /data/populate  POST /data/snapshot
  Policy    harness = agents runner (agent_id, max_steps, timeout)
            config  = model, credentials, system_prompt
  Eval      after seal: snapshot DIFF + criteria     POST /reward evidence
  Stream    weak: populate -> run -> seal -> grade
  Bridge    research proxy synth_http only (compat/archipelago.py)

  N  grading_snapshot  mcp_bind (POST /apps)  world_start/stop
     update_harness / restart_policy   IFF runner connects to a live env
     bind_policy_config  POST /reward (provided evidence)
  U  step  live_frames  true_checkpoint  restore  fork
     env-authored RewardSignal
  U  update_harness "keep world" if runner spawn+kills sandbox per traj

  POST /apps is mcp_bind, NOT update_harness.
```

#### τ-bench v1 — content (single-control)

```text
  World     domain DB + tools + policy document (retail / airline)
  Env       tool APIs; user is talk-only
  Policy    harness = ToolCalling / ReAct / FewShot; config = agent model
  Eval      task completion (actions / DB)
  Bridge    none native; reportbench vendor today

  N  bind_policy_config (next task)  poll
  U  true_checkpoint  live_frames  mcp_bind
  ?  update_harness  only if policy process is out-of-proc from DB
```

#### τ²-bench / tau3 — content (dual-control); tau3 = Synth wrap of τ²

```text
  World     shared tool world (agent tools + user tools)
  Env       domain state (telecom / airline / retail / banking)
  Policy    agent: harness = Orchestrator+LLMAgent; config = agent model
  Policy    user:  harness = UserSimulator; config = user model
  Eval      reward_basis combiner (db x actions x nl x communicate)
  Bridge    none; tau3 is our HTTP wrapper, not a third Sierra product

  N  (target)  update_harness / restart_policy on agent policy
               bind_policy_config on agent AND on user
               poll  POST /reward
  U  true_checkpoint  (tau3: request_snapshot_rerun = message replay)
     live_frames  mcp_bind  restore
  today tau3 fuses env + two policies per /rollout
        max_steps in policy.config, user_model in env.config  = the trick
```

#### GEPA Banking77 (A3) — optimizer child evals on a stateless world

```text
  World     classify-text (almost no env)
  Env       unused snapshots
  Policy    harness = classify loop; config = proposer model; code = prompt modules
  Optimizer GEPA search is OptimizerService; child evals are Containers
  Eval      task metric; not env reward
  Bridge    none

  N  bind_policy_config  update_policy_code  poll  concurrent child evals
  U  true_checkpoint  grading_snapshot  step  live_frames  mcp_bind
  .  restore  fork
```

#### SFT + checkpoint-eval (A4 / A6) — optimizer, env may stay

```text
  OptimizerService    hosted SFT job (not goex.sft.v1)
  Policy              load_adapter onto sampler
  Env                 Craftax (or other) STAYS UP across ckpt evals
  Eval                child eval campaigns; promotion != "ckpt ready"

  N  load_adapter  bind_policy_config  (eval world affordances as that world)
  U  treating training checkpoints as env true_checkpoint
```

#### PostTrainBench — research / later

```text
  World     long-lived training workspace (GPU, datasets, ckpts)
  Task      emit final_model under budget
  Eval      downstream benches + integrity judges (gate, not footnote)
  N  grading/audit snapshot  POST /reward
  U  process restore  step  live_frames
```

#### OpenEnv Echo (A7) — compatibility wrap, not a fold

```text
  World     unmodified Echo image / server
  Env       the OpenEnv process (reset / step / state)
  Policy    OUTSIDE (client): harness = gym loop; config = caller model
  Eval      env-authored reward + done
  Bridge    OPENENV GATEWAY  (see M.5)

  N  step  poll  (and sse/ws if the server has a real log)
  U  true_checkpoint  restore  fork  grading_snapshot
     (state() is a slice; drop checkpointable=True)
  .  bind_policy_config on the env (policy is the caller)
```

#### Prime GSM8K — compatibility wrap, after Harbor+Echo

```text
  Taskset   dataset + prompts + reward hooks + toolsets
  Policy    Prime Harness = harness (loop+proxy) + config (model controls)
            sandbox inside Prime Harness is fused env — advertise honestly
  Env       wiring; may NEST Harbor or OpenEnv (then inner env is the env)
  Eval      taskset reward AND/OR rubric metrics (metric != reward)
  Bridge    PRIME WRAP  (see M.5); no adapter in tree today

  N  whatever the inner env honestly has, plus declared reward authority
  U  inventing step if inner is Harbor; promoting metrics to reward
  chain on receipt:  Prime -> (Harbor fold | OpenEnv wrap | native)
```

#### dig.bench (A8) — native content, hosted env (not a fold)

```text
  World     their game id (public P-1 … P-21). Server is theirs.
  Env       Containers relay: session on api.digbench.ai
  Policy    basic  = ReAct / next-action over legal actions
            agentic = Codex (etc.) + mcp_bind digbench-mcp
  Eval      env status: completed→1  game_over→0  else null
  Stream    poll=N (our log of their steps)  sse=D  ws=U unless advertised
  Bridge    none (content). REST/MCP are how you talk to THEIR env.

  N  step  poll  world_start (start_session)  reconnect (get_session)
     bind_policy_config (next session)  credential_isolation
     mcp_bind on agentic  POST /reward
  D  sse (relay)
  U  live_frames  true_checkpoint  restore  fork  grading_snapshot
     blocking_trial  Harbor reward.txt
  .  mcp_bind on basic  update_policy_code on their server

  require live_frames        -> REFUSE
  require true_checkpoint    -> REFUSE
  Harbor-wrap their HTTP     -> fail A8
  OpenEnv-wrap start/step    -> fail A8
```

#### Others (ontology pressure, not this cut)

```text
  Harvey LAB     deliverables + criterion-to-artifact; independent judge
  Crosby         turns / branches / panel grades; Harbor-packaged today
  TaxCalcBench   edition + case; hierarchical metrics; no live world
  CardBench      content, like GameBench
  Inspect AI     do not fold
```

### M.5 Format bridges

Containers is the façade. A bridge is a receipt + adapter chain, not a shared `/step`.

```text
  content (no bridge)          format (bridge)              research
  -------------------          -----------------            --------
  GameBench / Craftax          Harbor  = FIRST-CLASS FOLD   Archipelago/APEX
  CardBench                    OpenEnv = wrap unmodified    Inspect
  τ / τ² / tau3                Prime   = wrap + chain
  dig.bench (hosted session)   (no bridge — relay is EnvironmentService)
  TB3/TBLite = Harbor datasets
```

#### Harbor fold (only first-class external format)

```text
  Harbor package                         Containers
  ----------------                       ----------
  environment/Dockerfile          -->    WorldDefinition
  instruction.md + tests/         -->    TaskDefinition + EvaluationPlan
  trial                           -->    Attempt  (blocking_trial)
  job                             -->    EvaluationRun
  agent                           -->    Policy (harness + config; already fused)
  tests/test.sh -> reward.txt     -->    script node; not env RewardSignal
  ATIF                            -->    projection after/during capture
  REB_INFERENCE_PROXY             -->    bind_inference / proxied_inference

  live template: live.harbor_eval.v1
  adapter today: compat/harbor.py  (labels only — fold is the work)
  nested content (GameBench DEO) stays a child environment_ref
```

#### OpenEnv wrap (compatibility target)

```text
  unmodified OpenEnv server              Containers gateway
  -------------------------              -------------------
  reset                           -->    episode open
  step  (obs, reward, done)       -->    EnvironmentService.step
                                         RewardSignal (env authority)
  state()                         -->    typed slice   NOT checkpoint
  policy client                   -->    PolicyService (harness=gym loop + config)

  do not rewrite the env
  do not advertise true_checkpoint because checkpointable=True
  promote only after Echo native-vs-wrapped + one multi-step env
  Gymnasium overlaps this wrap; no separate fold
```

#### Prime wrap (compatibility target, after Harbor + Echo)

```text
  Prime Verifiers                        Containers
  ---------------                        ----------
  Taskset                         -->    dataset + TaskInstance
                                         + declared reward authority
  Harness                         -->    Policy (harness facet + model config)
  Env                             -->    wiring / nested runtime
  metric                          -->    metric  (never silently = reward)
  load_environment(config)        -->    wrap; do not prime eval push

  if Prime internally calls Harbor or OpenEnv:
      receipt.adapter_chain = [prime, harbor] | [prime, openenv]
      inner affordances are the inner runtime's, not Prime's
```

#### Archipelago proxy (research, not a fold)

```text
  synth_http / compat/archipelago.py
      -->  keep as Compose-world + post-seal grade example
      -->  no compatibility promise
      -->  must not become a fourth fold by accident
```

```text
  bind
    recipe.require  vs  advertised (from native or from bridge)
    Harbor TB3 + require true_checkpoint     -> REFUSE
    OpenEnv Echo + require restore           -> REFUSE
    Prime(Harbor) + require step             -> REFUSE (inner Harbor has no step)
    Craftax + prefer true_checkpoint         -> start; restore only if proven
    APEX + require grading_snapshot          -> start
    tau3 + require true_checkpoint           -> REFUSE (replay is not restore)
    dig.bench + require live_frames          -> REFUSE
    dig.bench + require true_checkpoint      -> REFUSE
```

---

## 5. How scoring actually works (review)

This is the DAG we have to host, not a new invention. The Containers surface is **`POST /reward`** (§3). What follows is how today’s benches already compute that DAG — so the endpoint does not invent a new scoring theory.

### evals — shared shape

`evals/README.md`: run → **score** → save evidence → index.

Score = trusted **script** (`evaluator/score.py`, `eval/eval.sh`, `reb-score/result.json` + `reward.txt`) and/or **rubric verifier** (`codex_verifier/`, `rubric/rubric.md`), combined by a declared formula, gated fail-closed. Reward authority is explicit. Refusals leave reward **absent**, never `0.0`. Rig failure and agent failure are distinct statuses.

### REB (`evaluator/score.py` + `rubric.json`)

The scorer is the product. Anatomy: light `task.toml` / `instruction.md` over a heavy `workspace/` + sealed `reference/` + `evaluator/`. Harbor and dock adapters are mechanical copies.

- Two-phase: contract pass is legal, not good. Byte-identical baseline has zero uplift. Reward is efficacy on held-out, nowhere else.
- Trusted scorer image is separate (`score_execution_identity = root_linux`, `score_container_image` digest). Agent must not run the scorer.
- Roles: `trusted_scorer` + `codex_verifier`. Scorer may **refuse** → completed rejection, null score.
- Statistical gates, A/A control, cache identity. Wall clock must not be a reward term while a cache exists.
- Answer key leaks as code, not just prose. Sync scripts fail closed.

### InternBench / SwarmBench / GameBench dock

`eval.toml` requires `eval/eval.sh` + `rubric/rubric.md`. Formula is locked:

```text
reward = script_verdict.score × rubric_verdict.score
```

`eval.sh <evidence.json>` runs the **trusted repo copy**, never a worker-modified evaluator from the workspace. Script = deterministic gate fraction. Rubric = semantic judge. A semantic pass cannot waive missing structure. Named gates (e.g. `intern_owns_swarm_launch`, `comparison_evaluated`) are fail-closed nodes, not scores.

### GameBench native (Craftax interactive vs DEO)

Two different DAGs over the same world:

| Mode | DAG |
| --- | --- |
| Interactive (A1) | Env `RewardSignal` per step (engine payout table). Harness records deltas. Terminal score is env-sum, not a rubric. |
| Code-policy DEO (A2 Harbor) | Child env rollouts produce rewards → candidate score → baseline delta → **held-out** score → improvement **gate**. Optimizer, child rollout, and verifier identities stay separate. |

Do not flatten DEO held-out into the engine step reward.

### Harbor / Terminal-Bench 3

World = `environment/Dockerfile` (plus optional separate verifier image). Task = `instruction.md` + `tests/`. After the agent exits, `tests/test.sh` writes `/logs/verifier/reward.txt` or `reward.json`. That file is a **script-node** result. Harbor parses it; it is not proof the environment authored reward.

TB3 is a Harbor **dataset** (hidden tests, same verifier surface). Calibration (TBLite) is a dataset revision, not a runtime kind. Multi-metric `reward.json` maps to named scores/metrics; only the declared combiner field is the attempt reward.

### PostTrainBench

World = long-lived training workspace (base model, GPU, datasets, checkpoints, `final_model`). Task = “submit `final_model` under budget.” Scoring is **after seal**: downstream functional benches **and** independent integrity judges. High functional score + failed integrity → ineligible `PromotionVerdict`. Integrity is a DAG node, not a footnote.

### Archipelago

World = Compose: independently runnable env/MCP gateway + populated snapshot. Agent runner **is** the policy (harness + config). Grade **after** on before/after snapshot **diff** and selected artifacts. Criteria over deliverables. Snapshot is evidence, not restore. This is the provided-evidence path of `POST /reward`. Loop updates are `PUT /policy { harness }` (§4.9); `POST /apps` is `mcp_bind` on the env.

### τ-bench / τ² / tau3

Reward is a declared `reward_basis` combiner (DB check × gold actions × optional NL assertions × optional communicate). User-simulator cost is not agent cost. NL-assertion judge is an evaluator node. tau3’s in-process multiply belongs on `EvaluationPlan`. Message-history replay is not a checkpoint.

### dig.bench

Hosted text games. Terminal `status` is env-authored (`completed` / `game_over`). That is the `/reward` node. Levels beaten, lives, and steps remaining are **stats in the log**, not a second reward. Their leaderboard win-rate is an aggregate over many `/reward`s — OptimizerService / eval suite, not this endpoint. `get_session` resumes their session (reconnect), not a Synth checkpoint. MCP tools are the env API for the agentic harness.

### Containers code today

| Path | What it is | Gap |
| --- | --- | --- |
| `rubrics/v1.py` | Criterion/weight schema + `VerifierResultV1` | Not an executable DAG. `_clamp_score` **defaults missing to 0.0**. |
| `tracing/native_evaluation.py` | Attach native evaluator payload after seal | Import, not a live score endpoint. |
| `recovery.py` | `float(row.get("reward") or 0.0)` | Missing → 0. Same A5 bug. |
| HTTP | no `POST /reward` | Rollout embeds a float if anything. |
| Harbor compat | `REWARD_EMITTING: DERIVED` | Honest-ish, but no verifier execution. |

---

## 6. Native splits (do not flatten)

```text
  Harbor                         OpenEnv
  ------                         -------
  Task = instr + image + tests   Env server IS the product
  Trial = one agent run          reset / step / state
  Job   = bag of trials          env authors reward + done
  "environment" = docker sandbox policy is an external client
  agent  = Policy (harness + config)   gym loop is the client's harness
  verifier AFTER agent exits     no task/dataset, no verifier
  reward.txt = TEST OUTPUT       state() != checkpoint
  almost no live step stream     live stream = the whole thing
  profile: sandbox_artifact_task.v1

  Prime Verifiers                Archipelago
  ---------------                -----------
  Taskset = data + prompts       populate WORLD SNAPSHOT first
            + setup/update       MCP gateway = tools/world
            + reward hooks       agent runner is separate
            + toolsets           grade AFTER on snapshot DIFF
  Harness = program + proxy      snapshot is evidence, not restore
            + model controls     criteria over artifacts
            + sandbox            no env-authored step reward
  Env = wiring (eval OR train)   live stream is weak; seal then grade
  metric != reward               profile: professional_deliverable.v1
  often terminal aggregate only    (APEX/MCP/snapshot-diff extras)
  may internally call Harbor
  or OpenEnv  --> adapter chain
```

Harbor maps: task → `TaskDefinition`, trial → `Attempt`, job → `EvaluationRun`, agent → Policy (harness + config), verifier → `EvaluationExecution`.  
OpenEnv maps: server → `EnvironmentService`, `reset` → episode open, `step` → action/obs/reward/done, `state()` → typed slice **not** checkpoint; client loop → policy harness.  
Prime maps: Taskset → dataset + task instance + declared reward authority; Prime `Harness` → Policy (harness + config); sandbox/tools → env capabilities unless fused; diagnostic metric stays a metric.  
Archipelago maps: populated snapshot → `TaskWorld` + `WorkspaceSnapshot`; MCP gateway → env/tools; runner → Policy; post-seal grader → evaluator on a diff. Not a live gym.

GameBench / CardBench / τ-bench / τ²-bench are **content**, not extra wire formats. They execute through Containers (native or Harbor-packaged). Terminal-Bench / TBLite are Harbor **datasets**. tau3 is a Synth wrapper over τ², not a third Sierra product.

---

## 7. What goes wrong if we LCD all four

```text
                    +---------------------------+
                    |  "ContainerCompat v0"     |
                    |                           |
   Harbor --------> |  env    = whatever box    |
   OpenEnv -------> |  policy = whoever talks   |
   Prime ---------> |  task   = whatever yaml   |
   Archipelago ---> |  step   = whatever happens|
                    |  reward = some float      |
                    |  done   = some bool       |
                    |  state  = some json       |
                    +---------------------------+
                                |
                                v
                         looks runnable
                         is semantically false
```

### 3.1 Environment is four objects

```text
  Harbor        docker workspace the agent mutates
  OpenEnv       the process that owns transition + reward
  Prime         sandbox *inside* Harness; "Env" is the composition
  Archipelago   MCP gateway + populated snapshot

  LCD: one /reset /step.
       Harbor/Archipelago have no honest step.
       OpenEnv env-authored reward dies inside Harbor reward.txt.
```

### 3.2 Task / world / dataset collapse

```text
  Harbor        env image is the world; instruction+tests are the task
  OpenEnv       often none (Echo is a toy env)
  Prime         Taskset mixes dataset + prompt + reward + tools
  Archipelago   Compose+snapshot is the world; criteria are the task
  PostTrainBench  workspace is the world; "emit final_model" is the task

  LCD: "task_id".
       Cannot say whether you pinned a Compose graph, a dataset row,
       a test script, or a prompt template.
       Heavy world code ends up copied into every task.
```

### 3.3 Policy vs env vs “agent”

```text
  Harbor        one "agent" blob = policy (harness + config). Honest.
  OpenEnv       policy is outside (client loop + model); env is inside
  Prime         Prime Harness = policy (loop + proxy + model); sandbox may be fused
  Archipelago   runner = policy; MCP world = env

  LCD: "the model ran the env" OR a fake third HarnessService.
       Cannot restart policy without killing the world.
       Cannot swap Codex vs Luna without forking a new "task".
       A2 and A3 become impossible.
```

### 3.4 When and who scores

```text
       live step          after exit           after snapshot
          |                   |                      |
          v                   v                      v
       OpenEnv            Harbor verifier        Archipelago
       (env reward)       (tests/reward.txt)     (diff + criteria)
                              |
                              v
                           Prime
                    (taskset reward AND/OR
                     rubric metrics;
                     often only at the end)

  LCD: one reward field at one time.
       Harbor reward.txt becomes a fake env reward.
       Prime metrics become rewards (weight-0 included).
       Archipelago has no step reward, so you invent 0.
       Missing becomes zero. Fail closed is dead. A5 fails.
```

### 3.5 Snapshot vs checkpoint vs `state()`

```text
  OpenEnv state()          current slice         NOT restore
  Archipelago snapshot     before/after files    grade, not resume
  Harbor workspace         leftover sandbox      maybe artifacts
  Craftax checkpoint       engine+RNG+cursor     true restore

  LCD: checkpointable=True on everything.
       Already shipped in compat/openenv.py.
       Reconnect, restore, retry, branch, replay look the same.
```

The fix is §4: advertise snapshot *kind* + fidelity, default unsupported, bind require/prefer/unused. Harbor TB3 with `true_checkpoint=unsupported` is a valid world. MAPO on that world is a bind refusal, not a fake fork.

### 3.6 The live stream you would fake

```text
  OpenEnv        dense step events      real
  Craftax        NEV / frames           real, native
  Harbor         almost nothing live    then a terminal verifier
  Prime          often a dump at end
  Archipelago    populate -> run -> seal -> grade

  LCD: snapshot-diff SSE (today's http_adapter.py).
       Harbor "live" is a heartbeat until reward.txt appears.
       Visuals invent progress. Same CUA failure as CRAFTAX-LUNA-010.
```

### 3.7 Transitive identity

```text
  Prime  -->  (optional) Harbor  -->  docker
  Prime  -->  (optional) OpenEnv -->  gym server
  Harbor -->  Terminal-Bench dataset   (dataset, not a runtime)

  LCD: "we support Prime, therefore Harbor and OpenEnv."
       Adapter chain becomes the product. Receipts lie.
       Every transitive format becomes a first-class fold.
```

### 3.8 Net: four systems, one fake object

```text
  TaskWorld ----x---- Taskset ----x---- Harbor Task ----x---- Archipelago snapshot
       |                 |                  |                      |
       |                 +-- reward hooks   +-- tests              +-- criteria
       |                 +-- tools          +-- instruction        +-- MCP world
       v
  Environment ----x---- Prime "Env" (wiring) ----x---- OpenEnv server
       |
       x  Harbor sandbox
       x  Archipelago gateway

  Policy ----x---- Harbor agent ----x---- Prime Harness (owns model+sandbox+loop)
       |
       x  OpenEnv external client
       x  Archipelago runner

  Evaluator ----x---- Harbor verifier (separate execution, after)
            ----x---- Archipelago grader (snapshot diff, after)
            ----x---- Prime rubric (maybe; metrics != rewards)
            ----x---- OpenEnv (none; env already "scored")
```

Product damage: cannot pin policy independently; cannot connect a visual before work; cannot fail closed on missing reward/score; false checkpoints ship; Harbor dies as “another gym.”

---

## 8. Locked approach

| System | Level | What Containers does |
| --- | --- | --- |
| **Harbor** + ATIF 1.5–1.7 | **First-class fold** | Own adapter, launch/lease/supervision, live evidence, native-vs-wrapped verifier, ATIF as projection. Public template `live.harbor_eval.v1`. TB/TBLite enter as Harbor datasets. |
| **OpenEnv** | Compatibility target | Thin gateway over an **unmodified** server/image. Preserve Action/Observation/State. `state()` is a slice. Promote only after Echo + one multi-step official env (Chess tentative). |
| **Prime Verifiers** | Compatibility target | Wrap `load_environment(config)`. Keep Taskset / Harness / Env. Metrics ≠ rewards. Local tests never `prime eval push`. First proof `primeintellect/gsm8k`. **After** Harbor v1 + Echo. |
| **Archipelago / APEX** | Research | Informs snapshot-then-grade and MCP topology. No compatibility promise. Existing `synth_http` proxy stays research. |
| **GameBench rust HTTP** | Native content | EnvironmentService façade over gold HTTP. Keep NEV kinds. Not a GameBench protocol. |
| **Inspect AI** | Research | Do not fold. Echo tutorial path only. |

Compat means: wrap without rewriting task logic; normalized run is faithful; native evidence remains; adapter chain is on the receipt; acceptance suite passes. It does **not** mean a shared `/step`.

Harbor `reward.txt` is evaluator output, not proof the environment authored reward. Craftax step reward is a `RewardSignal`. Do not coerce one into the other.

---

## 9. Code today (the LCD already started)

| Path | What it is | What is wrong |
| --- | --- | --- |
| `compat/harbor.py` | Capability labels + HF dataset / build-context / evaluation resource refs. Blocking rollout. Reward/trace marked **derived**. | No launch, lease, supervision, trial/attempt split, verifier execution, or live log. Labels, not a fold. |
| `compat/openenv.py` | Gym-style capability surface. | `checkpointable=True` by default → `TRUE_ENVIRONMENT_SNAPSHOT`, restorable, forkable, long-horizon. `state()` is not that. **Drop the claim now.** No unmodified-server gateway. |
| `RuntimeCapabilitySurface` | Semantics enums + collapsing booleans | Booleans default/claim support. Use fidelity-per-affordance (§4). |
| `compat/archipelago.py` | `synth_http` proxy + task binding. | Research path. Must not become a fourth fold by accident. Keep as the Compose-world example. |
| Prime | **None.** | Correct until Harbor + Echo exist. |
| `http_adapter.py` | SSE/WS snapshot-diff with a local counter. Poll rejected. `GET /events` has no cursor. | LCD live stream. Disconnect loses identity. |
| `env` / `policy` bags | Untyped. | No `world_ref` / light `task_ref`. A1 cannot be a typed pin. |
| `rubrics/v1.py` | Criterion schema | Not a DAG. `_clamp_score` defaults missing to 0. |
| `recovery.py` | Resume helpers | `reward or 0.0`. Same bug. |
| HTTP score | **None** | No `POST /reward`. Rollout embeds a float if anything. Missing→0 in rubrics/recovery. |
| Policy player HTTP | **None** | GameBench already has `IsolatedPolicyProcess` (sandbox child, JSONL IPC, `close()`). Hillclimb shells `run_*_hillclimb_task.py` / `importlib` cache. No `PUT /policy` / `POST /policy/restart` on the Craftax container. |
| Harness HTTP | **None needed** | Harness is a policy facet (`PUT /policy { harness }`). Archipelago runner is already that policy process. tau3 stuffs `max_steps` into `policy.config` and `user_model` into `env.config`. |

---

## 10. First implementation cut (this file’s job)

0. **Stop lying.** OpenEnv: `checkpointable` defaults false; `state()` ≠ snapshot ≠ checkpoint. Harbor: `reward.txt` is a script node, not env reward. Rubric/recovery: missing stays missing (delete `or 0.0` / `_clamp_score` default). Affordances default `unsupported`; booleans are derived from fidelity, not the other way around.
1. **Type the pin.** `world_ref` (heavy), `task_ref` (light), `evaluation_plan_ref` (DAG), `environment_ref`, `policy_ref` (harness + config + optional code), `task_instance_id`, named stream. Logical service IDs on `/metadata` even in-process. No sibling `harness_ref`.
2. **Affordances + bind match.** Per-role `affordances` map (reconnect, grading_snapshot, true_checkpoint, restore, fork, pause, concurrent_episodes, step, `bind_policy_config`, `update_policy_code`, `restart_policy`, `world_start`, …) with `native|derived|approximate|unsupported`. Recipes declare `require|prefer|unused`. Refuse if required > advertised. Prove every native/derived claim. Policy configs are registered resources; hot-swap never mutates an in-flight attempt. Craftax code-policy: `PUT /policy` + `POST /policy/restart` are real routes on the playing container (`IsolatedPolicyProcess`), not a hillclimb shell.
3. **`POST /reward`.** Per-task EvaluationPlan: gates, script, rubric, optional env-reward and heldout/integrity. Produce from `rollout_id` or provided evidence. GET reads; POST computes (idempotent unless `rescore`). Combiner declared. Missing/gated/refused → `reward=null`. Long nodes `202` + execution events. Env RewardSignals stay in the log; `/reward` does not step.
4. **Durable log.** Poll + SSE (+ WS). Create-rollout echoes the full stream descriptor. Consumer cursor = **sequence** (`nev_cursor` internal only). `stream.subscribed` before start (C1-08). `auto` refused on authoritative/visual runs. Typed occupancy. Advertised retention. Kill snapshot-diff SSE.
5. **A1.** Replay Luna med 10× through Containers. World = Craftax engine; tasks = seeds 0–9. Visual connected before step 1. Env reward is a DAG node with environment authority. Advertise true_checkpoint only if the rust engine restore is proven.
6. **Harbor fold v1 (A2).** World = env image; task = instruction + tests; verifier is a script node (`tests/test.sh` → `reward.txt`/`json`) as a distinct execution. Two policies. `live.harbor_eval.v1`. Native-vs-wrapped verifier agrees. ATIF is a projection. `true_checkpoint` unsupported; grading leftover optional. Nested GameBench DEO: child Craftax container exposes `PUT /policy` / `POST /policy/restart`; Codex calls those, not `run_*_hillclimb_task.py`.
7. **A7.** Unmodified Echo image. Native-vs-wrapped fixed actions. Then stop until Prime/Chess.
8. **A8 dig.bench (headless).** Relay over their Agent REST. Mock target in PR. `--paid` one public game on nightly. `live_frames` / `true_checkpoint` unsupported. Two policy configs (basic vs agentic mcp_bind). `/reward` from env status. Token never in the log. Desktop visual is Workshop W3, not this step.

Do not: invent a GameBench protocol; fold Inspect; promote Archipelago to a fold (Compose-world is the lesson); treat Prime’s optional Harbor/OpenEnv internals as “we support all three”; mix two datasets into one SFT job; wait on Specta codegen; put Dockerfiles or Compose files on the task record; Harbor-wrap or OpenEnv-wrap dig.bench; invent frames for a text game.

---

## 11. Acceptance that this workstream owns

| ID | Owned here? | Note |
| --- | --- | --- |
| A1 Craftax Luna med 10× | **Yes** | Heavy world (engine) + light seed tasks + env-reward node + real stream. |
| A2 Harbor GameBench live | **Yes** | First-class fold, two policies, visual first, verifier as score-DAG script node. |
| A5 Durable stream | **Yes** | Shared with every later algorithm. |
| A7 OpenEnv Echo | **Yes** | Wrapper, not a fold. False checkpoint claim gone. |
| A8 dig.bench (headless C8) | **Yes** | Hosted content + relay. Mock in PR; `--paid` live nightly. Desktop visual is Workshop. |
| Score endpoint | **Yes** | `POST /reward` + `GET /reward` on `rollout_id` and on provided evidence. Missing ≠ 0. Harbor verifier is a script node (`202` while running). |
| Affordance bind | **Yes** | Harbor TB3 binds with `true_checkpoint=unsupported`. MAPO-class recipe refuses that world. Craftax may prefer restore. dig.bench refuses `live_frames` and `true_checkpoint`. No silent degrade. |
| A3 two GEPA / A4 two SFT / A6 SFT checkpoint-eval | No | Need this split first so child evals have somewhere to live. |

Pass rule is unchanged: real run, connect-before-start, persist-before-publish, no invented fields, no private Evals names on public surfaces.

Containers-first programmatic suite (this version, before evals/optimizers/workshop move): **§12**.

---

## 12. Containers-first programmatic acceptance

Ship a **containers version** when this suite passes. Then evals, optimizers, optimizers-beta, and Workshop consume the same receipts. They do not re-implement `/reward`, streams, or pins.

Workshop visuals and optimizer child-evals are **not** in this version. The suite still freezes the contracts they signed off: declared stream IDs (not constructed URLs), slot `stream` (not `live` or `jobs`), `stream.subscribed` before start, consumer cursor = **sequence**, create-rollout echoes the full stream descriptor, poll/SSE equivalence, missing ≠ 0, `/reward` nullable scalar + `node_results[]`, typed occupancy, advertised artifact retention, no `telemetry.transport=auto` on authoritative/visual runs, child `rollout_id`s as resource refs, two concurrent logs that do not cross.

### Runner

Lives in containers. Black-box against a running base URL; in-process reference server for CI.

```text
tests/conformance/container_compat/
  run.py --base-url URL --target TARGET [--paid] [--receipt PATH]
  targets:  craftax_engine | craftax_react | craftax_code_policy
            harbor_public  | deo_nested    | openenv_echo
            digbench_mock  | digbench_public
  receipt:  synth.container-compat-conformance.v1
```

Every run writes a digest-addressed receipt: containers version, target, adapter_chain, advertised affordances, evaluation_plan_ref, test id → pass/fail/skip, stream-descriptor digest, transport transcript digests, Trace V5 digest if sealed. Downstream repos pin that digest.

`--paid` is off in PR CI (no Luna, no live dig.bench token). Release/nightly may turn it on for `craftax_react` and `digbench_public`. Engine-acceptance (fixed-action / stub policy) is not a ReAct eval; the receipt says which. `digbench_mock` is a recorded/stub Agent API; it is not a model eval.

Skip is allowed only when the target honestly advertises `unsupported` and the test `require`s that affordance. Skip because “we will add SSE later” is a fail.

### Gate order

```text
  C0  pin + honesty          every target
  C1  stream (A5 / TS-C)     every target that advertises a live log
  C2  POST/GET /reward       every target
  C3  Craftax interactive    craftax_engine, craftax_react
  C4  code-policy / DEO      craftax_code_policy, deo_nested
  C5  Harbor fold            harbor_public
  C6  OpenEnv Echo           openenv_echo
  C7  Workshop/optimizer freeze   every target (headless consumers)
  C8  dig.bench              digbench_mock, digbench_public
```

C0–C2 + C7 are the shared floor. C3–C6 and C8 are target suites. Prime / Chess / APEX fold / A3–A4–A6 / GEPA-on-dig.bench are **out** of this version. **C7 and C8 are signed off** — implement the floor (plus the seven freezes below).

### C0 — pin and honesty

| ID | Pass when |
| --- | --- |
| C0-01 | `/metadata` names `world_ref`, `environment_ref`, `policy_ref` (harness + config + optional code), `evaluation_plan_ref`, `task_instance_id`. No sibling `harness_ref`. Logical service IDs present even in-process. |
| C0-02 | Affordances default `unsupported`. Booleans are derived from `level != unsupported`, not the other way around. |
| C0-03 | Bind: recipe `require true_checkpoint` against Harbor/Echo/dig.bench **refuses** and names the affordance. Recipe `require live_frames` against dig.bench **refuses**. Recipe `unused` does not call the route. |
| C0-04 | Receipt has `adapter_chain` (e.g. `[]` native, `[harbor]`, `[openenv]`). No private Evals runner names. |
| C0-05 | OpenEnv wrap: `checkpointable` is not true; `state()` is not advertised as `true_checkpoint`. |
| C0-06 | Harbor: `reward.txt` authority is `trusted_scorer` / script node, not `environment`. |
| C0-07 | Rubric/recovery helpers in this version do not coerce missing to `0.0`. Fixture with absent reward stays absent. |
| C0-08 | **Typed occupancy.** If `scale_leases` cannot admit another episode, create-rollout returns busy/queued and names the affordance. It does **not** silently share a world lease or append into another run’s log. |
| C0-09 | **Artifact retention advertised** (`run` vs TTL) on metadata / stream descriptor. Frames/artifacts fetchable by digest after `world_stop` for that window, **or** receipt says `retention: run` so the consumer copies during the live window. Silent 404 after stop **fails**. |

### C1 — durable stream (A5, TS-C01…C08)

What Workshop will bind as `live.*.v1` `stream` and what an optimizer visual will drill into for a child eval.

| ID | Pass when |
| --- | --- |
| C1-01 | Create-rollout **names** `poll` / `sse` / `websocket` (not `auto`). Response **echoes the full stream descriptor**: `stream.id`, `transports.poll.url`, `transports.sse.url` (null if not bound), `transports.websocket.url` (null if not advertised), `cursor.kind=sequence`, `reward.url`, `auth.mode`, `retention`. Asking `sse` when only poll exists **refuses** (authoritative run). `telemetry.transport=auto` **refuses** on authoritative / visual-attached runs. No silent degrade. |
| C1-02 | `poll` is implemented. Consumer cursor is **`sequence`**. `GET .../events?after=` uses sequence. Heartbeats do not advance it (TS-C04). Relay may keep `nev_cursor` / ordinal internally (`cursor.producer_kind`); the test client never sends `nev_cursor`. |
| C1-03 | If `sse` is advertised: poll and SSE yield the same ordered envelope IDs/digests (TS-C01). `Last-Event-ID` resumes from **sequence** (TS-C02). |
| C1-04 | If `websocket` is advertised: same envelopes as SSE, not a second schema. If not advertised, the field is omitted/null (TS-C08). |
| C1-05 | Disconnect after a sequence, reconnect, no loss; duplicates de-duplicable (TS-C03). Snapshot-diff SSE (local counter) **fails** this test. |
| C1-06 | EOF / `[DONE]` / socket close is not completeness. Completeness = `closed=true` and consumer cursor = `high_water` (TS-C06). |
| C1-07 | Missing sequence / usage / reward **fail closed** in the fixture (TS-A07). A consumer JSON that would feed a visual must not show `0` for those. |
| C1-08 | **Connect-before-start (headless TS-E01):** subscribe, then a **non-advancing** control record `stream.subscribed` (`stream.id`, `rollout_id`/`run_id`, `next_sequence`, `ready: true`) **before** `POST /rollouts` / first paid/mutating event (`start_session` for dig.bench). HTTP 200 on GET/subscribe is **not** ready. First **semantic** event is `trace.opened`. Spans open before data and close before parent terminal. Heartbeats never count as ready. |
| C1-09 | Slot/stream identity is a **declared** `stream.id` from the create-rollout descriptor. Fail if the test has to guess `/events` vs `/rollouts/{id}/stream`, or bind a slot named `live` **or `jobs`**. (Craftax `live` and Harbor `jobs` are Workshop bugs, encoded as a test.) |
| C1-10 | After `capture.closed`, seal Trace V5. Live high-water matches the seal (TS-D02/D03) or the target honestly has no live capture (Harbor may be weaker; then skip D with reason, do not fake a gym log). |

### C2 — `/reward`

| ID | Pass when |
| --- | --- |
| C2-01 | `GET /reward?rollout_id=` before POST → `status=absent`, `reward=null`, HTTP 200. Not `0.0`. |
| C2-02 | After terminal evidence, `POST /reward { rollout_id }` returns `EvaluationExecution`. Combiner matches the bound plan. |
| C2-03 | Second POST, same plan digest + evidence digest, same `execution_id` (idempotent). `rescore=true` → new id; old record remains. |
| C2-04 | `mode=terminal` on a live rollout → `409 incomplete` + `missing_evidence[]`. |
| C2-05 | `mode=provisional` refused unless `live_reward` is advertised. Craftax engine: provisional = sum of RewardSignals **in the log** up to cursor. Harbor and dig.bench: unsupported. |
| C2-06 | POST `/reward` does not call `/step` / mutate the env (spy or generation counter). |
| C2-07 | Gate fail / scorer refuse → `status=gated|refused`, `reward=null`, reason set. |
| C2-08 | Harbor (or long script node): `202` + `/evaluations/:id/events`; completion is execution status, not SSE EOF. |
| C2-09 | Env-sum node: `POST /reward` equals the sum of RewardSignals in the log (null if any required signal is missing — do not fill 0). Script node: parsed `reward.txt` is that node’s value, authority ≠ environment. |
| C2-10 | Provided-evidence path (APEX-shaped fixture or Harbor leftover tarball) works without a live env. Rollout XOR evidence enforced (`422` if both/neither). |
| C2-11 | Product combiner with a missing required basis → `reward=null`, not identity `1.0` (tau3 bug). |

### C3 — Craftax interactive (`craftax_engine`, `craftax_react`)

`craftax_engine`: stub/fixed-action policy (CI). `craftax_react`: ReAct + Luna med (`--paid`). Receipt labels which. Engine-acceptance must not be reported as a model eval.

| ID | Pass when |
| --- | --- |
| C3-01 | Ten task instances (seeds 0–9) through Containers HTTP with `world_ref` + `policy_ref={harness, config}` + named stream. Not evals gold CLI. |
| C3-02 | Engine stays up across the ten; `scale_leases` ≥ 10 or documented serial leases. Concurrent seeds have distinct `rollout_id`s and logs (C7-O02). An 11th create while full is **busy/queued** (C0-08), not a shared lease. |
| C3-03 | RewardSignals appear in the log **before** `/reward`. Frames if `live_frames=native`. NEV kinds verbatim. |
| C3-04 | `POST /reward` per rollout is env-sum; leaderboard JSON uses that field, not a parallel array of step rewards with holes filled. |
| C3-05 | `POST /policy-configs` luna_med and sol_med (or two stub configs). Next rollouts use the new config; engine generation unchanged. In-flight attempt keeps the old pin. |
| C3-06 | `true_checkpoint` / `restore` / `fork`: if advertised native, prove restore (RNG+cursor). If unproven, advertised `unsupported` and C0-03 still refuses MAPO. |
| C3-07 | Seal Trace V5; observation, action, reward, policy-call correlation holds. `--paid` additionally: no policy span before the C1-08 ready ACK. |
| C3-08 | Headless visual projection: replay cursor 0..N, subscribe N+1, then start. Scrub JSON at a cutoff shows only events ≤ cutoff (TS-E03 subset). |

### C4 — code-policy DEO (`craftax_code_policy`, `deo_nested`)

This is the refactored DEO: HTTP, not `run_*_hillclimb_task.py`.

| ID | Pass when |
| --- | --- |
| C4-01 | `PUT /policy { code: heuristic bytes }` → new `PolicyRevision`. Engine generation unchanged. |
| C4-02 | `POST /policy/restart` bounces IsolatedPolicyProcess (or equivalent); env leases and durable log survive. Isolation receipt present. |
| C4-03 | In-flight scored attempt still bound to the old revision after C4-01. |
| C4-04 | `POST /rollouts` with the new `policy_ref` produces a new episode; `POST /reward` is child env-sum. |
| C4-05 | `bind_policy_config` on the player is `unused` / refused (it is a program). |
| C4-06 | **`deo_nested`:** Harbor trial does **not** advertise `live_frames` / `step`. Child Craftax does. Parent `POST /reward` is the hillclimb DAG (delta + held-out **gate**), not a copy of child env-sum. |
| C4-07 | Test client never shells the hillclimb wrapper. If the only way to score is that script, C4 fails. |

### C5 — Harbor fold (`harbor_public`)

Public fixture (TB-shaped or packaged GameBench). Not a GameBench wire format.

| ID | Pass when |
| --- | --- |
| C5-01 | World = env image; task = instruction + tests; `blocking_trial=native`. |
| C5-02 | Two `policy_ref`s registered **before** start (e.g. two Codex/Luna configs). Mid-trial `bind_policy_config` refused. |
| C5-03 | Stream is trial/attempt/policy/verifier events. No fake Craftax map. Slot bind uses declared `stream.id` (C1-09). Fail slot `jobs` / `live`. |
| C5-04 | Native verifier vs wrapped `POST /reward` agree on the script-node value (tolerance declared). ATIF is a projection of the log, not the log. |
| C5-05 | `true_checkpoint=unsupported`. MAPO recipe refuse (C0-03). `grading_snapshot` optional. |
| C5-06 | Connect-before-start (C1-08) on the Harbor stream. Completeness is verifier execution, not heartbeat. |

### C6 — OpenEnv Echo (`openenv_echo`)

| ID | Pass when |
| --- | --- |
| C6-01 | Unmodified Echo image. Native client vs wrap: same reset/fixed-steps/state/reward/done. |
| C6-02 | C0-05 holds after the wrap. |
| C6-03 | `/reward` is env-authority. Wrap does not recompute a Harbor-style script reward. |
| C6-04 | Transport smoke: poll required; WS only if advertised. |

### C7 — freeze for Workshop + Optimizers (headless)

No Desktop, no optimizer sidecar. A test **consumer** that is the contract those repos will implement.

**Workshop visual consumer**

| ID | Pass when |
| --- | --- |
| C7-W01 | Subscribe to the **declared** `stream.id` and wait for `stream.subscribed` before start (C1-08). Fail on guessed URLs or slot `live` **or `jobs`**. |
| C7-W02 | Persist raw envelopes to disk. Kill the container. Replay the file: same **sequences**/digests, no duplicated facts (TS-E02 subset). This is persist-before-publish without Workshop. |
| C7-W03 | Projection JSON a visual would bind: missing reward/usage/score are `null`/omitted, never `0` / `$0.00`. |
| C7-W04 | Same core reducer over Craftax and Harbor fixtures; only data kinds differ (TS-E08 subset). Harbor fixture has no `live_frames`. Craftax fixture has no `reward.txt`. |
| C7-W05 | Collector/capability blobs do not appear in the exported receipt (TS-E05). |
| C7-W06 | After seal, a second consumer opens the Trace V5 only (engine gone) and still correlates the same rollout ids. |

**Optimizer child-eval surface** (what GEPA/SFT will attach as resource refs, not `optimizer_event` payloads)

| ID | Pass when |
| --- | --- |
| C7-O01 | Each child eval is a Containers `rollout_id` + stream id + `/reward` URL. No flattening child NEV into an optimizer envelope in this version. |
| C7-O02 | Two rollouts in parallel (or honestly queued): distinct logs, distinct usage, distinct `/reward`. Flipping which SSE client is read does not stall the other (A3 multiplex, container layer). Exhausted `scale_leases` → C0-08, not a shared lease. |
| C7-O03 | `PUT /policy` / `bind_policy_config` between children does not mutate an in-flight child’s pin. |
| C7-O04 | Summary fixture an optimizer visual would show: absent child reward stays empty; does not drop the child. |
| C7-O05 | Usage records nullable; missing usage is not `0` tokens. |

### C8 — dig.bench (`digbench_mock`, `digbench_public`)

Hosted content. Relay is EnvironmentService. Not a fold. `digbench_mock`: recorded/stub Agent API (PR). `digbench_public`: one pinned public game via `DIGBENCH_API_TOKEN` (`--paid`, nightly). Receipt labels which. Mock is not a model eval.

Normative split: §4.11 / §Map.

| ID | Pass when |
| --- | --- |
| C8-01 | Pin: `world_ref` = game id; `environment_ref` = relay session; `policy_ref` = harness + config; named stream. `adapter_chain=[]` (native relay), not `[harbor]` or `[openenv]`. |
| C8-02 | `live_frames=unsupported`. Recipe `require live_frames` refuses (C0-03). Log has the **seven** kinds: `session.opened`, `observation`, `legal_actions`, `stats`, `action`, `invalid_action`, `status`. Optional raw JSON payload on `observation` / `session.opened`. **No** second `state` event. No image frames. |
| C8-03 | `true_checkpoint` / `restore` / `fork` unsupported. `get_session` is advertised as `reconnect` only. |
| C8-04 | Two policy configs: **basic** (next-action / ReAct; `mcp_bind` unused/refused) and **agentic** (`mcp_bind` native for `digbench-mcp` tools). Agentic MCP calls ride the **same** eval/trace stream as policy spans (not a nested log). Mid-session `bind_policy_config` refused. |
| C8-05 | `POST /reward` after `completed` → `1.0`; after `game_over` → `0.0`; before done → C2-04 incomplete / GET null. Authority = environment. Lives/level are not the reward. POST does not call `step` or `start_session`. |
| C8-06 | First mutating/token event is `start_session`. C1-08 applies: `stream.subscribed` **before** that call. |
| C8-07 | Token / `Authorization` / `DIGBENCH_API_TOKEN` absent from envelopes, receipts, and Trace V5. |
| C8-08 | Same core reducer as C7-W04: dig.bench fixture has no `live_frames` and no `reward.txt`. Creative-mode control is omitted unless the API affordance is advertised native. |
| C8-09 | Two concurrent sessions (or honestly queued): distinct `session_id`s, logs, usage, `/reward`. Flip SSE without stall (C7-O02). Occupancy: C0-08. |
| C8-10 | Persist relay log; drop their session (mock: delete stub; paid: do not call get_session). Replay file + Trace V5 still correlate obs/action/status (C7-W02/W06). |
| C8-11 | Freeze **one** public game id on the receipt (P-1, or first `list_games` if P-1 is gone). Stub/random-legal-action allowed on mock. **PR `digbench_mock` may skip agentic MCP.** Nightly `digbench_public --paid` **must** run the agentic path. A8 Desktop needs **both** harnesses; basic-only Desktop + agentic-headless does not pass A8. |

### What this version does **not** claim

| Later | Why it waits |
| --- | --- |
| A1 in Workshop Desktop (full TS-E01…E08 UI) | Needs Workshop bind to C7-W receipts |
| A2 as a user-driven Harbor register in-app | Needs Workshop + evals recipe on this containers version |
| A3 two GEPA / A4 two SFT / A6 checkpoint-eval | Need optimizers / optimizers-beta; they **pin C7-O** |
| A8 in Workshop Desktop (`live.digbench.v1`) | Needs C8 receipt + Workshop W3. Headless C8 is this version. |
| Prime GSM8K, Chess OpenEnv, APEX fold | After Harbor v1 + Echo |
| GEPA/SFT on dig.bench, private dig.bench tiers | Out of cut |
| Paid Luna 10× / live dig.bench in PR CI | `--paid` nightly/release; C3 engine + C8 mock are the PR gate |

### Downstream consume rule

When this containers version ships:

```text
  evals              emit into the Containers relay; keep gold clients if needed
                     DEO recipes call PUT /policy, not hillclimb shell
  optimizers         child evals = C7-O resource refs; optimizer_event.v1
                     does not carry env frames; missing reward stays missing
  optimizers-beta    same; SFT checkpoint campaigns are sets of C3/C2 rollouts
  workshop           visuals bind C1 stream ids; persist C7-W02; templates
                     live.harbor_eval.v1 / Craftax live consume C7-W04 kinds
                     live.digbench.v1 consumes C8 kinds (text obs, no frames)
```

A downstream PR that constructs `/events`, fills missing reward with 0, or treats Harbor `reward.txt` as env reward fails **even if** the containers receipt passed.

### CI shape

```text
  PR       C0 + C1 + C2 + C3 engine + C4 HTTP + C5 fixture + C6 Echo + C7
           + C8 digbench_mock
           (no --paid, no hosted SFT, no Desktop, no live dig.bench token)
  nightly  + craftax_react --paid (creds) + deo_nested if Harbor image pinned
           + digbench_public --paid (DIGBENCH_API_TOKEN; one public game)
  release  receipt digest published; downstream repos bump the pin
```

---

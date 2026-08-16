# Eval

Use `eval` when the user has several **policy variants** and wants to know which
one is better, measured against a pinned evaluation container. It scores an
immutable candidate set on a fair `candidate x seed x scenario` matrix and
returns a promotable winner only when the recipe's gates pass.

`eval` is local-only (`source: "local"`). It never appears as hosted, and it has
no checkpoints and no inference endpoint — a scorecard is not a model.

Two statuses come back and neither substitutes for the other:

- run status: `completed | failed | cancelled` — did orchestration finish
- selection status: `promoted | no_champion | inconclusive | invalid_evidence` — did a candidate win

Report both. A `completed` run with `no_champion` is a successful run that found
no winner, and saying "completed" alone misrepresents it.

## Stage before you start

`start_recipe` takes a `candidate_set_id`, never a path or inline code. Create or
identify the policy files in the session workspace first, then freeze them:

```json
{"operation":"stage_eval_candidates","arguments":{
  "candidates":[
    {"label":"baseline","path":"policies/baseline","entrypoint":"heuristic_baseline:choose_actions","kind":"python-code.craftax-choose-actions.v1","baseline":true},
    {"label":"memory-v2","path":"policies/memory_v2","entrypoint":"heuristic_baseline:choose_actions","kind":"python-code.craftax-choose-actions.v1"}
  ]}}
```

Paths are **workspace-relative**; absolute paths and `..` are refused. The
host fills in the calling session, so omit `session_ref` rather than inventing
one. Mark
exactly one baseline — a recipe whose `decision_mode` is `promote` cannot compute
a paired lift without one, and will return `inconclusive`.

Then start with the returned id:

```json
{"operation":"start_recipe","arguments":{
  "recipe_id":"eval.craftax.code-policy.smoke.v1",
  "candidate_set_id":"policy_set_...",
  "open_visual":true}}
```

## Recipes

Call `list_recipes` and read `limits` — seeds, trials, parallelism, and the
selection rule are all published there. Availability is honest: a recipe whose
target image is not pinned reports `unavailable` with a reason, and starting it
fails rather than silently substituting a tag.

| Recipe | Candidate kind | Decision |
|---|---|---|
| `eval.fixture.policy-smoke.v1` | `python-code.v1` (`policy:Policy`) | promotes; deterministic, no benchmark |
| `eval.craftax.code-policy.smoke.v1` | `python-code.craftax-choose-actions.v1` | report-only |
| `eval.gamebench.craftax-code-policy.confirm.v1` | `python-code.craftax-choose-actions.v1` | promotes |
| `eval.craftax.llm-policy.smoke.v1` | `llm-policy.v1` | report-only |
| `eval.gamebench.llm-policy.confirm.v1` | `llm-policy.v1` | promotes |

### LLM candidates

For an `llm-policy.v1` recipe the candidate is **data, not code**: a directory
containing `policy.toml`, staged with `kind: "llm-policy.v1"` and
`entrypoint: "policy.toml"`.

```toml
model = "gpt-5.6-luna"   # must be in the recipe's published `models` allowlist
effort = "medium"        # must be in that model's `efforts`
temperature = 0
plan_min = 5
plan_max = 20
```

The route, the token rates, and the per-trial spend and call caps are recipe
data. Never offer to change a model's route or price, and never accept a model
outside the allowlist — say which models the recipe permits instead. These
recipes call a paid provider: state the recipe's `budget` (`max_llm_calls`,
`max_usd` per trial) and the trial count before asking for approval.

## Follow and present

Slices are `eval.runtime`, `eval.trials`, `eval.scorecard`, `eval.evidence`.

- `eval.scorecard` — one row per candidate per stage: valid/failed trial counts,
  per-metric means, paired lift against the baseline, cost, and any elimination
  reason. Present candidates as rows. Never collapse them into one aggregate.
- `eval.trials` — one row per `candidate x seed x scenario`.
- `eval.evidence` — sealed manifest digest, the seed ledger, the selection, and
  the evidence directory.
- `eval.runtime` — queue depth, running trials, semaphore leases held.

Rules when reporting:

- A failed trial is failed evidence. Never describe a missing metric as `0`.
- Quote the selection `reason` verbatim; it names the rule that decided.
- Every trial writes a `trace`. Point at the evidence directory rather than
  pasting rollouts.
- Never replace a policy on the user's behalf, even after `promoted`. Report the
  winner and the evidence link and let the user decide.

## Holding and stopping

`pause_run` holds the matrix: the worker stops dispatching new trials, and the
ones already in a container finish and seal. A paused run does not sit on a
semaphore token, so pausing frees capacity for another run. `resume_run` picks
up where it left off — a pause changes timing, not evidence, and the selection
is the same either way. Watch for `eval.run.paused` / `eval.run.resumed`.

Cancellation seals evidence: `cancel_run` asks the worker to stop its containers,
release its leases, and finish writing. Expect `cancelled` with
`invalid_evidence`, which is correct, not a bug.

The run opens `optimizer.eval.live.v1`: stage timeline, selection verdict,
candidate comparison, trial matrix, and sealed evidence. Open it when comparing
candidates is clearer there than in text — which is most of the time.

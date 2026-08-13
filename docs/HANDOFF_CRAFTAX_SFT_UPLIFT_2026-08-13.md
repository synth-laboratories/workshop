# Handoff — Craftax trace-to-SFT uplift, end to end

**Date:** 2026-08-13  
**Owner next:** engineer validates the product path, then launches one Workshop agent to execute it  
**Primary recipe:** `sft.craftax.nemotron-nano.tinker.v1`  
**Algorithm:** `sft` (never `goex.sft.v1`)  
**Student default:** `nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16`  
**Training backend:** hosted Tinker through `optimizers-beta`  
**Evaluation substrate:** Craftax Rust/GameBench through Containers  

## 1. Outcome

Prove that Workshop can autonomously run a complete Craftax improvement loop:

```text
baseline student evaluation on frozen seeds
  → teacher rollout generation on disjoint data seeds
  → durable Trace V5 collection
  → truthful trajectory curation
  → decision-level SFT dataset
  → hosted Tinker training with multiple checkpoints
  → checkpoint selection on fresh selection seeds
  → one promoted checkpoint
  → final base-vs-trained evaluation on untouched heldout seeds
  → report measured uplift and link every aggregate to inspectable evidence
```

This is not complete when training finishes. It is complete only when the promoted
checkpoint runs new Craftax rollouts and the report compares its score with the exact
untrained student under identical settings.

## 2. Existing floor

The following pieces already have real receipts and should be reused rather than forked:

- Craftax Containers rollouts with declared `stream` binding, subscribe-before-start,
  durable event log, `/reward`, and Trace V5.
- Hosted Tinker SFT lifecycle with real checkpoints, optimizer events, checkpoint
  sampling, child evaluation campaigns, promotion, and durable replay.
- Workshop `optimizer.sft.live.v1` with inspectable checkpoints, campaigns, children,
  metrics, and promotion.
- Local import/replay of completed SFT runs.
- Missing reward, validation loss, usage, and cost remain missing; they never become
  fabricated zeroes.

Reference receipts:

- `receipts/external-acceptance/local-slots/sft-banking77/receipt.json`
- `receipts/external-acceptance/local-slots/workshop-replay/receipt.json`
- `receipts/external-acceptance/a11-final-live/receipt.json`
- `receipts/external-acceptance/o1-final-live/receipt.json`

The remaining engineering work is the Craftax-specific **data generation and curation
front half**, plus the final paired uplift report. Do not build another optimizer log,
another child stream, or a second checkpoint evaluator.

## 3. Experimental design

### 3.1 Identities and frozen splits

Create and persist the split manifest before the first rollout. Seeds/world identities
must be disjoint across all four roles:

| Split | Initial size | Purpose |
|---|---:|---|
| `baseline_eval` | 20–50 | Score the unchanged student before training |
| `teacher_collection` | 50–100 | Produce candidate training trajectories |
| `checkpoint_selection` | 20–50 | Select among training checkpoints |
| `final_heldout` | 20–50 | One final base-vs-promoted comparison |

Rules:

1. Split by seed/world identity **before** generation or filtering.
2. No collection trajectory, prefix, frame, or derived example may enter selection or
   final heldout.
3. The final heldout split is not used to choose a checkpoint or tune a filter.
4. Base and trained policies run the same final seeds, harness, environment version,
   decoding parameters, step limit, and retry policy.
5. Persist the split digest in the dataset, optimizer run, and final receipt.

### 3.2 Policy roles

- **Teacher:** a pinned capable Craftax policy that generates demonstrations. Record
  provider, model, effort, prompt digest, harness, decoding, and cost. Do not silently
  substitute a fixed-action policy or the student.
- **Base student:** the exact Tinker base model before adaptation. It establishes the
  baseline and runs again in the paired final evaluation.
- **Checkpoint student:** each Tinker sampler checkpoint, evaluated only through the
  optimizer-owned opaque checkpoint sampler endpoint.
- **Promoted student:** the selected checkpoint. Promotion is a decision after selection
  evaluation; `checkpoint.ready` is not promotion.

## 4. Phase A — visual-first baseline

1. Resolve the registered Craftax environment, policy configuration, evaluator, and
   immutable seed manifest.
2. Open the Craftax comparison visual and wait for persisted readiness.
3. Prepare each base-student rollout and bind the exact declared `stream` descriptor.
4. Observe non-advancing `stream.subscribed` before the first model call or environment
   mutation.
5. Run the base student on `baseline_eval` seeds.
6. Seal each rollout to Trace V5 and fetch authoritative `/reward`.
7. Record reward, achievements/progress, episode length, invalid actions, policy
   failures, timeouts, usage, and metered cost when emitted.

Pass condition: every aggregate row links to its rollout id, stream id, reward receipt,
and Trace V5 digest. Failed or incomplete attempts remain visible and are not scored as
zero unless the evaluator authoritatively returns zero.

## 5. Phase B — teacher trace collection

Run 50–100 teacher rollouts on `teacher_collection`, using bounded parallelism and an
authorized spend ceiling. The visual must be connected before execution.

For every rollout preserve:

- run, rollout, seed/world, environment, evaluator, teacher-policy, and prompt identity;
- ordered observations, public model output/tool calls, valid actions, environment
  transitions, frames or frame refs, rewards, achievements, and terminal state;
- provider usage/cost and failure taxonomy;
- raw capture digest, live projection cursor, reward-receipt digest, and Trace V5 digest.

Private hidden reasoning is not training data. Only public model output and the exact
action/tool-call representation used at inference may become targets.

## 6. Phase C — curation

The curator consumes sealed traces and writes an immutable selection manifest. It must
not edit the original traces.

### Hard rejection

Reject a trajectory or segment when any of these hold:

- trace is unsealed, incomplete, corrupt, or identity-inconsistent;
- environment, provider, policy, or infrastructure failure contaminated the behavior;
- action is invalid or cannot be reproduced in the student inference format;
- reward receipt is missing where the filter requires a score;
- collection seed overlaps selection or heldout;
- target contains private reasoning, secrets, host paths, or unsupported tool syntax.

### Ranking and diversity

Do not select by terminal reward alone. Rank using a documented function over:

- authoritative reward and achievement/progress coverage;
- useful prefixes before a later failure;
- action validity and efficiency;
- seed/world diversity;
- strategy/achievement diversity;
- episode length and repeated-state/action penalties.

Retain approximately 20–40 diverse trajectories for the first run. Cap the contribution
from one seed, one achievement pattern, or one repeated action strategy. Save explicit
accept/reject reasons and the score components for every candidate.

The Workshop curation view should show the score distribution, acceptance funnel,
achievement coverage, seed coverage, action distribution, length distribution, and
inspectable accepted/rejected traces.

## 7. Phase D — dataset materialization

Convert retained trajectories into decision-level SFT examples using the exact student
inference representation:

```json
{
  "messages": [
    {"role": "system", "content": "<pinned Craftax policy instruction>"},
    {"role": "user", "content": "<observation and allowed history>"},
    {"role": "assistant", "content": "<exact valid next action/tool call>"}
  ],
  "provenance": {
    "rollout_id": "...",
    "trace_v5_digest": "...",
    "seed": 101,
    "step": 17,
    "curation_reason": "..."
  }
}
```

Requirements:

- deterministic ordering and canonical serialization;
- schema validation and tokenizer/context-length validation;
- no duplicate examples after canonical hashing;
- no heldout identities or secrets;
- `training_file_id`, example count, token count, source-trace count, and immutable
  `dataset_digest` emitted as `sft.dataset.validated`;
- the original selection manifest and example-to-trace index remain durable.

## 8. Phase E — hosted Tinker training

Use the product-owned recipe and model catalog. Callers may choose an allowlisted
`base_model`; they may not supply arbitrary commands, paths, or credentials.

First-run bounds:

- 20–40 source trajectories;
- 2–4 checkpoints;
- LoRA rank 8 or the recipe default;
- bounded steps/epochs and one accelerator slot;
- one explicit training spend ceiling.

Required sequence:

1. Workshop creates the hosted optimizer run and opens `optimizer.sft.live.v1`.
2. `optimizer.visual.ready` is persisted before training begins.
3. Tinker emits real aligned training metrics.
4. At every configured step, save sampler weights and state.
5. Emit `sft.checkpoint.created`, then `sft.checkpoint.ready` with real provider ids and
   digests.
6. Never promote from training loss alone.

## 9. Phase F — checkpoint selection campaigns

For every ready checkpoint:

1. Create an opaque authenticated checkpoint inference target.
2. Allocate one evaluation campaign on `checkpoint_selection` seeds.
3. For each child: prepare → bind declared `stream` → observe `stream.subscribed` →
   start → fetch `/reward` → seal Trace V5.
4. Emit one `synth.resource-ref.v1` per child and patch it on completion.
5. Compute mean/median, dispersion, success rate, achievements, invalid-action rate,
   failure rate, usage, and cost from non-missing authoritative values.
6. Emit `sft.checkpoint.promotion_evaluated`; promote exactly one checkpoint only when
   the configured rule passes.

Recommended first promotion rule:

- maximize mean selection reward;
- require no material reliability regression versus the base student;
- use achievement coverage and paired seed wins as tie-breakers;
- do not promote when every reward is missing.

## 10. Phase G — final paired evaluation

Run the unchanged base student and promoted checkpoint on the exact same untouched
`final_heldout` seeds. Randomize or interleave scheduling to reduce temporal bias, but
keep configurations identical.

The final report must contain:

- sample sizes and exact seed-manifest digest;
- base and trained mean, median, standard deviation, confidence interval, and success
  rate;
- absolute and relative score change;
- paired per-seed trained win/loss/tie counts and score deltas;
- achievement/progress coverage delta;
- episode-length and action-distribution changes;
- invalid-action, timeout, crash, and incomplete rates;
- teacher, base, checkpoint, environment, evaluator, dataset, and recipe identities;
- training/evaluation tokens and real metered cost, with missing cost kept null;
- links from every aggregate to inspectable base and trained rollout traces.

An uplift claim is allowed only when heldout evidence supports it. A negative or null
result is a valid completed experiment and must be reported honestly.

## 11. Required Workshop experience

The Workshop agent should expose one linked workspace with:

1. **Baseline:** base-student lanes and score distribution.
2. **Collection:** live teacher lanes, trace status, rewards, achievements, and failures.
3. **Curation:** accepted/rejected traces with reasons and diversity coverage.
4. **Training:** metrics and checkpoint rail (`created` ≠ `ready` ≠ `promoted`).
5. **Campaigns:** inspectable checkpoint children with exact resource refs and scores.
6. **Comparison:** base vs promoted distribution, paired seed matrix, coverage delta,
   reliability, cost, and promotion rationale.

All children, candidates/checkpoints, curation decisions, and proposer/agent traces must
be inspectable. Reopening after the agent, Tinker slot, and Craftax engine are stopped
must reconstruct the same state from durable logs and Trace V5.

## 12. Engineering readiness gate

The engineer must verify these before launching the agent:

### Containers

- Craftax teacher and student policies can be pinned independently.
- Base-model and opaque-checkpoint sampler targets use the same inference/action format.
- Declared stream, readiness, `/reward`, Trace V5, reconnect, and reopen pass.
- Batch seed registration refuses overlap across split roles.

### optimizers-beta / SFT

- Recipe is advertised available only when Tinker, Containers, model catalog, training
  data storage, and opaque sampler prerequisites are healthy.
- Trace ingestion/curation emits a deterministic dataset and provenance index.
- Tinker saves every configured checkpoint, not only the final one.
- Checkpoint sampler route is authenticated and never exposes provider credentials.
- Campaign children are durable resource refs; promotion is separate from readiness.
- Cancellation, idempotency, accelerator occupancy, budgets, and terminalization pass.

### Workshop

- Visual is created and ready before baseline, collection, training, and eval mutation.
- The agent can invoke the entire flow through registered MCP/recipe surfaces without
  constructing private URLs or caller-supplied shell commands.
- SFT visuals project dataset/curation, checkpoints, campaigns, children, promotion, and
  final comparison.
- Candidate/child/trace selection is keyboard accessible and works after durable replay.

### Mandatory dry tests

- fixture collection → curation → dataset digest;
- one tiny real teacher collection;
- one two-checkpoint Tinker smoke;
- one checkpoint sampled through Containers;
- one paired base/checkpoint rollout on the same seed;
- disconnect/reconnect and reopen with source services stopped;
- secret scan of logs, receipts, screenshots, visual bindings, and Trace V5.

Do not launch the full agent experiment if any readiness gate is red.

## 13. Prompt for the Workshop agent

Use this after the engineer marks the readiness gate green:

> Run the registered Craftax SFT uplift experiment end to end. Use the product-owned
> `sft.craftax.nemotron-nano.tinker.v1` recipe and an allowlisted student model. Before
> any paid call or environment mutation, create and verify the relevant Workshop visual
> and persist a disjoint seed manifest for baseline, teacher collection, checkpoint
> selection, and final heldout evaluation. Evaluate the unchanged student, collect
> bounded teacher rollouts with complete Trace V5 evidence, curate a diverse high-quality
> dataset with explicit accept/reject reasons, train multiple real Tinker checkpoints,
> evaluate every checkpoint through Containers on fresh selection seeds, and promote
> only from authoritative selection evidence. Then run the unchanged base model and the
> promoted checkpoint on the exact same untouched heldout seeds. Report score uplift,
> paired per-seed results, achievement coverage, reliability, usage, cost, dataset and
> trace digests, and links to every inspectable rollout. Preserve missing values as
> missing. Do not invent data, fixed-action policies, endpoints, costs, or successful
> scores. Stop safely and report a precise blocker if readiness, identity, budget, or
> evidence integrity cannot be established.

## 14. Acceptance receipt

Write one campaign directory containing:

```text
receipt.json
seed-manifest.json
baseline-summary.json
teacher-rollouts.json
curation-manifest.json
dataset-manifest.json
training-manifest.json
checkpoint-comparison.json
final-heldout-comparison.json
event-kind-counts.json
cursor-transcript.jsonl
cost-reconciliation.json
trace-index.json
visual-review.json
screenshots/
```

The receipt passes only if:

- every phase is visual-first and identity-consistent;
- teacher traces are sealed and curated with explicit provenance;
- the dataset is reproducible from its manifest and trace index;
- at least two real checkpoints exist and have fresh child evaluations;
- a promoted checkpoint runs new final-heldout rollouts;
- base and trained scores are paired on identical untouched seeds;
- all aggregates link to durable evidence;
- terminal replay works with compute stopped;
- receipt and process secret scans are clean.

Training success alone, a promoted checkpoint without final rollouts, fixture scores,
or a comparison on collection/selection seeds is a failure of this acceptance.

## 15. Status addendum — Workshop surface, 2026-08-13

Worked the §12 **Workshop** readiness line: *"SFT visuals project dataset/curation,
checkpoints, campaigns, children, promotion, and final comparison."* That gate was red.
Before this change the SFT workspace covered training, checkpoints, and campaigns only —
three of the six surfaces §11 requires. Baseline, curation, and the final paired
comparison had no projection and no panel, and `EvaluationSummaries` rendered the most
important numbers in the experiment as a bulleted list of `split · metric=score` strings.

### Landed (uncommitted, this worktree)

| File | Change |
|---|---|
| `visuals/templates/optimizer.run.v1/components/projectEvents.ts` | `SftState` gains `baseline`, `curation`, and `comparison`. New normalizers keep a missing reward as `null`. |
| `visuals/templates/optimizer.run.v1/overlays/sft/model.ts` | `sftComparison`, `sftDistribution`, `sftCurationFunnel`. Stage list grows to nine: baseline → collection → curation → dataset → training → checkpoints → campaigns → promotion → heldout. |
| `visuals/templates/optimizer.run.v1/overlays/sft/SftWorkspace.tsx` | Baseline, Curation, and Heldout-comparison panels. Existing panels moved off inline literals onto chrome classes. |
| `visuals/chrome/tokens.css` | Scale tokens (space/type/radius/tone) plus the shared panel, key-value, delta, bar, coverage, and outcome classes. |
| `visuals/tests/sft_workspace.test.mjs` | 9 new tests; suite is 12 SFT / 77 visuals, all green. |
| `docs/live_optimizers_gepa.md` | Event-catalog additions below, plus the payload shapes. |

### §10 report fields now rendered

Sample sizes and split digest; base/trained mean, median, sd and success rate; absolute
and relative change; **95% CI of the paired difference** (Student *t*, so small *n* is not
overstated); paired per-seed win/loss/tie with a proportion bar; achievement coverage
delta both directions; mean episode length change; and a per-seed matrix linking each row
to its rollout.

Statistics are computed over seeds where **both** arms returned an authoritative reward.
Seeds missing either side are counted and labelled `unpaired`, never imputed as `0` — a
regression test pins this, since zero-filling one missing seed in the fixture would move
the base mean from 1.00 to 0.75 and manufacture uplift.

### Two contract defects found

1. **Name drift.** The canonical catalog says `sft.heldout_evaluation.*`, but the
   projector only handled `sft.heldout_eval.*` — a producer emitting the canonical
   spelling was silently dropped. The projector now accepts both; producers must emit
   the canonical name.
2. **No events for the front half.** Phases A, B, C, and G had no vocabulary at all, so
   §11's baseline, collection, curation, and comparison surfaces were unbuildable. The
   additions are now in `live_optimizers_gepa.md`, shaped to match
   `scripts/run_craftax_sft_uplift.py` so the hosted producer can port that loop directly.

### Still red

- `optimizers-beta` emits none of the six new events yet. Until it does, those panels
  render an explicit "not emitted" state — which is the correct honest behaviour, but it
  is not a passing gate.
- The Containers, optimizers-beta/SFT, and Mandatory-dry-test sections of §12 are
  untouched by this change.
- **Nothing has been launched.** No paid compute, no teacher collection, no Tinker run.
  §12 still gates that on a green readiness review.

### 15.1 Dry test 1 — fixture collection → curation → dataset digest: GREEN

§12's first mandatory dry test now exists and passes, **46 checks, 0 failures**, with no
container, no Tinker, and no provider spend.

```
optimizers-beta-sft/
  scripts/lib/craftax_curation.py            # phases C and D
  acceptance/craftax_curation_dry_test.py    # the dry test
```

`run_craftax_sft_uplift.py` had no curator — its entire selection rule was *"keep turns
that produced a non-empty JSON actions plan."* The new module implements §6 hard
rejection, §6 ranking with diversity caps, and §7 materialization.

Covered: split roles disjoint and enforced at curation time; all twelve §6 hard rejections
(unsealed, no Trace V5 digest, missing reward receipt, invalid actions, heldout leak,
selection leak, unregistered seed, no observation, private reasoning in the target,
secret in the target, non-JSON target, empty action list); ranking over four weighted
components rather than terminal reward alone; per-seed and per-pattern caps; a stable
`dataset_digest` under input reordering; canonical-hash dedupe; refusal to materialize a
heldout identity; full §7 provenance; monotonic gap-free event sequence; secret scan.

**End-to-end integration verified.** The 17 emitted events were projected through
Workshop's `projectEvents.ts` and rendered by `SftWorkspace.tsx`: 15 considered → 3
accepted (20%), 12 distinct rejection reasons, 3 achievements covered, dataset digest
carried into the splits table. The baseline panel stays honestly empty and the comparison
panel refuses to claim uplift, because neither has run. Curator → events → projection →
visual is now one working chain.

Receipts (deterministic, reproduce byte-for-byte):

```
dataset digest        sha256:c3aba26e0d6aa463dff4703d9b05099b7d1094d2c59f772b58672792d3882f37
curation manifest     sha256:6dc16ae265543f0ac2940b4dee50cbdb944b4b72fceb5a62e824b04bcba0d711
split manifest        sha256:319d4014585a75da9815e80ea1eb630e4f34e2e959b0d72b5187912add7f9c30
```

### 15.2 Blocking defect: three divergent prompts — FIXED

The first read of this was "the training prompt has no observation." That was true
but understated. The uplift loop was asking **three different questions**:

| Path | User message | Built in |
|---|---|---|
| Teacher collection | objective + observation + `last_actions` + budgets + `valid_actions` | `craftax_gold.rs` |
| SFT training example | `seed=N reward_so_far=X\nExecute the next action batch.` | `collect_rows` |
| Evaluation | observation + `Plan 4-8 valid actions. Reply JSON only...` | `sample_actions` |

So the student was trained on a prompt containing **no game state**, then measured on a
differently-framed prompt it had never seen, against a teacher that answered a third
form. Nothing in that arrangement can produce uplift except by luck, and the mismatch is
invisible in every intermediate metric — training loss falls normally — until the final
comparison reports nothing and gets read as "SFT does not work here."

Root cause: the service **built** the observation into the teacher's prompt and then
**dropped it** from the returned turn record, which carried only `llm_call`, `assistant`,
and `actions`. The decision was not reconstructible, so `collect_rows` had nothing to
train on and substituted a placeholder.

**Fix, three parts:**

1. `gamebench/tasks/craftax-singleplayer/gold_rust/src/bin/craftax_gold.rs` — the turn
   record now carries `prompt`, `system_prompt`, `observation_text`, and `valid_actions`.
   `cargo check` clean.
2. `optimizers-beta-sft/scripts/lib/craftax_prompt.py` — one `build_decision_prompt`,
   mirroring the Rust format byte for byte.
3. `optimizers-beta-sft/scripts/run_craftax_sft_uplift.py` — `collect_rows` trains on the
   teacher's verbatim prompt and skips any turn that lacks one; `sample_actions` and
   `eval_policy` build the same prompt, threading real `valid_actions`, action history,
   and remaining budgets.

`acceptance/craftax_prompt_parity_test.py` pins it: **19 checks, 0 failures.** It holds a
golden string for the Python builder *and* reads the format literal out of the Rust
source, so drift on either side fails here rather than silently reopening the skew. It
also asserts the two dead prompt shapes are gone from the runner.

The documented no-cost readiness path still passes:

```
python3 scripts/run_craftax_sft_uplift.py --validate-only \
  --collect-seeds 101,102,103,104 --eval-seeds 501,502 \
  --train-steps 4 --batch-size 2 --rank 8 --lr 0.001
→ {"event": "recipe_validated", ...}
```

### 15.3 What this unblocks, and what it does not

Dry test 2 ("one tiny real teacher collection") is now runnable in principle: the curator
will accept turns from the fixed service instead of rejecting all of them. It has not
been run — it needs a live Craftax slot, and all five local slots were down.

Unverified end to end: the parity test compares the Python builder against the Rust
*source literal*, not against a live response. The first real collection should assert
`build_decision_prompt(...) == turn["prompt"]` on actual service output and promote that
into the parity test. Until then, treat prompt parity as pinned but not field-proven.

Still nothing launched: no teacher collection, no Tinker, no spend.

### 15.4 Each policy now sees what it needs

Unifying the prompt *format* (§15.2) was necessary but not sufficient. Three further
gaps in what each policy actually received:

**1. The student trained on the teacher's system prompt.** The example carried
`CHAMPION_PROMPT` — "collect food, eat cows, locate saplings, avoid enemies, prefer
batches of 4-8" — while evaluation served the bare `WEAK_EVAL_PROMPT`. The adapter was
being asked to transfer across a system message it never sees at inference, which
suppresses exactly the uplift the run is measuring. The weak eval prompt is a deliberate
design choice ("weight learning, not prompt text, must carry skill"); the fix honours it
properly. The teacher still gets the champion prompt to *elicit* a good demonstration,
but the example is now `{system: student prompt, user: same decision prompt, assistant:
teacher's action}` — identical question, better answer, which is what distillation is.

**2. The student was given a quarter of the teacher's output budget.** Teacher
`max_tokens=1024`; student `max_tokens=256`. A 4-8 action plan that runs long truncates
mid-JSON, the parse fails, and the fallback emits `do`. Both arms now get 1024, so
neither is budget-limited.

**3. Parse failures were laundered into policy decisions.** `sample_actions` returned
`["do"]` on any exception with no signal, so a model that never produced parseable output
logged a full run of deliberate-looking `do` actions. It now returns
`(actions, parse_failed)`; `eval_policy` counts failures per seed and per arm and emits
`parse_failures` and `invalid_action_rate` into the details, the arm summary, and the
headline next to uplift. §10's reliability requirement now has a real source.

**Checked and found sound:** `observation_text` itself. It carries level, position,
direction, front tile, local map, inventory, potions, learned spells, achievements,
nearby entities, and projectiles — and `is_default_inventory` explicitly exempts
`health|food|drink|energy|mana|xp`, so vitals are always present despite the
"anything not listed is 0" shorthand. No change needed there.

`acceptance/craftax_prompt_parity_test.py` is now **28 checks, 0 failures**, pinning the
system-prompt split, the token headroom, and the reliability telemetry alongside the
prompt format. Curation dry test 46/46, Workshop visuals 77/77, `--validate-only` green.

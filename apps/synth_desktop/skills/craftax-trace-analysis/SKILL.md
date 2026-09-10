---
name: craftax-trace-analysis
description: Analyse sealed Craftax / NanoHorizon policy rollouts — false beliefs, state grounding, plan/action mismatch, recovery after ineffective actions, milestone progress, and anchored execution quality — by selecting the Craftax annotation suite, milestone graph, and rubric, aggregating by policy arm and seed, and opening a Workshop visual from cached evidence bundles. Do not use for non-Craftax traces, and do not rerun rollouts.
---


# Craftax Trace Analysis

## What is authoritative

- Engine facts (reward, achievements, actions, inventory, termination) come from the sealed trace: `achievement_unlocked` events, `environment_step` spans, observation messages. They are never edited.
- Deterministic annotations (`craftax.belief_facts`, `craftax.plan_action_fidelity`, `craftax.recovery_facts`, `craftax.milestone_progress`) are free, reproducible, and cite exact selectors. Run them first.
- Model annotations (`craftax.belief`, `craftax.grounding`, `craftax.plan_action`, `craftax.progress`, `craftax.recovery`) are Codex app-server tasks with bounded paid compute. Use the smallest set that answers the question.
- Verifier judgments (`craftax.rubric_verifier` under `craftax.execution_quality`) score the rollout; they are not reward.
- Human review appends new revisions; it never rewrites.

Keep the observed / inferred / planned distinction when you write anything: a THOUGHT that says "the tree is in front" is *inferred* by the policy; `front_tile: grass` is *observed*; `ACTIONS: do, do` is *planned*; `do -> noop (nothing_to_do:grass)` is what the engine *applied*.

## Procedure

1. Select traces: `trace_manage list`; pick sealed GLM-5.3 Flash (or other arm) lanes. Never rerun a rollout.
2. Deterministic suite: `annotation_start` for each deterministic annotator; they complete in seconds and populate belief facts, plan/action labels, recovery labels, and the milestone lifecycle.
3. Only then decide which model annotator is needed (usually `craftax.belief` for belief errors the regex pass cannot read). One approval, one repeat unless you are measuring agreement.
4. Rubric: `trace-v5-verify` with `craftax.execution_quality`.
5. Aggregate by policy arm and seed (`suites/nonproduct/craftax#<model>:<effort>#s<seed>`). Compare paired seeds only; list unpaired seeds explicitly. Separate goal-mechanism, prompt-length, tool-ergonomics, seed, and infrastructure effects rather than attributing everything to the model.
6. Open the visual from the local evidence store (`visual_manage open` on the annotation overlay family); reloads read the cached sealed bundle by digest and must not trigger re-annotation.

## Presenting rationale

Show the retained `rationale` and the cited quotes. Never claim to have, or attempt to show, the annotator's hidden chain-of-thought; it is not captured.

## Report shape

- per arm: lanes, applied/abstained counts, labels per lane (belief.contradicted, plan_action.contradicted, recovery.failure_not_detected, milestone.verified ...)
- paired-seed deltas for the labels of interest
- rubric group aggregates (epistemic / execution / progress / survival) with the number of decisive vs non-decisive judgments
- exact ids: trace digests, annotator digests, rubric digest, job ids, bundle digests

Every `annotation_manage` call names the immutable `container_id` of the registered container that sealed the trace (from `container_list`); Workshop resolves its loopback URL from the registry and never accepts a URL from you.

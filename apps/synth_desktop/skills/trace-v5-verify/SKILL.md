---
name: trace-v5-verify
description: Run a declared rubric over annotated Trace V5 rollouts with verification_start and verification_get to seal VerifierResultV2 records with anchored, evidence-cited scores. Use when asked for qualitative execution quality, grounding, or progress scores. Do not use to discover beliefs or milestones — that stays on trace-v5-annotate; qualitative scores never modify environment reward.
---


# Trace V5 Verify

A rubric is a set of criteria with declared scales and anchors. A verifier applies it to one sealed trace and records one judgment per criterion. This is separate from an annotator taxonomy: annotators describe what happened; verifiers score it.

## Rules the agent must keep

- Qualitative scores are diagnostic. They are displayed beside environment reward and never merged into it or used as reward.
- Every decisive judgment cites resolvable Trace V5 selectors. Judgments without evidence become abstentions or are dropped, per the criterion's `allows_abstention`.
- Missing evidence means `abstained`, never a guessed score. `not_applicable` is only valid where the criterion allows it (for example context robustness when no compaction happened).
- Unmatched or incomplete traces (a lane that stopped on `driver_error`, a truncated capture) must not be compared as paired evidence. Verify them separately and label them as unpaired.
- `verified` milestones require engine evidence; a rubric score cannot promote a milestone.

## Procedure

1. Annotate first (`trace-v5-annotate`), so the verifier can cite belief / plan_action / recovery / milestone annotations as evidence.
2. `annotation_list_definitions` → pick the rubric (`rubric_id`, `digest`) and the verifier annotator (`craftax.rubric_verifier`, mode `verify`).
3. `annotation_estimate` with `mode: "verify"`; request approval once if `requires_reservation` and pass the host's `reservation_id`.
4. `verification_start(request, reservation_id?, session_id?)` enqueues (`accepted: true`); poll `verification_get(job_id)` until `terminal: true`.
5. Report `verifier_result_id`, `rubric_id` + `rubric_digest`, aggregate `score`/`passed` as recomputed by the rubric, and the per-criterion judgments with `status` (`decisive`, `abstained`, `not_applicable`). Report group aggregates (epistemic / execution / progress / survival) separately.

## Failure modes

- `rubric_required`: the annotator needs a rubric; pass `rubric_id`.
- `evidence_invalid`: the sealed result failed validation and nothing was persisted — inspect the job receipt's `findings` and rerun with a new `repeat_index` after fixing the request.
- A `cached` job means the identical rubric, program, model, and effort already produced a sealed result; reuse it.

Every `annotation_manage` call names the immutable `container_id` of the registered container that sealed the trace (from `container_list`); Workshop resolves its loopback URL from the registry and never accepts a URL from you.

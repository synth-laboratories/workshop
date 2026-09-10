---
name: use-human-annotations
description: Create, present, wait for, and export durable human review tasks in Workshop. Use when an agent needs structured judgment, a quiz, blinded preference, rubric scores, targeted critique, or audio commentary over a visual, file, image, video, rollout, trace, model call, or comparison.
---

# Use Human Annotations

Human judgments are durable evidence. They are not chat replies, visual overlay
labels, or machine-annotator findings. Use `human_annotation_manage` to create an
immutable assignment, show Workshop's native right-panel review, and read the
sealed result only after submission.

## Author the task

- Bind the subject and every necessary evidence item to an exact ID, revision,
  and SHA-256 digest. Snapshot mutable files or remote media before assignment.
- Include a bounded immutable `preview` when the reviewer must inspect content
  in place. Use line-addressable text for files, field-addressable objects for
  dataset rows, data URLs for captured images or videos, and event arrays with
  sequence, span, or call identities for traces. A path or URL alone is
  provenance, not reviewable evidence.
- Ask the shortest neutral question set that answers the research need. Prefer
  stable structured choices over redundant prose.
- Every question must name `decisionCriteria.evidence` (the exact visible
  object or field to inspect) and `decisionCriteria.answerRule` (the mechanical
  rule for choosing an answer). Yes/no questions also require user-visible
  `answerGuidance` for `yes`, `no`, and `not_enough_evidence` when enabled.
- Ask only for human judgment: preference, trust, usefulness, prioritization,
  or critique. Set `decisionCriteria.requiresHumanJudgment=true`. Never ask a
  human to confirm visible text, count fields, or perform another software check.
- Keep the interaction linear. Saving a response advances automatically. Use
  choice buttons, a minimal Next control for skipping ahead, and one final Submit.
- Be terse: prompt ≤140 characters, evidence ≤100, rule ≤120, and each
  answer hint ≤100. Prefer one short sentence per field; remove repeated facts.
  Creation rejects empty prompts such as "Is this good?" or "Give feedback."
  Qualitative terms are valid when the focus and choice rule define them.
- Make tie, neither, abstain, and not-enough-evidence explicit where legitimate.
  Missing or unavailable evidence is never score zero.
- Keep quiz keys, hidden gates, model identity, prior scores, and blinded fields
  out of the visible task. Quiz tasks name only a sealed backend key reference.
- Require contextual comments only when they improve adjudication or training
  value. A selector always binds to the evidence digest.

## Run the review

1. Call `human_annotation_preview` and inspect the digest, counts, randomized
   presentation, and warnings. Preview is read-only. Resolve material warnings
   before creating the immutable assignment.
2. Call `human_annotation_create` with the exact previewed task and a stable
   idempotency key.
   Reuse that task key with distinct reviewer IDs for independent assignments;
   Workshop retains one immutable task digest and separate session/result IDs.
3. Call `human_annotation_show` with the returned `sessionId`; it opens the
   product-owned Review document in the right panel.
4. Continue other useful work or call `human_annotation_wait`. Do not request or
   inspect private partial drafts.
5. After submission, the reviewer copies the visible `resultId` into chat. Call
   `human_annotation_get` with `{resultId}` to read the full immutable result and
   seal. Calls with `{taskId}` or `{sessionId}` remain private-safe status only.
   Export before handing evidence to another system.

The submitted screen must foreground the copyable result ID beside
`Exit & resume chat`. Treat the ID as the conversation-safe reference: use the
MCP record returned for that ID as authoritative, never filesystem archaeology.

The reviewer should select the smallest useful context before commenting:
quoted file lines, a dataset field, normalized image coordinates, a video time
range, a trace sequence/span/call range, or a semantic visual target. Workshop
validates that the selector matches the evidence kind and stores it with the
comment.

## Reconcile a campaign

- Read `campaign_status` to inspect current submitted results and per-question
  agreement. Superseded revisions remain visible as history but do not vote.
- If reviewers disagree, record an adjudication that names every source result,
  a structured decision, rationale, and adjudicator identity. Individual
  results remain sealed and are never overwritten by the aggregate decision.
- Close only after required adjudication. Workshop otherwise moves the campaign
  to `needs_adjudication` rather than silently resolving disagreement.
- Correct a sealed result with `supersede`. Review and submit the linked
  correction as a new result revision; the prior revision remains reopenable
  and exportable.

Audio is stored in Workshop CAS before it is called saved. The transcript is a
derived artifact produced by local Whisper when available and may be corrected
without replacing the original audio.
Never ask the reviewer to paste microphone data into chat or send it to a model.

For VisualsBench, use six anchored human criteria—task answerability,
information hierarchy, legibility, evidence access, interaction usefulness,
and trust calibration—plus task-specific truth questions. Keep machine
structural gates separate and retain individual results when aggregating.

For a clean-room rehearsal, use a fresh profile and isolated reviewer identity,
then prove preview, assignment, selectors, submission, reopen, export, and
campaign reconciliation without relying on prior draft state. Identity
isolation does not establish that a different human performed the review; state
that limitation explicitly in the receipt.

For implementation or task-authoring details, consult the Workshop-maintained
Human Annotation Workspace RFC and UI Style Guide in `docs/engineering/`.

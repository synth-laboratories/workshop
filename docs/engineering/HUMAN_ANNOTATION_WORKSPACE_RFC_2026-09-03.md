# RFC: Human annotation workspace

**Status:** proposed first-class Workshop subsystem
**First customer:** VisualsBench human reference scoring
**Surface:** native Workshop right panel
**Core rule:** a human judgment is durable evidence, not chat text and not an edit to the artifact being judged.

Normative interaction and authoring patterns live in
[`HUMAN_ANNOTATION_UI_STYLE_GUIDE.md`](./HUMAN_ANNOTATION_UI_STYLE_GUIDE.md).
The first VisualsBench authoring and QA lifecycle is specified in
[`VISUALSBENCH_WORKSHOP_TASK_LIFECYCLE.md`](./VISUALSBENCH_WORKSHOP_TASK_LIFECYCLE.md).

## Summary

Add a first-class human annotation workflow that an agent can prepare around any reviewable Workshop object: a visual revision, file or dataset row, image, video, rollout, trace span, model call, optimizer checkpoint, or comparison. The agent supplies the review packet and rubric; Workshop owns the review UI, input capture, durable storage, provenance, autosave, submission, export, and recovery.

The initial flow is VisualsBench:

1. An agent creates or selects the visual to grade.
2. The agent calls `human_annotation_create` with the exact visual revision, task instructions, rubric, and optional supporting evidence.
3. Workshop opens a native annotation workspace in the right panel.
4. The reviewer inspects the visual, assigns rubric scores, and records contextual text or audio comments pinned to exact targets.
5. Every edit is autosaved locally. Audio and transcript are both retained.
6. Submit seals an immutable review revision and returns a stable result reference.
7. VisualsBench consumes the sealed result as human reference evidence. Missing review remains unavailable, never zero.

This should later support dataset validation, file review, image critique, Craftax video commentary, per-call agent critique, preference pairs, and adjudication without inventing a separate product for each medium.

## What already exists

Workshop has useful pieces, but they do not yet form a human annotation product.

### Reuse

- `visual_annotations` already stores revision-bound selectors, text bodies, authors, supersession, tombstones, and timestamps.
- `VisualPane` already supports click-to-label for candidate, trial, trace-span, and normalized chart positions.
- `visualsbench_export` exports visual identity, revisions, labels, overlay digest, journal history, and screenshot-backed reviews.
- `annotation_campaigns`, `annotation_jobs`, `annotation_findings`, `rubric_results`, and `annotation_reviews` model agent/container-produced trace annotations and review decisions.
- The composer already records microphone audio and can run local Whisper transcription.
- Visuals already open natively in the right panel and persist independently of the producer.

### Do not conflate

- `visual_annotations` are lightweight overlay labels, not a complete task response or scored rubric.
- `annotation_campaigns` are machine-annotator orchestration and container evidence projections. A human review must not masquerade as a paid annotator job.
- `annotation_reviews` currently reviews an existing machine finding. It is not a general human review session.
- Composer voice input transcribes a temporary file and deletes the audio. That is correct for chat dictation and wrong for auditable annotation.

## Product model

Use three durable nouns.

### Annotation task

An immutable review assignment authored by an agent or program.

- Human-readable title and instructions
- Exact subject references and digests
- Ordered evidence items
- Rubric definition and digest
- Response requirements
- Ordered structured questionnaire and branching rules
- Reviewer eligibility and blind/reveal policy
- Dataset split and leakage policy
- Optional deadline and campaign membership

### Annotation session

A reviewer's resumable working state.

- Stable task and reviewer identity
- Current item, playback position, selected target, draft scores, and draft comments
- Autosave revision and recovery state
- Started, last-active, and submitted timestamps
- No final score authority

### Annotation result

An append-only submitted revision.

- Criterion scores and rationales
- Overall score, confidence, flags, and preference choice when applicable
- Text and audio comments with selectors
- Exact subject/evidence/rubric digests
- Reviewer identity or pseudonymous assignment identity
- Timing and interaction receipt
- Supersedes relation for corrections
- Seal digest and export status

A result can be `draft`, `submitted`, `superseded`, `withdrawn`, or `adjudicated`. Submitted content is never updated in place.

## Subject and evidence contract

The task is media-agnostic. Every item is a typed, digest-bound reference.

```json
{
  "schemaVersion": "synth.human-annotation-task.v1",
  "taskId": "hat_visualsbench_...",
  "campaignId": "hac_visualsbench_reference_v1",
  "title": "Score this live evaluation visual",
  "instructions": "Judge the visual itself, not whether you like the underlying model result.",
  "subject": {
    "kind": "visual_revision",
    "id": "vis_...",
    "revision": 7,
    "digest": "sha256:..."
  },
  "evidence": [
    {"kind": "visual", "id": "vis_...", "revision": 7, "role": "primary"},
    {"kind": "file", "path": "task/instruction.md", "digest": "sha256:...", "role": "task"},
    {"kind": "image", "artifactId": "art_...", "digest": "sha256:...", "role": "reference"},
    {"kind": "trace_v5", "id": "tracev5_...", "digest": "sha256:...", "role": "source"}
  ],
  "rubric": {
    "id": "visualsbench.human_reference.v1",
    "digest": "sha256:...",
    "criteria": []
  },
  "response": {
    "overallScore": {"required": true, "scale": {"min": 1, "max": 5}},
    "criterionRationale": "required_on_extremes",
    "confidence": "required",
    "allowText": true,
    "allowAudio": true,
    "allowTargetedComments": true
  },
  "questions": [
    {
      "id": "decision_supported",
      "type": "yes_no",
      "prompt": "Does the evidence shown support the headline decision?",
      "required": true,
      "allowAbstain": true,
      "rationale": "required_on_no_or_abstain"
    },
    {
      "id": "preferred_visual",
      "type": "preference",
      "prompt": "Which visual better supports the task?",
      "options": [
        {"id": "a", "subjectRef": "visual_a"},
        {"id": "b", "subjectRef": "visual_b"}
      ],
      "allowTie": true,
      "randomize": true,
      "required": true
    }
  ],
  "presentation": {
    "mode": "right_panel",
    "blindFields": ["model", "agent", "cost"],
    "startAt": {"kind": "overview"}
  }
}
```

Supported subject/evidence kinds should be a closed registry: `visual_revision`, `file_snapshot`, `dataset_row`, `image`, `video`, `rollout`, `trace_v5`, `trace_span`, `model_call`, `optimizer_checkpoint`, and `comparison`. New kinds need both a resolver and a trusted renderer.

No arbitrary path or URL rendering. File references are snapshotted into Workshop CAS at task creation, with source path retained only as provenance. Remote media must first become a registered artifact.

## Structured questions and quizzes

Questionnaires are first-class task content. Rubrics are one specialized scored questionnaire, not the only response shape.

### Question types

- `yes_no`: yes, no, and optional abstain/not enough evidence.
- `single_select`: exactly one stable option ID.
- `multi_select`: bounded minimum and maximum selections.
- `preference`: A/B or N-way preference with optional tie, neither, and abstain.
- `ranking`: total or partial ordering of stable option IDs.
- `likert`: labeled ordered scale such as strongly disagree through strongly agree.
- `numeric`: integer or decimal range with units and explicit bounds.
- `rubric_score`: criterion score with anchors, weight, and rationale policy.
- `short_text` and `long_text`: bounded text responses.
- `quiz_single`, `quiz_multi`, and `quiz_order`: objectively graded questions with sealed answer keys.
- `target_selection`: one or more required selectors on the evidence.
- `media_response`: audio, image markup, or another explicitly allowed response artifact.
- `composite`: a small atomic group, such as decision + confidence + rationale.

Every option has a stable machine ID and a separate display label. Results store IDs, never only display text. `Other` is explicit in the task schema and, when enabled, requires a companion text field.

### Shared question contract

```json
{
  "id": "uplift_boundary_visible",
  "type": "yes_no",
  "prompt": "Before opening any disclosure, does the header state that the runs use different seeds and horizons and therefore do not establish uplift?",
  "decisionCriteria": {
    "evidence": "Inspect only the initial comparison header.",
    "answerRule": "Answer yes only when different seeds, different horizons, and the non-uplift limitation are all visible."
  },
  "answerGuidance": {
    "yes": "All three required facts are visible in the initial header.",
    "no": "One or more required facts are absent from the initial header.",
    "not_enough_evidence": "The initial header is missing, cropped, or unreadable."
  },
  "required": true,
  "allowAbstain": true,
  "randomizeOptions": false,
  "validation": {},
  "rationale": {
    "mode": "required_on",
    "answers": ["no", "abstain"],
    "allowText": true,
    "allowAudio": true,
    "requireTarget": false
  },
  "visibility": {
    "when": {"questionId": "visual_rendered", "operator": "equals", "value": "yes"}
  },
  "scoring": {
    "mode": "reference",
    "answerKeyRef": "sealed:answer-key-item-17",
    "points": 1
  }
}
```

### Branching

Support declarative visibility and skip logic over earlier answers only. Conditions use stable question/option IDs and a small closed operator set: `equals`, `not_equals`, `contains`, `contains_any`, `answered`, and `not_answered`. Reject cycles at task creation. Hidden questions are recorded as `not_presented`, not unanswered or incorrect.

Branching must never reveal a sealed quiz answer or a blinded field. Presentation order and branch decisions are captured in the session receipt so the exact questionnaire can be replayed.

### Preference studies

Preference questions bind each option to a digest-backed subject or evidence reference. Workshop randomizes display order per reviewer, records the permutation, and stores the canonical option ID in the result. Supported outcomes are explicit: preference for one option, tie, neither acceptable, or insufficient evidence.

For pairwise VisualsBench comparison:

- hide model/agent identity and prior scores;
- use the same viewport and task context for both options;
- synchronize logical time when comparing live or replayed artifacts;
- require a targeted rationale for strong preferences when configured;
- retain order effects for later bias analysis;
- never convert `tie`, `neither`, or `abstain` into a loss for one arm.

### Quizzes

Quiz answer keys live in a sealed evaluator-owned object and are never sent to the renderer or agent-facing task payload. The renderer receives only the question and allowed response shape. Submission is graded by the trusted backend after the response is sealed.

Quiz results distinguish:

- `correct` / `incorrect` for a valid submitted answer;
- `partial` only when the scoring rule explicitly supports partial credit;
- `abstained` when allowed;
- `not_presented` due to branching;
- `unanswered` for an invalid incomplete submission;
- `unavailable` when the answer key or grader is missing;
- `invalidated` when integrity or leakage checks fail.

Never represent unavailable grading as zero. Store the submitted answer, score receipt, answer-key digest, grader version, and per-item outcome. Whether the correct answer becomes visible after submission is a task policy (`never`, `after_submit`, or `after_campaign_close`).

### Validation and navigation

- Validate locally for immediate feedback and authoritatively in Rust on save/submit.
- Required questions, selection bounds, numeric bounds, text limits, and rationale rules are schema data.
- A reviewer can navigate backward without losing answers.
- The task progress indicator counts presented required questions, not hidden branches.
- Submission errors link directly to the first incomplete question.
- Keyboard controls use native radio groups, checkboxes, selects, text inputs, and buttons.
- Autosave patches one answer at a time with the expected draft revision.

### Stored answer shape

```json
{
  "questionId": "preferred_visual",
  "questionRevision": 1,
  "presentedAt": "2026-09-03T14:00:00Z",
  "answeredAt": "2026-09-03T14:00:18Z",
  "presentation": {"optionOrder": ["b", "a"]},
  "value": {"kind": "preference", "optionId": "b"},
  "confidence": "medium",
  "rationale": {
    "text": "The evidence drill-down stays in context.",
    "audioArtifactId": null,
    "selectors": [{"evidenceDigest": "sha256:...", "selector": {}}]
  }
}
```

Question definitions are immutable within a task. Correcting wording produces a new task/questionnaire digest; it does not reinterpret already submitted responses.

## Native right-panel experience

The annotation workspace replaces the ordinary visual chrome while active; it does not stack a form below an already dense visual.

### Header

- `Review 3 of 20`
- Autosave state: `Saved`, `Saving`, or actionable error
- Exit-and-resume action
- Submit is disabled until required fields are satisfied

### Evidence stage

- Primary evidence gets most of the area.
- A compact evidence switcher exposes task, reference, trace, file, image, or video.
- Visuals render through the existing trusted visual host.
- Files use the native file viewer at a bound snapshot.
- Images use fit/actual-size controls.
- Video and rollout replay share play/pause, speed, scrubber, and logical time.
- Agent calls and annotations align to the same logical cursor when available.

### Review rail

- One compact rubric summary, not every criterion expanded.
- Current criterion expands in place; completed criteria collapse to score plus one-line rationale.
- Overall judgment follows criteria.
- Structured questions render one active question at a time using native controls; completed answers collapse to a one-line summary.
- Quiz correctness is not shown until the configured reveal phase.
- Preference options use matched evidence panes or a synchronized A/B switcher, never tiny thumbnails when details matter.
- `Add comment` enters target-selection mode on the current evidence.
- `Hold to talk` records a contextual comment. Stopping creates an audio attachment and a draft transcript; the reviewer can correct the transcript without altering the original audio.

### Targeted comments

Selectors are typed and portable:

- visual semantic target (`data-annotation-kind` + ID) with coordinate fallback
- text quote plus start/end and source digest
- image normalized box or point
- video/rollout time range plus optional spatial box
- trace sequence range, span ID, rollout ID, or model-call ID
- dataset row and field path

The selector always binds to the evidence item's digest. If the evidence changes, Workshop shows the comment as referring to an older revision; it never silently retargets.

## Audio is source evidence

Do not route annotation audio through the composer's delete-after-transcribe path.

On stop recording:

1. Stream or atomically write the encoded audio into Workshop CAS.
2. Record media type, byte length, duration, SHA-256, capture device class, and timestamps.
3. Add an attachment row to the draft before transcription begins.
4. Transcribe locally with the configured Whisper model when available.
5. Save transcript, segments, language, model identity, and transcript digest as a derived artifact.
6. Let the reviewer edit a `correctedText` field while preserving `machineText`.
7. Autosave the comment selector and attachment relation transactionally.

If transcription fails, the audio comment remains valid and resumable. If audio persistence fails, the UI must say so and must not discard the recording or claim it is saved.

## Storage

Add a migration with normalized durable tables. Large media stays in CAS; SQLite owns identities and relations.

```sql
CREATE TABLE human_annotation_campaigns (
  campaign_id TEXT PRIMARY KEY,
  schema_version TEXT NOT NULL,
  name TEXT NOT NULL,
  benchmark_family TEXT,
  dataset_split TEXT,
  task_count INTEGER NOT NULL,
  policy_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  closed_at TEXT
);

CREATE TABLE human_annotation_tasks (
  task_id TEXT PRIMARY KEY,
  campaign_id TEXT REFERENCES human_annotation_campaigns(campaign_id),
  schema_version TEXT NOT NULL,
  task_digest TEXT NOT NULL UNIQUE,
  subject_kind TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  subject_revision INTEGER,
  subject_digest TEXT NOT NULL,
  rubric_id TEXT NOT NULL,
  rubric_digest TEXT NOT NULL,
  task_json TEXT NOT NULL,
  state TEXT NOT NULL,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE human_annotation_sessions (
  session_id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES human_annotation_tasks(task_id),
  reviewer_id TEXT NOT NULL,
  state TEXT NOT NULL,
  draft_revision INTEGER NOT NULL DEFAULT 0,
  draft_json TEXT NOT NULL DEFAULT '{}',
  started_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  submitted_at TEXT
);

CREATE TABLE human_annotation_results (
  result_id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES human_annotation_tasks(task_id),
  session_id TEXT NOT NULL REFERENCES human_annotation_sessions(session_id),
  result_revision INTEGER NOT NULL,
  result_digest TEXT NOT NULL UNIQUE,
  state TEXT NOT NULL,
  result_json TEXT NOT NULL,
  supersedes_id TEXT REFERENCES human_annotation_results(result_id),
  created_at TEXT NOT NULL,
  UNIQUE(session_id, result_revision)
);

CREATE TABLE human_annotation_answers (
  answer_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES human_annotation_sessions(session_id),
  result_id TEXT REFERENCES human_annotation_results(result_id),
  question_id TEXT NOT NULL,
  question_revision INTEGER NOT NULL,
  state TEXT NOT NULL,
  answer_json TEXT NOT NULL,
  presented_at TEXT,
  answered_at TEXT,
  supersedes_id TEXT REFERENCES human_annotation_answers(answer_id),
  created_at TEXT NOT NULL,
  UNIQUE(session_id, question_id, question_revision, answer_id)
);

CREATE TABLE human_annotation_comments (
  comment_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES human_annotation_sessions(session_id),
  result_id TEXT REFERENCES human_annotation_results(result_id),
  evidence_digest TEXT NOT NULL,
  selector_json TEXT NOT NULL,
  body_text TEXT,
  audio_artifact_id TEXT,
  transcript_artifact_id TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  supersedes_id TEXT REFERENCES human_annotation_comments(comment_id),
  tombstoned INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

Every draft mutation also appends a journal event. SQLite gives immediate recovery; CAS gives durable media; a portable sealed bundle protects against database loss and supports benchmark export.

## Tool API

Expose a dedicated MCP surface, not overload machine `annotation_manage`.

### Agent-facing tools

- `human_annotation_create(task, idempotency_key)` validates and persists the task, returns task/session IDs.
- `human_annotation_show(task_id, session_id?, foreground_owner?)` opens the native right-panel workspace.
- `human_annotation_get(task_id|session_id)` returns status and stable result refs, not private draft content.
- `human_annotation_get(result_id)` returns the full immutable result and seal only after submission. The completion UI exposes this copyable ID for chat handoff.
- `human_annotation_wait(task_id, after_revision?, timeout_ms?)` waits for submit, clarification request, or cancellation.
- `human_annotation_list(campaign_id?, state?, limit?)` finds assignments and results.
- `human_annotation_export(result_id, format)` creates a portable JSON/JSONL bundle.
- `human_annotation_cancel(task_id, reason)` cancels only unsubmitted work.

The create tool may include `blocking: false`; agent turns should not sit open indefinitely. A submitted event can wake or notify the originating task. Agents may read sealed results only after submission unless the reviewer explicitly sends a clarification.

### Renderer bridge

Use narrow commands for draft mutations:

- `human_annotation_session_open`
- `human_annotation_draft_patch(expected_revision, patch)`
- `human_annotation_answer_set(expected_revision, question_id, answer)`
- `human_annotation_answer_clear(expected_revision, question_id)`
- `human_annotation_audio_begin/append/finish`
- `human_annotation_comment_create/supersede`
- `human_annotation_submit(expected_revision)`
- `human_annotation_resume`

Optimistic revisions prevent two windows from overwriting each other. Submission validates required criteria and atomically writes the result, seal, and terminal event.

## VisualsBench v1 human reference protocol

Start with a small, explicit rubric. Keep structural checks machine-scored and reserve human effort for perceptual and decision-quality judgments.

### Machine gates

- stable visual identity
- bound before mutation
- missing remains missing
- export completeness and digest integrity
- required wide/right-panel captures exist
- no runtime/render failure

### Human criteria, 1–5

1. **Task answerability:** can the reviewer answer the task from the visual without reading agent chat?
2. **Information hierarchy:** does the surface foreground the outcome and next useful action?
3. **Legibility:** is important text and encoding readable at the target viewport?
4. **Evidence access:** can a reviewer inspect the evidence behind claims without losing context?
5. **Interaction usefulness:** do filtering, selection, replay, and drill-down help rather than decorate?
6. **Trust calibration:** are uncertainty, missingness, provisional state, and provenance represented honestly?

Require a short rationale for scores 1, 2, or 5. Require at least one targeted comment. Allow audio to satisfy the rationale requirement once a transcript is available or explicitly marked transcription unavailable.

The reference record contains individual criterion scores, not only a mean. Retain reviewer-level rows for agreement analysis. Aggregate only after minimum reviewer count and adjudication policy are satisfied.

### Blindness

Default VisualsBench human review hides agent/model identity, cost, and prior machine score until submission. The task instruction and original user goal stay visible. For pairwise comparisons, randomize A/B ordering per reviewer and persist the randomization receipt.

### Output

```json
{
  "schemaVersion": "visualsbench.human-reference.v1",
  "taskId": "hat_...",
  "resultId": "har_...",
  "visual": {"id": "vis_...", "revision": 7, "digest": "sha256:..."},
  "rubric": {"id": "visualsbench.human_reference.v1", "digest": "sha256:..."},
  "criteria": [{"id": "hierarchy", "score": 4, "rationale": "..."}],
  "overallScore": 4,
  "confidence": "high",
  "comments": [{"selector": {}, "text": "...", "audioArtifactId": "art_..."}],
  "reviewer": {"assignmentId": "reviewer_slot_2"},
  "timing": {"activeMs": 184000, "startedAt": "...", "submittedAt": "..."},
  "sealDigest": "sha256:..."
}
```

VisualsBench reward stays null when required human reference is missing or the result seal is invalid. A low human score is a valid score, not an infrastructure failure.

## Recovery and safety

- Autosave after each score/comment change and periodically while recording.
- Store audio incrementally so a crash loses at most the current chunk.
- On restart, reopen the exact task, evidence item, selector, playback time, and draft revision.
- Never delete submitted results; corrections supersede them.
- Provide explicit export to task-owned filesystem (`json`, `jsonl`, and media bundle) in addition to SQLite/CAS.
- Hash every subject, evidence item, rubric, audio object, transcript, result, and export manifest.
- Mark PII/sensitive review campaigns and keep exports local unless explicitly shared.
- Do not send audio to a provider by default. Local Whisper is the default transcription path.

## Architecture boundary

```text
Agent/program
  creates immutable task + evidence refs + rubric
        |
        v
HumanAnnotationService (Rust domain service)
  task/session/result invariants, CAS, SQLite, journal, export
        |
        +--> native AnnotationWorkspace in right panel
        |      evidence resolver + trusted viewers
        |      rubric form + selectors + audio recorder
        |
        +--> sealed result reference
                   |
                   +--> VisualsBench verifier/reference set
                   +--> datasets / SFT preference data
                   +--> agent follow-up context
```

The renderer does not write arbitrary files or own evidence authority. The agent does not receive microphone bytes. Visual templates may expose semantic targets but cannot intercept or forge submission.

## Implementation sequence

### Phase 0: contract tests

- Task/result JSON schemas and digest fixtures
- Question registry, answer union, branching-cycle, randomization, and sealed-answer-key tests
- Selector validation for every initial evidence type
- Crash/reopen and optimistic-concurrency tests
- Missing audio transcript and failed-CAS tests
- VisualsBench null-vs-zero tests

### Phase 1: VisualsBench vertical slice

- New storage migration and Rust `HumanAnnotationService`
- MCP create/show/get/wait/export tools
- Right-panel workspace with visual revision subject only
- Yes/no, preference, quiz, 1–5 rubric, text comments, confidence, autosave, submit, durable reopen
- Existing visual click selectors reused through a shared selector adapter
- Sealed JSON/JSONL export consumed by VisualsBench

### Phase 2: durable audio

- CAS-backed recorder distinct from composer dictation
- Local Whisper transcript as derived artifact
- Transcript correction and segment/time selectors
- Playback inside comments

### Phase 3: mixed evidence

- file snapshot, dataset row, image, and video viewers
- Trace V5, rollout replay, model-call, and logical-time selectors
- evidence switcher and synchronized cursor

### Phase 4: campaigns and adjudication

- queues, assignment, blinded A/B, reviewer agreement, adjudication
- dataset export and provenance manifests
- progress visual for campaign operators

## Acceptance for the first release

1. An agent can create a VisualsBench review task for an exact visual revision and open it in the right panel.
2. The reviewer can score all criteria, pin a comment to a semantic visual target, record audio, edit its transcript, and submit.
3. Killing and reopening Workshop loses no completed score, saved audio, transcript, target, or draft text.
4. The submitted result is immutable, digest-bound, exportable, and recoverable without the visual producer running.
5. The originating agent can wait for and read the sealed result, then revise the same visual ID.
6. VisualsBench can distinguish missing review, invalid review infrastructure, and a valid low score.
7. A second reviewer can score the same visual blindly; individual results and aggregate agreement remain inspectable.
8. Right-panel and expanded layouts pass keyboard, screen-reader, clipping, and microphone-failure tests.
9. Yes/no, preference, ranking, multi-select, and quiz answers round-trip using stable IDs; ties, abstentions, hidden branches, and unavailable grades remain distinct.
10. Quiz answer keys never enter renderer state, visual bindings, agent-readable task payloads, or exported reviewer workspaces.

## Decisions

- Name the subsystem **Human annotations**; reserve **Review** for the user action and **VisualsBench reference** for the first campaign type.
- Keep human annotation storage separate from machine annotation jobs, but use shared evidence selectors and export adapters.
- Persist audio before transcription.
- Bind tasks to immutable revisions and digests.
- Make autosave and durable reopen release gates, not polish.
- Keep the initial UI task-focused: one evidence stage, one review rail, one active criterion, and contextual comments.

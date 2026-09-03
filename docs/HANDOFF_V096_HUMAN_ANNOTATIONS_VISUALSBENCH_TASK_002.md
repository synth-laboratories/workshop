# Handoff: Workshop v0.9.6 human annotations + VisualsBench task 002

**For:** engineer implementing and proving the v0.9.6 human-annotation workflow
**Date:** 2026-09-03
**Primary repository:** `/Users/joshuapurtell/GitHub/workshop`
**Release repository:** `/Users/joshuapurtell/GitHub/workshop-release`
**Target release:** Workshop `v0.9.6`
**Proof method:** native Workshop plus Computer Use
**Second deliverable:** author and freeze VisualsBench task instance 002, then score ten human references through the new interface

## Mission

Ship human annotation as a first-class Workshop subsystem, then dogfood it to create the second VisualsBench task instance entirely through Workshop.

An agent must be able to prepare a review packet containing a task, visual, file, dataset row, image, video, rollout, trace, model call, or comparison; ask a human structured questions; accept contextual text and durable audio comments; autosave safely; seal the submitted result; and export it for evaluation or training.

The end-to-end product proof is not complete when the components compile. Use Computer Use to:

1. create VisualsBench task instance 002 from pinned REB and RuneBench evidence;
2. run the authoring agent in Workshop;
3. inspect and iterate the agent-authored visual;
4. create a human-reference campaign;
5. complete **ten reference reviews** through the native right-panel interface;
6. include structured scores, truth questions, and contextual critique;
7. record audio on a representative subset and retain both audio and transcript;
8. prove crash/reopen, submission, export, aggregation, and clean-room task execution;
9. leave the final task, campaign, aggregate, and representative review open in Workshop;
10. write a screenshot-backed QA report and issue catalogue.

Do not bypass the interface by inserting rows directly into SQLite or generating reference JSON by script. Automation may set up inputs and inspect receipts, but the ten review submissions must traverse the same native commands, validation, autosave, and submit path a real reviewer uses.

## Read first

These three documents are the product contract:

1. [`engineering/HUMAN_ANNOTATION_WORKSPACE_RFC_2026-09-03.md`](./engineering/HUMAN_ANNOTATION_WORKSPACE_RFC_2026-09-03.md)
2. [`engineering/HUMAN_ANNOTATION_UI_STYLE_GUIDE.md`](./engineering/HUMAN_ANNOTATION_UI_STYLE_GUIDE.md)
3. [`engineering/VISUALSBENCH_WORKSHOP_TASK_LIFECYCLE.md`](./engineering/VISUALSBENCH_WORKSHOP_TASK_LIFECYCLE.md)

Also read:

- [`HANDOFF_V03_VISUALSBENCH_HARBOR_CODEX_2026-08-14.md`](./HANDOFF_V03_VISUALSBENCH_HARBOR_CODEX_2026-08-14.md)
- [`launch/v0.3-themes.md`](./launch/v0.3-themes.md), E1
- `/Users/joshuapurtell/GitHub/workshop-release/specs/end-to-end-quality-portfolio.md`, RuneBench section
- `/Users/joshuapurtell/GitHub/evals/temp/runebench-harbor-codex/README_WORKSHOP.md`

If implementation choices conflict with these documents, update the RFC explicitly and explain the invariant being changed. Do not silently improvise a second contract.

## Repository and data starting points

Resolve every path and commit again at the start. These were the inspected local candidates on 2026-09-03:

### Workshop

- Repo: `/Users/joshuapurtell/GitHub/workshop`
- Working tree is dirty with other work. Preserve unrelated edits and inspect overlaps before modifying files.
- Implement on a `codex/` branch for v0.9.6 unless an explicit release branch already exists.

### Workshop release/QA

- Repo: `/Users/joshuapurtell/GitHub/workshop-release`
- Add release manifests, CUA scenarios, screenshot manifests, receipts, and the final QA report here.

### ResearchEngineeringBench

- Candidate repo: `/Users/joshuapurtell/GitHub/ResearchEngineeringBench`
- Inspected commit: `b78c493866332f1ed2d044f082ca8fca52e342f7`
- Remote: `git@github.com:synth-laboratories/ResearchEngineeringBench.git`
- The checkout was dirty. Pin an exact source task path and commit; never treat untracked workspace material as hidden-test authority.

### RuneBench

- Workshop facade: `/Users/joshuapurtell/GitHub/evals/temp/runebench-harbor-codex`
- Inspected commit: `08994e6399fdc34a316ed7d7592508a17045ea20`
- Remote: `https://github.com/MaxBittker/runebench.git`
- Existing release evidence:
  - `/Users/joshuapurtell/GitHub/workshop-release/releases/v0.8.0/runebench-smoke-live-receipt.json`
  - `/Users/joshuapurtell/GitHub/workshop-release/releases/v0.8.0/runebench-long-live-receipt.json`
  - `/Users/joshuapurtell/GitHub/workshop-release/releases/v0.8.0/EVAL_REPORT.md`
- Existing successful runs:
  - `opt_eval_runebench_b6db3a876917` — reported 62 XP/min
  - `opt_eval_runebench_4dc98c6ad988` — reported 200 XP/min

Before using those as a comparison, verify policy, task, immutable save, duration, bounds, trace completeness, and score semantics. RuneBench IDs 780040–780044 are repeats over the same immutable save, not randomized world seeds. If the existing runs are not experimentally comparable, generate a paired source set first and record the new spend ceiling once.

## Product boundary

Keep these authorities separate:

| Object | Authority |
| --- | --- |
| Research task and split | REB repository |
| RuneScape environment and XP evidence | RuneBench |
| Run events, frames, media, and trace | container + Trace V5 |
| Visual identity and revisions | Workshop visual registry |
| Human task, draft, audio, answer, and result | new Workshop `HumanAnnotationService` |
| Visual-quality rubric and aggregation | VisualsBench |
| Isolated execution | Harbor |
| Release/CUA evidence | `workshop-release` |

Do not overload container machine-annotation jobs with human work. Reuse selectors and evidence conventions, but keep human tables, service, commands, events, and result schemas explicit.

## Part I: implement the v0.9.6 subsystem

### 1. Domain model and schemas

Implement versioned schemas for:

- `synth.human-annotation-campaign.v1`
- `synth.human-annotation-task.v1`
- `synth.human-annotation-session.v1`
- `synth.human-annotation-answer.v1`
- `synth.human-annotation-comment.v1`
- `synth.human-annotation-result.v1`
- `synth.human-annotation-result-seal.v1`
- `synth.human-annotation-export.v1`

The task binds exact subject/evidence/rubric revisions and digests. The session is resumable draft state. The result is an immutable submitted revision. Corrections supersede; they do not edit history.

Support these question types in v0.9.6:

- yes/no with optional `not_enough_evidence`;
- single-select;
- multi-select with min/max;
- A/B and N-way preference with tie, neither, and abstain;
- total or partial ranking;
- Likert;
- bounded numeric;
- anchored rubric score;
- short and long text;
- quiz single, multi, and ordered;
- target selection;
- media response;
- composite decision + confidence + rationale.

Use stable question and option IDs. Hidden branches are `not_presented`; they are not unanswered or incorrect. Missing answer keys or graders produce unavailable results, not zero.

### 2. Storage and durability

Add a migration with normalized tables for campaigns, tasks, sessions, answers, results, comments, attachments, and result seals. Large audio/media lives in Workshop CAS; SQLite owns relations and current state.

Required invariants:

- task and questionnaire definitions are immutable after assignment;
- every draft patch uses optimistic revision checks;
- autosave is transactional;
- submission atomically writes result, seal, terminal event, and session state;
- original audio is persisted before transcription starts;
- machine transcript and reviewer-corrected transcript remain separate;
- submitted records are append-only;
- exact subject/evidence/rubric digests appear in the result;
- database recovery and portable export both work;
- no human work exists only in renderer memory or chat.

Add indexes for campaign/state, task/state, reviewer/task, result/task, question/session, evidence digest, and selector target.

### 3. Rust service

Add a domain service such as:

```text
apps/synth_desktop/src-tauri/src/human_annotations/
  mod.rs
  models.rs
  validation.rs
  selectors.rs
  storage.rs
  audio.rs
  export.rs
  service.rs
```

The service owns:

- task/campaign validation;
- evidence resolution and CAS snapshots;
- question branching and validation;
- blinded presentation and option randomization receipts;
- draft revisioning;
- audio finalization;
- submission and seals;
- quiz grading against backend-only sealed keys;
- result export;
- aggregate/reference-set computation;
- durable journal events.

Do not put domain invariants in React event handlers.

### 4. Agent-facing MCP

Add a dedicated MCP binary or first-class tool group with:

- `human_annotation_create`
- `human_annotation_show`
- `human_annotation_get`
- `human_annotation_wait`
- `human_annotation_list`
- `human_annotation_export`
- `human_annotation_cancel`

Also provide campaign operations:

- `human_annotation_campaign_create`
- `human_annotation_campaign_add_task`
- `human_annotation_campaign_status`
- `human_annotation_campaign_aggregate`
- `human_annotation_campaign_close`

`create` is idempotent. `show` opens the native right panel. `wait` wakes on submitted, clarification-needed, cancelled, or invalidated. Agent reads submitted results, not private partial drafts by default.

Tool descriptions should teach the correct flow. Add a dedicated skill for authoring human annotation tasks and VisualsBench reference campaigns.

### 5. Renderer bridge

Expose narrow commands:

- session open/resume;
- task/evidence resolve;
- draft answer set/clear;
- contextual comment create/supersede;
- audio begin/append/finish;
- transcript correction;
- submit with expected draft revision;
- result reopen;
- campaign queue and aggregate reads.

Generate typed bindings through the existing Specta path. Do not hand-maintain parallel types.

### 6. Native right-panel UI

Build `HumanAnnotationWorkspace` as a product-owned surface, not an agent-supplied template.

Use the normative style guide. Default anatomy:

```text
Review header: task · progress · save state · exit
Evidence stage: trusted viewer + compact evidence switcher
Active question: one question and response at a time
Contextual comment: text/audio bound to exact target
Navigation: previous · save & next · review answers · submit
```

Evidence adapters required for the first proof:

- visual revision;
- file snapshot;
- image;
- video;
- RuneBench rollout replay;
- Trace V5 sequence/span/model call;
- A/B visual comparison.

The evidence and question hierarchy must work in a normal right panel first. Expanded view adds evidence space; it must not be required to finish a review.

### 7. Durable audio

Do not reuse composer dictation's temporary-delete behavior.

Required lifecycle:

1. record;
2. persist chunks/final audio to CAS;
3. write attachment relation;
4. show `Audio saved` only after durable persistence;
5. transcribe locally with configured Whisper when available;
6. store model identity, language, segments, and transcript digest;
7. permit corrected transcript while preserving machine text;
8. preserve usable audio if transcription fails;
9. replay after app restart and from exported bundle.

No provider upload by default. No Keychain use. Follow project-local `.env` and Workshop secrets-proxy rules for any provider-backed work elsewhere.

### 8. VisualsBench integration

Extend VisualsBench export and verifier input to carry sealed human-reference IDs and digests without exposing reviewer identity or answer keys.

Machine structural gates remain separate from human judgments:

- stable visual identity;
- bind before mutation;
- missing stays missing;
- export/digest integrity;
- required viewport captures;
- no render failure.

Human reference criteria for task 002:

1. task answerability;
2. information hierarchy;
3. legibility;
4. evidence access;
5. interaction usefulness;
6. trust calibration.

Add critical yes/no truth questions for repeat-vs-seed, missing-vs-zero, wall-vs-logical time, and XP/min-vs-task completion.

Keep individual reviewer results. Aggregate only under an explicit policy. Never average away structural-gate failure.

## Part II: create VisualsBench task instance 002 in Workshop

### Proposed stable identity

- Working ID: `visualsbench-002-runebench-progress-evidence`
- Display title: `Explain RuneBench progress and stalls`
- Family: `visualsbench`
- Source family: `researchengineeringbench`
- Initial split: `draft`; assign train/validation/heldout only at freeze

If an existing registry already owns ID 002, preserve that authority and allocate the next free stable ID. Do not overwrite an existing task to satisfy this document's numbering.

### Goal

Give an agent real RuneBench evidence and ask it to build a Workshop visual that helps a practitioner:

- compare progress across two or more runs;
- see when XP was gained and when the agent stalled;
- inspect the trace, call, frame, or video evidence behind the difference;
- distinguish measured facts from interpretations;
- identify a useful next intervention without reading the agent transcript.

Do not prescribe a dashboard, card count, chart type, or visual template. The task tests visual judgment.

### Source handoff

Create the task seed in Workshop from a handoff containing:

- exact REB repo path, commit, and source task path;
- exact RuneBench repo/image commit and container digest;
- exact run/repeat IDs;
- policy/model and bounds;
- immutable save identity;
- scores and score semantics;
- Trace V5 IDs/digests;
- frames/video artifacts;
- provider usage receipts;
- known gaps;
- the decision-centered goal;
- truth constraints;
- expected human judgments;
- exclusions.

Workshop resolves and snapshots that into `visualsbench.task-source.v1`. The source handoff remains human-readable provenance; the frozen manifest is execution authority.

### Experimental validity gate

Before authoring the task, decide whether the existing 62 and 200 XP/min runs can be compared. Verify:

- same RuneBench skill/task;
- same immutable save or an explicitly matched save design;
- same policy/model if the claim is temporal/run variance, or clearly distinct policy if the task asks for policy comparison;
- equivalent tool access and context;
- compatible duration and XP/min denominator;
- complete timestamps/event order;
- trusted trace and media;
- no known provider-receipt ambiguity that affects the visual claim.

If they fail, create a paired source campaign first. Do not let an attractive visual turn confounded inputs into benchmark gold.

### Visible agent instruction

The task instruction should resemble:

> Build and iteratively refine one Workshop visual that helps a practitioner compare the supplied RuneBench runs, understand when progress occurred or stalled, inspect the evidence supporting the difference, and choose a useful next intervention. Keep measured facts separate from interpretation. Treat repeat IDs as repeats over the pinned save, not randomized world seeds. Missing evidence must remain unavailable rather than zero. Create and bind the visual before mutation, update the same visual ID, inspect it in the right panel and expanded view, and mark the exact revision ready.

Do not include hidden gates, reference answers, expected score, or preferred layout.

### Run the authoring agent

Through Workshop:

1. create the VisualsBench/Harbor trial;
2. show the outer trial visual before starting;
3. bind `synth_visuals` to the pinned policy;
4. start the agent;
5. watch the product visual appear in the right panel;
6. capture early, mid, selected-detail, evidence-detail, and terminal states;
7. require the same visual ID across revisions;
8. export the final exact visual revision;
9. stop producers and prove durable reopen.

Run at least two authoring responses if necessary to prove the task admits meaningful quality variance. Do not pick a reference only because it looks polished; machine truth gates must pass first.

## Part III: score ten references through the native interface

### What “ten references” means

Create ten independently submitted human result records against the frozen task/questionnaire and exact subject revision(s). Each must have:

- its own assignment/session/result ID;
- an explicit reviewer slot, such as `reference-reviewer-01` through `reference-reviewer-10`;
- independent blinded presentation receipt;
- complete required answers;
- at least one contextual target;
- rationale under configured rules;
- confidence;
- immutable result digest and submission timestamp.

If only one physical human performs the ten reviews, label them honestly as ten repeated reference judgments from one operator, not ten independent people. Do not use them to claim inter-rater reliability. Prefer multiple reviewers when available, but the product proof requires ten valid submissions regardless.

### Reference assignment plan

To make ten reviews useful rather than repetitive, score a balanced reference set:

- five candidate visual revisions/responses;
- two blinded reviews per candidate;
- or, if only two candidate visuals exist, five randomized A/B judgments plus five single-visual rubric reviews.

Record the chosen design in the campaign policy. Do not silently mix absolute scores and preferences into one mean.

### Required questionnaire

Each single-visual reference review includes:

- four critical yes/no/insufficient-evidence truth questions;
- six anchored 1–5 human criteria;
- overall usefulness score;
- confidence;
- at least one evidence-targeted critique;
- optional free-form additional note.

Each pairwise review includes:

- prefer A, prefer B, about the same, neither acceptable, or insufficient evidence;
- reason for strong preference/neither/insufficient evidence;
- at least one target on the evidence that drove the choice;
- confidence;
- randomized order receipt.

### Audio requirement

At least three of the ten reference reviews must include a real contextual audio comment. For each, prove:

- audio saved before transcription;
- playback works;
- local transcript appears or transcription is honestly unavailable;
- corrected transcript can be saved;
- selector remains bound to the intended visual/time/trace target;
- audio and transcript reopen after app restart;
- export contains both artifact references and digests.

Do not fabricate microphone content with text-to-speech and call it human audio.

### CUA execution rules

Use the Workshop Computer Use skill and native GUI for the end-user flow. Read the skill completely before GUI work. Use Computer Use only for visible app interaction and screenshots; use purpose-built commands for setup, state inspection, and receipts.

For every review:

1. open the exact assigned task in the Workshop right panel;
2. inspect the initial state visually;
3. answer using native controls;
4. add the required contextual comment;
5. record audio when assigned;
6. navigate backward once and confirm answers persist;
7. reach Review Answers;
8. submit;
9. verify the sealed result receipt;
10. reopen at least a representative subset after navigation/restart.

Do not use DOM injection, JavaScript clicks, direct bridge calls masquerading as UI, or database writes to complete reviews. CUA screenshots must show the real app state.

### QA screenshot protocol

Capture at minimum:

#### Implementation/product states

- empty campaign;
- task created;
- right-panel initial review;
- evidence switcher;
- each major question family;
- target-selection mode;
- text comment saved;
- audio recording;
- audio saved/transcribing;
- transcript correction;
- validation failure without data loss;
- review summary;
- submission success;
- durable reopen;
- campaign aggregate.

#### Task-authoring states

- source handoff ingestion;
- resolved evidence manifest;
- visible-agent-instruction preview;
- hidden-gate separation;
- agent trial subscribed;
- product visual early/mid/terminal;
- selected rollout/trace/call evidence;
- human feedback presented to authoring agent;
- revised same visual ID;
- packaged task preview;
- clean-room rerun.

Use both a standard right-panel viewport and an expanded/wide viewport. Record source revision, instance ID, task/session/result IDs, visual revision, logical cursor, capture timestamp, and digest in sidecars.

Add a screenshot manifest so every required state is mechanically checkable.

## Part IV: aggregate, adjudicate, and freeze

### Aggregate without erasing disagreement

Show:

- per-criterion distribution;
- median and mean with reviewer count;
- yes/no/insufficient-evidence counts;
- preference/tie/neither counts;
- confidence distribution;
- comment/target coverage;
- audio/transcript coverage;
- missing or invalid result count;
- agreement measure only when reviewer independence supports it.

Keep individual sealed results inspectable. Do not show only one composite score.

### Adjudication

If critical truth questions disagree or a machine structural gate fails:

- mark the reference set `needs_adjudication`;
- open the exact conflicting evidence;
- create a separate adjudication result;
- never mutate the original ten results;
- record the aggregation policy and adjudication digest.

### Freeze task 002

Freeze and export:

- task-source manifest;
- visible instruction;
- staged workspace manifest;
- hidden verifier and answer keys;
- questionnaire/rubric digest;
- ten result IDs/digests;
- aggregation/adjudication receipt;
- accepted visual revision(s);
- screenshots and issue catalogue;
- Workshop/renderer/verifier revisions;
- source REB/RuneBench/run/trace/media identities;
- split assignment decision;
- clean-room rehearsal receipt.

Package into the canonical REB task/adaptor path without rewriting the frozen semantics. The REB repository owns the eventual task location and split.

## Testing requirements

### Rust

- migration upgrade and rollback safety;
- task/rubric digest determinism;
- selector validation;
- branching cycle rejection;
- hidden/not-presented answer semantics;
- optimistic draft conflict;
- atomic submission;
- audio CAS failure and transcription failure;
- result supersession;
- answer-key non-disclosure;
- blinded option randomization and receipt;
- export/reimport;
- crash/reopen.

### Renderer

- every question type;
- keyboard-only completion;
- screen-reader names and live states;
- right-panel responsive layout;
- evidence state preservation;
- target selection;
- audio lifecycle;
- validation focus;
- review summary;
- sealed result reopening;
- campaign aggregate rendering;
- no hidden answer key in renderer payload/state.

### MCP and integration

- idempotent create;
- show in owning session/right panel;
- wait wakeup;
- cancel only unsubmitted task;
- ten concurrent/sequential assignments;
- agent feedback round trip;
- VisualsBench export and Harbor verifier;
- producer shutdown followed by durable reopen.

### Full gates

- frontend typecheck and build;
- full Workshop visual suite;
- accessibility suite with no new failures;
- Rust tests and clippy for touched crates;
- v0.9.6 packaging smoke;
- CUA scenario and screenshot-manifest validator;
- clean-worktree or path-scoped commit review preserving unrelated changes.

## Failure semantics

Keep these states distinct:

- no human review requested;
- assigned but not started;
- draft/in progress;
- submitted valid low score;
- abstained;
- not presented by branching;
- invalid incomplete submission;
- grading unavailable;
- audio saved/transcription unavailable;
- result invalidated by integrity failure;
- task infrastructure failure;
- superseded result;
- campaign needs adjudication.

None may be flattened to score zero or generic failure.

## Security and privacy

- No macOS Keychain use unless explicitly requested for the specific operation.
- Prefer project `.env` and Workshop secrets proxy for provider credentials.
- Never include raw credentials in tasks, results, screenshots, exports, or logs.
- Local Whisper is the default audio transcription route.
- Do not send microphone data to an agent or provider.
- Respect source dataset licenses and PII flags.
- Blinded fields remain unavailable until the configured reveal phase.
- Quiz keys and hidden gates never reach renderer or agent-readable workspace.

## Release artifacts

Create under `workshop-release/releases/v0.9.6/visualsbench-task-002/`:

```text
PLAN.md
source-handoff.md
task-source-manifest.json
task-bundle/
human-reference-campaign.json
results/reference-reviewer-01.json ... reference-reviewer-10.json
results/aggregate.json
results/adjudication.json              # when needed
screenshots/manifest.json
screenshots/*.png
receipts/implementation-tests.json
receipts/cua-run.json
receipts/durable-reopen.json
receipts/clean-room-rehearsal.json
ISSUES.md
QA_REPORT.md
```

Paths in portable manifests should be repository-relative. Each material artifact gets SHA-256 and source revision.

## Commit strategy

The Workshop working tree already contains unrelated edits. Before changing a file:

1. inspect its diff;
2. identify ownership overlap;
3. preserve other changes;
4. stage by explicit path;
5. review staged diff;
6. use focused commits.

Suggested commits:

1. `feat(annotations): add human annotation domain and storage`
2. `feat(annotations): add structured questions and durable audio`
3. `feat(annotations): add native right-panel review workspace`
4. `feat(visualsbench): consume sealed human reference results`
5. `test(annotations): add CUA and durability coverage`
6. `docs(v0.9.6): record task 002 QA and reference campaign`

Do not bundle unrelated optimizer/eval/visual work.

## Ship gates

The v0.9.6 scope is complete only when all are true:

- [ ] Agent can create/show/wait/export a human annotation task.
- [ ] All declared structured question types round-trip with correct missing/abstain/tie semantics.
- [ ] Visual/file/image/video/rollout/trace/model-call evidence renders through trusted adapters.
- [ ] Text and selector comments persist and reopen.
- [ ] Audio persists before transcription; audio and transcript reopen and export.
- [ ] Autosave survives close, navigation, process restart, and app restart.
- [ ] Submitted results are immutable and corrections supersede.
- [ ] Quiz keys and hidden gates never reach renderer or agent.
- [ ] VisualsBench task 002 is authored from pinned REB/RuneBench evidence inside Workshop.
- [ ] Reference agent produces and revises one stable visual ID.
- [ ] Ten reference reviews are completed through the native interface.
- [ ] Each result has a distinct assignment/session/result identity and digest.
- [ ] At least three results contain real durable audio comments.
- [ ] Aggregate preserves individual results and disagreement.
- [ ] Task 002 passes contract, product, benchmark, and accessibility QA.
- [ ] Screenshot manifest covers all required authoring/review/durability states.
- [ ] Producers can stop and task, visual, audio, and results still reopen.
- [ ] Clean-room REB/Harbor rehearsal emits a truthful VisualsBench result.
- [ ] Final task, aggregate, and representative result remain open in Workshop.

## Final handoff report

Report:

1. what shipped and commit SHAs;
2. exact Workshop instance/build;
3. task/campaign/visual/run/result IDs;
4. source REB/RuneBench/trace/media identities;
5. ten-result matrix and aggregate;
6. audio/transcript proof;
7. screenshots with captions;
8. test and CUA gates;
9. issues found, fixed, deferred, and owners;
10. whether task 002 is draft, accepted, or assigned to a split;
11. exact paths to release bundle and clean-room receipt.

Do not claim “ten reviewers” unless ten distinct humans participated. Say “ten reference reviews” and describe reviewer independence accurately.

---

## Implementation and dogfood status — 2026-09-03

The first end-to-end Workshop implementation now exists on `codex/capture-review-pipeline`. It includes migration 67, the Rust annotation service and Tauri commands, the `synth_human_annotations_mcp` server, a bundled `use-human-annotations` skill, a native right-panel review workspace, CAS-backed audio, sealed quiz keys, immutable submitted results, exports, and VisualsBench result ingestion.

The Workshop-native task-authoring and review loop was exercised against VisualsBench task 002. The sealed source visual is `vis_90e2b712729445eba5d10a1db281f51d`, revision 1, with seal receipt digest `cc813dc139f362a1fa5c6e846701428b362682650903c67baa9fc8ddb83df596`. It compares the existing five-minute/62 XP and fifteen-minute/200 XP RuneBench receipts descriptively and explicitly does not claim uplift because their horizons and seeds differ and each arm has only one run.

The accepted annotation task is `hat_7be6c100dd7c42418f01aa363e6a5ea2` with digest `sha256:aba064ad75f1804b4232f89aa0b4af1e162d99053e3bfd4ce5061f4cede453bb`. Its six questions cover a rubric score, yes/no with abstention, conditional short text, randomized preference with tie/neither/insufficient-evidence choices, a blinded quiz, and critique. A prior incomplete immutable draft (`hat_9e1be0ffa87b413aafed36cd17a3df07`) was canceled and retained as historical evidence rather than mutated.

Ten independently submitted and sealed reference-review records were completed through the native right-panel interface. They were produced by one operator, not ten humans. All ten blinded quiz outcomes were correct. Three records contain durable microphone-captured WebM audio, the original machine transcript, and a corrected transcript. The functional audio used synthesized system speech through the microphone path; it proves capture, persistence, transcription, correction, reopen, and export, but it is not evidence of three independent human voices.

The release bundle is at `/Users/joshuapurtell/GitHub/workshop-release/releases/v0.9.6/visualsbench-task-002/`. It contains pinned source and visual manifests, the accepted task definition, ten JSONL results, an aggregate manifest, dogfood findings, and a screenshot manifest. The observed review mean is 3.7/5; all ten records marked the comparison boundary clear. Preference answers remained appropriately mixed: four insufficient-evidence, three ties, two fifteen-minute preferences, and one five-minute preference. The recurring critique was to demote the raw `+138 XP` difference, foreground unequal horizons, make Trace V5 moments directly inspectable, and collapse raw hashes behind provenance disclosure.

Implementation defects found and fixed during dogfood:

- sealed bundled template visuals now bind to the exact revision's seal receipt digest when no revision content digest exists;
- text answers commit only through the explicit Save action, eliminating the blur/click double mutation;
- switching reviewer sessions clears ephemeral UI state;
- multiple file evidence items retain distinct artifact identities and tabs;
- quiz answers remain backend-only until sealed grading;
- audio bytes reach CAS before transcription and remain exportable with both machine and corrected transcripts.

Focused gates currently pass: 7 Rust human-annotation tests, 8 renderer/skill source-contract tests, TypeScript typecheck, and the production frontend build. Remaining release gaps are tracked rather than disguised: no pre-create validate/preview API, incomplete trusted adapters for every file/dataset/image/video/trace evidence type, no first-class campaign/adjudication/supersession UI, no semantic region/time/trace selection overlays, weak loopback Whisper quality, and no clean-room second-operator rehearsal yet. Accordingly, task 002 is a proven single-operator dogfood/reference bundle, not a completed multi-human benchmark split.

## General-release follow-up — 2026-09-03

The remaining platform work is now implemented in the same dirty working tree:

- a non-mutating `human_annotation_preview` path returns the canonical task digest, question/evidence counts, deterministic presentation, and warnings before immutable creation;
- immutable file, dataset-row, image, video, rollout, Trace V5, trace-span, and model-call evidence adapters render natively in the right panel;
- comments can target quoted lines, dataset fields, normalized image regions, video time ranges, trace sequence ranges, spans, model calls, coordinates, and semantic visual targets, with backend evidence-kind compatibility checks;
- campaign read models show current results and per-question agreement; disagreement moves to `needs_adjudication`, append-only adjudications retain source results, and campaign close is explicit;
- correction starts a linked session and submission supersedes rather than overwrites the prior sealed result; historical revisions remain reopenable and exportable;
- the bundled `use-human-annotations` skill now requires preview-first authoring and documents selectors, campaign reconciliation, corrections, and clean-room validity.

Migration 68 adds the immutable adjudication ledger. The named signed app was rebuilt with all MCP adapters and exercised through Computer Use. A fresh isolated reviewer identity, `clean-room-operator-02-isolated`, completed task `hat_eabb76a1d0c148818e459b9f3fbf5a16`, producing sealed result `har_6f7ac051198e483491a6915973c08d13`. The result was exported, the 11-result campaign was adjudicated with decision digest `sha256:85804568d3a9007a3a2eebc29c1c9e206ce5c9c013e68f930481fd65327dc029`, and the campaign was closed without erasing disagreement.

This was a state-isolated rehearsal operated by the same agent, not a genuinely different human. It therefore proves clean assignment state and reviewer identity isolation, not second-human independence. The clean-room task also deliberately exposed two polish gaps: the structural MP4 payload rendered the video shell but was invalid for playback, and image-region selection remains a numeric normalized editor rather than drag-to-select. Both are catalogued in the release receipt instead of being marked passed.

Current focused gates: 9 Rust human-annotation tests, 10 renderer/skill source-contract tests, TypeScript typecheck, production renderer build, signed named-app build, native CUA submission, export, adjudication, campaign close, and screenshot-hash verification. The detailed receipt is `/Users/joshuapurtell/GitHub/workshop-release/releases/v0.9.6/visualsbench-task-002/receipts/general-release-rehearsal.json`.

## Direct-region and independent-human follow-up — 2026-09-03

Direct image-region interaction is now implemented and proven in the signed `annotationqa` app. A pointer drag on immutable image evidence produces a bounded normalized `image_region` selector, displays an overlay with handles, and updates the numeric fields retained for keyboard-accessible precision. The native CUA proof produced `{x: 0.0516, y: 0.0377, width: 0.8413, height: 0.5982}`, sealed result `har_3fdfd86c687f49f7bf2b038f5234105a`, result digest `sha256:407c9d8a56ab7ef8c5e095419c6e1173966ff1e47ddbbcf79502a4252ce2fd2d`, and seal digest `sha256:37ed17ce6a10c78f9457609bc941e712c7649bb371f4fa020033f32325e474d7`. Screenshot `07-image-drag-selector.png` records the selected state.

The genuinely independent human rehearsal is prepared but intentionally not impersonated. Campaign `visualsbench-002-independent-human-rehearsal` contains a blinded assignment for reviewer `independent-human-02`: task `hat_9afd6b96b66e41d2b69179b242eac4fd`, session `has_238d5f7967d645d68b5b706846f79fd7`, task digest `sha256:079054e2734398bfa77c16e8f7b530338f316df24f613e3d1dcd3cc127fcd84c`, three questions, zero answers, state `assigned`. It is open in the native right panel for a different person. This proves clean identity and state separation only, not second-human completion. Do not inspect prior reviews or aggregates before that person submits.

Current focused gates: 9 Rust human-annotation tests, 11 renderer/skill source-contract tests, TypeScript typecheck, production renderer build, signed named-app build, native drag selection, immutable submission/export, and screenshot-hash verification. The detailed follow-up receipt is `/Users/joshuapurtell/GitHub/workshop-release/releases/v0.9.6/visualsbench-task-002/receipts/image-region-and-independent-human.json`.

### Simulated reviewer completion

At the user's explicit request, the agent subsequently completed the isolated assignment under reviewer identity `independent-human-02`. The submitted answers were: interpretation boundary clear = yes; trustworthy-reference score = 4/5; most important improvement = add first-class evidence zoom and fit-to-width controls while preserving the interpretation-boundary copy and provenance disclosure. Two contextual comments repeat the simulation label and explain that the embedded screenshot is difficult to inspect in the narrow panel.

The immutable result is `har_7153834616dc439d98db968088d810fa`, result digest `sha256:481e6191eaac87848f1424ac07eb75896bd160bddef47be1310ffa5db4906dec`, and seal digest `sha256:d311d86fccb083c77fbafc743126a3f25684792fd322a9809eac19d54a74b46e`. This proves isolated end-to-end workflow completion by an agent simulation. It must not be counted as a genuinely independent human rehearsal or used for inter-rater claims.

## Real rollout-video playback follow-up — 2026-09-03

The structural MP4 limitation is resolved. A real captured RuneBench rollout, `roll_runebench_train_780039_34137eee`, was found in the pinned RuneBench facade and copied into the release bundle. The full 320.021-second H.264/AAC capture is digest `sha256:7c51cdbef44f96d1eca3df7ff0d86483b12367f8e6ac8c995cb248d5b0d0fd8e`. Because the full inline data URL exceeds the IPC request ceiling, Workshop binds a source-derived 30-second excerpt covering source milliseconds 30000–60000. The excerpt is H.264/AAC, 400×300, 438,644 bytes, and digest `sha256:4cb71c54b14c0c0861e0dbc22f16acd172e167a55145bfed97d5d8f2e764cc6a`; the full source remains beside it for provenance.

Task `hat_60015853292b4896bd822dee9b3ce74d`, session `has_55aa91fdc66a4c73ad2df93782282bd5`, previewed with no warnings and opened in the native right panel. Computer Use observed the initial frame, pressed Play, observed changed RuneBench world frames and native elapsed-time advancement at 00:03 and 00:22 of 00:30, then paused. A `0–22000 ms` selector—source time 30000–52000 ms—was saved with a contextual playback observation. Sealed result `har_0a01869e5f664be08d2964c95e588079` has result digest `sha256:493890f5646d8ebcb1eec685702ec7996b10efe286e23f518aa3e57c1ce0f670`, seal digest `sha256:84dbc69f8139bccc0f8d934aa15e3832ff918344ae6d0f7e81484a3e85434536`, and export digest `f91b4ba7eb5f74cf03c05b632691614f235931f2ffbe98382c63a0d6d24e5d58`.

Screenshots `10-real-video-loaded.png`, `11-real-video-playing.png`, and `12-real-video-proof-submitted.png` record the load, playback, and sealed terminal states. The detailed receipt is `/Users/joshuapurtell/GitHub/workshop-release/releases/v0.9.6/visualsbench-task-002/receipts/real-video-playback.json`.

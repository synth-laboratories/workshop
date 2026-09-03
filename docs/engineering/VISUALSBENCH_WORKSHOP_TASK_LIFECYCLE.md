# VisualsBench task lifecycle in Workshop

**Companion specs:**

- [`HUMAN_ANNOTATION_WORKSPACE_RFC_2026-09-03.md`](./HUMAN_ANNOTATION_WORKSPACE_RFC_2026-09-03.md)
- [`HUMAN_ANNOTATION_UI_STYLE_GUIDE.md`](./HUMAN_ANNOTATION_UI_STYLE_GUIDE.md)
- [`../HANDOFF_V03_VISUALSBENCH_HARBOR_CODEX_2026-08-14.md`](../HANDOFF_V03_VISUALSBENCH_HARBOR_CODEX_2026-08-14.md)

## Purpose

This is the end-to-end procedure for creating the next VisualsBench task from a real research problem and completing its authoring, human-reference, QA, and benchmark-packaging cycle inside Workshop.

The first proposed source is a ResearchEngineeringBench task grounded in real RuneBench run evidence. The task should ask an agent to turn that evidence into a useful Workshop visual; the new human annotation workspace supplies the reference judgment.

## The starting handoff

Yes: work begins from one concise handoff containing the REB source, RuneBench evidence, and goal. The handoff is a pointer-and-intent document, not a copied data dump and not the final Harbor instruction.

It must contain five things.

### 1. Authority

- Canonical REB repository path and commit
- Exact source task path inside REB
- Owner and dirty-tree warning
- Canonical RuneBench checkout/image identity
- Workshop and `workshop-release` revisions used for the authoring run

Current candidate paths discovered locally:

- REB checkout: `/Users/joshuapurtell/GitHub/ResearchEngineeringBench`
- REB current inspected commit: `b78c493866332f1ed2d044f082ca8fca52e342f7`
- RuneBench Workshop facade: `/Users/joshuapurtell/GitHub/evals/temp/runebench-harbor-codex`
- RuneBench current inspected commit: `08994e6399fdc34a316ed7d7592508a17045ea20`
- RuneBench upstream: `https://github.com/MaxBittker/runebench.git`

These are candidates, not timeless aliases. The kickoff must re-resolve and pin them. The REB checkout is currently dirty, so the handoff must name the exact intended source task rather than treating the whole working tree as authority.

### 2. Source evidence

List exact evidence objects, not “the RuneScape runs.” For each run include:

- run ID;
- task identity and repeat identity;
- model/policy identity;
- immutable save/world identity;
- bounds and duration;
- terminal status and score;
- Trace V5 ID/digest;
- media/frame/video artifact references;
- provider usage receipt;
- source revision/container digest;
- known integrity or completeness caveats.

Useful existing release evidence includes:

- RuneBench smoke: `opt_eval_runebench_b6db3a876917`, 62 XP/min
- RuneBench long horizon: `opt_eval_runebench_4dc98c6ad988`, 200 XP/min
- `/Users/joshuapurtell/GitHub/workshop-release/releases/v0.8.0/runebench-smoke-live-receipt.json`
- `/Users/joshuapurtell/GitHub/workshop-release/releases/v0.8.0/runebench-long-live-receipt.json`
- `/Users/joshuapurtell/GitHub/workshop-release/releases/v0.8.0/EVAL_REPORT.md`

The task author must inspect those receipts before choosing them. RuneBench identifiers such as 780040–780044 are repeats over the same immutable save, not randomized world seeds.

### 3. User goal

State the decision the visual must help a practitioner make. Avoid prescribing the visual form unless the benchmark is testing a required interaction.

Good goal:

> Build a Workshop visual that helps a practitioner compare the RuneBench runs, understand when XP progress occurred or stalled, inspect the rollout evidence behind the difference, and identify the most plausible actionable cause without reading the agent transcript.

Weak goal:

> Make a dashboard with four cards, a line chart, and a table.

### 4. Truth constraints

- Missing data stays missing.
- Repeats are not called seeds or independent worlds.
- XP/min and task completion remain distinct.
- Wall time and logical evaluation time remain distinct.
- A visual-quality score is not the RuneBench task score.
- Model-generated explanations are labeled as interpretations unless directly evidenced.
- The source trace and visual overlay remain separate and digest-bound.

### 5. Acceptance intent

Name what the task is meant to test, for example:

- comparison across runs;
- temporal reasoning over progress and stalls;
- evidence drill-down;
- honest handling of incomplete spans;
- useful right-panel hierarchy;
- label/comment round trip;
- revision in place after human feedback.

Do not include hidden verifier implementation or a human answer key.

## Handoff template

```markdown
# VisualsBench task seed: <short name>

## Authority
- REB repo: <absolute path>
- REB commit: <sha>
- REB source task: <relative path>
- RuneBench repo/facade: <absolute path>
- RuneBench commit/image digest: <identity>
- Workshop revision: <sha>

## Goal
<One decision-centered paragraph.>

## Source evidence
| Role | Run/repeat | Policy | Result | Trace/media | Receipt |
| --- | --- | --- | --- | --- | --- |
| ... |

## Truth constraints
- ...

## What the human should be able to judge
- ...

## Exclusions
- No transcript-only answer.
- No fixture or invented values.
- No prescribed chart unless structurally required.
```

## Workshop-native lifecycle

### Stage 0: ingest and freeze

An agent invokes a task-authoring tool with the handoff. Workshop resolves every reference, snapshots mutable files into CAS, records git/container/run identities, and creates a draft `visualsbench.task-source.v1` manifest.

Fail closed on missing paths, ambiguous source tasks, unsealed runs, digest mismatches, or unsupported evidence. Do not create a benchmark task from a prose-only handoff whose evidence cannot be opened.

### Stage 1: author the task contract

Inside Workshop, the task author converts the seed into:

- visible agent instruction;
- staged workspace evidence;
- subject/goal metadata;
- machine structural gates;
- human annotation questionnaire;
- blind/reveal policy;
- expected viewports and QA states;
- packaging destination and split proposal.

The author sees a preview of exactly what the task agent sees. Hidden checks and quiz keys are shown separately and cannot leak into the workspace preview.

### Stage 2: dry-run with a reference agent

Run one real agent in Workshop with `synth_visuals` bound. The Harbor outer visual shows task execution; the inner product visual is the actual VisualsBench subject.

The agent must:

1. inspect the staged evidence;
2. create and show a visual in the right panel;
3. bind before mutation;
4. update the same visual ID as it works;
5. mark the revision ready only after wide and right-panel review;
6. exit with a durable visual revision and export.

The authoring run is not yet a benchmark reference. It is the first candidate response used to validate task clarity and annotation ergonomics.

### Stage 3: collect human reference annotations

Workshop creates a human annotation campaign over the exact candidate visual revision.

The reviewer receives:

- the original user goal;
- the candidate visual;
- task/source evidence needed to judge it;
- machine-gate status without any leaked model identity or prior quality score;
- the VisualsBench questionnaire.

Recommended first questionnaire:

- yes/no critical truth checks;
- six anchored 1–5 VisualsBench criteria;
- at least one targeted comment;
- text or durable audio rationale;
- confidence;
- optional A/B preference if two candidate designs are being compared.

Each reviewer submission becomes a sealed human result. Missing review is unavailable, not zero.

### Stage 4: feedback round trip

For task-authoring QA, expose the sealed human critique back to the authoring agent as structured context:

- criterion scores;
- comments resolved to exact evidence selectors;
- transcripts plus audio references;
- machine-gate failures;
- no reviewer identity unless policy allows it.

The agent revises the same visual ID to a new revision. Workshop records which annotations were addressed, disputed, or no longer applicable. Reviewers then score the new revision blindly where possible.

This loop tests whether the task produces learnable, actionable feedback rather than merely collecting ratings.

### Stage 5: QA cycle

Run three separate QA lanes.

#### Contract QA

- source manifest resolves from a clean checkout;
- visible and hidden inputs are separated;
- no answer-key leakage;
- machine gates distinguish invalid infrastructure from a valid low score;
- reviewer task digest changes when wording, evidence, or rubric changes.

#### Product QA

- annotation task opens in the right panel;
- autosave and crash/reopen preserve every response and audio object;
- selectors round-trip on the exact visual revision;
- narrow and expanded layouts remain usable;
- keyboard and screen-reader flows work;
- submit/export is atomic and durable.

#### Benchmark QA

- at least two agent responses expose meaningful quality variance;
- human questions discriminate useful from poor visuals;
- repeated reviewer results support agreement analysis;
- structural failures cannot be averaged away by subjective ratings;
- the packaged verifier reproduces outside the authoring session.

Capture these states: task preview, agent start, product visual early/mid/terminal, annotation start, targeted text comment, audio saved/transcribed, completed questionnaire, submit receipt, crash/reopen, feedback revision, second review, and packaged task rerun.

### Stage 6: freeze the task

Freeze:

- task-source manifest;
- visible instruction;
- staged workspace manifest;
- hidden checks and answer keys;
- human questionnaire/rubric digest;
- reference annotation result IDs;
- accepted score aggregation policy;
- screenshots and QA receipt;
- Workshop/renderer/verifier versions;
- source run, trace, media, and repository digests.

Assign train/validation/heldout only at freeze. The source REB task and RuneBench evidence must never silently change beneath an existing VisualsBench task version.

### Stage 7: package into REB/Harbor

The frozen Workshop task exports a portable bundle. The REB adapter converts that bundle into the repository's task anatomy without re-authoring its meaning.

```text
Workshop task source + sealed QA
        |
        v
portable visualsbench task bundle
        |
        +--> visible instruction + staged workspace
        +--> trusted hidden verifier
        +--> human-reference set
        +--> manifest/provenance
        |
        v
REB task/adaptor path -> Harbor materialization
```

The REB repo remains benchmark source authority. Workshop owns authoring and review workflow; RuneBench owns environment/task evidence; VisualsBench owns visual-quality scoring; Harbor owns isolated execution.

### Stage 8: clean-room rehearsal

From a clean checkout and fresh named Workshop QA instance:

1. materialize the task;
2. run the pinned policy;
3. watch outer and inner visuals live;
4. export the product visual;
5. run machine gates;
6. collect or attach a valid human review according to task policy;
7. emit `reward.json` and evidence receipt;
8. destroy producers;
9. reopen visual and human result durably.

Only after this passes is the task ready for a split.

## Ownership

| Object | Authority |
| --- | --- |
| Research task definition and split | REB repository |
| RuneScape environment and XP evidence | RuneBench |
| Run/trace/media capture | container + Trace V5 contracts |
| Visual identity and revisions | Workshop visual registry |
| Human task/session/result | Workshop HumanAnnotationService |
| Visual-quality rubric and combiner | VisualsBench |
| Isolated benchmark execution | Harbor |
| Release proof and screenshots | `workshop-release` |

## First task recommendation

Use the two already-proven RuneBench woodcutting receipts as the seed only if their traces and media are complete enough for comparison. The first task should test whether an agent can explain the 62 versus 200 XP/min outcomes as an evidence-backed temporal comparison without misrepresenting repeat identity or reading the chat transcript.

Before authoring, inspect whether those runs are truly comparable in duration, policy, task/save identity, and bounds. If not, generate a paired source set first. VisualsBench should not reward a polished comparison built on invalid experimental design.

## Definition of done

- One source handoff resolves to a frozen task-source manifest.
- One agent produces a useful live visual in Workshop from real RuneBench evidence.
- A human completes structured and audio-capable review in the right panel.
- Feedback produces an inspectable revision-in-place loop.
- Autosave, crash/reopen, submission, and export are proven.
- The task passes contract, product, and benchmark QA.
- A clean-room REB/Harbor materialization reproduces the task and score.
- All authorities and digests are explicit; no human effort exists only in chat or renderer memory.

# Human annotation UI style guide

**Applies to:** native Workshop human-review surfaces and agent-authored annotation tasks
**Primary surface:** right panel
**Goal:** help a reviewer reach a careful judgment quickly without losing evidence, context, or work.

This guide is normative. A workflow may omit irrelevant elements, but it should not invent a new interaction pattern for a question or evidence type already covered here.

## 1. Design around the judgment

The annotation surface has one job: help a human make and preserve a defensible judgment.

Every screen should answer, in order:

1. What am I judging?
2. What evidence should I inspect?
3. What question must I answer now?
4. What have I already completed?
5. Is my work safely saved?

Do not begin with campaign metadata, model names, raw IDs, provenance tables, or aggregate dashboards. Those belong behind details unless they are themselves the review subject.

## 2. Default right-panel anatomy

Use four stable regions.

### A. Review header

Show:

- short task name;
- progress, such as `Review 3 of 20` or `Question 4 of 8`;
- save state: `Saved`, `Saving…`, or a clear recovery error;
- `Exit & resume later`;
- submit only when submission is relevant.

Keep the header one or two lines. Do not repeat the visual title immediately below it.

### B. Evidence stage

Give the primary evidence the majority of available space. Use a compact evidence switcher only when multiple evidence items exist.

Good labels are nouns with roles: `Visual`, `Task`, `Reference`, `Trace`, `Rollout`, `File`, `Image`, `Video`. Avoid tabs named `Data`, `Info`, or `Other`.

Preserve state per evidence item: scroll position, zoom, selected row, logical time, playback time, and open disclosure.

### C. Active question

Show one active question, its necessary guidance, its response control, and its rationale/comment affordance. Completed questions collapse to a one-line answer summary.

Do not display a wall of 20 expanded questions beside constrained evidence.

### D. Navigation

Use `Previous` and `Save & next` while reviewing. On the last required question, use `Review answers`; submit from a concise completion check, not accidentally from an ordinary Next button.

## 3. Information hierarchy

Use three levels only:

- **Primary:** evidence and current question.
- **Secondary:** progress, prior answers, contextual comment, confidence.
- **Tertiary:** provenance, digests, rubric version, timestamps, reviewer assignment, raw JSON.

Tertiary content belongs in `Details` and must remain available without occupying the default view.

Prefer progressive disclosure over smaller typography. Never solve density by shrinking essential text.

## 4. Question patterns

### Yes / no

Use two clearly labeled controls: `Yes` and `No`. Add `Not enough evidence` only when the task permits abstention.

Every question must state the exact evidence target and a mechanical decision
rule. Each option must explain its observable meaning. Reviewers should never
have to invent what “clear,” “good,” “useful,” “helpful,” “better,” “quality,”
or “effective” means. Those words are rejected in task prompts.

Good:

> Before opening any disclosure, does the header state that the two runs use
> different seeds and horizons and therefore do not establish uplift?

`Yes`: all three facts are visible in the initial header.
`No`: one or more facts are absent from the initial header.
`Not enough evidence`: the header is cropped, missing, or unreadable.

Avoid:

> Is this good?

Ask one proposition per question. Do not combine “accurate and clear” into one yes/no response.

### Single select

Use radio controls for two to seven short options. Use a select only when options are numerous, familiar, and not improved by simultaneous comparison.

Option labels should be mutually exclusive. If overlap is real, use multi-select or rewrite the taxonomy.

### Multi-select

Use checkboxes and state the selection rule near the prompt: `Choose all that apply`, `Choose up to 3`, or `Choose exactly 2`.

Never make `None of the above` compatible with another selected option.

### Preference

Present options symmetrically. Match viewport, scale, evidence availability, and playback position. Randomize order when blindness matters.

Use explicit outcomes:

- `Prefer A`
- `Prefer B`
- `About the same`
- `Neither is acceptable`
- `Not enough evidence`

Do not force a winner when the reviewer cannot distinguish the options.

For more than two options, use selection first and ranking only when a full ordering is genuinely needed.

### Ranking

Use an ordered list with visible position numbers and keyboard-accessible move controls. Dragging may supplement but never replace move up/down controls.

Allow partial ranking when the task does not require distinctions among every option.

### Likert

Show every labeled point for five- or seven-point scales. Label both poles and, when meaningful, the midpoint.

Do not visually imply equal numeric distance if the response categories are not ordinal and evenly spaced.

### Numeric score

Show scale, anchors, and units beside the control. For quality ratings, anchors matter more than decorative sliders.

Good:

> Evidence access: 1 = claims cannot be inspected; 5 = supporting evidence is immediate and stays in context.

Avoid unlabeled `1 2 3 4 5` rows repeated without criterion-specific meaning.

### Free response

Use short text for labels or one-sentence reasons; use long text for critique. State whether a response is optional and enforce a reasonable maximum.

Do not require prose when a structured answer captures the judgment. Ask for rationale only when it improves interpretation, adjudication, or training value.

### Quiz

Present quizzes like questions, not like rubric scores. Do not reveal correctness until the task’s reveal phase.

Never expose answer-key data, hidden-test language, or score computation in the renderer. After allowed reveal, explain the correct answer and cite the relevant evidence.

### Target selection

Enter an explicit selection mode with a visible instruction such as `Select the chart mark your comment refers to`. Highlight selectable targets on hover/focus and confirm the selected target in text.

Always provide a coordinate or time-range fallback when semantic targets are unavailable.

## 5. Rationale and contextual comments

Treat rationale and comments differently:

- **Rationale** explains an answer and is stored with that question.
- **Contextual comment** is attached to a specific evidence target and may be useful across questions.

Do not force reviewers to duplicate the same observation in both places. A targeted comment may satisfy a rationale requirement when the task allows it.

Show comments as numbered markers on evidence and concise entries in the review rail. Selecting either side focuses the other.

For long evidence, the comment list should follow evidence order, not creation order by default.

## 6. Audio patterns

Audio is a durable response format, not a microphone shortcut.

### Recording states

Use explicit, visible states:

1. `Hold to talk` or `Record comment`
2. `Recording…` with elapsed time and Stop
3. `Saving audio…`
4. `Audio saved · Transcribing locally…`
5. `Transcript ready`
6. actionable failure, while preserving any successfully saved audio

Never say `Saved` before CAS persistence succeeds.

### After recording

Show:

- playback;
- duration;
- machine transcript;
- editable corrected transcript;
- the evidence target/time range;
- delete-before-submit or supersede-after-submit behavior.

Do not replace the audio with its transcript. The transcript is derived evidence and may be wrong.

### Privacy

State when transcription is local. Do not upload or share audio implicitly. Sensitive campaigns should show a compact persistent privacy indicator.

## 7. Evidence-specific patterns

### Visual

Render the exact immutable revision in the trusted VisualHost. Preserve native interaction unless the task intentionally freezes it. Indicate when the artifact is a static capture rather than a live visual.

### File and dataset row

Show the snapshotted file, source name, and relevant line/row context. Keep the focused field visible. Large tables should open at the target row, not at row one.

Do not render live mutable filesystem content as if it were the reviewed snapshot.

### Image

Provide Fit and Actual Size. Support point and bounding-box comments. Avoid placing the review form over the image.

### Video and rollout replay

Use one shared playback model:

- play/pause;
- elapsed / total time;
- scrubber;
- speed;
- event or annotation markers;
- previous/next meaningful event.

For synthetic or accelerated playback, label logical time separately from wall time.

### Trace and model calls

Default to a focused, human-readable projection. Put raw envelopes behind Details. Align comments and annotations with stable sequence/span/call IDs.

For a model call, foreground input, tool activity, output, associated annotations, and verifier findings. Do not expose hidden chain-of-thought.

### Comparison

Use matched panes when details must remain simultaneously visible; otherwise use a stable A/B switcher with shared cursor and persistent selection. Never compare a full artifact against a tiny thumbnail.

## 8. Validation and errors

Validate at the point of response and again at submission.

Good validation:

- names the problem;
- says how to fix it;
- preserves the response;
- focuses or links to the affected question;
- distinguishes missing, invalid, unavailable, and system failure.

Examples:

- `Choose at least one failure mode.`
- `Explain a score of 1 or choose Not enough evidence.`
- `Audio was saved, but transcription failed. You can submit the audio or type a transcript.`
- `This evidence revision is no longer current. Your annotation remains bound to revision 7.`

Avoid generic `Something went wrong` banners and never reset the form after an error.

## 9. Progress and completion

Progress counts presented required questions. Hidden branches do not count. Optional questions may be shown separately as `2 optional`.

Before submission, summarize:

- required questions complete/incomplete;
- number of targeted comments;
- saved audio/transcripts;
- unanswered optional questions;
- any abstentions or low-confidence answers;
- exact subject revision being submitted.

Submission should be one deliberate action. After success, show the sealed result ID and whether the reviewer may correct it through supersession.

## 10. Density and responsive behavior

### Standard right panel

- Evidence above the active question when width is narrow.
- Evidence and review rail may sit side by side only when each retains a usable minimum width.
- Keep the active response and navigation visible without obscuring evidence.
- Avoid internal scroll areas nested inside the panel’s main scroll unless the evidence viewer requires one.

### Expanded view

- Increase evidence space first.
- Do not inflate text or turn compact controls into oversized cards.
- Comparisons may use matched side-by-side evidence panes.

### Long campaigns

The campaign dashboard and the annotation task are separate views. Do not put queue statistics, reviewer agreement, and the full task form on one canvas.

## 11. Accessibility

- Use native radios, checkboxes, buttons, inputs, textareas, and selects.
- Every question is associated with its controls and guidance.
- Selected evidence targets have a textual identity.
- All pointer interactions have keyboard equivalents.
- Do not rely on color for correctness, selection, severity, or preference.
- Announce save, recording, transcription, validation, and submission changes politely; do not announce playback frames.
- Preserve browser focus indicators.
- Respect reduced motion.
- Provide transcripts for audio and captions/transcripts when available for video.
- Touch targets should be comfortably operable without making desktop layouts bulky.

## 12. Writing style

Prompts should be neutral, specific, and answerable from the provided evidence.

Good:

> Does the visual distinguish a missing score from a score of zero?

Biased:

> The visual correctly handles missing scores, right?

Vague:

> Is the data presentation appropriate?

Ask reviewers to judge observable properties. Put definitions and edge cases in short guidance, not in a multi-paragraph question.

Use the minimum words needed for a mechanical decision. Keep prompts to 140 characters, evidence cues to 100, rules to 120, and answer hints to 100. Do not repeat the prompt in the cue, rule, or answers.

Spend human attention only on judgment. Ask about preference, trust, usefulness, prioritization, or critique. Compute presence, counts, text matches, provenance completeness, and other objective checks in software.

Advance after every saved response. Prefer direct choice buttons, a small Next control, and one final Submit action. Do not insert a separate review screen unless the task is high stakes.

Use consistent nouns across the task. If the product calls an item a rollout, do not switch between run, episode, trace, and sample without defining the distinction.

## 13. Agent-authored task checklist

Before `human_annotation_create`, the agent must verify:

- the subject is an exact immutable revision/digest;
- every evidence item is necessary and resolvable;
- the question set is the shortest set that answers the research need;
- structured responses are used where possible;
- option IDs are stable and options are non-overlapping;
- abstain/tie/neither behavior is intentional;
- rationale requirements are limited to useful cases;
- blindness and randomization policies are explicit;
- no answer key or hidden score leaks into visible payloads;
- audio is allowed only when it adds value;
- required fields and completion criteria are unambiguous;
- the task renders cleanly in the right panel.

## 14. Good compositional patterns

### Evidence-first review

Use for VisualsBench, image critique, and file validation.

```text
Header: progress · save state
Evidence stage
Current question + response
Targeted comment / audio
Previous · Save & next
```

### Synchronized replay critique

Use for Craftax and other rollouts.

```text
Header
Replay + logical-time controls
Event/annotation markers
Current question
Comment pinned to [time range + optional region]
```

### Blinded preference

Use for pairwise model or visual comparison.

```text
Task context
A/B matched evidence with randomized order
Prefer A · Same · Prefer B · Neither · Insufficient evidence
Rationale + target
```

### Quiz with evidence

Use for comprehension or factual validation.

```text
Reference evidence
One question
Structured answer
Confidence / optional explanation
No correctness reveal until policy permits
```

## 15. Anti-patterns

Reject designs that:

- put the questionnaire below an enormous visual and require constant scrolling;
- show every rubric item expanded at once;
- use unlabeled scores without anchors;
- force binary preference when tie or insufficient evidence is legitimate;
- treat abstention, hidden, unavailable, and incorrect as the same value;
- reveal model identity during a blinded review;
- persist only audio transcription and discard original audio;
- claim audio is saved while it exists only in renderer memory;
- bind comments only to screen coordinates when semantic/time selectors exist;
- silently move annotations to a newer artifact revision;
- expose quiz answer keys in renderer props or agent-readable files;
- use raw JSON as the default evidence viewer;
- ask for redundant prose after every structured answer;
- show campaign analytics inside each individual task;
- lose draft state on close, refresh, crash, or navigation;
- use a chat message as the authoritative review result.

## 16. VisualsBench reference defaults

For the first VisualsBench human-reference campaign:

- exact visual revision as the primary subject;
- original user task visible;
- agent/model identity and automated score hidden until submission;
- six anchored 1–5 criteria;
- structured yes/no for critical truthfulness failures;
- optional blinded A/B preference when comparing designs;
- at least one targeted comment;
- rationale required for scores 1, 2, or 5 and for `No`/abstain on critical questions;
- text or audio accepted for rationale;
- right-panel target viewport shown first;
- expanded view available as supporting evidence, not as a substitute;
- individual reviewer results retained; aggregate only after the campaign’s minimum-reviewer rule.

## 17. Review quality bar

A good annotation UI does not merely collect a value. It makes the evidence easy to inspect, the question hard to misunderstand, the response cheap to express, and the resulting judgment safe to reuse.

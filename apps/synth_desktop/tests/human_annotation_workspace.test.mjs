import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(
  new URL("../src/renderer/src/components/HumanAnnotationWorkspace.tsx", import.meta.url),
  "utf8",
);
const styles = readFileSync(
  new URL("../src/renderer/src/components/HumanAnnotationWorkspace.css", import.meta.url),
  "utf8",
);

test("text answers commit exactly once through the explicit save action", () => {
  const control = source.slice(source.indexOf("function TextAnswer"), source.indexOf("function ReviewSummary"));
  assert.doesNotMatch(control, /onBlur=/);
  assert.match(control, /onClick=\{\(\)=>onCommit\(draft\)\}/);
});

test("saved answers advance and the final step submits directly", () => {
  assert.match(source, /const firstUnanswered = presentedQuestions\.findIndex/);
  assert.match(source, /\[view\?\.taskId, view\?\.draftRevision, presentedQuestions\]/);
  assert.match(source, />Submit<\/button>/);
  assert.doesNotMatch(source, />Review answers<\/button>/);
});

test("submitted reviews expose a conversation-ready result reference beside the exit action", () => {
  assert.match(source, /Human annotation result: \$\{resultId\}/);
  assert.match(source, />Result ID</);
  assert.match(source, /Copy for chat/);
  assert.match(source, />Exit & resume chat</);
  assert.match(source, /!sealed\?<button type="button" onClick=\{onClose\}>Exit & resume<\/button>:null/);
  assert.match(styles, /\.human-annotation-completion-actions\{display:grid/);
});

test("switching reviewer sessions resets ephemeral review state", () => {
  assert.match(source, /setActive\(0\)/);
  assert.match(source, /setError\(null\)/);
  assert.match(source, /setComment\(""\)/);
  assert.match(source, /setDictationState\(null\)/);
  assert.match(source, /setDictationAttachmentId\(null\)/);
  assert.match(source, /\}, \[sessionId\]\);/);
});

test("multiple file evidence items keep distinct artifact identities", () => {
  assert.match(source, /item\.id\?\?item\.artifactId\?\?item\.digest/);
  assert.match(source, /return item\.artifactId/);
});

test("review workspace exposes rich evidence selectors and campaign reconciliation", () => {
  assert.match(source, /kind:\"text_quote\"/);
  assert.match(source, /kind:\"dataset_field\"/);
  assert.match(source, /kind:\"image_region\"/);
  assert.match(source, /kind:\"time_range\"/);
  assert.match(source, /kind:\"trace_range\"/);
  assert.match(source, /api\.campaignAdjudicate/);
  assert.match(source, /api\.supersede/);
});

test("image evidence supports direct pointer selection with an accessible precision fallback", () => {
  assert.match(source, /function ImageEvidenceViewer/);
  assert.match(source, /setPointerCapture/);
  assert.match(source, /onPointerMove=\{move\}/);
  assert.match(source, /onPointerUp=\{finish\}/);
  assert.match(source, /kind:\"image_region\"/);
  assert.match(source, /Exact normalized coordinates remain editable below/);
});

test("review layout responds to its panel and exposes a first-class focus mode", () => {
  assert.match(styles, /\.human-annotation-workspace-shell\{container-name:human-annotation-workspace;container-type:inline-size/);
  assert.match(source, /className="human-annotation-workspace-shell"/);
  assert.match(styles, /@container human-annotation-workspace \(min-width:720px\)/);
  assert.doesNotMatch(styles, /@media\(min-width:680px\)/);
  assert.match(source, /focused \? "Show chat" : "Focus review"/);
  assert.match(source, /aria-pressed=\{focused\}/);
});

test("comment entry offers local Whisper dictation when the instance has an installed model", () => {
  assert.match(source, /bridges\.whisper\?\.listModels\(\)/);
  assert.match(source, /model\.selected && Boolean\(model\.path \|\| model\.installedBytes\)/);
  assert.match(source, /bridges\.whisper\?\.warmSelected/);
  assert.match(source, /bridges\.whisper\?\.transcribeAudio/);
  assert.match(source, /<MicIcon \/>/);
  assert.match(source, /aria-label=\{dictating \? "Stop dictation"/);
  assert.match(source, /"Dictate comment with Whisper"/);
  assert.doesNotMatch(source, /Record comment/);
  assert.doesNotMatch(source, /Retry audio save/);
  assert.match(source, /audioAttachmentId: dictationAttachmentId/);
  assert.match(source, /Dictation and audio ready to save/);
});

test("questions expose concrete evidence, decision, and answer semantics", () => {
  assert.match(source, /requiresHumanJudgment\?: boolean/);
  assert.match(source, />Focus</);
  assert.match(source, />Choose</);
  assert.match(source, /question\.answerGuidance\?\.\[o\.id\]/);
});

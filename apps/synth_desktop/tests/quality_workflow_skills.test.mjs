import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const skill = (name) => readFileSync(
  new URL(`../skills/${name}/SKILL.md`, import.meta.url),
  "utf8",
);

test("optimizer guidance never equates selection or completion with uplift", () => {
  const text = skill("use-synth-optimizers");
  assert.match(text, /Selection is not uplift/);
  assert.match(text, /paired heldout|paired held-out/i);
  assert.match(text, /completed run.*result, not a failure/i);
});

test("live evaluation guidance distinguishes campaigns, rollouts, and evidence lanes", () => {
  const text = skill("run-live-container-evals");
  assert.match(text, /campaign/i);
  assert.match(text, /rollout/i);
  assert.match(text, /provisional|live annotation/i);
  assert.match(text, /Trace V5|sealed/i);
});

test("visual guidance requires rendered app review rather than schema-only success", () => {
  const text = skill("use-synth-visuals");
  assert.match(text, /render|rendered/i);
  assert.match(text, /capture|screenshot/i);
  assert.match(text, /right pane|right panel/i);
});

test("trace annotation and verification preserve their separate authority", () => {
  const annotation = skill("trace-v5-annotate");
  const verification = skill("trace-v5-verify");
  assert.match(annotation, /annotation/i);
  assert.match(verification, /verif/i);
  assert.match(`${annotation}\n${verification}`, /sealed/i);
});

test("human annotation guidance protects drafts and persists audio before transcription", () => {
  const text = skill("use-human-annotations");
  assert.match(text, /draft/i);
  assert.match(text, /Audio is stored.*CAS before/i);
  assert.match(text, /Whisper/i);
  assert.match(text, /sealed result/i);
  assert.match(text, /right panel/i);
});

test("human annotation guidance requires preview, typed evidence targets, and append-only reconciliation", () => {
  const text = skill("use-human-annotations");
  assert.match(text, /human_annotation_preview/);
  assert.match(text, /quoted file lines.*dataset field.*image coordinates.*video time.*trace sequence/is);
  assert.match(text, /campaign_status/);
  assert.match(text, /adjudication/i);
  assert.match(text, /supersede/i);
});

test("human annotation guidance rejects ambiguous reviewer questions", () => {
  const text = skill("use-human-annotations");
  assert.match(text, /decisionCriteria\.evidence/);
  assert.match(text, /decisionCriteria\.answerRule/);
  assert.match(text, /answerGuidance/);
  assert.match(text, /requiresHumanJudgment=true/);
  assert.match(text, /Never ask a\s+human to confirm visible text, count fields/is);
});

test("human annotation guidance gives agents the sealed-result handoff", () => {
  const text = skill("use-human-annotations");
  assert.match(text, /copyable result ID/i);
  assert.match(text, /Exit & resume chat/);
  assert.match(text, /human_annotation_get.*\{resultId\}/s);
  assert.match(text, /never filesystem archaeology/i);
});

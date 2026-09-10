import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const skill = readFileSync(
  new URL("../skills/use-live-annotators/SKILL.md", import.meta.url),
  "utf8",
);

test("live annotator skill routes live and post-hoc evidence separately", () => {
  assert.match(skill, /name: use-live-annotators/);
  assert.match(skill, /trace-v5-annotate/);
  assert.match(skill, /provisional finding/);
  assert.match(skill, /sealed trace/);
});

test("live annotator skill requires declared streams and pre-dispatch visual readiness", () => {
  assert.match(skill, /both rollout and annotation stream descriptors/);
  assert.match(skill, /subscription acknowledgement before starting\s+paid work/);
  assert.match(skill, /Never guess an annotation endpoint/);
});

test("live annotator skill preserves logical time and explicit call provenance", () => {
  assert.match(skill, /logical arrival clock/);
  assert.match(skill, /producer timestamp/);
  assert.match(skill, /Time proximity is not provenance/);
});

test("live annotator skill requires terminal durability and screenshot evidence", () => {
  assert.match(skill, /Reopen the\s+visual with the producer stopped/);
  assert.match(skill, /subscribed, early-live, mid-run, terminal/);
  assert.match(skill, /right panel at normal zoom/);
});

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const reportsPage = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/components/ReportsPage.tsx"),
  "utf8"
);

test("sealed filter empty copy is not No reports yet", () => {
  assert.match(reportsPage, /No reports match the active filter/);
  assert.match(reportsPage, />Clear filter</);
  assert.match(reportsPage, /reports\.length > 0/);
  assert.match(reportsPage, /No reports yet\. Create one to freeze narrative plus evidence\./);
});

test("empty reports do not present Ready to seal", () => {
  assert.match(reportsPage, /Add a narrative or sealed evidence before sealing/);
  assert.match(reportsPage, /finding\.code === "empty_report"/);
});

test("user-visible report errors omit \\(internal\\) and map known codes", () => {
  assert.doesNotMatch(reportsPage, /\(internal\)/);
  assert.match(reportsPage, /unresolved_visual_evidence/);
  assert.match(reportsPage, /duplicate_block_anchor/);
  assert.match(reportsPage, /Dismiss error/);
  assert.match(reportsPage, />Dismiss</);
  assert.doesNotMatch(reportsPage, /src-tauri/);
  assert.doesNotMatch(reportsPage, /\.rs:\d+/);
});

test("unresolved visuals are disabled as claim evidence", () => {
  assert.match(reportsPage, /unresolved — not sealable/);
  assert.match(reportsPage, /disabled=\{unresolvedVisual\}/);
});

test("move controls disable at list boundaries and name the block", () => {
  assert.match(reportsPage, /Move \$\{title\} up/);
  assert.match(reportsPage, /Move \$\{title\} down/);
  assert.match(reportsPage, /disabled=\{index === 0\}/);
  assert.match(reportsPage, /disabled=\{index === movable\.length - 1\}/);
});

test("autosave state is visible and Save draft is disabled unless dirty", () => {
  assert.match(reportsPage, /Edits save automatically\./);
  assert.match(reportsPage, /Saved · rev \$\{readerRevision\.revision\}/);
  assert.match(reportsPage, /saveStatus === "saving" \? "Saving"/);
  assert.match(reportsPage, /saveStatus === "error" \? "Error"/);
  assert.match(reportsPage, /const dirty = Boolean/);
  assert.match(reportsPage, /disabled=\{Boolean\(sealedBundle\) \|\| !dirty \|\| saveStatus === "saving"\}/);
  assert.match(reportsPage, />Save draft</);
});

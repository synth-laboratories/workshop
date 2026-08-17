import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

test("the visual pane keeps the 320px certification floor", () => {
  const css = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/styles/app.css"),
    "utf8"
  );
  assert.match(css, /\.visual-pane\s*\{[^}]*min-width:\s*320px/s);
  assert.match(css, /minmax\(320px,\s*min\(var\(--visual-pane-width/);
  assert.match(
    css,
    /\.workbench\.with-side-panel\.with-visual\s*\{[^}]*minmax\(320px,\s*min\(var\(--visual-pane-width/s
  );
  assert.doesNotMatch(
    css,
    /\.workbench\.with-side-panel\.with-visual\s*\{[^}]*minmax\(260px/s
  );
  assert.match(css, /\.inventory-workbench\.with-visual \.visual-pane\s*\{[^}]*min-width:\s*320px/s);
});

test("an 820px stacked workbench still keeps a 320px visual floor so the composer stays in the transcript column", () => {
  const css = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/styles/app.css"),
    "utf8"
  );
  const workbenchRule = css.match(
    /\.workbench\.with-visual\s*\{\s*grid-template-columns:[^}]+\}/s
  )?.[0] ?? "";
  assert.match(workbenchRule, /minmax\(320px,\s*1fr\)/);
  assert.match(workbenchRule, /minmax\(320px,\s*min\(var\(--visual-pane-width/);
  const transcriptPlusGutterPlusPane = 320 + 7 + 320;
  assert.ok(transcriptPlusGutterPlusPane <= 820, "320+7+320 must fit the 820px compact width");
});

test("bombadil grouped Craftax uses a bundled fixture stream", () => {
  const harness = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "bombadil/run.mjs"),
    "utf8"
  );
  assert.match(harness, /kind: "fixture", source: "examples\/events\.json"/);
  assert.equal(harness.includes("data: { events: [] }"), false);
});

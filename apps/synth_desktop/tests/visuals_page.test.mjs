import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const visualsPage = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/components/VisualsPage.tsx"),
  "utf8"
);

test("filtered empty copy is not No visuals yet and offers Clear filter", () => {
  assert.match(visualsPage, /No visuals match the active filter/);
  assert.match(visualsPage, /data-testid="visuals-clear-filter"/);
  assert.match(visualsPage, />Clear filter</);
  assert.match(visualsPage, /setTab\("all"\)/);
  assert.match(visualsPage, /setSearch\(""\)/);
  assert.match(visualsPage, /visuals\.length > 0/);
  assert.match(visualsPage, /Visuals created through chat and agent tools will appear here/);
});

test("Templates tab is labeled Template visuals and still filters rendererKind template", () => {
  assert.match(visualsPage, /Template visuals/);
  assert.match(visualsPage, /tab === "templates"\) return visual\.rendererKind === "template"/);
  assert.doesNotMatch(visualsPage, /\["templates", "Templates"\]/);
});

test("report integration remains internal without rendering report controls", () => {
  assert.match(visualsPage, /alreadyAdded/);
  assert.match(visualsPage, /block\.anchor === `visual-\$\{visualId\}`/);
  assert.match(visualsPage, /getRevision\(reportTarget\)/);
  assert.doesNotMatch(visualsPage, /data-testid="visual-add-to-report"/);
  assert.doesNotMatch(visualsPage, /aria-label="Report destination"/);
  assert.doesNotMatch(visualsPage, /decideVisualEvidence/);
});

test("registry rows keep raw identity out of sight while preview actions expose Rename and Archive", () => {
  assert.doesNotMatch(visualsPage, /visuals-card-identity-\$\{visual\.id\}/);
  assert.match(visualsPage, /formatVisualAdmissionIdentity/);
  assert.match(visualsPage, />Rename</);
  assert.match(visualsPage, />Archive</);
  assert.match(visualsPage, /window\.prompt\("Rename visual"/);
  assert.match(visualsPage, /bridges\.visuals\.update\(visual\.id, \{ title \}\)/);
  assert.match(visualsPage, /window\.confirm\(`Archive/);
  assert.match(visualsPage, /bridges\.visuals\.archive\(visual\.id\)/);
});

test("Expand hides library chrome and remains a viewing surface", () => {
	assert.match(visualsPage, /"Expand"/);
	assert.match(visualsPage, /"Show library"/);
	assert.doesNotMatch(visualsPage, /Exit canvas/);
	assert.match(visualsPage, /setFocusVisualId\(focusVisualId \? null : selected\.id\)/);
});

test("preview header omits report references and moves provenance into overflow", () => {
	assert.doesNotMatch(visualsPage, /visuals-preview-report/);
	assert.doesNotMatch(visualsPage, /aria-label="Report placement"/);
	assert.match(visualsPage, /Details &amp; provenance/);
	assert.match(visualsPage, /setPreviewDetailsOpen/);
	assert.match(visualsPage, /previewDetailsOpen \? \(/);
	assert.match(visualsPage, /visuals-preview-actions/);
	assert.match(visualsPage, /statusLabel\(selected\.status\).*rev.*selected\.currentRevision.*visualKindLabel\(selected\)/s);
});

const canvasShell = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../../../visuals/families/analysis/blank.canvas.v1/shell.tsx"),
  "utf8"
);

test("blank canvas empty state explains bind, compose, sourced_visual, and MCP authoring", () => {
  assert.match(canvasShell, /Bind a document input/);
  assert.match(canvasShell, /compose\.visual\.v1/);
  assert.match(canvasShell, /sourced_visual/);
  assert.match(canvasShell, /MCP create\/update/);
  assert.match(canvasShell, /sandboxed HTML\/SVG document/);
  assert.match(canvasShell, /not a drawing editor/);
  assert.doesNotMatch(canvasShell, /No canvas document has been authored yet/);
  assert.doesNotMatch(canvasShell, /contentEditable/);
});

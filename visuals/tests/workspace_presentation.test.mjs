/**
 * Presentation contracts for the optimizer workspaces.
 *
 * Each assertion here corresponds to a defect found in the 2026-09-02 Banking77
 * aesthetic review (docs/qa/BANKING77_VISUAL_SUITE_AESTHETIC_REVIEW_2026-09-02.md):
 * a pinned header that covered its own evidence, a chip wall with no hierarchy,
 * charts drawn from too few points, duplicate candidate labels, repeated
 * paragraph-length absence copy, and a tab badge counting the wrong rows.
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { projectAtCursor } from "../families/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import { sftMissingPrerequisites } from "../families/optimizers/_shared/optimizer.run.v1/overlays/sft/model.ts";
import { candidateLabels } from "../families/optimizers/_shared/optimizer.run.v1/overlays/gepa/model.ts";

const visualsRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => readFileSync(join(visualsRoot, relative), "utf8");

const SHARED = "families/optimizers/_shared/optimizer.run.v1";

test("only the identity line pins; the metric block scrolls with the canvas", () => {
  const css = read("chrome/tokens.css");
  const header = css.slice(css.indexOf(".sv-workspace-header {"), css.indexOf(".sv-workspace-metrics {"));
  assert.doesNotMatch(header.slice(0, header.indexOf("}")), /position: sticky/);
  assert.match(css, /\.sv-workspace-identity \{[\s\S]*?position: sticky/);
});

test("workspace metrics are tiered, and untiered metrics stay visible", () => {
  const chrome = read(`${SHARED}/components/workspace/WorkspaceChrome.tsx`);
  assert.match(chrome, /metric\.tier \?\? "primary"/);
  assert.match(chrome, /metric\.tier === "detail"/);
  assert.match(chrome, /Run details/);
});

test("SFT and CISPO each keep exactly four primary header metrics", () => {
  const workspace = read(`${SHARED}/overlays/sft/SftWorkspace.tsx`);
  // Untiered, the header carried ten chips for SFT and seventeen for CISPO.
  const explicitPrimary = (workspace.match(/tier: "primary"/g) ?? []).length;
  const familyPrimary = (workspace.match(/tier: isCispo \? "detail" : "primary"/g) ?? []).length;
  // CISPO: clip + group size + advantage + heldout uplift.
  assert.equal(explicitPrimary, 4);
  // SFT: heldout uplift + step/epoch + checkpoints + uplift.
  assert.equal(1 + familyPrimary, 4);
});

test("a single metric record reports a stat, not a one-dot chart", () => {
  const workspace = read(`${SHARED}/overlays/sft/SftWorkspace.tsx`);
  assert.match(workspace, /points\.length < 2 \?[\s\S]*?<NotEnoughData/);
});

test("the frontier draws no matrix when there are no example dimensions", () => {
  const frontier = read(`${SHARED}/overlays/gepa/FrontierPanel.tsx`);
  assert.match(frontier, /allExamples\.length === 0 \?[\s\S]*?<NotEnoughData/);
  // The legend and the coverage sentence belong to the matrix, not to the page.
  const emptyBranch = frontier.slice(frontier.indexOf("allExamples.length === 0"), frontier.indexOf(") : ("));
  assert.doesNotMatch(emptyBranch, /Frontier coverage/);
});

test("colliding candidate display names fall back to stable short ids", () => {
  const candidates = [
    { id: "gepa_cand_aaaaaaaa1111", source: "seed" },
    { id: "gepa_cand_bbbbbbbb2222", parentId: null },
    { id: "gepa_cand_cccccccc3333", parentId: "gepa_cand_aaaaaaaa1111", generation: 1, proposal_index: 0 }
  ];
  const labels = candidateLabels(candidates);
  assert.equal(labels.get("gepa_cand_aaaaaaaa1111"), "Seed cand_aaa");
  assert.equal(labels.get("gepa_cand_bbbbbbbb2222"), "Seed cand_bbb");
  assert.notEqual(labels.get("gepa_cand_aaaaaaaa1111"), labels.get("gepa_cand_bbbbbbbb2222"));
  // A name that does not collide is left exactly as it was.
  assert.equal(labels.get("gepa_cand_cccccccc3333")?.startsWith("Gen 1 proposal"), true);
});

test("GEPA setup that has reported nothing collapses instead of repeating 'pending'", () => {
  const overview = read(`${SHARED}/overlays/gepa/SearchOverviewPanel.tsx`);
  assert.match(overview, /const unreported = pendingRows === card\.rows\.length/);
  assert.match(overview, /Not reported yet/);
  // The outcome and the search contract are rendered before the setup grid.
  assert.equal(
    overview.indexOf("gepa-outcome-card") < overview.indexOf("gepa-experiment-context"),
    true
  );
  assert.match(overview, /setup incomplete · \$\{setupPending\}\/\$\{setupRows\.length\} fields pending/);
});

test("missing SFT prerequisites are stated once, in order, with a reason", () => {
  const projected = projectAtCursor(
    { id: "sft_empty", algorithmId: "sft", status: "running" },
    [
      {
        occurredAt: "2026-09-02T00:00:00Z",
        optimizerRunId: "sft_empty",
        algorithmId: "sft",
        sequenceNumber: 1,
        type: "sft.run.created"
      }
    ]
  );
  const missing = sftMissingPrerequisites(projected.sft);
  assert.deepEqual(
    missing.map((item) => item.id),
    ["baseline", "collection", "training", "evaluation", "heldout"]
  );
  assert.equal(missing.every((item) => item.why.length > 0), true);
});

test("panels no longer repeat the long absence paragraphs the checklist owns", () => {
  const workspace = read(`${SHARED}/overlays/sft/SftWorkspace.tsx`);
  assert.match(workspace, /<PrerequisitesPanel missing=\{missingPrerequisites\}/);
  assert.doesNotMatch(workspace, /there is nothing to measure uplift against/);
  assert.doesNotMatch(workspace, /before a dataset can claim provenance/);
});

test("CISPO leads with its own identity, learning signal, and rollout groups", () => {
  const workspace = read(`${SHARED}/overlays/sft/SftWorkspace.tsx`);
  assert.match(workspace, /function CispoLearningSignalPanel/);
  const body = workspace.slice(workspace.indexOf("<StageTimeline stages={stages}"));
  assert.equal(body.indexOf("CispoLearningSignalPanel") < body.indexOf("<CurvesPanel"), true);
  assert.equal(body.indexOf("isCispo ? (\n        <RolloutBrowser") < body.indexOf("<CurvesPanel"), true);
  assert.equal(body.indexOf("<CurvesPanel") < body.lastIndexOf("<BaselinePanel"), true);
});

test("the rollout detail tab counts rubric grades, never annotation findings", () => {
  const shell = read("families/first_class_example_containers/live.annotated_rollouts.v1/shell.tsx");
  assert.match(shell, /function rubricTabLabel/);
  assert.match(shell, /return `Verifier · \$\{findings\} finding/);
  assert.match(shell, /return `Rubric · \$\{grades\.filter\(\(row\) => row\.criteria_met === true\)\.length\}\/\$\{grades\.length\}`/);
  assert.match(shell, /Rubric unavailable/);
  assert.doesNotMatch(shell, /`Rubric · \$\{active\.length\}`/);
});

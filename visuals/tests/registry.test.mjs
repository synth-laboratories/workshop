import assert from "node:assert/strict";
import { readFileSync, existsSync, readdirSync, lstatSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const familiesDir = join(root, "families");

function discoverTemplates(directory = familiesDir, found = new Map()) {
  for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const path = join(directory, entry.name);
    assert.equal(lstatSync(path).isSymbolicLink(), false, `registry must not contain symlink ${path}`);
    if (!entry.isDirectory()) continue;
    const manifest = join(path, "template.json");
    if (existsSync(manifest)) {
      const meta = JSON.parse(readFileSync(manifest, "utf8"));
      assert.equal(meta.id, entry.name, `manifest identity must match directory at ${path}`);
      assert.equal(found.has(meta.id), false, `duplicate ${meta.id}: ${found.get(meta.id)?.path} and ${path}`);
      found.set(meta.id, { meta, path });
    } else {
      discoverTemplates(path, found);
    }
  }
  return found;
}

const EXPECTED_IDS = [
  "analysis.visual.v1",
  "annotation.overlay.v1",
  "blank.canvas.v1",
  "craftax.eval_matrix.v1",
  "craftax.rollout_scrub.v1",
  "diagram.mermaid.v1",
  "diagram.systems.dynamic.v1",
  "diagram.systems.v1",
  "live.container_rollouts.v1",
  "live.craftax.v1",
  "live.digbench.v1",
  "live.eval_stream.v1",
  "live.harbor_eval.v1",
  "live.intern_acceptance.v1",
  "model.compare.v1",
  "optimizer.gepa.candidate.v1",
  "optimizer.gepa.evaluations.v1",
  "optimizer.gepa.frontier.v1",
  "optimizer.gepa.live.v1",
  "optimizer.run.v1",
  "optimizer.sft.checkpoints.v1",
  "optimizer.sft.dataset.v1",
  "optimizer.sft.examples.v1",
  "optimizer.sft.lineage.v1",
  "optimizer.sft.live.v1",
  "optimizer.sft.rollouts.v1",
  "posttrain.rollout_viewer.v1",
  "reward.breakdown.v1",
  "trace.rollout_inspector.v1",
];

test("visuals package exposes the registered templates", () => {
  const templates = discoverTemplates();
  assert.deepEqual([...templates.keys()].sort(), EXPECTED_IDS);
  assert.equal(existsSync(join(root, "templates", "optimizer.dag.live.v1", "template.json")), false, "optimizer.dag.live.v1 is a v0.4 surface");
  for (const id of EXPECTED_IDS) {
    const { meta, path } = templates.get(id);
    assert.equal(meta.id, id);
    assert.equal(meta.schemaVersion, "synth.visual-template.v1");
    if (!id.startsWith("diagram.")) assert.ok(existsSync(join(path, "shell.tsx")));
    if (
      id === "live.harbor_eval.v1" ||
      id === "live.container_rollouts.v1" ||
      id === "live.eval_stream.v1" ||
      id === "live.craftax.v1" ||
      id === "live.digbench.v1"
    ) {
      assert.deepEqual(meta.slots.map((slot) => slot.name), ["stream"]);
    }
    if (id.startsWith("optimizer.")) {
      const slotNames = meta.slots.map((slot) => slot.name);
      assert.deepEqual(slotNames, ["optimizer_run"]);
      assert.ok(!slotNames.includes("live"), `${id} must not bind slot live`);
      assert.ok(!slotNames.includes("jobs"), `${id} must not bind slot jobs`);
      assert.ok(!slotNames.includes("stream"), `${id} must not invent a second stream slot`);
    }
  }
  const mermaidPath = templates.get("diagram.mermaid.v1").path;
  const mermaid = templates.get("diagram.mermaid.v1").meta;
  assert.equal(mermaid.id, "diagram.mermaid.v1");
  assert.equal(mermaid.genre, "diagram");
  assert.equal(mermaid.rendererKind, "mermaid");
  assert.equal(mermaid.slots.length, 0);
  assert.ok(!existsSync(join(mermaidPath, "shell.tsx")));
  assert.ok(!existsSync(join(mermaidPath, "examples")));
  for (const [id, rendererKind] of [
    ["diagram.systems.v1", "systems"],
    ["diagram.systems.dynamic.v1", "systems-dynamic"],
  ]) {
    const { meta, path } = templates.get(id);
    assert.equal(meta.id, id);
    assert.equal(meta.genre, "diagram");
    assert.equal(meta.rendererKind, rendererKind);
    assert.deepEqual(meta.slots, []);
    assert.ok(!existsSync(join(path, "shell.tsx")));
  }
});

test("TypeScript registry is recursive and fails duplicate IDs closed", () => {
  const source = readFileSync(join(root, "registry/index.ts"), "utf8");
  assert.match(source, /families\/\*\*\/template\.json/);
  assert.match(source, /families\/\*\*\/shell\.tsx/);
  assert.match(source, /Duplicate visual template id/);
  assert.match(source, /existing\.manifestPath/);
  assert.match(source, /entry\.manifestPath/);
});

test("eval catalog declares the initial versioned family", () => {
  const catalog = JSON.parse(readFileSync(join(root, "catalog", "evals.v1.json"), "utf8"));
  assert.equal(catalog.schemaVersion, "synth.visual-template-catalog.v1");
  assert.deepEqual(catalog.templates.map((template) => template.id), [
    "eval.overview.v1", "eval.case_table.v1", "eval.model_compare.v1",
    "eval.failure_analysis.v1", "eval.rollout_inspector.v1", "eval.live_run.v1",
    "eval.regression.v1"
  ]);
  for (const template of catalog.templates.filter((template) => template.status === "available")) {
    assert.ok(discoverTemplates().has(template.implementation), template.id);
  }
});

test("fixtures exist for matrix/rollout/live", () => {
  for (const name of [
    "craftax_matrix_slice.json",
    "rollout_steps.json",
    "live_eval_events.json",
    "reward_breakdown.json",
    "model_compare.json",
    "annotation_markers.json",
  ]) {
    assert.ok(existsSync(join(root, "fixtures", name)), name);
  }
});

test("MCP tools schema lists agent entrypoints", () => {
  const tools = JSON.parse(readFileSync(join(root, "mcp/tools.json"), "utf8"));
  const names = (tools.tools || tools).map((t) => t.name);
  for (const required of [
    "visual_list_templates",
    "visual_create_from_template",
    "visual_bind_data_source",
    "visual_open_in_pane",
	"visual_authoring_context",
	"visual_review",
	"visual_mark_ready",
  ]) {
    assert.ok(names.includes(required), required);
  }
});

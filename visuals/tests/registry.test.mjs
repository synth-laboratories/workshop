import assert from "node:assert/strict";
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

test("visuals package exposes nine templates", () => {
  const templatesDir = join(root, "templates");
  const ids = readdirSync(templatesDir).filter((name) =>
    existsSync(join(templatesDir, name, "template.json")),
  );
  assert.equal(ids.length, 9);
  for (const id of [
    "craftax.eval_matrix.v1",
    "craftax.rollout_scrub.v1",
    "posttrain.rollout_viewer.v1",
    "reward.breakdown.v1",
    "annotation.overlay.v1",
    "model.compare.v1",
    "live.eval_stream.v1",
    "live.dock_harbor.v1",
    "live.intern_acceptance.v1",
  ]) {
    assert.ok(ids.includes(id), `missing ${id}`);
    const meta = JSON.parse(
      readFileSync(join(templatesDir, id, "template.json"), "utf8"),
    );
    assert.equal(meta.id, id);
    assert.ok(existsSync(join(templatesDir, id, "shell.tsx")));
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
    "visual_save_tsx",
    "visual_open_in_pane",
    "visual_stream_live_eval",
  ]) {
    assert.ok(names.includes(required), required);
  }
});

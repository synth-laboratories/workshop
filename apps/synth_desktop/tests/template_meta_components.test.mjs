import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const workshopRoot = join(appRoot, "../..");

function read(rel) {
  return readFileSync(join(workshopRoot, rel), "utf8");
}

test("list_templates copies advertised components from template.json", () => {
  const rust = read("apps/synth_desktop/src-tauri/src/visuals/templates.rs");
  assert.match(rust, /pub components: Vec<Value>/);
  assert.match(rust, /\.get\("components"\)/);
  const protocol = read("apps/synth_desktop/src/renderer/src/generated/protocol.ts");
  assert.match(protocol, /export type TemplateMeta = \{[^}]*inputs\?: unknown/s);
  assert.match(protocol, /export type TemplateMeta = \{[^}]*components\?: unknown/s);
  const skill = read("apps/synth_desktop/skills/use-synth-visuals/SKILL.md");
  assert.match(skill, /list_templates[\s\S]*components\[\]/);
  assert.match(skill, /There is no `list_components` verb/);

  const compose = JSON.parse(
    read("visuals/families/analysis/compose.visual.v1/template.json")
  );
  assert.deepEqual(
    compose.components.map((row) => row.id),
    ["event_stream.v1", "detail_modal.v1", "metrics.v1", "scrubber.v1", "candidate_inspector.v1"]
  );
  assert.deepEqual(compose.components[0].consumes.sort(), ["optimizer_run", "stream"]);
  const sourced = JSON.parse(
    read("visuals/families/analysis/sourced.visual.v1/template.json")
  );
  assert.deepEqual(
    sourced.components.map((row) => row.id),
    ["event_stream.v1", "detail_modal.v1"]
  );
  assert.deepEqual(sourced.components[0].consumes, ["stream"]);
});

test("PLUGIN_NAV does not treat Laguna as a plugin row", () => {
  const nav = read("apps/synth_desktop/src/renderer/src/runtime/pluginNav.ts");
  assert.match(nav, /pluginId: "optimizers"/);
  assert.match(nav, /pluginId: "computer-use"/);
  assert.doesNotMatch(nav, /id:\s*"laguna"/);
  assert.doesNotMatch(nav, /pluginId:\s*"laguna"/);
  const pluginsMcp = read("apps/synth_desktop/src-tauri/src/bin/synth_plugins_mcp.rs");
  assert.match(pluginsMcp, /"enum":\["optimizers"\]/);
  assert.doesNotMatch(pluginsMcp, /"enum":\["optimizers","laguna"\]/);
});

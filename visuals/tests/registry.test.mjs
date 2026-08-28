import assert from "node:assert/strict";
import { readFileSync, existsSync, readdirSync, lstatSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const familiesDir = join(root, "families");

function declaredInputs(meta) {
  return meta.inputs ?? meta.slots ?? [];
}

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
  "analysis.chart.v1",
  "analysis.visual.v1",
  "annotation.overlay.v1",
  "blank.canvas.v1",
  "compose.visual.v1",
  "craftax.eval_matrix.v1",
  "craftax.rollout_scrub.v1",
  "craftax.trace_workbench.v1",
  "diagram.mermaid.v1",
  "diagram.systems.dynamic.v1",
  "diagram.systems.v1",
  "experiment.overview.v1",
  "live.container_rollouts.v1",
  "live.craftax.v1",
  "live.eval_stream.v1",
  "live.harbor_eval.v1",
  "live.intern_acceptance.v1",
  "model.compare.v1",
  "optimizer.eval.live.v1",
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
  "sourced.visual.v1",
  "trace.catalog.v1",
  "trace.rollout_inspector.v1",
  "trace.workbench.v1",
];

test("visuals package exposes the registered templates", () => {
  const templates = discoverTemplates();
  assert.deepEqual([...templates.keys()].sort(), EXPECTED_IDS);
  assert.equal(existsSync(join(root, "templates", "optimizer.dag.live.v1", "template.json")), false, "optimizer.dag.live.v1 is a v0.4 surface");
  for (const id of EXPECTED_IDS) {
    const { meta, path } = templates.get(id);
    assert.equal(meta.id, id);
    assert.equal(meta.schemaVersion, "synth.visual-template.v1");
    if (!id.startsWith("diagram.") && meta.rendererKind !== "chart") {
      assert.ok(existsSync(join(path, "shell.tsx")));
    }
    if (id === "live.harbor_eval.v1" || id === "live.container_rollouts.v1") {
      assert.deepEqual(declaredInputs(meta).map((slot) => slot.name), ["stream"]);
    }
    if (id === "live.craftax.v1") {
      assert.deepEqual(declaredInputs(meta).map((slot) => slot.name), ["stream", "optimizer_run"]);
      assert.equal(meta.inputs[0].required, true);
      assert.equal(meta.inputs[1].required, false);
    }
    if (id === "live.eval_stream.v1") {
      assert.equal(meta.slots, undefined);
      assert.deepEqual((meta.inputs ?? []).map((input) => input.name), ["stream"]);
      assert.equal(meta.inputs[0].required, true);
      assert.ok(!(meta.inputs ?? []).some((input) => input.name === "optimizer_run"));
      assert.deepEqual(
        (meta.components ?? []).map((row) => row.id).sort(),
        ["detail_modal.v1", "event_stream.v1", "metrics.v1", "scrubber.v1"]
      );
    }
    if (id === "trace.workbench.v1") {
      // The family-agnostic workstation reads a run like the Craftax one, but
      // must not demand rendered frames: liveFrames-unsupported and post_hoc
      // families can never satisfy a minimum-frame readiness requirement.
      assert.deepEqual(declaredInputs(meta).map((slot) => slot.name), ["optimizer_run"]);
      assert.equal(meta.observationContract.readiness.minimumRenderedFrameCount, undefined);
      assert.equal(meta.observationContract.readiness.minimumRolloutCount, 1);
    }
    if (id === "craftax.trace_workbench.v1") {
      // The workstation replays one container-eval run's relayed trials. It
      // reads the run, not a stream: the frames it shows are host-stored media
      // referenced from that run's events, not bodies fetched from a URL.
      assert.deepEqual(declaredInputs(meta).map((slot) => slot.name), ["optimizer_run"]);
      assert.deepEqual(declaredInputs(meta)[0].accepts, ["optimizer_run"]);
      assert.equal(declaredInputs(meta)[0].required, true);
    }
    if (id === "compose.visual.v1") {
      const declared = declaredInputs(meta);
      assert.deepEqual(declared.map((slot) => slot.name), ["spec", "stream", "optimizer_run"]);
      assert.equal(declared[0].required, true);
      assert.equal(declared[1].required, false);
      assert.equal(declared[2].required, false);
      assert.deepEqual(
        (meta.components ?? []).map((row) => row.id).sort(),
        ["candidate_inspector.v1", "detail_modal.v1", "event_stream.v1", "metrics.v1", "scrubber.v1"]
      );
    }
    if (id === "sourced.visual.v1") {
      const declared = declaredInputs(meta);
      assert.deepEqual(declared.map((slot) => slot.name), ["stream"]);
      assert.equal(declared[0].required, false);
      assert.equal(meta.rendererKind, "tsx");
      assert.deepEqual(
        (meta.components ?? []).map((row) => row.id).sort(),
        ["detail_modal.v1", "event_stream.v1"]
      );
    }
    if (id.startsWith("optimizer.")) {
      const slotNames = declaredInputs(meta).map((slot) => slot.name);
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
  assert.equal(declaredInputs(mermaid).length, 0);
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
    assert.deepEqual(declaredInputs(meta), []);
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

test("catalog is bundled tiers union runtime user templates, and runtime never shadows", () => {
  const source = readFileSync(join(root, "registry/index.ts"), "utf8");
  // The union itself: list and resolve both read the runtime map.
  assert.match(source, /\[\.\.\.ORDERED_ENTRIES, \.\.\.RUNTIME_BY_ID\.values\(\)\]/);
  // Bundled first on resolve, so a runtime id can add but never redefine.
  assert.match(source, /BY_ID\.get\(id\) \?\? RUNTIME_BY_ID\.get\(id\)/);
  // No-shadow on the way in: a bundled id is refused and reported, not applied.
  assert.match(source, /if \(BY_ID\.has\(id\) \|\| next\.has\(id\)\) \{\n      shadowed\.push\(id\);/);
  // Only user-authored rows join the runtime tier; a bundled row served by the
  // host must not be re-registered without its static shell importer.
  assert.match(source, /record\.sourceKind !== USER_TEMPLATE_SOURCE_KIND\) continue;/);
  // getShellImporter stays bundled-only.
  assert.match(source, /export function getShellImporter\(id: string\) \{\n  return shellImporters\[id\];\n\}/);
});

test("the pane branches on source kind, not on one template id", () => {
  const host = readFileSync(
    join(root, "..", "apps", "synth_desktop", "src", "renderer", "src", "components", "VisualHost.tsx"),
    "utf8",
  );
  assert.match(host, /const userAuthored = isUserTemplate\(templateId\);/);
  assert.match(host, /if \(isSourcedTemplate\(templateId\) \|\| userAuthored\) \{/);
  assert.match(host, /visuals\.templateShellSource\(templateId\)/);
  // Every failure on this path renders in the pane with the validator's words.
  assert.match(host, /setShell\(\(\) => sourcedInvalidShell\(compiled\.error\)\)/);
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
  assert.ok(!names.includes("visual_list_components"));
  assert.ok(!names.includes("list_components"));
  assert.ok(!names.includes("reports_promote"));
});

test("an edit on disk reaches the pane without a rebuild", () => {
  const bridge = readFileSync(
    join(root, "..", "apps", "synth_desktop", "src", "renderer", "src", "runtime", "desktopBridge.ts"),
    "utf8",
  );
  // The host says the root moved; the renderer answers by re-asking, never by
  // reading the event payload.
  assert.match(bridge, /onTemplatesChanged\?\.\(\(\) => \{ void refreshRuntimeTemplates\(\); \}\)/);
  assert.match(bridge, /listen\(EVENT_CHANNELS\.VISUAL_TEMPLATES/);
  // Whatever the watcher missed while the window was in the background --
  // quietly, so an alt-tab does not remount every open pane.
  assert.match(bridge, /addEventListener\("focus", \(\) => \{ void rescanRuntimeTemplates\(\); \}\)/);
  const source = readFileSync(join(root, "registry/index.ts"), "utf8");
  assert.match(source, /export function rescanRuntimeTemplates\(\)/);
  assert.match(source, /if \(options\.quiet && sameSnapshot\(snapshot, runtimeSnapshot\)\)/);

  const watcher = readFileSync(
    join(root, "..", "apps", "synth_desktop", "src-tauri", "src", "visuals", "user_templates.rs"),
    "utf8",
  );
  // Fingerprint the root rather than trusting mtime on the directory alone,
  // and stat without following links so a swap registers as a change.
  assert.match(watcher, /fn root_fingerprint\(\) -> String/);
  assert.match(watcher, /symlink_metadata\(&path\)/);
  assert.match(watcher, /EventChannel::VISUAL_TEMPLATES/);
});

test("a user template that disappears explains itself in the pane", () => {
  const source = readFileSync(join(root, "registry/index.ts"), "utf8");
  assert.match(source, /export function wasUserTemplate\(id: string\): boolean/);
  const host = readFileSync(
    join(root, "..", "apps", "synth_desktop", "src", "renderer", "src", "components", "VisualHost.tsx"),
    "utf8",
  );
  // Not `setFailed`, which blanks: the same in-pane surface every other
  // user-template failure uses.
  assert.match(host, /if \(wasUserTemplate\(templateId\)\) \{[\s\S]*?sourcedInvalidShell\(/);
});

test("authoring writes are verified by the registry, not by a second copy of its rules", () => {
  const writer = readFileSync(
    join(root, "..", "apps", "synth_desktop", "src-tauri", "src", "visuals", "user_templates.rs"),
    "utf8",
  );
  // Write, then ask the reader whether what was written is a template.
  assert.match(writer, /fn write_verified\(/);
  assert.match(writer, /super::templates::resolve_template\(id\)/);
  assert.match(writer, /restore\.restore\(\);/);
  // The root is never re-derived; that is item 23's bug and conform counts it.
  assert.match(writer, /super::templates::user_templates_root\(\)/);
  assert.doesNotMatch(writer.replace(/#\[cfg\(test\)\][\s\S]*$/, ""), /\.join\("visuals"\)/);
  // And the import allowlist is not reimplemented here.
  assert.doesNotMatch(writer, /SOURCED_ALLOWED_IMPORTS|"react\/jsx-runtime"/);
  assert.match(writer, /sourcedValidate\.ts/);
});

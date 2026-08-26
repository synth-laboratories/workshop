import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { COMPOSE_COMPONENTS, parseComposeSpec } from "../runtime/composeSpec.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const template = JSON.parse(
  readFileSync(join(root, "families/analysis/compose.visual.v1/template.json"), "utf8")
);

const validSpec = {
  schemaVersion: "synth.visual.compose_spec.v1",
  title: "Harbor smoke · live stream",
  placements: [
    {
      id: "log",
      component: "event_stream.v1",
      slot: "stream",
      config: { includeKinds: ["rollout.finished"] }
    },
    { id: "inspect", component: "detail_modal.v1", from: "log" }
  ]
};

test("compose.visual.v1 advertises components on the template, not as templates", () => {
  assert.equal(template.id, "compose.visual.v1");
  assert.deepEqual(template.slots.map((slot) => slot.name), ["spec", "stream", "optimizer_run"]);
  assert.deepEqual(template.inputs.map((input) => input.name), ["spec", "stream", "optimizer_run"]);
  assert.equal(template.slots[0].required, true);
  assert.equal(template.slots[1].required, false);
  assert.equal(template.slots[2].required, false);
  assert.deepEqual(template.slots[2].accepts, ["optimizer_run", "fixture", "inline"]);
  assert.deepEqual(
    template.components.map((row) => row.id).sort(),
    Object.keys(COMPOSE_COMPONENTS).sort()
  );
  assert.deepEqual(COMPOSE_COMPONENTS["event_stream.v1"].consumes, ["stream", "optimizer_run"]);
  assert.deepEqual(COMPOSE_COMPONENTS["metrics.v1"].consumes, ["stream", "optimizer_run"]);
  assert.equal(COMPOSE_COMPONENTS["metrics.v1"].protocolId, "metrics.reduce.v1");
  assert.deepEqual(COMPOSE_COMPONENTS["scrubber.v1"].emits, ["cursor"]);
  assert.deepEqual(COMPOSE_COMPONENTS["candidate_inspector.v1"].consumes, ["optimizer_run"]);
  assert.equal(existsSync(join(root, "components/event_stream.v1/template.json")), false);
  assert.equal(existsSync(join(root, "components/detail_modal.v1/template.json")), false);
  assert.equal(existsSync(join(root, "components/metrics.v1/template.json")), false);
  assert.equal(existsSync(join(root, "components/scrubber.v1/template.json")), false);
  assert.equal(existsSync(join(root, "components/candidate_inspector.v1/template.json")), false);
});

test("parseComposeSpec accepts the advertised event_stream + detail_modal pair", () => {
  const parsed = parseComposeSpec(validSpec);
  assert.equal(parsed.ok, true);
  assert.deepEqual(parsed.spec.placements.map((row) => row.id), ["log", "inspect"]);
});

test("unknown compose component ids fail closed", () => {
  const parsed = parseComposeSpec({
    ...validSpec,
    placements: [{ id: "log", component: "not.a.thing.v1", slot: "stream" }]
  });
  assert.equal(parsed.ok, false);
  assert.match(parsed.error, /Unknown compose component "not\.a\.thing\.v1"/);
});

test("detail_modal requires from that emits a cursor", () => {
  const missing = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    placements: [{ id: "inspect", component: "detail_modal.v1" }]
  });
  assert.equal(missing.ok, false);
  assert.match(missing.error, /requires from/);

  const dangling = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    placements: [
      { id: "log", component: "event_stream.v1", slot: "stream" },
      { id: "inspect", component: "detail_modal.v1", from: "missing" }
    ]
  });
  assert.equal(dangling.ok, false);
  assert.match(dangling.error, /does not exist/);
});

test("duplicate placement ids fail closed", () => {
  const parsed = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    placements: [
      { id: "log", component: "event_stream.v1", slot: "stream" },
      { id: "log", component: "detail_modal.v1", from: "log" }
    ]
  });
  assert.equal(parsed.ok, false);
  assert.match(parsed.error, /Duplicate placement id "log"/);
});

test("event_stream may consume optimizer_run instead of stream", () => {
  const parsed = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    title: "CISPO clip · optimizer_run",
    placements: [
      {
        id: "log",
        component: "event_stream.v1",
        input: "optimizer_run",
        config: { includeKinds: ["candidate.accepted", "cispo.clip.identity"] }
      },
      { id: "inspect", component: "detail_modal.v1", from: "log" }
    ]
  });
  assert.equal(parsed.ok, true);
  assert.equal(parsed.spec.placements[0].input, "optimizer_run");
  assert.equal(parsed.spec.placements[0].slot, "optimizer_run");
});

test("placement slot is accepted as a one-release alias of input", () => {
  const parsed = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    placements: [{ id: "log", component: "event_stream.v1", slot: "optimizer_run" }]
  });
  assert.equal(parsed.ok, true);
  assert.equal(parsed.spec.placements[0].input, "optimizer_run");
});

test("placement input and slot disagree fail closed", () => {
  const parsed = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    placements: [{ id: "log", component: "event_stream.v1", input: "stream", slot: "optimizer_run" }]
  });
  assert.equal(parsed.ok, false);
  assert.match(parsed.error, /disagree/);
});

test("unknown event_stream slot fails closed", () => {
  const parsed = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    placements: [{ id: "log", component: "event_stream.v1", slot: "jobs" }]
  });
  assert.equal(parsed.ok, false);
  assert.match(parsed.error, /must consume input "stream" or "optimizer_run"/);
});

test("optimizer_run example binds the optimizer dialect, not stream", () => {
  const example = JSON.parse(
    readFileSync(join(root, "families/analysis/compose.visual.v1/examples/optimizer_run_binding.json"), "utf8")
  );
  const spec = (example.inputs ?? example.slots).find((row) => (row.input ?? row.slot) === "spec").data;
  const parsed = parseComposeSpec(spec);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.spec.placements[0].input, "optimizer_run");
  assert.equal((example.inputs ?? example.slots).some((row) => (row.input ?? row.slot) === "stream"), false);
  assert.deepEqual(
    example.slots.find((slot) => (slot.input ?? slot.slot) === "optimizer_run").data.events.map((event) => event.type),
    ["optimizer.visual.ready", "candidate.accepted", "sft.training.metrics", "cispo.clip.identity"]
  );
});

test("metrics and scrubber consume stream or optimizer_run; candidate_inspector is optimizer_run only", () => {
  const streamOk = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    placements: [
      { id: "strip", component: "metrics.v1", input: "stream" },
      { id: "playhead", component: "scrubber.v1", input: "stream" },
      { id: "inspect", component: "detail_modal.v1", from: "playhead" }
    ]
  });
  assert.equal(streamOk.ok, true);

  const optimizerOk = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    placements: [
      { id: "strip", component: "metrics.v1", input: "optimizer_run" },
      { id: "candidates", component: "candidate_inspector.v1", input: "optimizer_run" },
      { id: "inspect", component: "detail_modal.v1", from: "candidates" }
    ]
  });
  assert.equal(optimizerOk.ok, true);

  const inspectorOnStream = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    placements: [{ id: "candidates", component: "candidate_inspector.v1", input: "stream" }]
  });
  assert.equal(inspectorOnStream.ok, false);
  assert.match(inspectorOnStream.error, /must consume input "optimizer_run"/);

  const metricsJobs = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    placements: [{ id: "strip", component: "metrics.v1", input: "jobs" }]
  });
  assert.equal(metricsJobs.ok, false);
  assert.match(metricsJobs.error, /must consume input "stream" or "optimizer_run"/);

  const modalFromMetrics = parseComposeSpec({
    schemaVersion: "synth.visual.compose_spec.v1",
    placements: [
      { id: "strip", component: "metrics.v1", input: "stream" },
      { id: "inspect", component: "detail_modal.v1", from: "strip" }
    ]
  });
  assert.equal(modalFromMetrics.ok, false);
  assert.match(modalFromMetrics.error, /does not emit a cursor/);
});

test("later components example uses input and advertises the kit", () => {
  const example = JSON.parse(
    readFileSync(join(root, "families/analysis/compose.visual.v1/examples/later_components_binding.json"), "utf8")
  );
  const spec = example.inputs.find((row) => row.input === "spec").data;
  assert.equal(spec.placements.every((row) => row.slot == null || row.input != null), true);
  const parsed = parseComposeSpec(spec);
  assert.equal(parsed.ok, true);
  assert.deepEqual(
    parsed.spec.placements.map((row) => row.component),
    ["metrics.v1", "scrubber.v1", "candidate_inspector.v1", "detail_modal.v1"]
  );
  assert.equal(parsed.spec.placements[0].input, "stream");
  assert.equal(parsed.spec.placements[2].input, "optimizer_run");
});

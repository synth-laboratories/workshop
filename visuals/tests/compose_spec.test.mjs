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
  assert.deepEqual(template.slots.map((slot) => slot.name), ["spec", "stream"]);
  assert.equal(template.slots[0].required, true);
  assert.equal(template.slots[1].required, false);
  assert.deepEqual(
    template.components.map((row) => row.id).sort(),
    Object.keys(COMPOSE_COMPONENTS).sort()
  );
  assert.equal(existsSync(join(root, "components/event_stream.v1/template.json")), false);
  assert.equal(existsSync(join(root, "components/detail_modal.v1/template.json")), false);
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

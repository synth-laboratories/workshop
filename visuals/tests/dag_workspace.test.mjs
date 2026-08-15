import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { projectAtCursor } from "../families/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import {
  dagStages,
  formatKnownSpend,
  formatNodeCost
} from "../families/optimizers/_shared/optimizer.run.v1/overlays/dag/model.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE = JSON.parse(
  readFileSync(join(root, "tests/fixtures/optimizer.dag.events.json"), "utf8")
);
const RUN = FIXTURE.run;
const base = { occurredAt: "2026-08-14T18:00:00Z", optimizerRunId: "opt_dag_cost", algorithmId: "dag" };

test("fixture projects source running, behavior sealed unmetered, known spend is source 4.10", () => {
  const projected = projectAtCursor(RUN, FIXTURE.events);
  assert.ok(projected.dag, "dag slice must be present");
  const byId = Object.fromEntries(projected.dag.nodes.map((node) => [node.id, node]));
  assert.equal(byId.source.status, "running");
  assert.equal(byId.source.partitionsSealed, 128);
  assert.equal(byId.source.partitionsTotal, 160);
  assert.equal(byId.source.costUsd, 4.1);
  assert.equal(byId.behavior.status, "sealed");
  assert.equal(byId.behavior.unmetered, true);
  assert.equal(byId.behavior.costUsd, null, "unmetered cost stays null, not 0");
  assert.equal(formatNodeCost(byId.behavior), "—");
  assert.equal(projected.dag.knownCostUsd, 4.1);
  assert.ok(projected.dag.unmeteredCount >= 1);
  assert.equal(projected.dag.missingMeterCount, 0);
  assert.equal(formatKnownSpend(projected.dag), "$4.10 · 4 unmetered");
  assert.equal(projected.usage.costUsd, 4.1);
});

test("sealed metered node missing cost nulls knownCostUsd and counts missingMeterCount", () => {
  const events = [
    { ...base, sequenceNumber: 1, type: "node.started", item: { id: "source", status: "running" }, delta: { node: "source" }, usageDelta: { cost_usd: 4.1 } },
    { ...base, sequenceNumber: 2, type: "node.sealed", item: { id: "compile", status: "sealed" }, delta: { node: "compile", unmetered: false } }
  ];
  const projected = projectAtCursor({ id: "opt_dag_cost", algorithmId: "dag", status: "running" }, events);
  assert.ok(projected.dag);
  const compile = projected.dag.nodes.find((node) => node.id === "compile");
  assert.equal(compile.costUsd, null, "missing cost stays null, not 0");
  assert.equal(projected.dag.knownCostUsd, null, "headline must not invent a total");
  assert.ok(projected.dag.missingMeterCount >= 1);
  assert.match(formatKnownSpend(projected.dag), /^known \$4\.10 · 1 missing$/);
});

test("dagStages: running is active, sealed is completed, planned is pending", () => {
  const projected = projectAtCursor(RUN, FIXTURE.events);
  const stages = dagStages(projected.dag, "running");
  const byId = Object.fromEntries(stages.map((stage) => [stage.id, stage]));
  assert.equal(byId.source.status, "active");
  assert.equal(byId.behavior.status, "completed");
  assert.equal(byId.select.status, "pending");
  assert.equal(byId.compile.status, "pending");
  assert.equal(byId.teacher_gate.status, "completed");
  assert.equal(byId.train.status, "active");
});

test("algorithmId dag.craftax_annotated_fbc still fills projected.dag", () => {
  const projected = projectAtCursor(
    { ...RUN, algorithmId: "dag.craftax_annotated_fbc" },
    FIXTURE.events
  );
  assert.ok(projected.dag, "prefixed dag.* algorithmId must project the dag slice");
  assert.ok(projected.dag.nodes.length >= 8);
  assert.equal(projected.sft, undefined);
  assert.equal(projected.gepa, undefined);
});

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const source = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/components/TrainingWorkspace.tsx"),
  "utf8"
);

test("training placements are projected from the Optimizers recipe catalog", () => {
  assert.match(source, /bridges\.optimizers\?\.listRecipes\(\)/);
  assert.match(source, /recipe\.availability === "available"/);
  assert.match(source, /localAvailable \? <option value="mlx">/);
  assert.match(source, /hostedAvailable \? <option value="tinker">/);
  assert.match(source, /data-testid="training-recipe-unavailable"/);
});

test("start rechecks recipe admission instead of trusting a selected label", () => {
  assert.match(source, /selectedRecipe\?\.availability !== "available"/);
  assert.match(source, /disabled=\{!selectedPlacementAvailable \|\| \(placement === "mlx" && !targets\.length\)/);
  assert.match(source, /if \(placement === "mlx" && !targetId\)/);
  assert.match(source, /algorithm === "cispo" && placement === "mlx" && !parentArtifact/);
  assert.match(source, /\.\.\.\(placement === "mlx" \? \{ containerId: targetId \} : \{\}\)/);
  assert.match(source, /cispo\.banking77\.tinker\.v1/);
  assert.match(source, /cispo\.hosted\.tinker\.v1/);
});

test("the training workspace reads the shared run read model and never polls an event prefix", () => {
  assert.match(source, /useOptimizerRun\(durableRunId\)/);
  assert.match(source, /useRunCollection\(durableRunId, "evaluations", \{ limit: 100/);
  assert.equal(source.includes(".eventsAfter("), false, "no eventsAfter(run.id, 0, 2000) prefix read");
  assert.equal(source.includes("setInterval("), false, "no one-second poll timer");
  assert.equal(source.includes("bridge.refresh("), false, "status comes from the durable summary, not a refresh loop");
  assert.match(source, /data-testid="training-evaluations-stale"/, "stale evaluations stay visible with a marker");
});

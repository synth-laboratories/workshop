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
  assert.match(source, /disabled=\{!selectedPlacementAvailable \|\| !targets\.length/);
});

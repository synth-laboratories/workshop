import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const source = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/components/OptimizersPage.tsx"),
  "utf8"
);

test("hosted CISPO form is gated by the runtime recipe admission", () => {
  assert.match(source, /recipe\.id === "cispo\.slime\.hosted\.v1"/);
  assert.match(source, /hostedCispoRecipe\?\.availability === "available"/);
  assert.match(source, /hostedCispoAdmitted \? <><div className="optimizer-training-form">/);
  assert.match(source, /data-testid="hosted-cispo-not-admitted"/);
});

test("the unavailable state names the real blocker and offers local CISPO", () => {
	assert.match(source, /Hosted slime CISPO has not passed runtime admission\./);
	assert.match(source, /Adding a model or SFT checkpoint will not unlock hosted CISPO\./);
	assert.match(source, /localCispoRecipe\?\.availability === "available"/);
	assert.match(source, /startBoundedRecipe\("cispo\.mlx\.v1"/);
	assert.match(source, /Run CISPO on this Mac/);
  assert.match(source, /data-testid="local-cispo-not-available"/);
});

test("checkpoint cards do not offer hosted CISPO while its recipe is unavailable", () => {
  assert.match(source, /hostedCispoAdmitted && hostedSftWarmStarts\.some/);
  assert.match(source, /Use for hosted CISPO/);
});

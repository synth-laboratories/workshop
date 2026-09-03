import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const source = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/components/OptimizersPage.tsx"),
  "utf8"
);

test("hosted CISPO is a one-click admitted recipe, not a container form", () => {
  assert.match(source, /cispo\.banking77\.tinker\.v1/);
  assert.match(source, /cispo\.hosted\.tinker\.v1/);
  assert.match(source, /findHostedCispoRecipe/);
  assert.match(source, /hostedCispoRecipe\?\.availability === "available"/);
  assert.match(source, /data-testid="start-cispo-hosted"/);
  assert.match(source, /data-testid="hosted-cispo-not-admitted"/);
  assert.doesNotMatch(source, /data-testid="hosted-cispo-warm-start"/);
  assert.doesNotMatch(source, /Local Container URL/);
  assert.doesNotMatch(source, /data-testid="review-hosted-training-launch"/);
});

test("the unavailable state names the real blocker and offers local CISPO", () => {
	assert.match(source, /Hosted Tinker CISPO has not passed runtime admission\./);
	assert.match(source, /will not unlock hosted CISPO/);
	assert.match(source, /localCispoRecipe\?\.availability === "available"/);
	assert.match(source, /LOCAL_CISPO_RECIPE_ID/);
	assert.match(source, /Run CISPO on this Mac/);
  assert.match(source, /data-testid="local-cispo-not-available"/);
  assert.match(source, /data-testid="start-cispo-mlx"/);
});

test("SFT and CISPO cards stay on the launch grid with This Mac and hosted buttons", () => {
  assert.doesNotMatch(source, /OPTIMIZER_GUIDES\.filter\(\(guide\) => guide\.id !== "sft"/);
  assert.match(source, /data-testid="start-sft-mlx"/);
  assert.match(source, /data-testid="start-sft-hosted"/);
  assert.match(source, /data-testid="start-cispo-mlx"/);
  assert.match(source, /HOSTED_SFT_RECIPE_ID/);
});

test("checkpoint cards do not send people through a hosted CISPO container form", () => {
  assert.doesNotMatch(source, /Use for hosted CISPO/);
  assert.doesNotMatch(source, /use-for-cispo-/);
});

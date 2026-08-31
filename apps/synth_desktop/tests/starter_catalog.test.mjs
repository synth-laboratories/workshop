import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const source = join(appRoot, "src/renderer/src/runtime/starterCatalog.ts");
const compiled = join(compiledDir, "starterCatalog.mjs");
writeFileSync(compiled, transformSync(readFileSync(source, "utf8"), {
	loader: "ts",
	format: "esm",
	target: "es2022",
	sourcefile: source
}).code);

const { WORKSHOP_STARTERS, starterPromptForRecipe, workshopStarter } = await import(pathToFileURL(compiled).href);

test("the first-run catalog pins Craftax and NanoHorizon recipe identities", () => {
	assert.deepEqual(WORKSHOP_STARTERS.map((starter) => starter.recipeId), [
		"eval.craftax.code-policy.smoke.v1",
		"nanohorizon.craftax.glm-5.3-flash.eval.v1"
	]);
	assert.equal(workshopStarter("nanohorizon-craftax").maxCostUsd, 2.45);
	assert.equal(workshopStarter("unsupported"), null);
});

test("starter prompts require preflight and approval instead of auto-running", () => {
	for (const starter of WORKSHOP_STARTERS) {
		assert.match(starter.prompt, /Do not start compute yet/);
		assert.match(starter.prompt, /explicit approval/);
		assert.match(starter.prompt, new RegExp(starter.recipeId.replaceAll(".", "\\.")));
	}
	assert.match(workshopStarter("nanohorizon-craftax").prompt, /github\.com\/synth-laboratories\/nanohorizon/);
	assert.match(workshopStarter("nanohorizon-craftax").prompt, /thinking budget 640/);
	assert.match(workshopStarter("nanohorizon-craftax").prompt, /780000 through 780004/);
	assert.match(workshopStarter("nanohorizon-craftax").prompt, /stop for my explicit approval before provider use or any run/);
});

test("only the exact referred recipe receives the bounded starter prompt", () => {
	const starter = workshopStarter("nanohorizon-craftax");
	assert.equal(
		starterPromptForRecipe(starter, "nanohorizon.craftax.glm-5.3-flash.eval.v1", "fallback"),
		starter.prompt
	);
	assert.equal(starterPromptForRecipe(starter, "another.recipe", "fallback"), "fallback");
});

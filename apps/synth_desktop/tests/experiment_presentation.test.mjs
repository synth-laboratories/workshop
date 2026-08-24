import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const source = join(appRoot, "src/renderer/src/runtime/experimentPresentation.ts");
const compiled = join(compiledDir, "experimentPresentation.mjs");
writeFileSync(compiled, transformSync(readFileSync(source, "utf8"), { loader: "ts", format: "esm", target: "es2022", sourcefile: source }).code);
const { formatExperimentResult } = await import(pathToFileURL(compiled).href);

test("experiment index summarizes baseline, variant, and zero delta without JSON", () => {
	assert.equal(formatExperimentResult({ baseline: { reward: 0.5 }, variant: { reward: 0.5 }, reward_delta: 0 }), "0.5 → 0.5 · Δ 0");
});

test("experiment result presentation keeps missing distinct from zero", () => {
	assert.equal(formatExperimentResult({ baseline: { reward: 0 }, variant: { reward: null }, uplift: null }), "0 → —");
	assert.equal(formatExperimentResult(null), "—");
});

test("unknown structured results receive an honest compact fallback", () => {
	assert.equal(formatExperimentResult({ verdict: "insufficient evidence" }), "insufficient evidence");
	assert.equal(formatExperimentResult({ nested: { opaque: true } }), "Result recorded");
});

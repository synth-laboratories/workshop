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
const { formatExperimentResult, formatNodeFailureReason } = await import(pathToFileURL(compiled).href);
const indexSource = readFileSync(join(appRoot, "src/renderer/src/experiments/ExperimentIndex.tsx"), "utf8");
const appCss = readFileSync(join(appRoot, "src/renderer/src/styles/app.css"), "utf8");

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

test("failed nodes surface a one-line reason or Reason unavailable", () => {
	assert.equal(formatNodeFailureReason({ status: "completed", provenance: { error: "hidden" } }), null);
	assert.equal(formatNodeFailureReason({ status: "failed", provenance: { error: "gold env refused reset" } }), "gold env refused reset");
	assert.equal(formatNodeFailureReason({ status: "failed", provenance: { assessment: { reason: "timeout" } } }), "timeout");
	assert.equal(formatNodeFailureReason({ status: "failed", provenance: { terminalReceipt: "exit 1: policy crash" } }), "exit 1: policy crash");
	assert.equal(formatNodeFailureReason({ status: "failed", provenance: {} }), "Reason unavailable");
});

test("compact experiment index keeps status, result, runs, and timestamp without invented member kinds", () => {
	assert.match(indexSource, /formatExperimentResult\(row\.bestResult\)/);
	assert.match(indexSource, /row\.members\.length/);
	assert.match(indexSource, /experiment-col-updated/);
	assert.match(indexSource, /className="experiment-task-disclosure"/);
	assert.match(indexSource, /<summary>Task<\/summary>/);
	assert.match(indexSource, /rows\.map\(\(row\) =>/);
	assert.doesNotMatch(indexSource, /memberKind\s*===?\s*["'](baseline|variant|result)["']/);
	assert.doesNotMatch(indexSource, /kind\s*===?\s*["'](baseline|variant|result)["']/);
	assert.match(appCss, /html\.compact-workbench \.experiment-row:not\(\.heading\)/);
	assert.match(appCss, /html\.compact-workbench \.experiment-task-disclosure/);
	assert.match(appCss, /grid-template-areas:[\s\S]*status result runs updated/);
	assert.doesNotMatch(appCss, /\.experiment-row > :nth-child\(2\),\s*\.experiment-row > :nth-child\(4\),\s*\.experiment-row > :nth-child\(6\)\s*\{\s*display:\s*none/);
});

import assert from "node:assert/strict";
import { buildSync } from "esbuild";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const compiled = join(compiledDir, "starterResult.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/starterResult.ts")],
	outfile: compiled,
	bundle: true,
	format: "esm",
	platform: "node",
	target: "node22"
});

const { matchingStarterRun, projectStarterResult } = await import(`${pathToFileURL(compiled).href}?v=${Date.now()}`);

function run(overrides = {}) {
	return {
		schemaVersion: "optimizer.run.v1",
		id: "run-1",
		algorithmId: "eval",
		status: "completed",
		source: "local",
		createdAt: "2026-08-30T00:00:00Z",
		summary: { recipeId: "nanohorizon.craftax.glm-5.3-flash.eval.v1" },
		usage: {},
		visualRefs: [{ kind: "visual", id: "visual-1" }],
		...overrides
	};
}

function aggregate(overrides = {}) {
	return {
		schemaVersion: "eval.aggregate.v1",
		runId: "run-1",
		asOfSequence: 9,
		projectionRevision: 2,
		lifecycle: "terminal",
		work: { planned: 5, succeeded: 5 },
		evidence: { completeness: "complete", refs: [{ kind: "trace", id: "trace-1" }] },
		selection: "promotion_not_applicable",
		meanReward: 0.42,
		scoredTrials: 5,
		evaluatorEvidence: 5,
		traceCount: 5,
		evidenceRefCount: 1,
		...overrides
	};
}

test("completed requires an exact starter recipe, finite metric, and inspectable complete evidence", () => {
	const result = projectStarterResult(run(), aggregate());
	assert.equal(result.state, "completed");
	assert.deepEqual(result.headlineMetric, { label: "Mean reward", value: 0.42 });
	assert.equal(result.usage.costUsd, null);
	assert.match(result.usage.reason, /never reported as zero/);
	assert.deepEqual(result.comparison, {
		baseline: null,
		candidate: null,
		delta: null,
		reason: "Authoritative evaluation aggregate is missing: baseline, candidate, delta. No values were inferred."
	});
});

test("comparison uses only producer aggregate fields and labels partial evidence", () => {
	const result = projectStarterResult(run(), aggregate({ comparison: { baseline: 0.3, candidate: 0.42, delta: 0.12 } }));
	assert.deepEqual(result.comparison, {
		baseline: 0.3, candidate: 0.42, delta: 0.12,
		reason: "Producer-recorded comparison from the authoritative evaluation aggregate."
	});
	const partial = projectStarterResult(run(), aggregate({ baselineReward: 0.3 }));
	assert.equal(partial.comparison.baseline, 0.3);
	assert.match(partial.comparison.reason, /candidate, delta/);
});

test("agent starter binding chooses the first new exact-recipe run", () => {
	const old = run({ id: "old", createdAt: "2026-08-29T23:59:59Z" });
	const unrelated = run({ id: "other", createdAt: "2026-08-30T00:00:02Z", summary: { recipeId: "other" } });
	const match = run({ id: "new", createdAt: "2026-08-30T00:00:01Z" });
	assert.equal(matchingStarterRun([unrelated, match, old], {
		recipeId: "nanohorizon.craftax.glm-5.3-flash.eval.v1",
		notBefore: "2026-08-30T00:00:00Z"
	}).id, "new");
});

test("unknown recipes and nonterminal runs do not project as starter results", () => {
	assert.equal(projectStarterResult(run({ summary: { recipeId: "other" } }), aggregate()), null);
	assert.equal(projectStarterResult(run({
		inputRefs: [{ kind: "recipe", id: "banking77.distilbert.eval.v1" }]
	}), aggregate()), null);
	assert.equal(projectStarterResult(run({ status: "running" }), aggregate({ lifecycle: "running" })), null);
});

test("completed compute is inconclusive when metric or evidence is incomplete", () => {
	assert.equal(projectStarterResult(run(), aggregate({ meanReward: null })).state, "inconclusive");
	assert.match(projectStarterResult(run(), aggregate({ meanReward: null })).reason, /without a valid headline metric/);
	assert.equal(projectStarterResult(run(), aggregate({ evidence: { completeness: "partial", reason: "one trace missing" } })).state, "inconclusive");
	assert.match(projectStarterResult(run(), aggregate({ evidence: { completeness: "partial", reason: "one trace missing" } })).reason, /one trace missing/);
	assert.equal(projectStarterResult(run({ visualRefs: [] }), aggregate({ evidence: { completeness: "complete", refs: [] } })).state, "inconclusive");
});

test("terminal failures and cancellation remain distinct and preserve evidence", () => {
	const failed = projectStarterResult(run({ status: "failed" }), aggregate({ evidence: { completeness: "partial", reason: "evaluator failed", refs: [{ kind: "trace", id: "trace-1" }] } }));
	assert.equal(failed.state, "failed");
	assert.match(failed.reason, /evaluator failed/);
	assert.equal(failed.evidence.references[0].id, "trace-1");
	assert.equal(projectStarterResult(run({ status: "cancelled" }), aggregate()).state, "cancelled");
	assert.equal(projectStarterResult(run({ status: "failed_evidence" }), aggregate()).state, "inconclusive");
});

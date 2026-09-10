/**
 * Readiness counts come from the durable run view, not from how many raw
 * events the renderer happened to hydrate.
 */
import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const outfile = join(compiledDir, "semanticCounts.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/runProgress/semanticCounts.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile
});
const { semanticCountsFromRunView } = await import(pathToFileURL(outfile).href);

const header = (work = {}) => ({ work: { planned: null, queued: null, running: null, succeeded: null, failed: null, cancelled: null, ...work } });

test("a projection-only GEPA visual reports its candidates and scored rollouts with zero raw events", () => {
	const view = {
		algorithm: "gepa",
		header: header({ succeeded: 1080 }),
		projection: {
			candidateOrder: Array.from({ length: 10 }, (_, index) => `cand_${index}`),
			candidates: {},
			evaluations: Array.from({ length: 1080 }, (_, index) => ({ id: `eval_${index}` })),
			proposerCalls: [{ generation: 0 }, { generation: 1 }],
			frontierHistory: ["cand_0", "cand_4"],
			rolloutsScored: 1080,
			rolloutsFailed: 0
		}
	};
	const counts = semanticCountsFromRunView(view, 0);
	assert.equal(counts.source, "projection");
	assert.equal(counts.rollouts, 1080);
	assert.equal(counts.semanticEvents, 10 + 1080 + 2 + 2);
});

test("without a projection the raw event count is the floor, and it is labelled as such", () => {
	assert.deepEqual(semanticCountsFromRunView(undefined, 17), { semanticEvents: 17, rollouts: 17, source: "raw" });
});

test("training and eval views count their own durable facts", () => {
	const sft = semanticCountsFromRunView({
		algorithm: "sft",
		header: header({ succeeded: 3 }),
		projection: { checkpoints: ["a", "b", "c"], evaluations: [{ id: "a" }, { id: "b" }], metrics: { points: Array.from({ length: 1500 }, (_, step) => ({ step })) } }
	}, 0);
	assert.equal(sft.rollouts, 2);
	assert.equal(sft.semanticEvents, 3 + 2 + 1500 + 3);
	const evalView = semanticCountsFromRunView({
		algorithm: "eval",
		header: header({ succeeded: 40, failed: 2 }),
		projection: { candidates: ["policy"], evidenceLedger: Array.from({ length: 42 }, (_, index) => ({ workItemId: String(index) })), scoredTrials: 40 }
	}, 0);
	assert.equal(evalView.rollouts, 42);
});

test("VisualHost publishes the projection-derived counts, not the raw array length", () => {
	const host = readFileSync(join(appRoot, "src/renderer/src/components/VisualHost.tsx"), "utf8");
	assert.match(host, /semanticCountsFromRunView\(/);
	assert.match(host, /data-visual-semantic-event-count=\{String\(semanticCounts\.semanticEvents\)\}/);
	assert.match(host, /data-visual-rollout-count=\{String\(semanticCounts\.rollouts\)\}/);
	assert.match(host, /data-visual-raw-event-count=\{String\(boundEvents\.length\)\}/);
	assert.equal(host.includes("data-visual-rollout-count={String(boundEvents.length)}"), false);
});

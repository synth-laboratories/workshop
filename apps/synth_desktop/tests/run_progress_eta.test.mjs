/**
 * Honest-ETA rules.
 *
 * The estimator's job is to refuse, so most of these tests assert that it does:
 * no denominator, too few samples, a paused run, and a disrupted rig all have
 * to produce a state the UI can render as words rather than a number.
 */
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

function bundle(relative, outName) {
	const outfile = join(compiledDir, outName);
	buildSync({
		entryPoints: [join(appRoot, relative)],
		bundle: true,
		format: "esm",
		target: "es2022",
		platform: "node",
		// The renderer resolves family internals through the same alias Vite and
		// tsconfig use; esbuild needs it spelled out.
		alias: { "@synth/visual-templates": join(appRoot, "../../visuals/families") },
		outfile
	});
	return pathToFileURL(outfile).href;
}

const { usableCompletions, estimatePhaseEta } = await import(
	bundle("src/renderer/src/runtime/runProgress/eta.ts", "runProgressEta.mjs")
);
const { formatEta } = await import(
	bundle("src/renderer/src/runtime/runProgress/format.ts", "runProgressFormat.mjs")
);

const T0 = Date.UTC(2026, 7, 17, 12, 0, 0);
/** Completions every `stepMs`, starting at T0. */
function evenly(count, stepMs) {
	return Array.from({ length: count }, (_, index) => T0 + index * stepMs);
}

test("no denominator is unavailable, and says which unit is missing", () => {
	const eta = estimatePhaseEta({
		phaseId: "training",
		completions: evenly(8, 10_000),
		unit: "step"
	});
	assert.equal(eta.state, "unavailable");
	assert.match(eta.unavailableReason, /no step denominator/);
	assert.equal(formatEta(eta), "Unavailable");
});

test("a caller-supplied reason is used verbatim — the producer's gap, not ours", () => {
	const eta = estimatePhaseEta({
		phaseId: "training",
		completions: evenly(9, 10_000),
		remainingUnits: 500,
		unit: "step",
		unavailableReason: "provider did not declare total steps"
	});
	assert.equal(eta.state, "unavailable");
	assert.equal(eta.unavailableReason, "provider did not declare total steps");
});

test("one completion is warming, not an estimate built from a single sample", () => {
	const eta = estimatePhaseEta({
		phaseId: "minibatch",
		completions: [T0],
		remainingUnits: 40,
		unit: "rollout"
	});
	assert.equal(eta.state, "estimating");
	assert.equal(eta.confidence, "warming");
	assert.equal(formatEta(eta), "Estimating…");
});

test("two completions give a widened range, never a point estimate", () => {
	const eta = estimatePhaseEta({
		phaseId: "minibatch",
		completions: evenly(2, 6_000),
		remainingUnits: 10,
		unit: "rollout"
	});
	assert.equal(eta.state, "range");
	assert.equal(eta.confidence, "low");
	assert.ok(eta.lowMs < eta.remainingMs && eta.remainingMs < eta.highMs);
	assert.match(formatEta(eta), /^about \d+(–\d+)? min remaining$/);
});

test("a steady phase with enough windowed rates settles into a high-confidence point", () => {
	const eta = estimatePhaseEta({
		phaseId: "full_train",
		completions: evenly(12, 5_000),
		remainingUnits: 12,
		unit: "rollout"
	});
	assert.equal(eta.state, "point");
	assert.equal(eta.confidence, "high");
	assert.equal(eta.remainingMs, 5_000 * 12);
	assert.match(eta.basis, /windowed rate/);
	assert.match(eta.basis, /phase full_train/);
	assert.equal(formatEta(eta), "~1 min remaining");
});

test("one outlier cannot move the estimate: the median rate ignores it", () => {
	const steady = evenly(9, 5_000);
	const withOutlier = [...steady, steady[8] + 600_000];
	const eta = estimatePhaseEta({
		phaseId: "full_train",
		completions: withOutlier,
		remainingUnits: 12,
		unit: "rollout"
	});
	assert.equal(eta.remainingMs, 5_000 * 12);
});

test("a genuinely inconsistent phase widens to a range instead of averaging", () => {
	// First half finishes a unit every 2s; second half every 20s.
	const fast = evenly(6, 2_000);
	const slow = Array.from({ length: 6 }, (_, index) => fast[5] + (index + 1) * 20_000);
	const eta = estimatePhaseEta({
		phaseId: "minibatch",
		completions: [...fast, ...slow],
		remainingUnits: 10,
		unit: "rollout"
	});
	assert.equal(eta.state, "range");
	assert.ok(eta.highMs > eta.lowMs * 1.5, "an inconsistent phase must not present a narrow band");
});

test("observed throughput carries effective concurrency; nothing divides by a configured size", () => {
	// Four workers finishing a 20s rollout each produce a completion every 5s.
	const fast = estimatePhaseEta({
		phaseId: "minibatch",
		completions: evenly(6, 5_000),
		remainingUnits: 20,
		unit: "rollout"
	});
	// The same rollout duration with one worker: a completion every 20s.
	const slow = estimatePhaseEta({
		phaseId: "minibatch",
		completions: evenly(6, 20_000),
		remainingUnits: 20,
		unit: "rollout"
	});
	assert.equal(fast.remainingMs, 100_000);
	assert.equal(slow.remainingMs, 400_000);
});

test("a disruption discards the samples before it and drops confidence", () => {
	const completions = [...evenly(12, 5_000), T0 + 70_000, T0 + 75_000, T0 + 80_000];
	const eta = estimatePhaseEta({
		phaseId: "minibatch",
		completions,
		remainingUnits: 10,
		unit: "rollout",
		disruptedAtMs: T0 + 60_000
	});
	assert.equal(usableCompletions({ phaseId: "x", completions, unit: "rollout", disruptedAtMs: T0 + 60_000 }).length, 3);
	assert.ok(eta.state === "range" || eta.state === "estimating");
	assert.match(eta.basis, /samples restarted after a disruption|spanning real time/);
});

test("a paused run freezes rather than counting the pause as work time", () => {
	const eta = estimatePhaseEta({
		phaseId: "minibatch",
		completions: evenly(6, 5_000),
		remainingUnits: 12,
		unit: "rollout",
		paused: true
	});
	assert.equal(eta.state, "paused");
	assert.equal(eta.remainingMs, undefined);
	assert.equal(formatEta(eta), "Paused");
});

test("zero remaining work is zero remaining time, not an estimate", () => {
	const eta = estimatePhaseEta({
		phaseId: "heldout",
		completions: evenly(4, 5_000),
		remainingUnits: 0,
		unit: "rollout"
	});
	assert.equal(eta.state, "point");
	assert.equal(eta.remainingMs, 0);
});

test("phase samples never blend: the same completions under two phase ids stay separate", () => {
	const minibatch = estimatePhaseEta({
		phaseId: "minibatch",
		completions: evenly(6, 2_000),
		remainingUnits: 10,
		unit: "rollout"
	});
	const fullTrain = estimatePhaseEta({
		phaseId: "full_train",
		completions: evenly(6, 20_000),
		remainingUnits: 10,
		unit: "rollout"
	});
	assert.match(minibatch.basis, /phase minibatch/);
	assert.match(fullTrain.basis, /phase full_train/);
	assert.notEqual(minibatch.remainingMs, fullTrain.remainingMs);
});

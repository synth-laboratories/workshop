/**
 * Honest-ETA rules.
 *
 * The estimator's job is to refuse, so most of these assert that it does. The
 * shape of the rules was set by real runs rather than by taste — see
 * `run_progress_live_runs.test.mjs` and the header of `eta.ts` — and the two that
 * matter most are:
 *
 *   · a rate is completions divided by elapsed wall time, never the gap between
 *     two completions, because real producers report in bursts;
 *   · a window whose longest idle stretch dominates its span is not evidence
 *     about time at all, however many completions it contains.
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
		alias: { "@synth/visual-templates": join(appRoot, "../../visuals/families") },
		outfile
	});
	return pathToFileURL(outfile).href;
}

const { completionRate, estimatePhaseEta, windowRates } = await import(
	bundle("src/renderer/src/runtime/runProgress/eta.ts", "runProgressEta.mjs")
);
const { formatEta } = await import(
	bundle("src/renderer/src/runtime/runProgress/format.ts", "runProgressFormat.mjs")
);

const T0 = Date.UTC(2026, 7, 17, 12, 0, 0);

/** Completions every `stepMs`, starting at T0, with the clock at the last one. */
function steady(count, stepMs, overrides = {}) {
	const completions = Array.from({ length: count }, (_, index) => T0 + index * stepMs);
	return { completions, nowMs: completions.at(-1), ...overrides };
}

/* ── Refusals ─────────────────────────────────────────────────────────── */

test("no denominator is unavailable, and says which unit is missing", () => {
	const eta = estimatePhaseEta({ phaseId: "training", ...steady(20, 10_000), unit: "step" });
	assert.equal(eta.state, "unavailable");
	assert.match(eta.unavailableReason, /no step denominator/);
	assert.equal(formatEta(eta), "Unavailable");
});

test("a caller-supplied reason is used verbatim, and the basis does not invent one", () => {
	const eta = estimatePhaseEta({
		phaseId: "training",
		...steady(20, 10_000),
		remainingUnits: 500,
		unit: "step",
		unavailableReason: "provider did not declare total steps"
	});
	assert.equal(eta.state, "unavailable");
	assert.equal(eta.unavailableReason, "provider did not declare total steps");
	// The run does have a denominator; the basis must not claim otherwise.
	assert.doesNotMatch(eta.basis, /no denominator/);
	assert.match(eta.basis, /20 completed steps observed in phase training/);
});

test("one completion is warming, not an estimate built from a single sample", () => {
	const eta = estimatePhaseEta({
		phaseId: "minibatch",
		completions: [T0],
		nowMs: T0,
		remainingUnits: 40,
		unit: "rollout"
	});
	assert.equal(eta.state, "estimating");
	assert.equal(eta.confidence, "warming");
	assert.equal(formatEta(eta), "Estimating…");
});

test("a window dominated by one idle stretch refuses, however many completions it has", () => {
	// Twenty rollouts in a two-second burst, then two minutes of silence: this is
	// the real GEPA shape, and it is where a gap-based estimator lied by 4.7×.
	const burst = Array.from({ length: 20 }, (_, index) => T0 + index * 100);
	const eta = estimatePhaseEta({
		phaseId: "seed",
		completions: burst,
		nowMs: T0 + 120_000,
		remainingUnits: 100,
		unit: "rollout"
	});
	assert.equal(eta.state, "unavailable");
	assert.match(eta.unavailableReason, /longest stretch with no completed rollout/);
	assert.match(eta.unavailableReason, /does not predict when this run finishes/);
	assert.equal(formatEta(eta), "Unavailable");
});

test("a burst cannot deflate the rate: the same work measured over its real span", () => {
	// Four bursts of five, one per minute. A gap-based estimator would read the
	// 10ms intra-burst gaps; the honest reading is 20 rollouts over 3 minutes.
	const completions = [];
	for (let minute = 0; minute < 4; minute += 1) {
		for (let index = 0; index < 5; index += 1) completions.push(T0 + minute * 60_000 + index * 10);
	}
	const rate = completionRate(completions);
	assert.ok(rate != null);
	// 19 intervals over the 180s span, not one interval per 10ms.
	assert.ok(1 / rate > 9_000 && 1 / rate < 10_000, `${(1 / rate).toFixed(0)}ms per rollout`);
});

/* ── Estimates ────────────────────────────────────────────────────────── */

test("a steady, well-covered phase settles into a high-confidence point", () => {
	const eta = estimatePhaseEta({
		phaseId: "full_train",
		...steady(13, 5_000),
		remainingUnits: 12,
		unit: "rollout"
	});
	assert.equal(eta.state, "point");
	assert.equal(eta.confidence, "high");
	assert.equal(eta.remainingMs, 5_000 * 12);
	assert.match(eta.basis, /13 completed rollouts in 1\.0min of phase full_train/);
	assert.match(eta.basis, /12 rollouts remaining/);
	assert.equal(formatEta(eta), "~1 min remaining");
});

test("too few completions give a range, never a point, even when perfectly steady", () => {
	const eta = estimatePhaseEta({
		phaseId: "minibatch",
		...steady(6, 5_000),
		remainingUnits: 12,
		unit: "rollout"
	});
	assert.equal(eta.state, "range");
	assert.equal(eta.remainingMs, 5_000 * 12);
	assert.match(formatEta(eta), /^about \d+(–\d+)? min remaining$/);
});

test("observed rate carries effective concurrency; nothing divides by a configured size", () => {
	// Four workers each taking 20s complete one rollout every 5s; one worker
	// taking 20s completes one every 20s. Neither is told the worker count.
	const fast = estimatePhaseEta({ phaseId: "minibatch", ...steady(13, 5_000), remainingUnits: 20, unit: "rollout" });
	const slow = estimatePhaseEta({ phaseId: "minibatch", ...steady(13, 20_000), remainingUnits: 20, unit: "rollout" });
	assert.equal(fast.remainingMs, 100_000);
	assert.equal(slow.remainingMs, 400_000);
});

test("a changing throughput widens the range and says so", () => {
	// Thirteen completions: the first seven every 5s, the rest every 20s.
	const completions = [T0];
	for (let index = 1; index < 7; index += 1) completions.push(completions.at(-1) + 5_000);
	for (let index = 7; index < 13; index += 1) completions.push(completions.at(-1) + 20_000);
	const eta = estimatePhaseEta({
		phaseId: "minibatch",
		completions,
		nowMs: completions.at(-1),
		remainingUnits: 20,
		unit: "rollout"
	});
	assert.equal(eta.state, "range");
	assert.match(eta.basis, /throughput is still changing/);
	assert.ok(eta.highMs > eta.lowMs, "a changing throughput must not present one number");
});

test("a stall is information, not an outlier to discard", () => {
	const settled = estimatePhaseEta({ phaseId: "seed", ...steady(13, 5_000), remainingUnits: 12, unit: "rollout" });
	// The same run, plus a 30s stall up to now, is genuinely slower than it was.
	const stalled = steady(13, 5_000);
	const eta = estimatePhaseEta({
		phaseId: "seed",
		completions: stalled.completions,
		nowMs: stalled.nowMs + 30_000,
		remainingUnits: 12,
		unit: "rollout"
	});
	assert.ok(
		eta.state === "unavailable" || eta.remainingMs > settled.remainingMs,
		`a stall must slow the estimate or withdraw it, got ${eta.state} ${eta.remainingMs}`
	);
});

/* ── Sampling ─────────────────────────────────────────────────────────── */

test("the window ends at now, so trailing idle time counts against the rate", () => {
	const completions = steady(13, 5_000).completions;
	const atLastCompletion = windowRates({
		phaseId: "seed",
		completions,
		nowMs: completions.at(-1),
		unit: "rollout"
	});
	const oneMinuteLater = windowRates({
		phaseId: "seed",
		completions,
		nowMs: completions.at(-1) + 60_000,
		unit: "rollout"
	});
	assert.ok(oneMinuteLater.overall < atLastCompletion.overall, "idle time must lower the rate");
	assert.ok(oneMinuteLater.longestIdleMs >= 60_000, "the tail is part of the longest idle stretch");
});

test("a disruption discards the samples before it and says so", () => {
	const completions = [...steady(13, 5_000).completions, T0 + 100_000, T0 + 110_000, T0 + 120_000];
	const evidence = {
		phaseId: "minibatch",
		completions,
		nowMs: T0 + 120_000,
		remainingUnits: 10,
		unit: "rollout",
		disruptedAtMs: T0 + 90_000
	};
	assert.equal(windowRates(evidence).samples, 3, "only the completions after the disruption remain");
	const eta = estimatePhaseEta(evidence);
	assert.ok(["range", "unavailable"].includes(eta.state), eta.state);
	if (eta.state === "range") assert.match(eta.basis, /samples restarted after a disruption/);
});

test("completions after the clock are not evidence yet", () => {
	const window = windowRates({
		phaseId: "seed",
		completions: [T0, T0 + 1_000, T0 + 999_999],
		nowMs: T0 + 1_000,
		unit: "rollout"
	});
	assert.equal(window.samples, 2);
});

/* ── Non-estimate states ──────────────────────────────────────────────── */

test("a paused run freezes rather than counting the pause as work time", () => {
	const eta = estimatePhaseEta({
		phaseId: "minibatch",
		...steady(13, 5_000),
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
		...steady(13, 5_000),
		remainingUnits: 0,
		unit: "rollout"
	});
	assert.equal(eta.state, "point");
	assert.equal(eta.remainingMs, 0);
});

test("phase samples never blend: the same completions under two phase ids stay separate", () => {
	const minibatch = estimatePhaseEta({ phaseId: "minibatch", ...steady(13, 2_000), remainingUnits: 10, unit: "rollout" });
	const fullTrain = estimatePhaseEta({ phaseId: "full_train", ...steady(13, 20_000), remainingUnits: 10, unit: "rollout" });
	assert.match(minibatch.basis, /phase minibatch/);
	assert.match(fullTrain.basis, /phase full_train/);
	assert.notEqual(minibatch.remainingMs, fullTrain.remainingMs);
});

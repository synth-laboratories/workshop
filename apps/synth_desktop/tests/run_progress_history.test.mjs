/**
 * The historical estimator, validated against real runs.
 *
 * This is the test that licenses putting a number on screen. It replays sixteen
 * curves sealed from runs that actually executed on this machine
 * (`docs/receipts/2026-08-17/v0.5-run-progress/recipe-history.json`, produced by
 * the same `buildCurve` the backfill ships) and holds the shipping estimator to a
 * measured error band, leave-one-out.
 *
 * The band is not a taste judgement. The alternatives, measured the same way:
 *
 *   | estimator                       | median | p90  | median absolute |
 *   |---------------------------------|--------|------|-----------------|
 *   | median total alone              | 100%   | 389% | —               |
 *   | rollout throughput              | 74%    | 594% | —               |
 *   | elapsed ÷ progress              | 205%   | 520% | 79s             |
 *   | this one                        | 30%    | 57%  | 10s             |
 *
 * If a change pushes the error past the asserted band, the change is wrong — or
 * the band needs re-measuring on new evidence and this comment needs rewriting.
 */
import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const workshopRoot = join(appRoot, "../..");
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

const {
	estimateFromHistory,
	historicalShape,
	MIN_COMPARABLE_RUNS,
	progressHistoryOf,
	recipeKeyOf
} = await import(bundle("src/renderer/src/runtime/runProgress/history.ts", "runProgressHistory.mjs"));
const { estimatePhaseEta } = await import(
	bundle("src/renderer/src/runtime/runProgress/eta.ts", "runProgressEtaHistory.mjs")
);
const { formatEta } = await import(
	bundle("src/renderer/src/runtime/runProgress/format.ts", "runProgressFormatHistory.mjs")
);
const { buildCurve } = await import(
	pathToFileURL(join(appRoot, "scripts/backfill-progress-history.mjs")).href
);

const CURVES = JSON.parse(
	readFileSync(join(workshopRoot, "docs/receipts/2026-08-17/v0.5-run-progress/recipe-history.json"), "utf8")
);
const CURVE_POINTS = 19;

/** A run record as `list()` returns it, carrying its sealed curve. */
function peerRecord(entry) {
	return {
		id: entry.id,
		algorithmId: entry.algorithmId,
		status: "completed",
		summary: {
			progressHistory: {
				schemaVersion: entry.schemaVersion,
				unit: entry.unit,
				totalUnits: entry.totalUnits,
				wallTimeMs: entry.wallTimeMs,
				curve: entry.curve
			}
		}
	};
}

function median(values) {
	const sorted = [...values].sort((left, right) => left - right);
	const middle = Math.floor(sorted.length / 2);
	return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle];
}

test("the receipt holds real sealed curves in the shipped shape", () => {
	assert.ok(CURVES.length >= 12, `${CURVES.length} curves`);
	for (const entry of CURVES) {
		assert.equal(entry.schemaVersion, "run_progress_history.v1");
		assert.equal(entry.curve.length, CURVE_POINTS);
		assert.ok(entry.totalUnits >= 8 && entry.wallTimeMs > 0);
		// A curve is a cumulative fraction: monotonic and inside the unit interval.
		for (let index = 1; index < entry.curve.length; index += 1) {
			assert.ok(entry.curve[index] >= entry.curve[index - 1], `${entry.id} regressed`);
		}
		assert.ok(entry.curve.every((value) => value >= 0 && value <= 1));
		// Round-trips through the reader the renderer actually uses.
		assert.ok(progressHistoryOf(peerRecord(entry)));
	}
});

test("the backfill and the sealed receipt agree on the curve", () => {
	// A synthetic stream through the shipped builder, checked against the shape the
	// Rust sealer asserts in its own tests: 20 rollouts, one per second, reported
	// twice each.
	const events = [];
	for (let index = 0; index < 20; index += 1) {
		for (let repeat = 0; repeat < 2; repeat += 1) {
			events.push({
				type: "optimizer.evaluation_result.received",
				occurredAt: new Date(Date.UTC(2026, 7, 17, 12, 0, index)).toISOString(),
				delta: { rollout_id: `rollout_${index}` }
			});
		}
	}
	const history = buildCurve("gepa", "completed", events);
	assert.equal(history.totalUnits, 20, "duplicate reports must not double the work");
	assert.equal(history.wallTimeMs, 19_000);
	assert.equal(history.curve.length, CURVE_POINTS);
	assert.equal(buildCurve("gepa", "failed", events), null, "only completed runs teach");
	assert.equal(buildCurve("go-ex", "completed", events), null, "uncounted algorithms teach nothing");
});

test("recipe identity keeps unrelated runs out of each other's history", () => {
	assert.equal(
		recipeKeyOf({ id: "banking77_gepa_luna_med_c90c6c72", algorithmId: "gepa", status: "completed" }),
		"banking77_gepa_luna_med"
	);
	// A declared recipe id wins over the id prefix.
	assert.equal(
		recipeKeyOf({
			id: "anything_at_all",
			algorithmId: "gepa",
			status: "completed",
			summary: { recipeId: "gepa.banking77.luna.v1" }
		}),
		"gepa.banking77.luna.v1"
	);
});

test("history is refused until enough comparable runs agree", () => {
	const peers = CURVES.filter((entry) => entry.totalUnits === 140).map(peerRecord);
	const subject = { id: "new_run", algorithmId: "gepa", status: "running", summary: {} };
	// A different recipe pools nothing, however many runs it has.
	assert.equal(historicalShape({ ...subject, id: "other_recipe_1" }, peers, 140), null);
	const sameRecipe = { id: "banking77_gepa_luna_med_new", algorithmId: "gepa", status: "running", summary: {} };
	assert.ok(historicalShape(sameRecipe, peers, 140), "six comparable runs pool a shape");
	assert.equal(
		historicalShape(sameRecipe, peers.slice(0, MIN_COMPARABLE_RUNS - 1), 140),
		null,
		"below the floor there is no shape"
	);
});

test("a differently sized run does not borrow an incomparable history", () => {
	const subject = { id: "banking77_gepa_luna_med_new", algorithmId: "gepa", status: "running", summary: {} };
	const all = CURVES.map(peerRecord);
	const at140 = historicalShape(subject, all, 140);
	const at240 = historicalShape(subject, all, 240);
	assert.ok(at140 && at240);
	// The 140-rollout and 240-rollout runs are separate populations.
	assert.notEqual(at140.medianWallTimeMs, at240.medianWallTimeMs);
	assert.ok(at140.runs >= 3 && at240.runs >= 3);
	// A size nothing matches pools nothing.
	assert.equal(historicalShape(subject, all, 5_000), null);
});

/* ── The measured error band ──────────────────────────────────────────── */

/**
 * Leave-one-out: hold each run out, pool the rest, and predict its remaining
 * time at every 5% of its progress from its own recorded trajectory.
 */
function leaveOneOut() {
	const relative = [];
	const absolute = [];
	const within = [];
	for (const held of CURVES) {
		const subject = { id: held.id, algorithmId: held.algorithmId, status: "running", summary: {} };
		const peers = CURVES.filter((entry) => entry.id !== held.id).map(peerRecord);
		const shape = historicalShape(subject, peers, held.totalUnits);
		if (!shape) continue;
		for (let step = 1; step <= CURVE_POINTS; step += 1) {
			const progress = step / (CURVE_POINTS + 1);
			const elapsed = held.curve[step - 1] * held.wallTimeMs;
			const truth = held.wallTimeMs - elapsed;
			if (elapsed <= 0 || truth <= 1_000) continue;
			const estimate = estimateFromHistory(shape, progress, elapsed);
			if (!estimate) continue;
			const error = Math.abs(estimate.remainingMs - truth);
			relative.push(error / truth);
			absolute.push(error);
			within.push(error <= 30_000 || error / truth <= 0.5);
		}
	}
	return { relative, absolute, within };
}

test("the estimator stays inside its measured error band", () => {
	const { relative, absolute, within } = leaveOneOut();
	assert.ok(relative.length >= 100, `only ${relative.length} predictions`);
	const medianRelative = median(relative);
	const sorted = [...relative].sort((left, right) => left - right);
	const p90 = sorted[Math.floor(0.9 * sorted.length)];
	const medianAbsolute = median(absolute);
	const share = within.filter(Boolean).length / within.length;
	const report = `median ${(medianRelative * 100).toFixed(0)}% · p90 ${(p90 * 100).toFixed(0)}% · median abs ${(medianAbsolute / 1000).toFixed(0)}s · within ${(share * 100).toFixed(0)}%`;
	// Measured at 30% / 57% / 10s / 82%. The bands allow real drift and still
	// fail long before the estimator becomes as bad as anything it replaced.
	assert.ok(medianRelative <= 0.45, `median relative error too high: ${report}`);
	assert.ok(p90 <= 1.2, `p90 relative error too high: ${report}`);
	assert.ok(medianAbsolute <= 25_000, `median absolute error too high: ${report}`);
	assert.ok(share >= 0.7, `too few predictions land close: ${report}`);
});

test("the estimator beats every alternative it replaced, on the same data", () => {
	const { relative } = leaveOneOut();
	const ours = median(relative);
	// Prior alone: the median total minus elapsed, ignoring live progress.
	const priorOnly = [];
	const naive = [];
	for (const held of CURVES) {
		const subject = { id: held.id, algorithmId: held.algorithmId, status: "running", summary: {} };
		const peers = CURVES.filter((entry) => entry.id !== held.id).map(peerRecord);
		const shape = historicalShape(subject, peers, held.totalUnits);
		if (!shape) continue;
		for (let step = 1; step <= CURVE_POINTS; step += 1) {
			const progress = step / (CURVE_POINTS + 1);
			const elapsed = held.curve[step - 1] * held.wallTimeMs;
			const truth = held.wallTimeMs - elapsed;
			if (elapsed <= 0 || truth <= 1_000) continue;
			priorOnly.push(Math.abs(Math.max(0, shape.medianWallTimeMs - elapsed) - truth) / truth);
			naive.push(Math.abs(Math.max(0, elapsed / progress - elapsed) - truth) / truth);
		}
	}
	assert.ok(ours < median(priorOnly), `history ${ours} vs prior-only ${median(priorOnly)}`);
	assert.ok(ours < median(naive), `history ${ours} vs elapsed/progress ${median(naive)}`);
});

/* ── How it reaches the card ───────────────────────────────────────────── */

test("the ETA prefers history over throughput and explains where it came from", () => {
	const peers = CURVES.filter((entry) => entry.totalUnits === 140).map(peerRecord);
	const subject = { id: "banking77_gepa_luna_med_live", algorithmId: "gepa", status: "running", summary: {} };
	const shape = historicalShape(subject, peers, 140);
	const eta = estimatePhaseEta({
		phaseId: "seed",
		// Bursty completions that would make the throughput path refuse outright.
		completions: Array.from({ length: 20 }, (_, index) => 1_000 + index * 10),
		nowMs: 120_000,
		remainingUnits: 70,
		unit: "rollout",
		history: shape,
		progressFraction: 0.5,
		elapsedMs: 90_000
	});
	assert.ok(["point", "range"].includes(eta.state), `${eta.state}: ${eta.basis}`);
	assert.match(eta.basis, /50% of the work done/);
	assert.match(eta.basis, /previous runs? of this recipe/);
	assert.match(formatEta(eta), /remaining$/);
});

test("a run heading far past its recipe's history withdraws the number", () => {
	const peers = CURVES.filter((entry) => entry.totalUnits === 140).map(peerRecord);
	const subject = { id: "banking77_gepa_luna_med_slow", algorithmId: "gepa", status: "running", summary: {} };
	const shape = historicalShape(subject, peers, 140);
	const eta = estimatePhaseEta({
		phaseId: "seed",
		completions: [1_000, 2_000],
		nowMs: 9_000_000,
		remainingUnits: 70,
		unit: "rollout",
		history: shape,
		progressFraction: 0.5,
		// An hour, against a recipe that usually finishes in about three minutes.
		elapsedMs: 3_600_000
	});
	assert.equal(eta.state, "unavailable");
	assert.match(eta.unavailableReason, /taking far longer than/);
	assert.equal(formatEta(eta), "Unavailable");
});

test("without history the ETA falls back to refusing, not to throughput", () => {
	const eta = estimatePhaseEta({
		phaseId: "seed",
		completions: Array.from({ length: 20 }, (_, index) => 1_000 + index * 10),
		nowMs: 120_000,
		remainingUnits: 70,
		unit: "rollout",
		progressFraction: 0.5,
		elapsedMs: 90_000
	});
	assert.equal(eta.state, "unavailable");
});

test("history is not consulted for a paused run", () => {
	const peers = CURVES.filter((entry) => entry.totalUnits === 140).map(peerRecord);
	const shape = historicalShape(
		{ id: "banking77_gepa_luna_med_paused", algorithmId: "gepa", status: "paused", summary: {} },
		peers,
		140
	);
	const eta = estimatePhaseEta({
		phaseId: "seed",
		completions: [1_000, 2_000, 3_000],
		nowMs: 90_000,
		remainingUnits: 70,
		unit: "rollout",
		history: shape,
		progressFraction: 0.5,
		elapsedMs: 90_000,
		paused: true
	});
	assert.equal(eta.state, "paused", "a paused run reports paused, not an estimate");
});

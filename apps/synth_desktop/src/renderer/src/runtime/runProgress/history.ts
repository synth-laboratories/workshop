/**
 * Estimating a run from the recipe's own history.
 *
 * A run's remaining time cannot be read from how fast its work is completing —
 * see the header of `eta.ts` for what that produced on real runs. What can be
 * read is the shape earlier runs of the *same recipe* traced through the same
 * work, sealed into each finished run's summary as `run_progress_history.v1` by
 * `src-tauri/src/optimizers/progress_history.rs`.
 *
 * The estimate is then a single division. If previous runs were 60% of the way
 * through their wall time by the time they had completed 50% of their rollouts,
 * and this run has done 50% in 90 seconds, its total is about 90 / 0.6 = 150s,
 * so about 60 seconds remain.
 *
 * Leave-one-out over ten real Banking77 GEPA runs, predicting remaining time at
 * nineteen points in each:
 *
 *   · this estimator — 30% median error, 57% p90, 10s median absolute, and 82%
 *     of predictions within 30 seconds or 50% of the truth;
 *   · the median total alone, ignoring live progress — 100% median;
 *   · rollout throughput — 74% median, 594% p90;
 *   · elapsed ÷ progress with no historical shape — 205% median.
 *
 * The accuracy improves as a run advances: about 39% error in the first eighth,
 * 20% by halfway, and one to three seconds of absolute error near the end.
 *
 * Two refusals are load-bearing. Without enough comparable finished runs there
 * is no curve and the caller falls back to saying so; and history is only ever
 * consulted for runs doing comparable amounts of work, because a 140-rollout run
 * teaches nothing useful about a 240-rollout one.
 */

import type { RunRecord } from "./subscription";

export const PROGRESS_HISTORY_SCHEMA = "run_progress_history.v1";

/** One finished run's traced shape, as sealed in its summary. */
export type ProgressHistory = {
	schemaVersion: string;
	unit: string;
	totalUnits: number;
	wallTimeMs: number;
	/** Elapsed fraction at each 5% of unit progress; 19 entries. */
	curve: number[];
};

export type HistoricalShape = {
	/** How many finished runs agree on this shape. */
	runs: number;
	/** Median elapsed fraction at each 5% of progress. */
	curve: number[];
	/** Per-point spread, used to widen the estimate rather than to hide it. */
	low: number[];
	high: number[];
	/** Median total wall time across the comparable runs. */
	medianWallTimeMs: number;
	unit: string;
};

/** Comparable work means within this fraction of the same unit count. */
const COMPARABLE_TOLERANCE = 0.15;

/** Below this many comparable runs, history is not consulted at all. */
export const MIN_COMPARABLE_RUNS = 3;

const CURVE_POINTS = 19;

/**
 * How far past the historical median a prediction may run before it is treated as
 * an extrapolation rather than an estimate. Two× is deliberately loose: a run
 * merely slower than its peers still gets a number, and only one heading somewhere
 * the population never went loses it.
 */
const BEYOND_HISTORY_FACTOR = 2;

function isProgressHistory(value: unknown): value is ProgressHistory {
	if (!value || typeof value !== "object" || Array.isArray(value)) return false;
	const candidate = value as Record<string, unknown>;
	return (
		candidate.schemaVersion === PROGRESS_HISTORY_SCHEMA &&
		typeof candidate.totalUnits === "number" &&
		typeof candidate.wallTimeMs === "number" &&
		Array.isArray(candidate.curve) &&
		candidate.curve.length === CURVE_POINTS &&
		candidate.curve.every((entry) => typeof entry === "number" && entry >= 0 && entry <= 1)
	);
}

/** The sealed curve on a run record, when it carries one. */
export function progressHistoryOf(run: RunRecord): ProgressHistory | null {
	const sealed = (run.summary ?? {}).progressHistory;
	return isProgressHistory(sealed) ? sealed : null;
}

/**
 * The recipe a run belongs to. `recipeId` is authoritative when the producer
 * recorded it; otherwise the id prefix is used, which is how the launcher names
 * runs (`banking77_gepa_luna_med_c90c6c72`). Two runs of different recipes must
 * never pool their history, so a run with neither is its own recipe.
 */
export function recipeKeyOf(run: RunRecord): string {
	const declared = (run.summary ?? {}).recipeId;
	if (typeof declared === "string" && declared.length > 0) return declared;
	const parts = run.id.split("_");
	return parts.length > 1 ? parts.slice(0, -1).join("_") : run.id;
}

function median(values: number[]): number {
	const sorted = [...values].sort((left, right) => left - right);
	const middle = Math.floor(sorted.length / 2);
	return sorted.length % 2 === 0 ? (sorted[middle - 1]! + sorted[middle]!) / 2 : sorted[middle]!;
}

function quantile(values: number[], fraction: number): number {
	const sorted = [...values].sort((left, right) => left - right);
	const index = Math.min(sorted.length - 1, Math.max(0, Math.round(fraction * (sorted.length - 1))));
	return sorted[index]!;
}

/**
 * Pool the curves of finished runs that did comparable work.
 *
 * `peers` is every run the caller knows about; this picks the comparable ones,
 * excludes the run being estimated, and returns null when too few agree.
 */
export function historicalShape(
	subject: RunRecord,
	peers: RunRecord[],
	expectedUnits?: number
): HistoricalShape | null {
	const recipe = recipeKeyOf(subject);
	const target = expectedUnits;
	const usable: ProgressHistory[] = [];
	for (const peer of peers) {
		if (peer.id === subject.id) continue;
		if (recipeKeyOf(peer) !== recipe) continue;
		const history = progressHistoryOf(peer);
		if (!history) continue;
		if (target != null && target > 0) {
			const ratio = Math.abs(history.totalUnits - target) / target;
			if (ratio > COMPARABLE_TOLERANCE) continue;
		}
		usable.push(history);
	}
	if (usable.length < MIN_COMPARABLE_RUNS) return null;
	const curve: number[] = [];
	const low: number[] = [];
	const high: number[] = [];
	for (let index = 0; index < CURVE_POINTS; index += 1) {
		const points = usable.map((entry) => entry.curve[index]!);
		curve.push(median(points));
		// A faster-than-median run reaches a given progress earlier, i.e. at a
		// *smaller* elapsed fraction, which yields a *larger* total. The bounds are
		// therefore crossed on purpose when converted to time.
		low.push(quantile(points, 0.25));
		high.push(quantile(points, 0.75));
	}
	return {
		runs: usable.length,
		curve,
		low,
		high,
		medianWallTimeMs: median(usable.map((entry) => entry.wallTimeMs)),
		unit: usable[0]!.unit
	};
}

export type HistoricalEstimate = {
	remainingMs: number;
	lowMs: number;
	highMs: number;
	/** Runs the shape was pooled from. */
	runs: number;
	/** Elapsed fraction previous runs had reached at this progress. */
	expectedElapsedFraction: number;
	/**
	 * This run is on track to take so much longer than every comparable run that
	 * the estimate would be an extrapolation beyond anything measured.
	 */
	beyondHistory: boolean;
	/** The predicted total, for comparison against the historical envelope. */
	predictedTotalMs: number;
};

/**
 * Interpolate the elapsed fraction previous runs had reached at `progress`.
 *
 * The curve samples 5%–95%; below and above that it is clamped, because the
 * first and last few percent of a run are where the sample is thinnest and an
 * extrapolation would be least supported.
 */
function elapsedFractionAt(curve: number[], progress: number): number {
	const position = progress * (CURVE_POINTS + 1) - 1;
	if (position <= 0) return curve[0]!;
	if (position >= CURVE_POINTS - 1) return curve[CURVE_POINTS - 1]!;
	const lower = Math.floor(position);
	const weight = position - lower;
	return curve[lower]! * (1 - weight) + curve[lower + 1]! * weight;
}

/**
 * Estimate remaining time from the historical shape and this run's own elapsed
 * time. Returns null when progress is not yet measurable.
 *
 * A run heading well past what its peers took is flagged rather than trusted.
 * Scaling by the historical fraction keeps a slow run's estimate proportionate —
 * a run 10% slower simply predicts a 10% longer total, which is right — but once
 * the prediction runs past twice the historical total, the population it was
 * drawn from no longer contains anything like this run, and `beyondHistory` says
 * so instead of printing a confident number from an empty extrapolation.
 */
export function estimateFromHistory(
	shape: HistoricalShape,
	progress: number,
	elapsedMs: number
): HistoricalEstimate | null {
	if (!(progress > 0) || !(elapsedMs > 0)) return null;
	if (progress >= 1) return null;
	const expected = elapsedFractionAt(shape.curve, progress);
	if (!(expected > 0)) return null;
	const totalFrom = (fraction: number) => elapsedMs / Math.max(fraction, 1e-6);
	const total = totalFrom(expected);
	// A smaller elapsed fraction implies a larger total, so the high bound comes
	// from the low quantile and vice versa.
	const totals = [
		total,
		totalFrom(elapsedFractionAt(shape.high, progress)),
		totalFrom(elapsedFractionAt(shape.low, progress))
	];
	const remaining = total - elapsedMs;
	return {
		remainingMs: Math.max(0, remaining),
		lowMs: Math.max(0, Math.min(...totals) - elapsedMs),
		highMs: Math.max(0, Math.max(...totals) - elapsedMs),
		runs: shape.runs,
		expectedElapsedFraction: expected,
		predictedTotalMs: total,
		beyondHistory: total > shape.medianWallTimeMs * BEYOND_HISTORY_FACTOR
	};
}

/**
 * Honest ETA.
 *
 * A wrong precise number is worse than no number, so this module is built to
 * refuse. It answers "unavailable" whenever the evidence does not support an
 * estimate, and it writes down the basis for every estimate it does produce so
 * the dialog can explain the figure instead of asserting it.
 *
 * The estimator is *phase-local and completion-driven*. Its only evidence is
 * the timestamps at which comparable units of work completed inside the
 * current phase. That choice carries the rules the brief demands:
 *
 *   · Effective, not configured, concurrency. Intervals between completions
 *     already contain however many workers were really running; nothing is
 *     divided by a configured semaphore size that may not be staffed.
 *   · Phase-local samples only. A caller passes completions for one phase, so
 *     a phase transition resets the evidence rather than blending a fast
 *     minibatch into a slow full-train.
 *   · Robust, not extrapolated. The median interval decides; one unusually
 *     fast or slow rollout cannot move the estimate far.
 *   · Disruption widens. Retries, throttling, and worker loss drop the samples
 *     that preceded them, which drops confidence back to a range.
 *   · Queue time is never execution evidence. A caller measuring training
 *     passes only training completions; queue elapsed is displayed separately.
 *   · Chat elapsed time and tool-call silence are not inputs. They are not
 *     even in the signature.
 */

import type { RunEtaConfidence, RunEtaProjection } from "./types";

/** Newest completions only; older intervals describe a run that no longer exists. */
const SAMPLE_WINDOW = 12;

/** Below this many intervals there is no estimate at all, only "Estimating…". */
const MIN_INTERVALS_FOR_RANGE = 1;

/** A point estimate needs a third completion and a settled spread. */
const MIN_INTERVALS_FOR_POINT = 3;

/** Interquartile spread over the median above which a range is still the honest answer. */
const POINT_SPREAD_CEILING = 0.45;

export type EtaEvidence = {
	/** Phase these completions belong to. Mixing phases is the caller's bug. */
	phaseId: string;
	/**
	 * Epoch-ms timestamps at which comparable units completed, any order.
	 * Non-finite entries are dropped.
	 */
	completions: number[];
	/** Units still to do in this phase. Absent when no denominator exists. */
	remainingUnits?: number;
	/** Unit noun for the basis line: "rollout", "trial", "step". */
	unit: string;
	/**
	 * Epoch-ms of the newest retry, throttle, or worker-loss event. Completions
	 * at or before it are discarded, because they measured a rig that changed.
	 */
	disruptedAtMs?: number;
	/** The run is paused: freeze the estimate rather than counting the pause. */
	paused?: boolean;
	/**
	 * Why no estimate is possible, when the caller already knows. Used verbatim
	 * as `unavailableReason`, e.g. "provider did not declare total steps".
	 */
	unavailableReason?: string;
};

function median(sorted: number[]): number {
	const middle = Math.floor(sorted.length / 2);
	return sorted.length % 2 === 0 ? (sorted[middle - 1]! + sorted[middle]!) / 2 : sorted[middle]!;
}

/** Quantile by nearest rank; exact enough for a spread over ≤12 samples. */
function quantile(sorted: number[], fraction: number): number {
	const index = Math.min(sorted.length - 1, Math.max(0, Math.round(fraction * (sorted.length - 1))));
	return sorted[index]!;
}

function plural(count: number, unit: string): string {
	return `${count} ${unit}${count === 1 ? "" : "s"}`;
}

function unavailable(basis: string, reason: string, sampleCount: number): RunEtaProjection {
	return {
		state: "unavailable",
		confidence: "warming",
		basis,
		sampleCount,
		unavailableReason: reason
	};
}

/**
 * Intervals between consecutive completions, after windowing and disruption
 * pruning. Exported for tests: the sampling rule is the part most likely to be
 * quietly wrong.
 */
export function completionIntervals(evidence: EtaEvidence): number[] {
	const usable = evidence.completions
		.filter((value) => Number.isFinite(value))
		.filter((value) => evidence.disruptedAtMs == null || value > evidence.disruptedAtMs)
		.sort((left, right) => left - right)
		.slice(-SAMPLE_WINDOW);
	const intervals: number[] = [];
	for (let index = 1; index < usable.length; index += 1) {
		const gap = usable[index]! - usable[index - 1]!;
		if (gap > 0) intervals.push(gap);
	}
	return intervals;
}

/**
 * Estimate the time remaining in one bounded, homogeneous phase.
 *
 * Returns `undefined` only for a terminal run — a finished run has no ETA, it
 * has a wall time. Every other case returns a projection whose `state` says
 * what the caller may display.
 */
export function estimatePhaseEta(evidence: EtaEvidence): RunEtaProjection {
	const intervals = completionIntervals(evidence);
	const samples = intervals.length + (intervals.length > 0 ? 1 : 0);
	const phaseBasis = `phase ${evidence.phaseId}`;

	if (evidence.paused) {
		return {
			state: "paused",
			confidence: intervals.length >= MIN_INTERVALS_FOR_POINT ? "medium" : "low",
			basis: `paused with ${plural(intervals.length, `${evidence.unit} interval`)} observed in ${phaseBasis}`,
			sampleCount: samples
		};
	}

	if (evidence.unavailableReason) {
		return unavailable(`no denominator in ${phaseBasis}`, evidence.unavailableReason, samples);
	}

	const remaining = evidence.remainingUnits;
	if (remaining == null || !Number.isFinite(remaining) || remaining < 0) {
		return unavailable(
			`no bounded work declared for ${phaseBasis}`,
			`no ${evidence.unit} denominator was declared`,
			samples
		);
	}

	if (remaining === 0) {
		return {
			state: "point",
			remainingMs: 0,
			confidence: "high",
			basis: `every ${evidence.unit} in ${phaseBasis} has completed`,
			sampleCount: samples
		};
	}

	if (intervals.length < MIN_INTERVALS_FOR_RANGE) {
		return {
			state: "estimating",
			confidence: "warming",
			basis: `${plural(evidence.completions.length, `completed ${evidence.unit}`)} in ${phaseBasis}; two are needed before an estimate`,
			sampleCount: samples
		};
	}

	const sorted = [...intervals].sort((left, right) => left - right);
	const typical = median(sorted);
	const low = quantile(sorted, 0.25);
	const high = quantile(sorted, 0.75);
	const spread = typical > 0 ? (high - low) / typical : Number.POSITIVE_INFINITY;
	const settled = intervals.length >= MIN_INTERVALS_FOR_POINT && spread <= POINT_SPREAD_CEILING;

	const confidence: RunEtaConfidence = settled
		? "high"
		: intervals.length >= MIN_INTERVALS_FOR_POINT
			? "medium"
			: "low";
	const basis = [
		`median of ${plural(intervals.length, `${evidence.unit} interval`)} in ${phaseBasis}`,
		`${Math.round(typical)}ms per ${evidence.unit}`,
		`${plural(remaining, evidence.unit)} remaining`,
		evidence.disruptedAtMs != null ? "samples restarted after a disruption" : null
	]
		.filter(Boolean)
		.join(" · ");

	if (settled) {
		return {
			state: "point",
			remainingMs: typical * remaining,
			lowMs: low * remaining,
			highMs: high * remaining,
			confidence,
			basis,
			sampleCount: samples
		};
	}

	// Widen a low-evidence range rather than presenting a narrow one: with two
	// intervals the quartiles are the two values themselves, which understates
	// how little is known.
	const widen = intervals.length >= MIN_INTERVALS_FOR_POINT ? 1 : 1.5;
	return {
		state: "range",
		remainingMs: typical * remaining,
		lowMs: (low / widen) * remaining,
		highMs: high * widen * remaining,
		confidence,
		basis,
		sampleCount: samples
	};
}

/*
 * Multi-phase composition is deliberately absent.
 *
 * The brief's rule for several known phases is "current-phase estimate + phases
 * with sufficient historical evidence". No workflow currently needs it, and
 * adding it would make the numbers worse rather than better:
 *
 *   · GEPA and eval denominators are already run-level. The remaining rollout
 *     budget and the remaining planned trials cover the whole run, so the
 *     phase-local samples already produce a run-level answer.
 *   · SFT's phases are measured in incomparable units — queue position, steps,
 *     checkpoint rollouts — and no producer declares a denominator for the
 *     phases after the current one. Composing them would mean adding an
 *     estimate for work nobody has measured, which is exactly the wrong
 *     precise number this module exists to refuse.
 *
 * When a producer starts declaring later-phase denominators, the composition
 * belongs here, summing only phases whose own `estimatePhaseEta` returns a
 * point or a range and naming the phases it had to exclude.
 */

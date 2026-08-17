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
 *   · Effective, not configured, concurrency. Observed completion rates already
 *     contain however many workers were really running; nothing is divided by a
 *     configured semaphore size that may not be staffed.
 *   · Phase-local samples only. A caller passes completions for one phase, so
 *     a phase transition resets the evidence rather than blending a fast
 *     minibatch into a slow full-train.
 *   · Rates over sub-windows, not gaps between completions. Real producers
 *     report in bursts: sixteen rollouts finish and land milliseconds apart.
 *     The gap between two of those is the reporting burst, not the work, and
 *     taking its median once produced a 23ms-per-rollout estimate on a real
 *     Banking77 run. Each sub-window instead spans real time and yields
 *     `intervals ÷ elapsed`, which a burst cannot deflate.
 *   · Robust, not extrapolated. The median sub-window rate decides, so one
 *     stalled or unusually fast stretch cannot move the estimate — it widens
 *     the range and drops the confidence instead.
 *   · Disruption widens. Retries, throttling, and worker loss drop the samples
 *     that preceded them, which drops confidence back to a range. Routine
 *     telemetry is not disruption: a queue-depth update every few seconds must
 *     not keep resetting the window, or the estimate never settles at all.
 *   · Queue time is never execution evidence. A caller measuring training
 *     passes only training completions; queue elapsed is displayed separately.
 *   · Chat elapsed time and tool-call silence are not inputs. They are not
 *     even in the signature.
 */

import type { RunEtaConfidence, RunEtaProjection } from "./types";

/**
 * Newest completions only; older rates describe a run that no longer exists.
 * Wide enough to span several reporting bursts, because a burst contributes many
 * completions and almost no elapsed time.
 */
const SAMPLE_WINDOW = 60;

/** Completions per sub-window. Below this a sub-window cannot span real work. */
const MIN_PER_SUB_WINDOW = 3;

/** Sub-windows to split the sample window into, at most. */
const MAX_SUB_WINDOWS = 4;

/** A point estimate needs this many independent rates and a settled spread. */
const MIN_RATES_FOR_POINT = 3;

/** Spread over the median above which a range is still the honest answer. */
const POINT_SPREAD_CEILING = 0.5;

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

/** Quantile by nearest rank; exact enough for a spread over a handful of rates. */
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

/** Duration formatted for a basis line, in the coarsest unit that stays exact-ish. */
function perUnitLabel(msPerUnit: number, unit: string): string {
	if (msPerUnit >= 60_000) return `${(msPerUnit / 60_000).toFixed(1)}min per ${unit}`;
	if (msPerUnit >= 1_000) return `${(msPerUnit / 1_000).toFixed(1)}s per ${unit}`;
	return `${Math.round(msPerUnit)}ms per ${unit}`;
}

/** Completions in the current window, ordered, after disruption pruning. */
export function usableCompletions(evidence: EtaEvidence): number[] {
	return evidence.completions
		.filter((value) => Number.isFinite(value))
		.filter((value) => evidence.disruptedAtMs == null || value > evidence.disruptedAtMs)
		.sort((left, right) => left - right)
		.slice(-SAMPLE_WINDOW);
}

/**
 * Completion rates, in units per millisecond, over contiguous sub-windows of the
 * sample window.
 *
 * Each sub-window holds at least `MIN_PER_SUB_WINDOW` completions and its rate is
 * `intervals ÷ elapsed`, so it measures work over real time. A reporting burst
 * inflates the count and the elapsed time of the *same* sub-window, which is why
 * this survives bursts where per-completion gaps do not.
 *
 * Exported because the sampling rule is the part most likely to be quietly wrong.
 */
export function completionRates(evidence: EtaEvidence): number[] {
	const usable = usableCompletions(evidence);
	if (usable.length < 2) return [];
	const subWindows = Math.max(1, Math.min(MAX_SUB_WINDOWS, Math.floor(usable.length / MIN_PER_SUB_WINDOW)));
	const size = Math.floor(usable.length / subWindows);
	const rates: number[] = [];
	for (let index = 0; index < subWindows; index += 1) {
		// The last sub-window absorbs the remainder so no completion is dropped.
		const from = index * size;
		const to = index === subWindows - 1 ? usable.length : from + size;
		const slice = usable.slice(from, to);
		if (slice.length < 2) continue;
		const span = slice.at(-1)! - slice[0]!;
		if (span <= 0) continue;
		rates.push((slice.length - 1) / span);
	}
	return rates;
}

/**
 * Estimate the time remaining in one bounded, homogeneous phase.
 *
 * Every case returns a projection whose `state` says what the caller may
 * display: a number, a range, "Estimating…", "Unavailable", or "Paused".
 */
export function estimatePhaseEta(evidence: EtaEvidence): RunEtaProjection {
	const usable = usableCompletions(evidence);
	const rates = completionRates(evidence);
	const samples = usable.length;
	const phaseBasis = `phase ${evidence.phaseId}`;

	if (evidence.paused) {
		return {
			state: "paused",
			confidence: rates.length >= MIN_RATES_FOR_POINT ? "medium" : "low",
			basis: `paused with ${plural(samples, `completed ${evidence.unit}`)} observed in ${phaseBasis}`,
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

	if (rates.length === 0) {
		return {
			state: "estimating",
			confidence: "warming",
			basis: `${plural(samples, `completed ${evidence.unit}`)} in ${phaseBasis}; two spanning real time are needed before an estimate`,
			sampleCount: samples
		};
	}

	const sorted = [...rates].sort((left, right) => left - right);
	const typicalRate = median(sorted);
	if (!(typicalRate > 0)) {
		return {
			state: "estimating",
			confidence: "warming",
			basis: `${plural(samples, `completed ${evidence.unit}`)} in ${phaseBasis} arrived too close together to time`,
			sampleCount: samples
		};
	}
	// A faster rate means less time left, so the fastest rate is the low bound.
	const fastest = quantile(sorted, 0.75);
	const slowest = quantile(sorted, 0.25);
	const spread = (fastest - slowest) / typicalRate;
	const settled = rates.length >= MIN_RATES_FOR_POINT && spread <= POINT_SPREAD_CEILING;

	const confidence: RunEtaConfidence = settled
		? "high"
		: rates.length >= MIN_RATES_FOR_POINT
			? "medium"
			: "low";
	const basis = [
		`median of ${plural(rates.length, "windowed rate")} over ${plural(samples, `completed ${evidence.unit}`)} in ${phaseBasis}`,
		perUnitLabel(1 / typicalRate, evidence.unit),
		`${plural(remaining, evidence.unit)} remaining`,
		evidence.disruptedAtMs != null ? "samples restarted after a disruption" : null
	]
		.filter(Boolean)
		.join(" · ");

	const remainingMs = remaining / typicalRate;
	if (settled) {
		return {
			state: "point",
			remainingMs,
			lowMs: remaining / fastest,
			highMs: remaining / slowest,
			confidence,
			basis,
			sampleCount: samples
		};
	}

	// Widen a low-evidence range rather than presenting a narrow one: with a
	// single rate the bounds are that rate itself, which understates how little
	// is known.
	const widen = rates.length >= MIN_RATES_FOR_POINT ? 1 : 1.5;
	return {
		state: "range",
		remainingMs,
		lowMs: remaining / (fastest * widen),
		highMs: (remaining / slowest) * widen,
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

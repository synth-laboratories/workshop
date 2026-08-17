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
 *
 * There is one estimator that beat all of this, and it does not use throughput at
 * all: the shape earlier runs of the same recipe traced through the same work.
 * When a caller supplies that history the estimate comes from it, and the
 * throughput machinery below is not consulted. See `history.ts` for the measured
 * comparison.
 */

import { estimateFromHistory, type HistoricalShape } from "./history";
import type { RunEtaConfidence, RunEtaProjection } from "./types";

/**
 * How much recent wall clock the rate is measured over. Wide enough to contain
 * the idle stretches between bursts — the observed worst case is a 150s proposer
 * call — so a window that happens to end mid-burst is not mistaken for speed.
 */
const WINDOW_MS = 600_000;

/** Completions a window needs before its rate is worth reporting at all. */
const MIN_COMPLETIONS = 2;

/** Completions a window needs before it can state a single number. */
const MIN_FOR_POINT = 12;

/**
 * The largest idle stretch a window may contain, as a share of its span, before
 * its completions stop being evidence about time.
 *
 * Measured against real runs, this is the rule that matters most. A window whose
 * biggest gap is most of its span is not watching a steady pipeline; it is
 * watching a run that spends long stretches doing work the completion counter
 * cannot see, and completions per second says nothing about when that finishes.
 * On the captured runs: GEPA's largest gap is 150s of a 223s run (67%), the eval
 * campaign's is 111s of 136s (82%). Both refuse, correctly — extrapolating from
 * them gave a median error of 4.7× and a p90 of 11×.
 */
const MAX_IDLE_SHARE = 0.25;

/** Half-to-half disagreement, over the window rate, above which a range is the honest answer. */
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
	 * Wall clock at the moment of projection. The window ends here, not at the
	 * last completion, so a run currently sitting in a proposer call is measured
	 * as the slower thing it presently is. Defaults to the last completion when a
	 * caller has no clock, which only under-counts trailing idle time.
	 */
	nowMs?: number;
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
	/**
	 * The recipe's own history, pooled from comparable finished runs. When
	 * present it is the estimate: it measured at 30% median error against 74% for
	 * throughput, and it is the only form that survived validation on real runs.
	 */
	history?: HistoricalShape;
	/** Work completed over work planned, 0–1. The input history is read at. */
	progressFraction?: number;
	/** Wall time this run has been going, for the historical division. */
	elapsedMs?: number;
};

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

/** A duration for a basis line, in the coarsest unit that stays legible. */
function formatSpan(ms: number): string {
	if (ms >= 60_000) return `${(ms / 60_000).toFixed(1)}min`;
	if (ms >= 1_000) return `${(ms / 1_000).toFixed(1)}s`;
	return `${Math.round(ms)}ms`;
}

/** Completions in the current window, ordered, after disruption pruning. */
export function usableCompletions(evidence: EtaEvidence): number[] {
	const nowMs = evidence.nowMs;
	return evidence.completions
		.filter((value) => Number.isFinite(value))
		.filter((value) => evidence.disruptedAtMs == null || value > evidence.disruptedAtMs)
		.filter((value) => nowMs == null || value <= nowMs)
		.sort((left, right) => left - right);
}

/**
 * Completions per millisecond across a run of timestamps, or null when they do
 * not span measurable time.
 *
 * `n - 1` intervals over the elapsed span: a burst raises the count and the span
 * together, so it cannot inflate the rate.
 */
export function completionRate(completions: number[]): number | null {
	if (completions.length < 2) return null;
	const span = completions.at(-1)! - completions[0]!;
	if (span <= 0) return null;
	return (completions.length - 1) / span;
}

export type EtaWindow = {
	/** Completions per millisecond over the window, or null when unmeasurable. */
	overall: number | null;
	first: number | null;
	second: number | null;
	/** Completions inside the window. */
	samples: number;
	/** Wall time the window covers, idle included. */
	spanMs: number;
	/** The longest stretch with no completion, including the tail up to now. */
	longestIdleMs: number;
};

/**
 * The window rate and its two half rates.
 *
 * The window runs from the first usable completion (or `WINDOW_MS` ago, whichever
 * is later) to *now*, and the rate is completions ÷ that elapsed time. Idle
 * stretches are inside the span on purpose: while a GEPA run is in a proposer
 * call it really is completing no rollouts, and an estimate that ignored the gap
 * would promise a finish time the run cannot meet.
 */
export function windowRates(evidence: EtaEvidence): EtaWindow {
	const usable = usableCompletions(evidence);
	const empty: EtaWindow = {
		overall: null,
		first: null,
		second: null,
		samples: 0,
		spanMs: 0,
		longestIdleMs: 0
	};
	if (usable.length === 0) return empty;
	const nowMs = evidence.nowMs ?? usable.at(-1)!;
	const earliest = Math.max(
		usable[0]!,
		nowMs - WINDOW_MS,
		evidence.disruptedAtMs ?? Number.NEGATIVE_INFINITY
	);
	const inWindow = usable.filter((value) => value >= earliest);
	const spanMs = nowMs - earliest;
	// The tail counts: a run that completed nothing for the last two minutes is
	// idle now, whatever it was doing before.
	let longestIdleMs = inWindow.length > 0 ? nowMs - inWindow.at(-1)! : spanMs;
	for (let index = 1; index < inWindow.length; index += 1) {
		longestIdleMs = Math.max(longestIdleMs, inWindow[index]! - inWindow[index - 1]!);
	}
	if (inWindow.length < MIN_COMPLETIONS || spanMs <= 0) {
		return { ...empty, samples: inWindow.length, spanMs: Math.max(0, spanMs), longestIdleMs };
	}
	const midpoint = earliest + spanMs / 2;
	const firstHalf = inWindow.filter((value) => value < midpoint).length;
	const secondHalf = inWindow.length - firstHalf;
	const halfSpan = spanMs / 2;
	// When the window opens on a completion, that completion marks a boundary
	// rather than a unit of work inside the span, so the span holds one fewer
	// interval than it holds completions. When the window opens on the clock
	// instead, every completion inside it counts.
	const anchored = inWindow[0] === earliest;
	return {
		overall: (inWindow.length - (anchored ? 1 : 0)) / spanMs,
		// A half with no completions has a real rate of zero, which is honest but
		// unusable as a bound; it is dropped so the range widens instead.
		first: firstHalf > 0 ? firstHalf / halfSpan : null,
		second: secondHalf > 0 ? secondHalf / halfSpan : null,
		samples: inWindow.length,
		spanMs,
		longestIdleMs
	};
}

/**
 * The estimate drawn from comparable finished runs of the same recipe.
 *
 * Returns null when there is no usable history, no measurable progress, or the
 * run is paused — the caller then falls through to the throughput machinery,
 * which will usually refuse.
 */
function estimateHistorical(evidence: EtaEvidence): RunEtaProjection | null {
	const shape = evidence.history;
	if (!shape || evidence.paused) return null;
	const progress = evidence.progressFraction;
	const elapsed = evidence.elapsedMs;
	if (progress == null || elapsed == null) return null;
	const estimate = estimateFromHistory(shape, progress, elapsed);
	if (!estimate) return null;

	const sampleCount = usableCompletions(evidence).length;
	const runs = `${estimate.runs} previous run${estimate.runs === 1 ? "" : "s"} of this recipe`;
	if (estimate.beyondHistory) {
		// The prediction has run past everything the recipe has ever done. A
		// confident-looking number drawn from a population this run no longer
		// resembles is worse than admitting the history has been left behind.
		return {
			state: "unavailable",
			confidence: "low",
			basis: `${runs} finished well before this one is projected to`,
			sampleCount,
			unavailableReason: `this run is already taking far longer than ${runs}, so how much longer it needs is beyond anything measured`
		};
	}

	// The spread of the historical band decides whether a single number is fair.
	const band = estimate.highMs - estimate.lowMs;
	const settled = estimate.remainingMs > 0 && band / estimate.remainingMs <= 0.6;
	const basis = [
		`${Math.round(progress * 100)}% of the work done in ${formatSpan(elapsed)}`,
		`${runs} were ${Math.round(estimate.expectedElapsedFraction * 100)}% through their time at this point`
	].join(" · ");
	return {
		state: settled ? "point" : "range",
		remainingMs: estimate.remainingMs,
		lowMs: estimate.lowMs,
		highMs: estimate.highMs,
		confidence: settled ? (estimate.runs >= 5 ? "high" : "medium") : "low",
		basis,
		sampleCount
	};
}

/**
 * Estimate the time remaining in one bounded, homogeneous phase.
 *
 * Every case returns a projection whose `state` says what the caller may
 * display: a number, a range, "Estimating…", "Unavailable", or "Paused".
 */
export function estimatePhaseEta(evidence: EtaEvidence): RunEtaProjection {
	const { overall, first, second, samples, spanMs, longestIdleMs } = windowRates(evidence);
	const phaseBasis = `phase ${evidence.phaseId}`;

	// History first, and history only, whenever it is available: it is the one
	// estimator that measured well, and mixing a worse signal into a better one
	// does not improve it.
	const historical = estimateHistorical(evidence);
	if (historical) return historical;

	if (evidence.paused) {
		return {
			state: "paused",
			confidence: samples >= MIN_FOR_POINT ? "medium" : "low",
			basis: `paused with ${plural(samples, `completed ${evidence.unit}`)} observed in ${phaseBasis}`,
			sampleCount: samples
		};
	}

	// A caller that already knows no estimate is possible says so, and its reason
	// is used verbatim. The basis still reports what was observed rather than
	// asserting a missing denominator the run may well have.
	if (evidence.unavailableReason) {
		return unavailable(
			`${plural(samples, `completed ${evidence.unit}`)} observed in ${phaseBasis}`,
			evidence.unavailableReason,
			samples
		);
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

	if (overall == null || !(overall > 0)) {
		return {
			state: "estimating",
			confidence: "warming",
			basis: `${plural(samples, `completed ${evidence.unit}`)} in ${phaseBasis}; two spanning real time are needed before an estimate`,
			sampleCount: samples
		};
	}

	// The evidence-quality gate. Completions are only evidence about *time* when
	// they arrive steadily enough that the gaps between them are small next to the
	// window. Otherwise the run is spending its wall clock somewhere this counter
	// cannot see, and any number derived from the counter would be fiction.
	const idleShare = longestIdleMs / spanMs;
	if (idleShare > MAX_IDLE_SHARE) {
		return unavailable(
			`${plural(samples, `completed ${evidence.unit}`)} in ${formatSpan(spanMs)} of ${phaseBasis}, but its longest idle stretch is ${formatSpan(longestIdleMs)}`,
			`the longest stretch with no completed ${evidence.unit} is ${formatSpan(longestIdleMs)} of a ${formatSpan(spanMs)} window, so ${evidence.unit} throughput does not predict when this run finishes`,
			samples
		);
	}

	// Faster means less time left, so the faster half gives the low bound.
	const halves = [first, second].filter((rate): rate is number => rate != null && rate > 0);
	const bounded = halves.length === 2;
	const fastest = bounded ? Math.max(...halves) : overall;
	const slowest = bounded ? Math.min(...halves) : overall;
	const spread = bounded ? (fastest - slowest) / overall : Number.POSITIVE_INFINITY;
	const settled = samples >= MIN_FOR_POINT && bounded && spread <= POINT_SPREAD_CEILING;

	const confidence: RunEtaConfidence = settled
		? "high"
		: samples >= MIN_FOR_POINT
			? "medium"
			: "low";
	const basis = [
		`${plural(samples, `completed ${evidence.unit}`)} in ${formatSpan(spanMs)} of ${phaseBasis}`,
		`${formatSpan(1 / overall)} each`,
		`${plural(remaining, evidence.unit)} remaining`,
		bounded && !settled ? "throughput is still changing" : null,
		evidence.disruptedAtMs != null ? "samples restarted after a disruption" : null
	]
		.filter(Boolean)
		.join(" · ");

	const remainingMs = remaining / overall;
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

	// Widen a low-evidence range rather than presenting a narrow one: a window
	// with only one usable half knows nothing about its own trend.
	const widen = bounded ? 1 : 1.5;
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

/**
 * Evaluation campaigns → `run_progress.v1`.
 *
 * The denominator here is real and frozen: the campaign plan declares
 * `plannedTrials` before compute starts, so the bar can honestly say "68 / 100
 * trials". What it must never say is anything about reward. The bar measures
 * campaign completion; a candidate scoring badly is a *result*, not a lack of
 * progress, and a run that finished with no champion finished.
 *
 * Retries are counted, not double-counted. The projection keys trials by id,
 * so a retried trial re-enters the same slot; the retry count comes from the
 * retry events themselves and is shown beside the completed count rather than
 * inflating it.
 */

import type {
	EvalState,
	ProjectedState
} from "@synth/visual-templates/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import { estimatePhaseEta, type EtaEvidence } from "./eta";
import type {
	RunProgressDetail,
	RunProgressPhase,
	RunProgressProjection,
	RunProgressResult,
	RunProgressWork
} from "./types";
import type { AdapterInput } from "./adapterShared";
import {
	baseProjection,
	evidenceOf,
	frozenWork,
	lastDisruptionMs,
	milestoneFromEvents,
	rolloutCompletionTimes,
	usageProjection
} from "./adapterShared";

/** A trial reaching an end state, valid or not. The only completion signal. */
const EVAL_COMPLETION_TYPES = ["eval.trial.terminal"];
const EVAL_START_TYPES = new Set(["eval.trial.queued", "eval.trial.started"]);

export enum EvalProjectionErrorCode {
	TrialCountMismatch = "eval_trial_count_mismatch"
}

export class EvalProjectionError extends Error {
	constructor(readonly code: EvalProjectionErrorCode, message: string) {
		super(`${code}: ${message}`);
		this.name = "EvalProjectionError";
	}
}

const STAGE_ORDER: Array<{ id: string; label: string }> = [
	{ id: "plan", label: "Plan" },
	{ id: "screen", label: "Screen" },
	{ id: "prune", label: "Prune" },
	{ id: "confirm", label: "Confirm" },
	{ id: "select", label: "Select" }
];

/**
 * Trial counts. `terminal` is trials that reached an end state, which is the
 * only honest numerator for campaign completion — a running trial has produced
 * nothing yet.
 */
function counts(state: EvalState) {
	let terminal = 0;
	let valid = 0;
	let failed = 0;
	let running = 0;
	let queued = 0;
	for (const trial of state.trials) {
		if (trial.status === "running") running += 1;
		else if (trial.status === "queued") queued += 1;
		else {
			terminal += 1;
			if (trial.valid) valid += 1;
			else failed += 1;
		}
	}
	return { terminal, valid, failed, running, queued };
}

function evalPhases(state: EvalState, terminal: boolean, failed: boolean): RunProgressPhase[] {
	const screen = state.trials.filter((trial) => trial.stage === "screen");
	const confirm = state.trials.filter((trial) => trial.stage === "confirm");
	const screenCards = state.scorecards.filter((card) => card.stage === "screen");
	const confirmCards = state.scorecards.filter((card) => card.stage === "confirm");
	const eliminated = screenCards.filter((card) => card.eliminationReason);
	const hasConfirmSeeds = (state.seedLedger?.confirmation.length ?? 0) > 0;
	const settledStage = (trials: typeof screen, cards: typeof screenCards) =>
		trials.length > 0 && cards.length > 0 &&
		trials.every((trial) => trial.status !== "queued" && trial.status !== "running");

	const settle = (
		id: string,
		label: string,
		started: boolean,
		done: boolean,
		detail?: string
	): RunProgressPhase => {
		if (done) return { id, label, status: "completed", detail };
		if (started) return { id, label, status: terminal ? (failed ? "failed" : "completed") : "active", detail };
		return { id, label, status: terminal ? "skipped" : "pending", detail };
	};

	const screenDone = settledStage(screen, screenCards);
	return [
		settle(
			STAGE_ORDER[0]!.id,
			STAGE_ORDER[0]!.label,
			state.plannedTrials > 0,
			state.seedLedger !== null,
			state.plannedTrials > 0 ? `${state.plannedTrials} trials planned` : undefined
		),
		settle(
			STAGE_ORDER[1]!.id,
			STAGE_ORDER[1]!.label,
			screen.length > 0,
			screenDone,
			screen.length > 0 ? `${screen.length} trials` : undefined
		),
		screenDone && eliminated.length === 0
			? { id: STAGE_ORDER[2]!.id, label: STAGE_ORDER[2]!.label, status: "skipped", detail: "no rule fired" }
			: settle(
					STAGE_ORDER[2]!.id,
					STAGE_ORDER[2]!.label,
					eliminated.length > 0,
					eliminated.length > 0,
					eliminated.length > 0 ? `${eliminated.length} eliminated` : undefined
				),
		hasConfirmSeeds
			? settle(
					STAGE_ORDER[3]!.id,
					STAGE_ORDER[3]!.label,
					confirm.length > 0,
					settledStage(confirm, confirmCards),
					confirm.length > 0 ? `${confirm.length} trials` : undefined
				)
			: { id: STAGE_ORDER[3]!.id, label: STAGE_ORDER[3]!.label, status: "skipped", detail: "report-only recipe" },
		settle(
			STAGE_ORDER[4]!.id,
			STAGE_ORDER[4]!.label,
			state.selection !== null,
			state.selection !== null,
			state.selection?.status
		)
	];
}

/**
 * Retries, derived rather than announced: the eval producer has no retry event
 * type, so a re-queue is a trial id entering the queue again after it has
 * already been seen. Counting it here is what keeps `completed` honest — the
 * projection keys trials by id, so a retried trial re-enters its own slot
 * instead of adding a second completion.
 *
 * The newest re-queue also invalidates the timing evidence before it: a trial
 * that had to be run twice says nothing about how long the next one takes.
 */
function retryEvidence(events: AdapterInput["events"]): { retried: number; lastRequeueMs?: number } {
	// Attempts are counted per trial *per spelling*, because a producer may emit
	// `queued`, `started`, or both for one attempt. Taking the larger of the two
	// counts gives the attempt count either way, and attempts − 1 is the retries.
	const attempts = new Map<string, Map<string, number>>();
	let lastRequeueMs: number | undefined;
	for (const event of events) {
		if (!EVAL_START_TYPES.has(event.type)) continue;
		const trialId = event.delta?.trial_id;
		if (trialId == null) continue;
		const perTrial = attempts.get(String(trialId)) ?? new Map<string, number>();
		const seen = (perTrial.get(event.type) ?? 0) + 1;
		perTrial.set(event.type, seen);
		attempts.set(String(trialId), perTrial);
		if (seen > 1) {
			const at = Date.parse(event.occurredAt);
			if (Number.isFinite(at)) lastRequeueMs = at;
		}
	}
	let retried = 0;
	for (const perTrial of attempts.values()) {
		retried += Math.max(0, Math.max(...perTrial.values()) - 1);
	}
	return { retried, ...(lastRequeueMs != null ? { lastRequeueMs } : {}) };
}

function evalDetails(state: EvalState): RunProgressDetail[] {
	const details: RunProgressDetail[] = [];
	details.push({ label: "Candidates", value: String(state.candidates.length) });
	if (state.candidateSetId) {
		details.push({ label: "Candidate set", value: state.candidateSetId, note: "content-addressed at staging" });
	}
	if (state.manifestDigest) details.push({ label: "Plan digest", value: state.manifestDigest });
	if (state.parallelism != null) details.push({ label: "Parallelism", value: String(state.parallelism) });
	if (state.globalCapacity != null) details.push({ label: "Global capacity", value: String(state.globalCapacity) });
	if (state.seedLedger) {
		details.push({
			label: "Seeds",
			value: `${state.seedLedger.screening.length} screening · ${state.seedLedger.confirmation.length} confirmation`
		});
	}
	if (state.evidenceDir) details.push({ label: "Evidence", value: state.evidenceDir });
	return details;
}

/** Failure classes, so a rig problem never reads as a bad policy. */
function failureWarnings(state: EvalState): string[] {
	const byGate = new Map<string, number>();
	for (const card of state.scorecards) {
		for (const [gate, count] of Object.entries(card.gateFailures)) {
			byGate.set(gate, (byGate.get(gate) ?? 0) + count);
		}
	}
	const worst = [...byGate.entries()].sort((left, right) => right[1] - left[1])[0];
	if (!worst) return [];
	return [`${worst[1]} trial${worst[1] === 1 ? "" : "s"} failed the ${worst[0]} gate`];
}

function evalResult(state: EvalState, tally: ReturnType<typeof counts>, failed: boolean): RunProgressResult {
	const partial = tally.failed > 0 || tally.terminal < state.plannedTrials;
	const selection = state.selection;
	if (failed && !selection) {
		return { headline: "Campaign failed", partial: true };
	}
	if (!selection) {
		return { absentReason: "no selection decision was emitted", partial };
	}
	if (selection.status === "promoted" && selection.winnerId) {
		return {
			headline: `Promoted ${selection.winnerId}`,
			detail: selection.lift != null
				? `${selection.lift >= 0 ? "+" : ""}${selection.lift.toFixed(3)} ${selection.primaryMetric} vs baseline`
				: selection.reason,
			partial
		};
	}
	return {
		absentReason: selection.reason || `selection was ${selection.status.replaceAll("_", " ")}`,
		detail: `${tally.valid} valid of ${tally.terminal} finished trials`,
		partial
	};
}

export function projectEval(input: AdapterInput, projected: ProjectedState): RunProgressProjection {
	const state = projected.eval;
	const base = baseProjection(input, "eval");
	if (!state) return base;

	const terminal = base.terminal;
	const failed = base.status === "failed";
	const tally = counts(state);
	const represented = tally.terminal + tally.running + tally.queued;
	if (state.plannedTrials > 0 && represented > state.plannedTrials) {
		throw new EvalProjectionError(
			EvalProjectionErrorCode.TrialCountMismatch,
			`plan declares ${state.plannedTrials} trials but lifecycle state contains ${represented}`
		);
	}
	const { retried, lastRequeueMs } = retryEvidence(input.events);
	const frozen = terminal ? frozenWork(input.run) : undefined;
	// Terminal counts come from the sealed manifest when there is one, so a late
	// poll cannot restate how the campaign ended.
	const planned = frozen?.planned ?? (state.plannedTrials > 0 ? state.plannedTrials : undefined);
	const settled = frozen != null
		? (frozen.succeeded ?? 0) + (frozen.failed ?? 0)
		: tally.terminal;
	const failures = frozen?.failed ?? tally.failed;
	const workEvidence = evidenceOf(input, state.trials.length + state.plannedTrials, "trial");
	// The count is omitted, not zeroed, when nothing proves it. `0 / 10 trials`
	// on a campaign that ran all ten is a worse answer than saying so — and a
	// campaign that has not declared a plan yet has no denominator to count
	// against either, so it reports neither rather than a bare `0 trials`.
	const measured =
		workEvidence.state === "present" &&
		(frozen != null || state.plannedTrials > 0 || state.trials.length > 0);
	const work: RunProgressWork = measured
		? {
				completed: settled,
				active: tally.running,
				queued: tally.queued,
				...(failures > 0 ? { failed: failures } : {}),
				...(retried > 0 ? { retried } : {}),
				...(planned != null ? { total: planned } : {}),
				unit: "trials"
			}
		: { unit: "trials" };

	const phases = evalPhases(state, terminal, failed);
	const active = phases.find((phase) => phase.status === "active");
	const determinate = measured && planned != null;
	const fraction = determinate ? Math.min(1, settled / planned!) : undefined;

	// A paused campaign holds the matrix; in-flight trials still seal, so the
	// status is paused while `active` may stay non-zero. That is not a conflict.
	const paused = state.paused || base.status === "paused";
	const etaEvidence: EtaEvidence = {
		phaseId: active?.id ?? (terminal ? "select" : "screen"),
		completions: rolloutCompletionTimes(input.events, EVAL_COMPLETION_TYPES),
		remainingUnits: determinate ? Math.max(0, planned! - settled) : undefined,
		unit: "trial",
		disruptedAtMs: (() => {
			const breaker = lastDisruptionMs(input.events, ["rollout.circuit_breaker.tripped"]);
			if (breaker == null) return lastRequeueMs;
			if (lastRequeueMs == null) return breaker;
			return Math.max(breaker, lastRequeueMs);
		})(),
		paused,
		unavailableReason: determinate ? undefined : "the campaign plan declared no trial count"
	};

	const warnings = [...base.warnings, ...failureWarnings(state)];
	if (terminal && tally.queued > 0) {
		warnings.unshift(
			`This campaign is ${failed ? "failed" : "finished"}, but ${tally.queued} ${tally.queued === 1 ? "trial is" : "trials are"} still queued.`
		);
	}
	if (workEvidence.state !== "present" && workEvidence.reason) {
		warnings.unshift(
			workEvidence.diagnostic
				? `${workEvidence.reason} · ${workEvidence.diagnostic}`
				: workEvidence.reason
		);
	}
	const rate = (() => {
		const times = etaEvidence.completions;
		if (times.length < 2) return undefined;
		const window = times.filter((time) => time >= times.at(-1)! - 60_000);
		if (window.length < 2) return undefined;
		const span = window.at(-1)! - window[0]!;
		return span > 0 ? ((window.length - 1) * 60_000) / span : undefined;
	})();

	const milestone = (() => {
		// A distribution milestone is only worth stating once enough trials are
		// scored for the mean to mean anything.
		const scored = state.scorecards.filter((card) => card.trials.valid >= 3);
		const best = scored
			.map((card) => ({ card, mean: card.metrics[0]?.mean }))
			.filter((entry): entry is { card: typeof scored[number]; mean: number } => entry.mean != null)
			.sort((left, right) => right.mean - left.mean)[0];
		if (!best) return milestoneFromEvents(input.events);
		return {
			label: `${best.card.label} leads at ${best.mean.toFixed(3)}`,
			detail: `${best.card.trials.valid} valid trials${
				best.card.pairedLift != null ? ` · ${best.card.pairedLift >= 0 ? "+" : ""}${best.card.pairedLift.toFixed(3)} paired lift` : ""
			}`
		};
	})();
	const milestones = milestone ? [...base.milestones, milestone] : base.milestones;

	return {
		...base,
		status: paused && !terminal ? "paused" : base.status,
		phase: active ?? {
			id: terminal ? "select" : "plan",
			label: terminal ? "Campaign finished" : "Planning campaign",
			status: terminal ? "completed" : "active"
		},
		phases,
		work,
		evidence: workEvidence,
		progress: {
			...(fraction != null ? { fraction } : {}),
			semantics: "campaign completion",
			determinate
		},
		timing: {
			...base.timing,
			...(terminal ? {} : { eta: estimatePhaseEta(etaEvidence) })
		},
		usage: usageProjection(projected, input.events, planned, "container"),
		...(rate != null
			? {
					throughput: {
						label: `${rate >= 10 ? rate.toFixed(0) : rate.toFixed(1)} trials/min`,
						detail: state.parallelism != null ? `${state.parallelism} parallel` : undefined
					}
				}
			: state.parallelism != null
				? { throughput: { label: `${state.parallelism} parallel`, detail: "no completions timed yet" } }
				: {}),
		milestone: milestones.at(-1),
		milestones,
		warning: warnings[0],
		warnings,
		details: evalDetails(state),
		...(terminal ? { result: evalResult(state, tally, failed) } : {})
	};
}

/**
 * GEPA → `run_progress.v1`.
 *
 * The GEPA workspace already derives phase, rollout budget, proposer calls,
 * concurrency, queue depth, rollouts/minute, cost, and elapsed time. This
 * adapter reads that same projection rather than re-deriving anything from raw
 * events, so the transcript card and the full visual cannot disagree.
 *
 * Progress uses explicit bounded work — the `total_rollouts` limit — and never
 * frontier quality. An incumbent score of 0.86 is not "86% complete"; it is a
 * measurement of a candidate, and treating it as progress would let a run that
 * found a good candidate early look nearly finished when it has 200 rollouts
 * left to spend.
 */

import type { GepaState, ProjectedState } from "@synth/visual-templates/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import { estimatePhaseEta, type EtaEvidence } from "./eta";
import type {
	RunProgressDetail,
	RunProgressMilestone,
	RunProgressPhase,
	RunProgressProjection,
	RunProgressResult,
	RunProgressWork
} from "./types";
import type { AdapterInput } from "./adapterShared";
import {
	baseProjection,
	lastDisruptionMs,
	milestoneFromEvents,
	rolloutCompletionTimes,
	usageProjection
} from "./adapterShared";

/** Rollout-completion event types GEPA emits, in producer spelling. */
const GEPA_COMPLETION_TYPES = [
	"optimizer.evaluation_result.received",
	"optimizer.child_rollout.completed"
];

/**
 * Events that invalidate earlier timing samples: the rig changed under us.
 *
 * `optimizer.rollout_queue.updated` is deliberately *not* here. It is routine
 * telemetry — a real Banking77 run emits it every few seconds — and treating it
 * as a disruption reset the sample window continuously, so the estimate thrashed
 * between "Estimating…" and a number 113 times across one run and never settled.
 * A changed queue depth is not a changed rig.
 */
const GEPA_DISRUPTION_TYPES = [
	"optimizer.child_rollout.failed",
	"optimizer.child_rollout.retried",
	"optimizer.rollout.retried",
	"rollout.circuit_breaker.tripped"
];

function gepaPhases(gepa: GepaState): RunProgressPhase[] {
	return gepa.stages.map((stage) => ({
		id: stage.id,
		label: stage.label,
		status: stage.status,
		detail: stage.detail,
		startedAt: stage.startedAt,
		endedAt: stage.endedAt
	}));
}

function gepaWork(gepa: GepaState): RunProgressWork {
	const rolloutLimit = gepa.limits.find((limit) => limit.kind === "total_rollouts");
	const failed = gepa.failedAttempts.length;
	return {
		completed: gepa.rolloutsCompleted,
		...(gepa.runtime.activeWorkers != null ? { active: gepa.runtime.activeWorkers } : {}),
		...(gepa.runtime.queuedRollouts != null ? { queued: gepa.runtime.queuedRollouts } : {}),
		...(failed > 0 ? { failed } : {}),
		...(rolloutLimit?.max != null ? { total: rolloutLimit.max } : {}),
		unit: "rollouts"
	};
}

function gepaThroughput(gepa: GepaState): RunProgressProjection["throughput"] {
	const rate = gepa.runtime.rolloutsPerMinute;
	const active = gepa.runtime.activeWorkers;
	const queued = gepa.runtime.queuedRollouts;
	const concurrency = [
		active != null ? `${active} active` : null,
		queued != null ? `${queued} queued` : null
	].filter(Boolean).join(" · ");
	if (rate != null && Number.isFinite(rate) && rate > 0) {
		return {
			label: `${rate >= 10 ? rate.toFixed(0) : rate.toFixed(1)} rollouts/min`,
			detail: concurrency || undefined
		};
	}
	if (concurrency) return { label: concurrency };
	// A configured semaphore is capacity, not throughput; say which it is.
	return gepa.runtime.semaphoreSize != null
		? { label: `${gepa.runtime.semaphoreSize} worker capacity`, detail: "no completions timed yet" }
		: undefined;
}

function gepaDetails(gepa: GepaState): RunProgressDetail[] {
	const details: RunProgressDetail[] = [];
	if (gepa.models.policy) details.push({ label: "Policy", value: gepa.models.policy });
	if (gepa.models.proposer) details.push({ label: "Proposer", value: gepa.models.proposer });
	details.push({ label: "Candidates", value: String(gepa.candidates.length) });
	if (gepa.incumbentId) details.push({ label: "Incumbent", value: gepa.incumbentId });
	if (gepa.best?.trainReward != null) {
		details.push({
			label: "Best train reward",
			value: gepa.best.trainReward.toFixed(3),
			note: "train evidence — not heldout"
		});
	}
	for (const limit of gepa.limits) {
		const label = limit.kind === "total_rollouts"
			? "Rollout budget"
			: limit.kind === "proposer_calls"
				? "Proposer calls"
				: limit.kind === "cost_usd"
					? "Cost limit"
					: limit.kind === "wall_time_seconds"
						? "Wall-time limit"
						: limit.kind.replaceAll("_", " ");
		details.push({
			label,
			value: `${limit.spent ?? "—"} / ${limit.max ?? "—"}`,
			note: limit.kind === gepa.nearestLimit?.kind ? "nearest limit" : undefined
		});
	}
	return details;
}

function gepaMilestone(gepa: GepaState): RunProgressMilestone | undefined {
	const newest = gepa.frontierHistory.at(-1);
	if (!newest?.bestCandidateId) return undefined;
	return {
		label: `New incumbent ${newest.bestCandidateId}`,
		detail: newest.bestTrainReward != null
			? `train reward ${newest.bestTrainReward.toFixed(3)}`
			: newest.reason,
		occurredAt: newest.occurredAt,
		sequence: newest.sequence
	};
}

function gepaResult(gepa: GepaState, failed: boolean): RunProgressResult {
	const heldout = gepa.heldout;
	const partial = gepa.failedAttempts.length > 0 || failed;
	if (failed) {
		return {
			headline: "Search failed",
			detail: gepa.activity.detail,
			partial: true
		};
	}
	if (heldout?.reward != null) {
		return {
			headline: `Heldout ${heldout.reward.toFixed(3)}`,
			detail: heldout.candidateId ? `candidate ${heldout.candidateId}` : undefined,
			partial
		};
	}
	if (heldout?.skipped || heldout?.blocked) {
		return {
			absentReason: heldout.reason ?? "heldout evaluation did not run",
			detail: gepa.best?.trainReward != null
				? `best train reward ${gepa.best.trainReward.toFixed(3)} — train evidence only`
				: undefined,
			partial
		};
	}
	if (gepa.best?.trainReward != null) {
		return {
			headline: `Best train reward ${gepa.best.trainReward.toFixed(3)}`,
			detail: "no heldout evaluation was emitted",
			partial
		};
	}
	return { absentReason: "no candidate was scored", partial };
}

export function projectGepa(input: AdapterInput, projected: ProjectedState): RunProgressProjection {
	const gepa = projected.gepa;
	const base = baseProjection(input, "gepa");
	if (!gepa) return base;

	const phases = gepaPhases(gepa);
	const active = phases.find((phase) => phase.status === "active");
	const work = gepaWork(gepa);
	const terminal = base.terminal;
	const determinate = work.total != null && work.total > 0;
	const fraction = determinate && work.completed != null
		? Math.min(1, work.completed / work.total!)
		: undefined;

	const remaining = determinate && work.completed != null
		? Math.max(0, work.total! - work.completed)
		: undefined;
	const phaseId = active?.id ?? gepa.activity.phase;
	const evidence: EtaEvidence = {
		phaseId,
		completions: rolloutCompletionTimes(input.events, GEPA_COMPLETION_TYPES),
		remainingUnits: remaining,
		unit: "rollout",
		nowMs: input.now,
		disruptedAtMs: lastDisruptionMs(input.events, GEPA_DISRUPTION_TYPES),
		paused: base.status === "paused",
		/*
		 * GEPA does not get a time estimate, and this is not a gap to be filled
		 * later — it is what the run's shape allows.
		 *
		 * The rollout budget is a truthful denominator for *progress*: 68 of 100
		 * rollouts is a fact. It is not a basis for *time*, because a GEPA run
		 * alternates rollout evaluation with proposer calls that complete no
		 * rollouts at all. On the captured Banking77 run the largest such gap is
		 * 150 seconds of a 223-second run, and rollouts arrive 13ms apart inside
		 * bursts. Extrapolating remaining time from rollout throughput therefore
		 * missed by a median of 4.7× and by 11× at the p90 — worse than saying
		 * nothing, which is what the card now does.
		 *
		 * The counts, the phase, and the observed rollout rate are all still
		 * shown; only the promise about the clock is withheld.
		 */
		unavailableReason: determinate
			? "a GEPA run alternates rollouts with proposer calls that complete none, so rollout throughput does not predict when it finishes"
			: "no rollout budget was declared for this run"
	};

	const warnings = [...base.warnings];
	if (gepa.runtime.job?.state === "terminated") {
		warnings.unshift(
			gepa.runtime.job.message ?? "the rollout circuit breaker terminated this run"
		);
	}
	if (gepa.runtime.costTelemetryComplete === false) {
		warnings.push("some rollouts reported no cost; the total below is a floor");
	}

	const milestone = gepaMilestone(gepa) ?? milestoneFromEvents(input.events);
	const milestones = milestone ? [...base.milestones, milestone] : base.milestones;

	return {
		...base,
		phase: active ?? {
			id: phaseId,
			label: gepa.activity.label,
			status: terminal ? "completed" : "active",
			detail: gepa.activity.detail
		},
		phases,
		work,
		progress: {
			...(fraction != null ? { fraction } : {}),
			semantics: "rollout budget spent",
			determinate
		},
		timing: {
			...base.timing,
			startedAt: gepa.timing.startedAt ?? base.timing.startedAt,
			...(terminal ? {} : { eta: estimatePhaseEta(evidence) })
		},
		usage: usageProjection(projected, input.events, work.total, "container"),
		throughput: gepaThroughput(gepa),
		milestone: milestones.at(-1),
		milestones,
		warning: warnings[0],
		warnings,
		details: gepaDetails(gepa),
		...(terminal ? { result: gepaResult(gepa, base.status === "failed") } : {})
	};
}

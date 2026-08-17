/**
 * Long-running environment workflows → `run_progress.v1`.
 *
 * Craftax and other container-backed environment runs are not GEPA and not an
 * eval campaign. They count steps (or episodes) the environment actually
 * executed. Reward is a result, never progress — a poor episode that ran to
 * `max_steps` is 100% complete.
 *
 * The producer declares a denominator in `environment.run.planned` when it
 * has one. Without it the bar is indeterminate and the ETA is withheld.
 */

import type { ProjectedState } from "@synth/visual-templates/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
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
	lastDisruptionMs,
	milestoneFromEvents,
	rolloutCompletionTimes,
	usageProjection
} from "./adapterShared";

const STEP_TYPES = ["environment.step.completed"];
const EPISODE_TERMINAL_TYPES = ["environment.episode.terminal", "container.rollout.completed"];
const DISRUPTION_TYPES = ["container.rollout.failed", "environment.run.failed", "rollout.circuit_breaker.tripped"];

function numberField(source: Record<string, unknown> | undefined, keys: string[]): number | undefined {
	if (!source) return undefined;
	for (const key of keys) {
		const value = source[key];
		if (typeof value === "number" && Number.isFinite(value) && value >= 0) return value;
	}
	return undefined;
}

function planOf(events: AdapterInput["events"]) {
	const planned = [...events].reverse().find((event) => event.type === "environment.run.planned");
	const snapshot = planned?.snapshot ?? planned?.delta ?? {};
	return {
		maxSteps: numberField(snapshot, ["max_steps", "maxSteps", "planned_steps", "plannedSteps"]),
		plannedEpisodes: numberField(snapshot, ["planned_episodes", "plannedEpisodes", "episode_count", "episodeCount"]),
		seed: numberField(snapshot, ["seed"]),
		runtimeFamily: typeof snapshot.runtime_family === "string"
			? snapshot.runtime_family
			: typeof snapshot.runtimeFamily === "string"
				? snapshot.runtimeFamily
				: undefined
	};
}

function environmentPhases(
	events: AdapterInput["events"],
	terminal: boolean,
	failed: boolean
): RunProgressPhase[] {
	const prepared = events.some((event) =>
		event.type === "environment.run.planned" ||
		event.type === "container.task_info.loaded" ||
		event.type === "container.contract.verified"
	);
	const started = events.some((event) =>
		event.type === "environment.run.started" ||
		event.type === "environment.episode.started" ||
		event.type === "container.rollout.start"
	);
	const stepped = events.some((event) => event.type === "environment.step.completed");
	const sealed = events.some((event) =>
		event.type === "environment.episode.terminal" ||
		event.type === "container.rollout.completed" ||
		event.type === "environment.run.completed"
	);
	const settle = (
		id: string,
		label: string,
		hasStarted: boolean,
		done: boolean,
		detail?: string
	): RunProgressPhase => {
		if (done) return { id, label, status: "completed", detail };
		if (hasStarted) {
			return { id, label, status: terminal ? (failed ? "failed" : "completed") : "active", detail };
		}
		return { id, label, status: terminal ? "skipped" : "pending", detail };
	};
	return [
		settle("prepare", "Prepare", prepared, prepared && started, "container contract"),
		settle("episode", "Episode", started || stepped, sealed, stepped ? "environment steps" : undefined),
		settle("seal", "Seal", sealed || terminal, terminal && !failed, "trace and accounting")
	];
}

function environmentWork(events: AdapterInput["events"], plan: ReturnType<typeof planOf>): RunProgressWork {
	const steps = events.filter((event) => event.type === "environment.step.completed").length;
	const episodes = events.filter((event) => EPISODE_TERMINAL_TYPES.includes(event.type)).length;
	if (plan.maxSteps != null) {
		return { completed: steps, total: plan.maxSteps, unit: "steps" };
	}
	if (plan.plannedEpisodes != null) {
		return { completed: episodes, total: plan.plannedEpisodes, unit: "episodes" };
	}
	if (steps > 0) return { completed: steps, unit: "steps" };
	if (episodes > 0) return { completed: episodes, unit: "episodes" };
	return { unit: "steps" };
}

function environmentDetails(events: AdapterInput["events"], plan: ReturnType<typeof planOf>): RunProgressDetail[] {
	const details: RunProgressDetail[] = [];
	if (plan.runtimeFamily) details.push({ label: "Runtime", value: plan.runtimeFamily });
	if (plan.seed != null) details.push({ label: "Seed", value: String(plan.seed) });
	const task = events.find((event) => event.type === "container.task_info.loaded");
	const taskName = task?.delta?.task_name ?? task?.delta?.taskName;
	if (typeof taskName === "string") details.push({ label: "Task", value: taskName });
	const lastStep = [...events].reverse().find((event) => event.type === "environment.step.completed");
	const action = lastStep?.delta?.action;
	if (typeof action === "string") details.push({ label: "Last action", value: action });
	return details;
}

function environmentResult(events: AdapterInput["events"], failed: boolean): RunProgressResult {
	if (failed) return { headline: "Environment run failed", partial: true };
	const sealed = [...events].reverse().find((event) => event.type === "environment.episode.terminal")
		?? [...events].reverse().find((event) => event.type === "container.rollout.completed");
	const raw = sealed?.item?.raw;
	const reward = sealed?.delta?.reward ?? sealed?.delta?.value ?? raw?.reward;
	if (typeof reward === "number" && Number.isFinite(reward)) {
		return { headline: `Reward ${reward}`, partial: false };
	}
	if (sealed) {
		return { absentReason: "episode finished without a reported reward", partial: true };
	}
	return { absentReason: "no episode reached a terminal record", partial: true };
}

export function projectEnvironment(input: AdapterInput, projected: ProjectedState): RunProgressProjection {
	const base = baseProjection(input, "environment");
	const plan = planOf(input.events);
	const failed = base.status === "failed";
	const phases = environmentPhases(input.events, base.terminal, failed);
	const active = phases.find((phase) => phase.status === "active");
	const rawWork = environmentWork(input.events, plan);
	const workEvidence = evidenceOf(input, input.events.length, "step");
	const work = workEvidence.state === "present" ? rawWork : { unit: rawWork.unit };
	const determinate = work.total != null && work.total > 0 && work.completed != null;
	const fraction = determinate ? Math.min(1, work.completed! / work.total!) : undefined;
	const unit = work.unit === "episodes" ? "episode" : "step";
	const completions = work.unit === "episodes"
		? rolloutCompletionTimes(input.events, EPISODE_TERMINAL_TYPES)
		: rolloutCompletionTimes(input.events, STEP_TYPES);
	const evidence: EtaEvidence = {
		phaseId: active?.id ?? (base.terminal ? "seal" : "episode"),
		completions,
		remainingUnits: determinate ? Math.max(0, work.total! - work.completed!) : undefined,
		unit,
		disruptedAtMs: lastDisruptionMs(input.events, DISRUPTION_TYPES),
		paused: base.status === "paused",
		unavailableReason: determinate ? undefined : "the environment run declared no step or episode count"
	};

	const milestone = (() => {
		const last = [...input.events].reverse().find((event) =>
			event.type === "environment.episode.terminal" || event.type === "environment.step.completed"
		);
		if (last?.type === "environment.episode.terminal") {
			return { label: "Episode sealed", occurredAt: last.occurredAt, sequence: last.sequenceNumber };
		}
		if (last?.type === "environment.step.completed") {
			const step = last.delta?.step;
			return {
				label: typeof step === "number" ? `Step ${step}` : "Environment step",
				occurredAt: last.occurredAt,
				sequence: last.sequenceNumber
			};
		}
		return milestoneFromEvents(input.events);
	})();
	const milestones = milestone ? [...base.milestones, milestone] : base.milestones;

	const rate = (() => {
		if (completions.length < 2) return undefined;
		const span = completions.at(-1)! - completions[0]!;
		if (span <= 0) return undefined;
		const perMinute = ((completions.length - 1) * 60_000) / span;
		return `${perMinute >= 10 ? perMinute.toFixed(0) : perMinute.toFixed(1)} ${unit}s/min`;
	})();

	return {
		...base,
		phase: active ?? {
			id: base.terminal ? "seal" : "prepare",
			label: base.terminal ? "Finished" : "Preparing environment",
			status: base.terminal ? "completed" : "active"
		},
		phases,
		work,
		evidence: workEvidence,
		progress: {
			...(fraction != null ? { fraction } : {}),
			semantics: work.unit === "episodes" ? "episode completion" : "environment steps",
			determinate
		},
		timing: {
			...base.timing,
			...(base.terminal ? {} : { eta: estimatePhaseEta(evidence) })
		},
		usage: usageProjection(projected, input.events, work.total, "container"),
		...(rate ? { throughput: { label: rate } } : {}),
		milestone: milestones.at(-1),
		milestones,
		details: environmentDetails(input.events, plan),
		...(base.terminal ? { result: environmentResult(input.events, failed) } : {})
	};
}

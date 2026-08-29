/**
 * Shared adapter scaffolding: the parts of `run_progress.v1` that do not depend
 * on which algorithm produced the run.
 *
 * The two jobs here are the ones most easily got wrong per-algorithm:
 *
 *   · Usage coverage. Every figure is counted, not assumed. `observedUnits` is
 *     how many events actually carried the field; `expectedUnits` is the run's
 *     own denominator. A total from 3 of 40 rollouts is shown as 7% covered
 *     rather than as the run's cost, and a field nobody reported stays absent.
 *   · Timing evidence. Completion timestamps come from durable event
 *     `occurredAt` values only. Wall-clock now is used for elapsed time on a
 *     live run and never as evidence that work completed.
 */

import type {
	OptimizerEvent,
	ProjectedState
} from "@synth/visual-templates/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import type { RunRecord } from "./subscription";
import {
	RUN_PROGRESS_SCHEMA_VERSION,
	isTerminalRunStatus,
	normalizeRunStatus,
	type RunKind,
	type RunProgressCapabilities,
	type RunProgressEvidence,
	type RunProgressMilestone,
	type RunProgressProjection,
	type RunUsageProjection
} from "./types";
import { coveredMetric, unavailableMetric } from "./usage";

export type AdapterInput = {
	run: RunRecord;
	events: OptimizerEvent[];
	/** Known-incomplete history: counts are a floor. */
	stale: boolean;
	cursorSeq: number;
	/** Epoch ms used for elapsed time on a live run. Injected so tests are stable. */
	now: number;
};

function parseTime(value: string | null | undefined): number | undefined {
	if (!value) return undefined;
	const parsed = Date.parse(value);
	return Number.isFinite(parsed) ? parsed : undefined;
}

/**
 * Elapsed wall time. A terminal run measures to its own end; a live run to now.
 * A run that never started has no elapsed time — not zero.
 */
export function elapsedMs(run: RunRecord, events: OptimizerEvent[], now: number): number | undefined {
	const started = parseTime(run.startedAt) ?? parseTime(run.createdAt) ?? parseTime(events[0]?.occurredAt);
	if (started == null) return undefined;
	const lastEventAt = parseTime(events.at(-1)?.occurredAt);
	const end = isTerminalRunStatus(run.status)
		? parseTime(run.finishedAt) ?? lastEventAt ?? now
		: now;
	return Math.max(0, end - started);
}

export function capabilitiesOf(run: RunRecord): RunProgressCapabilities {
	const advertised = run.capabilities ?? {};
	return {
		pause: advertised.pause === true,
		resume: advertised.resume === true,
		cancel: advertised.cancel === true
	};
}

/** Completion timestamps, in event order, for the named producer event types. */
export function rolloutCompletionTimes(events: OptimizerEvent[], types: string[]): number[] {
	const wanted = new Set(types);
	const times: number[] = [];
	for (const event of events) {
		if (!wanted.has(event.type)) continue;
		const at = parseTime(event.occurredAt);
		if (at != null) times.push(at);
	}
	return times;
}

/**
 * When the timing evidence was last invalidated — a retry, a throttle, a lost
 * worker. Samples at or before it describe a rig that no longer exists.
 */
export function lastDisruptionMs(events: OptimizerEvent[], types: string[]): number | undefined {
	const wanted = new Set(types);
	let latest: number | undefined;
	for (const event of events) {
		if (!wanted.has(event.type)) continue;
		const at = parseTime(event.occurredAt);
		if (at != null && (latest == null || at > latest)) latest = at;
	}
	return latest;
}

/** How many events reported a usage field. This is the coverage numerator. */
function usageObservations(events: OptimizerEvent[], keys: string[]): number {
	let count = 0;
	for (const event of events) {
		const delta = event.usageDelta;
		if (!delta) continue;
		const nested = (delta as Record<string, unknown>).usage;
		const nestedRecord = nested && typeof nested === "object" && !Array.isArray(nested)
			? (nested as Record<string, unknown>)
			: {};
		const present = keys.some((key) =>
			Object.prototype.hasOwnProperty.call(delta, key) ||
			Object.prototype.hasOwnProperty.call(nestedRecord, key)
		);
		if (present) count += 1;
	}
	return count;
}

/**
 * Build `RunUsageProjection` from the projected totals plus counted coverage.
 *
 * `expectedUnits` is the run's declared denominator when there is one. Without
 * it a figure still renders, but it renders as "provider reported" rather than
 * claiming a coverage share it cannot compute.
 */
export function usageProjection(
	projected: ProjectedState,
	events: OptimizerEvent[],
	expectedUnits: number | undefined,
	source: "provider" | "container" | "derived" = "provider"
): RunUsageProjection {
	const totals = projected.usage ?? {};
	const costObserved = usageObservations(events, ["cost_usd", "costUsd"]);
	const promptObserved = usageObservations(events, ["prompt_tokens", "promptTokens"]);
	const completionObserved = usageObservations(events, ["completion_tokens", "completionTokens"]);
	const rolloutsObserved = usageObservations(events, ["rollouts", "rollout_count", "rolloutCount"]);
	return {
		costUsd: coveredMetric(totals.costUsd, source, costObserved, expectedUnits),
		promptTokens: coveredMetric(totals.promptTokens, source, promptObserved, expectedUnits),
		completionTokens: coveredMetric(totals.completionTokens, source, completionObserved, expectedUnits),
		rollouts: coveredMetric(totals.rollouts, source, rolloutsObserved, expectedUnits)
	};
}

/** Producer-authored messages worth a milestone line, newest last. */
const MILESTONE_TYPES = new Set([
	"optimizer.state.transitioned",
	"optimizer.run.completed",
	"optimizer.run.failed",
	"optimizer.run.cancelled",
	"gepa.run.finished",
	"eval.selection.decided",
	"sft.checkpoint.ready",
	"training.checkpoint.ready",
	"training.artifact.materialized",
	"sft.checkpoint.promoted",
	"sft.run.completed"
]);

/**
 * A fallback milestone from the event stream when the algorithm slice has no
 * better one. Only producer-authored `message` text is used; nothing is
 * invented from an event type alone.
 */
export function milestoneFromEvents(events: OptimizerEvent[]): RunProgressMilestone | undefined {
	for (let index = events.length - 1; index >= 0; index -= 1) {
		const event = events[index]!;
		if (!MILESTONE_TYPES.has(event.type)) continue;
		const message = event.delta?.message;
		if (typeof message !== "string" || message.length === 0) continue;
		return {
			label: message.slice(0, 160),
			occurredAt: event.occurredAt,
			sequence: event.sequenceNumber
		};
	}
	return undefined;
}

type VisualLink = { id?: unknown; kind?: unknown; role?: unknown };

/** The primary visual instance the run published, for "Open visual". */
export function primaryVisualRef(
	run: RunRecord,
	authoritativeRefs: readonly VisualLink[] = run.visualRefs ?? []
): string | undefined {
	const summary = run.summary ?? {};
	const visualIds = summary.visualIds && typeof summary.visualIds === "object" && !Array.isArray(summary.visualIds)
		? summary.visualIds as Record<string, unknown>
		: {};
	for (const candidate of [visualIds.primary, summary.visualId]) {
		if (typeof candidate === "string" && candidate.trim()) return candidate.trim();
	}
	const refGroups = [authoritativeRefs, run.visualRefs ?? []];
	for (const refs of refGroups) {
		for (const ref of refs) {
			if (ref.kind !== "visual" || ref.role !== "primary") continue;
			const id = ref.id;
			if (typeof id === "string" && id.trim()) return id.trim();
		}
	}
	for (const refs of refGroups) {
		for (const ref of refs) {
			if (ref.kind !== "visual") continue;
			const id = ref.id;
			if (typeof id === "string" && id.trim()) return id.trim();
		}
	}
	return undefined;
}

function titleOf(run: RunRecord, kind: RunKind): string {
	const workflow = kind === "gepa"
		? "GEPA"
		: kind === "sft"
			? "SFT"
			: kind === "environment"
				? "Environment"
				: "Evaluation";
	const subject = typeof run.objective === "string" && run.objective.length > 0 ? run.objective : run.id;
	return `${workflow} · ${subject}`;
}


/**
 * The sealed terminal manifest the run carries once it settles, if any.
 *
 * Terminal numbers come from here, frozen at the cursor the terminal event
 * advanced to. A later poll may see more events — post-terminal enrichment,
 * a reconcile — but it may not restate how the run ended.
 */
export function terminalManifest(run: RunRecord): Record<string, unknown> | undefined {
	const manifest = (run.summary as Record<string, unknown> | undefined)?.terminalManifest;
	if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) return undefined;
	return manifest as Record<string, unknown>;
}

function manifestCount(
	manifest: Record<string, unknown> | undefined,
	key: string
): number | undefined {
	const work = manifest?.work;
	if (!work || typeof work !== "object" || Array.isArray(work)) return undefined;
	const value = (work as Record<string, unknown>)[key];
	return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

/**
 * Read the frozen work counts off a sealed manifest.
 *
 * Returns `undefined` for anything the manifest recorded as null: a run that
 * never declared a plan has not declared a plan of zero.
 */
export function frozenWork(run: RunRecord): {
	planned?: number;
	succeeded?: number;
	failed?: number;
	skipped?: number;
} | undefined {
	const manifest = terminalManifest(run);
	if (!manifest) return undefined;
	const planned = manifestCount(manifest, "planned");
	const succeeded = manifestCount(manifest, "succeeded");
	const failed = manifestCount(manifest, "failed");
	const skipped = manifestCount(manifest, "skipped");
	if (planned == null && succeeded == null && failed == null && skipped == null) return undefined;
	return {
		...(planned != null ? { planned } : {}),
		...(succeeded != null ? { succeeded } : {}),
		...(failed != null ? { failed } : {}),
		...(skipped != null ? { skipped } : {})
	};
}

/**
 * Classify what the projection is standing on.
 *
 * `observedUnits` is how many work items the durable events actually proved.
 * When a run is terminal, has no manifest, and proved nothing, the honest
 * answer is that its evidence is missing — not that it did no work.
 */
export function evidenceOf(
	input: AdapterInput,
	observedUnits: number,
	unit: string
): RunProgressEvidence {
	const run = input.run;
	const terminal = isTerminalRunStatus(run.status);
	if (normalizeRunStatus(run.status) === "degraded") {
		const degradation = (run.summary as Record<string, unknown> | undefined)
			?.evidenceDegradation as Record<string, unknown> | undefined;
		return {
			state: "degraded",
			reason: "the run finished but its evidence did not persist",
			diagnostic: typeof degradation?.reason === "string"
				? `${String(degradation.stage ?? "evidence")}: ${degradation.reason}`
				: `no recorded evidence at cursor ${input.cursorSeq}`
		};
	}
	if (observedUnits > 0 || frozenWork(run) != null) return { state: "present" };
	if (input.stale) {
		return {
			state: "unavailable",
			reason: "event history is incomplete",
			diagnostic: `read to cursor ${input.cursorSeq} of ${run.cursorSeq ?? "unknown"}`
		};
	}
	if (terminal) {
		return {
			state: "unavailable",
			reason: "this run published no progress evidence",
			diagnostic: `no ${unit} events and no terminal manifest at cursor ${input.cursorSeq}`
		};
	}
	// A live run that has not reported yet is not missing evidence; it has not
	// produced any. Both render without counts, but only one is a fault.
	return { state: "present" };
}

/**
 * The algorithm-neutral skeleton. An adapter overlays its own phase, work,
 * progress, and details on top; a run whose slice has not arrived yet renders
 * this alone, honestly showing status and elapsed time and nothing more.
 */
export function baseProjection(input: AdapterInput, kind: RunKind): RunProgressProjection {
	const status = normalizeRunStatus(input.run.status);
	const terminal = isTerminalRunStatus(input.run.status);
	const warnings = input.stale
		? ["event history is incomplete; counts are a floor, not a total"]
		: [];
	return {
		schemaVersion: RUN_PROGRESS_SCHEMA_VERSION,
		runId: input.run.id,
		runKind: kind,
		title: titleOf(input.run, kind),
		status,
		terminal,
		phase: {
			id: status,
			label: status === "queued" ? "Queued" : terminal ? "Finished" : "Running",
			status: terminal ? "completed" : status === "queued" ? "pending" : "active"
		},
		phases: [],
		work: {},
		evidence: evidenceOf(input, 0, "progress"),
		timing: {
			startedAt: input.run.startedAt ?? input.run.createdAt ?? undefined,
			elapsedMs: elapsedMs(input.run, input.events, input.now),
			lastEventAt: input.events.at(-1)?.occurredAt
		},
		usage: {
			costUsd: unavailableMetric(),
			promptTokens: unavailableMetric(),
			completionTokens: unavailableMetric(),
			rollouts: unavailableMetric()
		},
		capabilities: capabilitiesOf(input.run),
		milestones: [],
		warning: warnings[0],
		warnings,
		details: [],
		fullVisualRef: primaryVisualRef(input.run),
		cursorSeq: input.cursorSeq,
		terminalCursor: input.cursorSeq,
		stale: input.stale
	};
}

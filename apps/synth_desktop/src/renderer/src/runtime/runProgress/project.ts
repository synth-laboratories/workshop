/**
 * `RunProgressAdapter` dispatch: the durable kernel V2 view becomes one
 * `run_progress.v1` projection. Event reduction remains only for the
 * non-optimizer environment card and injected legacy tests.
 *
 * Raw terminal-lane events are still carried for timeline/evidence diagnostics
 * and explicit time travel. They do not decide the live optimizer lifecycle,
 * progress, usage, or result.
 */

import {
	projectAtCursor,
	type OptimizerEvent,
	type OptimizerRun
} from "@synth/visual-templates/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import { projectEnvironment } from "./adapterEnvironment";
import { projectEval } from "./adapterEval";
import { projectGepa } from "./adapterGepa";
import { projectSft } from "./adapterSft";
import type { AdapterInput } from "./adapterShared";
import { splitEventLanes } from "./lanes";
import type { RunProgressSnapshot, RunRecord } from "./subscription";
import { projectRunViewV2 } from "./viewV2";
import {
	isRunKind,
	type RunKind,
	type RunProgressProjection,
	type RunProgressStatus
} from "./types";

export type RunProgressAdapter = (
	input: AdapterInput,
	projected: ReturnType<typeof projectAtCursor>
) => RunProgressProjection;

const ADAPTERS: Partial<Record<RunKind, RunProgressAdapter>> = {
	gepa: projectGepa,
	eval: projectEval,
	sft: projectSft,
	environment: projectEnvironment
};

/**
 * Which workflows chat offers a card for. Registered kernel algorithms all
 * have a V2 card; environment remains on the legacy diagnostic adapter.
 */
export function runKindOf(algorithmId: string | null | undefined): RunKind | null {
	return isRunKind(algorithmId) ? algorithmId : null;
}

function asOptimizerRun(run: RunRecord): OptimizerRun {
	return {
		id: run.id,
		algorithmId: run.algorithmId,
		status: run.status,
		source: run.source,
		objective: run.objective,
		cursorSeq: run.cursorSeq,
		capabilities: run.capabilities,
		summary: run.summary,
		usage: run.usage as OptimizerRun["usage"],
		executionBindings: run.executionBindings,
		error: run.error
	};
}

/**
 * Only events with a usable sequence and type reduce. A malformed page is
 * dropped rather than throwing on a render path — the subscription already
 * reported the gap that produced it.
 */
export function usableEvents(events: unknown[]): OptimizerEvent[] {
	const out: OptimizerEvent[] = [];
	for (const candidate of events) {
		if (!candidate || typeof candidate !== "object" || Array.isArray(candidate)) continue;
		const event = candidate as Record<string, unknown>;
		const sequence = Number(event.sequenceNumber ?? event.sequence_number);
		if (!Number.isSafeInteger(sequence) || sequence < 1) continue;
		if (typeof event.type !== "string") continue;
		out.push({ ...(event as unknown as OptimizerEvent), sequenceNumber: sequence });
	}
	return out.sort((left, right) => left.sequenceNumber - right.sequenceNumber);
}

const INTERRUPTED_CONNECTION = new Set(["interrupted", "failed"]);

function overlayConnection(
	projection: RunProgressProjection,
	snapshot: RunProgressSnapshot
): RunProgressProjection {
	if (projection.terminal || !INTERRUPTED_CONNECTION.has(snapshot.state)) return projection;
	const warning = snapshot.error
		? `subscription interrupted · ${snapshot.error}`
		: "subscription interrupted — reconnecting resumes from the retained cursor";
	const warnings = [warning, ...projection.warnings.filter((entry) => entry !== warning)];
	return {
		...projection,
		status: "interrupted" satisfies RunProgressStatus,
		timing: { ...projection.timing, eta: undefined },
		warning: warnings[0],
		warnings
	};
}

/**
 * Project a subscription snapshot. Returns null when the snapshot has no run
 * record yet, or when the run's algorithm has no chat card.
 */
export function projectRunProgress(
	snapshot: RunProgressSnapshot,
	now: number
): RunProgressProjection | null {
	const run = snapshot.run;
	if (!run) return null;
	if (snapshot.viewV2) {
		if (snapshot.viewV2.header.runId !== run.id) {
			throw new Error("optimizer run view identity does not match the subscribed run");
		}
		return overlayConnection(projectRunViewV2(snapshot.viewV2, run, now), snapshot);
	}
	const kind = runKindOf(run.algorithmId);
	if (!kind) return null;
	const adapter = ADAPTERS[kind];
	if (!adapter) return null;
	const lanes = splitEventLanes(run, usableEvents(snapshot.events));
	const input: AdapterInput = {
		run,
		events: lanes.terminalEvents,
		stale: snapshot.gap || snapshot.state === "stale",
		cursorSeq: lanes.terminalCursor,
		now
	};
	const projected = projectAtCursor(asOptimizerRun(run), lanes.terminalEvents);
	const projection = adapter(input, projected);
	return overlayConnection({
		...projection,
		cursorSeq: lanes.terminalCursor,
		terminalCursor: lanes.terminalCursor,
		...(lanes.enrichmentCursor != null ? { enrichmentCursor: lanes.enrichmentCursor } : {}),
		...(lanes.enrichmentEvents.length > 0 ? { enrichmentEventCount: lanes.enrichmentEvents.length } : {})
	}, snapshot);
}

/**
 * The four facts the compact card, the dialog, and the right pane must agree
 * on. Built from `run_progress.v1` so a surface cannot invent a second reading.
 */
export type ProgressAgreement = {
	phaseId: string;
	phaseLabel: string;
	status: RunProgressStatus;
	completed?: number;
	total?: number;
	progressFraction?: number;
	costUsd: number | null;
	promptTokens: number | null;
	completionTokens: number | null;
	terminal: boolean;
	resultHeadline?: string;
	resultAbsentReason?: string;
};

export function progressAgreement(projection: RunProgressProjection): ProgressAgreement {
	return {
		phaseId: projection.phase.id,
		phaseLabel: projection.phase.label,
		status: projection.status,
		...(projection.work.completed != null ? { completed: projection.work.completed } : {}),
		...(projection.work.total != null ? { total: projection.work.total } : {}),
		...(projection.progress?.fraction != null ? { progressFraction: projection.progress.fraction } : {}),
		costUsd: projection.usage.costUsd.value ?? null,
		promptTokens: projection.usage.promptTokens.value ?? null,
		completionTokens: projection.usage.completionTokens.value ?? null,
		terminal: projection.terminal,
		...(projection.result?.headline ? { resultHeadline: projection.result.headline } : {}),
		...(projection.result?.absentReason ? { resultAbsentReason: projection.result.absentReason } : {})
	};
}

export function splitSnapshotEvents(run: RunRecord, events: unknown[]) {
	return splitEventLanes(run, usableEvents(events));
}

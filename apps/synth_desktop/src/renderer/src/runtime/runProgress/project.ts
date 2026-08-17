/**
 * `RunProgressAdapter` dispatch: durable run record + event history in, one
 * `run_progress.v1` projection out.
 *
 * The reduction itself is `projectAtCursor` from the shared optimizer family —
 * the same function the full visual uses. Nothing here re-derives an event
 * meaning, so the transcript card and the workspace cannot drift apart.
 */

import {
	projectAtCursor,
	type OptimizerEvent,
	type OptimizerRun
} from "@synth/visual-templates/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import type { HistoricalShape } from "./history";
import { projectEval } from "./adapterEval";
import { projectGepa } from "./adapterGepa";
import { projectSft } from "./adapterSft";
import type { AdapterInput } from "./adapterShared";
import type { RunProgressSnapshot, RunRecord } from "./subscription";
import { isRunKind, type RunKind, type RunProgressProjection } from "./types";

export type RunProgressAdapter = (
	input: AdapterInput,
	projected: ReturnType<typeof projectAtCursor>
) => RunProgressProjection;

const ADAPTERS: Record<RunKind, RunProgressAdapter> = {
	gepa: projectGepa,
	eval: projectEval,
	sft: projectSft
};

/**
 * Which workflows chat offers a card for. `go-ex` and `dag` runs stream through
 * the same transport but have no product card yet; returning null is how the
 * transcript declines rather than rendering an empty shell.
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
function usableEvents(events: unknown[]): OptimizerEvent[] {
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

/**
 * Project a subscription snapshot. Returns null when the snapshot has no run
 * record yet, or when the run's algorithm has no chat card.
 */
export function projectRunProgress(
	snapshot: RunProgressSnapshot,
	now: number,
	history?: HistoricalShape
): RunProgressProjection | null {
	const run = snapshot.run;
	if (!run) return null;
	const kind = runKindOf(run.algorithmId);
	if (!kind) return null;
	const events = usableEvents(snapshot.events);
	const input: AdapterInput = {
		run,
		events,
		stale: snapshot.gap || snapshot.state === "stale",
		cursorSeq: snapshot.cursor,
		now,
		...(history ? { history } : {})
	};
	const projected = projectAtCursor(asOptimizerRun(run), events);
	return ADAPTERS[kind](input, projected);
}

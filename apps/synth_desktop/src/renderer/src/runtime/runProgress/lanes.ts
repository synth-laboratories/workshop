/**
 * Terminal cursor vs enrichment lane.
 *
 * Coordinated with Optimizers O-5 (frozen terminal cursor) and O-11
 * (post-terminal events tagged `lane: enrichment`). This is the Workshop
 * projection side: authoritative usage, phase, progress, and result reduce
 * only through the frozen terminal cursor. Late enrichment is visible, but
 * it cannot rewrite those fields.
 *
 * Until O-5/O-11 land, a terminal run without a declared cursor freezes at
 * the last non-enrichment event. Untagged events after that freeze still
 * ride the enrichment lane so a late append cannot mutate the result.
 */

import type { OptimizerEvent } from "@synth/visual-templates/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import { eventLaneOf } from "./protocol";
import { isTerminalRunStatus } from "./types";
import type { RunRecord } from "./subscription";

export type EventLanes = {
	/** Events the authoritative projection may reduce. */
	terminalEvents: OptimizerEvent[];
	/** Post-terminal enrichment; never mixed into terminal usage/result. */
	enrichmentEvents: OptimizerEvent[];
	/** Frozen at terminal transition. Equal to the live cursor while running. */
	terminalCursor: number;
	/** Newest enrichment sequence, when any enrichment exists. */
	enrichmentCursor?: number;
};

function declaredTerminalCursor(run: RunRecord): number | undefined {
	const summary = run.summary ?? {};
	for (const key of ["terminalCursor", "optimizer_terminal_cursor", "optimizerTerminalCursor"]) {
		const value = Number(summary[key]);
		if (Number.isSafeInteger(value) && value >= 0) return value;
	}
	return undefined;
}

function maxSequence(events: OptimizerEvent[]): number {
	let max = 0;
	for (const event of events) {
		if (event.sequenceNumber > max) max = event.sequenceNumber;
	}
	return max;
}

function occurredMs(event: OptimizerEvent): number | undefined {
	const parsed = Date.parse(event.occurredAt);
	return Number.isFinite(parsed) ? parsed : undefined;
}

export function splitEventLanes(run: RunRecord, events: OptimizerEvent[]): EventLanes {
	const live: OptimizerEvent[] = [];
	const taggedEnrichment: OptimizerEvent[] = [];
	for (const event of events) {
		if (eventLaneOf(event) === "enrichment") taggedEnrichment.push(event);
		else live.push(event);
	}

	const declared = declaredTerminalCursor(run);
	const liveCursor = maxSequence(live);
	const terminal = isTerminalRunStatus(run.status);
	const finishedAt = run.finishedAt ? Date.parse(run.finishedAt) : Number.NaN;
	const freezeFromFinish = Number.isFinite(finishedAt)
		? maxSequence(live.filter((event) => {
			const at = occurredMs(event);
			return at == null || at <= finishedAt;
		}))
		: liveCursor;
	const terminalCursor = declared ?? (terminal ? freezeFromFinish : liveCursor);

	const terminalEvents = live.filter((event) => event.sequenceNumber <= terminalCursor);
	const afterFreeze = live.filter((event) => event.sequenceNumber > terminalCursor);
	const enrichmentEvents = [...taggedEnrichment, ...afterFreeze].sort(
		(left, right) => left.sequenceNumber - right.sequenceNumber
	);
	const enrichmentCursor = enrichmentEvents.length > 0 ? maxSequence(enrichmentEvents) : undefined;

	return {
		terminalEvents,
		enrichmentEvents,
		terminalCursor,
		...(enrichmentCursor != null ? { enrichmentCursor } : {})
	};
}

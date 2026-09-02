/**
 * Algorithm-neutral semantic counts from the durable run view.
 *
 * Readiness used to be `boundEvents.length` twice over: a healthy
 * projection-only GEPA visual reported zero semantic events and zero
 * rollouts even though its projection proved ten candidates and 1,080 scored
 * rollouts. Readiness has to describe what the run *is*, not whether the
 * renderer happened to hydrate the raw journal.
 *
 * `semanticEvents` counts the facts the projection holds — candidates,
 * evaluations, proposer calls, checkpoints, metric points, work items —
 * and `rollouts` counts the rollout-shaped ones. Raw events are the floor
 * only when no projection is present at all.
 */

import type { OptimizerRunViewV2 } from "../../generated/protocol";

export type SemanticCounts = {
	semanticEvents: number;
	rollouts: number;
	/** Where the numbers came from; `raw` means no projection was available. */
	source: "projection" | "raw";
};

function count(value: unknown): number {
	if (Array.isArray(value)) return value.length;
	if (value && typeof value === "object") return Object.keys(value as object).length;
	if (typeof value === "number" && Number.isFinite(value) && value >= 0) return Math.floor(value);
	return 0;
}

function sum(...values: Array<number | null | undefined>): number {
	return values.reduce<number>((total, value) => total + (typeof value === "number" && Number.isFinite(value) ? value : 0), 0);
}

export function semanticCountsFromRunView(
	view: OptimizerRunViewV2 | null | undefined,
	rawEventCount: number
): SemanticCounts {
	if (!view) {
		return { semanticEvents: rawEventCount, rollouts: rawEventCount, source: "raw" };
	}
	const work = view.header.work;
	const workItems = sum(work.planned, work.queued, work.running, work.succeeded, work.failed, work.cancelled);
	const projection = view.projection as Record<string, unknown>;
	switch (view.algorithm) {
		case "gepa": {
			// The wire view is bounded: collection-shaped arrays such as
			// `evaluations` may be empty even though the run scored thousands of
			// rollouts. The scalar counters are always present, so they are the
			// floor for both numbers.
			const evaluations = count(projection.evaluations);
			const rollouts = Math.max(evaluations, count(projection.rolloutsScored) + count(projection.rolloutsFailed));
			const candidates = count(projection.candidateOrder) || count(projection.candidates);
			return {
				semanticEvents: candidates + rollouts + count(projection.proposerCalls) + count(projection.frontierHistory),
				rollouts,
				source: "projection"
			};
		}
		case "eval": {
			const ledger = count(projection.evidenceLedger);
			const scored = count(projection.scoredTrials);
			return {
				semanticEvents: count(projection.candidates) + ledger + workItems,
				rollouts: Math.max(ledger, scored, workItems),
				source: "projection"
			};
		}
		case "sft":
		case "cispo": {
			const metrics = projection.metrics as { points?: unknown[] } | undefined;
			const points = count(metrics?.points);
			const evaluations = count(projection.evaluations);
			return {
				semanticEvents: count(projection.checkpoints) + evaluations + points + workItems,
				// Completed checkpoint evaluations are also succeeded work items on
				// the header, which survives the bounded wire view when the
				// evaluation rows do not.
				rollouts: view.algorithm === "cispo" ? workItems : (evaluations || sum(work.succeeded)),
				source: "projection"
			};
		}
		case "go-ex": {
			return {
				semanticEvents: count(projection.themes) + count(projection.candidateIds) + count(projection.childEvalRunIds) + workItems,
				rollouts: workItems,
				source: "projection"
			};
		}
		default:
			return { semanticEvents: workItems, rollouts: workItems, source: "projection" };
	}
}

import type { EvalAggregate, OptimizerRunViewV2 } from "../generated/protocol";

export type CanonicalEvalState = Extract<OptimizerRunViewV2, { algorithm: "eval" }>;

/**
 * Accept an eval view only when its aggregate is bound to the same run and
 * exact kernel revision. Surfaces may format the returned aggregate, but must
 * not combine it with independently reduced score, work, or evidence slices.
 */
export function canonicalEvalState(
	view: OptimizerRunViewV2,
	runId: string
): CanonicalEvalState {
	if (view.algorithm !== "eval") {
		throw new Error(`optimizer run ${runId} returned a ${view.algorithm} view`);
	}
	if (view.header.runId !== runId || view.aggregate.runId !== runId) {
		throw new Error(`eval view identity does not match optimizer run ${runId}`);
	}
	if (
		view.aggregate.asOfSequence !== view.header.asOfSequence
		|| view.aggregate.projectionRevision !== view.header.projectionRevision
	) {
		throw new Error(`eval aggregate revision does not match run view ${runId}`);
	}
	return view;
}

/** Resolve the exact aggregate object bound by each accepted surface shape. */
export function evalAggregateFromSurface(
	value: unknown,
	runId: string
): EvalAggregate {
	const candidate = value && typeof value === "object"
		? value as Record<string, unknown>
		: {};
	const aggregate = (
		candidate.schemaVersion === "eval.aggregate.v1"
			? candidate
			: candidate.aggregate
	) as Partial<EvalAggregate> | undefined;
	if (
		aggregate?.schemaVersion !== "eval.aggregate.v1"
		|| aggregate.runId !== runId
		|| !Number.isSafeInteger(aggregate.asOfSequence)
		|| !Number.isSafeInteger(aggregate.projectionRevision)
	) {
		throw new Error(`surface does not carry a revisioned aggregate for ${runId}`);
	}
	return aggregate as EvalAggregate;
}

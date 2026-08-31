import type { EvalAggregate, OptimizerResourceRef, OptimizerRunRecord } from "../generated/protocol";
import { workshopStarterForRecipe, type WorkshopStarter } from "./starterCatalog";
import { isTerminalRunStatus } from "./runProgress/types";

export type StarterResultState = "completed" | "inconclusive" | "failed" | "cancelled";

export type RunOutcome = {
	schemaVersion: "workshop.starter-result.v1";
	starter: WorkshopStarter;
	runId: string;
	state: StarterResultState;
	reason: string;
	headlineMetric: { label: "Mean reward"; value: number } | null;
	comparison: {
		baseline: number | null;
		candidate: number | null;
		delta: number | null;
		reason: string;
	};
	evidence: {
		complete: boolean;
		inspectable: boolean;
		references: readonly OptimizerResourceRef[];
		reason: string;
	};
	usage: { costUsd: number | null; reason: string };
	visualId: string | null;
	nextExperimentPrompt: string;
};

function objectValue(value: unknown): Record<string, unknown> | null {
	return value && typeof value === "object" && !Array.isArray(value)
		? value as Record<string, unknown>
		: null;
}

/** Read only producer-recorded recipe identity; title/objective text is never identity. */
export function starterRecipeId(run: OptimizerRunRecord): string | null {
	const summary = objectValue(run.summary);
	const identities = new Set<string>();
	if (typeof summary?.recipeId === "string" && summary.recipeId.length > 0) identities.add(summary.recipeId);
	for (const reference of run.inputRefs ?? []) {
		if (reference.kind === "recipe" && reference.id) identities.add(reference.id);
	}
	return identities.size === 1 ? [...identities][0] : null;
}

export type PendingStarterRun = {
	recipeId: string;
	notBefore: string;
};

/** Bind an agent-assisted starter only to a newly-created run with exact recipe identity. */
export function matchingStarterRun(
	runs: readonly OptimizerRunRecord[],
	pending: PendingStarterRun
): OptimizerRunRecord | null {
	const threshold = Date.parse(pending.notBefore);
	return runs
		.filter((run) => starterRecipeId(run) === pending.recipeId)
		.filter((run) => {
			const created = Date.parse(run.createdAt);
			return Number.isFinite(created) && Number.isFinite(threshold) && created >= threshold;
		})
		.sort((left, right) => Date.parse(left.createdAt) - Date.parse(right.createdAt))[0] ?? null;
}

function finiteField(value: unknown): number | null {
	return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function authoritativeComparison(aggregate: EvalAggregate): RunOutcome["comparison"] {
	const raw = aggregate as unknown as Record<string, unknown>;
	const nested = objectValue(raw.comparison);
	// These are producer-owned aggregate fields. Do not infer comparison values
	// from rollout text, labels, or the headline mean.
	const baseline = finiteField(nested?.baseline ?? raw.baselineReward);
	const candidate = finiteField(nested?.candidate ?? nested?.variant ?? raw.candidateReward ?? raw.variantReward);
	const delta = finiteField(nested?.delta ?? raw.rewardDelta);
	const missing = [
		baseline == null ? "baseline" : null,
		candidate == null ? "candidate" : null,
		delta == null ? "delta" : null
	].filter((field): field is string => field != null);
	return {
		baseline,
		candidate,
		delta,
		reason: missing.length === 0
			? "Producer-recorded comparison from the authoritative evaluation aggregate."
			: `Authoritative evaluation aggregate is missing: ${missing.join(", ")}. No values were inferred.`
	};
}

function terminalReason(
	run: OptimizerRunRecord,
	aggregate: EvalAggregate,
	metricValid: boolean,
	evidenceComplete: boolean,
	evidenceInspectable: boolean
): { state: StarterResultState; reason: string } {
	if (run.status === "cancelled") {
		return { state: "cancelled", reason: "The run was cancelled. Retained evidence remains available below." };
	}
	if (run.status === "failed") {
		return { state: "failed", reason: aggregate.evidence.reason ?? "The run failed before producing a complete starter result." };
	}
	if (run.status !== "completed") {
		return {
			state: "inconclusive",
			reason: aggregate.evidence.reason ?? `The run ended as ${run.status.replaceAll("_", " ")}; no completed result is claimed.`
		};
	}
	if (!metricValid) {
		return { state: "inconclusive", reason: "The run completed without a valid headline metric." };
	}
	if (!evidenceComplete) {
		return { state: "inconclusive", reason: aggregate.evidence.reason ?? "The run completed, but its evidence is not complete." };
	}
	if (!evidenceInspectable) {
		return { state: "inconclusive", reason: "The run completed, but it has no inspectable evidence reference." };
	}
	return { state: "completed", reason: "A valid metric and complete inspectable evidence were recorded." };
}

export function projectStarterResult(
	run: OptimizerRunRecord,
	aggregate: EvalAggregate
): RunOutcome | null {
	if (run.algorithmId !== "eval" || !isTerminalRunStatus(run.status)) return null;
	if (aggregate.runId !== run.id || aggregate.lifecycle !== "terminal") return null;
	const starter = workshopStarterForRecipe(starterRecipeId(run));
	if (!starter) return null;

	const references: OptimizerResourceRef[] = [];
	const seen = new Set<string>();
	const add = (reference: OptimizerResourceRef) => {
		const key = `${reference.kind}:${reference.id}`;
		if (!reference.id || seen.has(key)) return;
		seen.add(key);
		references.push(reference);
	};
	for (const reference of aggregate.evidence.refs ?? []) add(reference);
	for (const reference of run.outputRefs ?? []) add(reference);
	for (const reference of run.visualRefs ?? []) add(reference);

	const metricValid = typeof aggregate.meanReward === "number" && Number.isFinite(aggregate.meanReward);
	const evidenceComplete = aggregate.evidence.completeness === "complete";
	const evidenceInspectable = aggregate.evidenceRefCount > 0 && references.length > 0;
	const terminal = terminalReason(run, aggregate, metricValid, evidenceComplete, evidenceInspectable);
	const costUsd = typeof run.usage?.costUsd === "number" && Number.isFinite(run.usage.costUsd)
		? run.usage.costUsd
		: null;
	const visualId = run.visualRefs?.find((reference) => reference.kind === "visual" && reference.id)?.id ?? null;

	return {
		schemaVersion: "workshop.starter-result.v1",
		starter,
		runId: run.id,
		state: terminal.state,
		reason: terminal.reason,
		headlineMetric: metricValid ? { label: "Mean reward", value: aggregate.meanReward as number } : null,
		comparison: authoritativeComparison(aggregate),
		evidence: {
			complete: evidenceComplete,
			inspectable: evidenceInspectable,
			references,
			reason: aggregate.evidence.reason
				?? (evidenceInspectable ? `${references.length} inspectable reference${references.length === 1 ? "" : "s"}` : "No inspectable evidence reference was recorded.")
		},
		usage: {
			costUsd,
			reason: costUsd == null ? "Cost unavailable; missing usage is never reported as zero." : "Settled run usage."
		},
		visualId,
		nextExperimentPrompt: `Review starter run ${run.id} for ${starter.title}. Use only its retained evidence. Propose one bounded next experiment and show the exact change, expected signal, maximum cost, and approval boundary. Do not execute it or modify external state until I explicitly approve.`
	};
}

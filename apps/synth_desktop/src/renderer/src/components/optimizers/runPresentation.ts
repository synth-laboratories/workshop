/**
 * Presentation helpers shared by the optimizer run list and the run inspector.
 * Pure string functions only — no bridge access, so they are importable from
 * plain Node tests.
 */

import type { OptimizerRunRecord } from "@synth/runtime-protocol";

export function algorithmLabel(id: string): string {
	if (id === "gepa") return "GEPA";
	if (id === "go-ex") return "GELO";
	if (id === "sft") return "SFT";
	if (id === "cispo") return "CISPO · slime";
	if (id === "eval") return "Eval";
	return id;
}

export function formatWhen(iso: string | null | undefined): string {
	if (!iso) return "—";
	try {
		return new Date(iso).toLocaleString();
	} catch {
		return iso;
	}
}

/** Lifecycle words with underscores read as identifiers; chips show words. */
export function statusText(status: string): string {
	return status.replaceAll("_", " ");
}

/**
 * The status chip class. Statuses are producer-controlled single tokens
 * (`cap_reached`, `infrastructure_lost`, …); anything else falls to the
 * neutral base chip via a class CSS does not know.
 */
export function statusChipClass(status: string): string {
	return `optimizer-status ${status}`;
}

/** Middle truncation: run ids carry identity at both ends (family + suffix). */
export function truncateMiddle(value: string, max = 24): string {
	if (value.length <= max) return value;
	const keep = Math.max(4, Math.floor((max - 1) / 2));
	return `${value.slice(0, keep)}…${value.slice(-keep)}`;
}

export type SealedWorkCounts = {
	planned?: number;
	succeeded?: number;
	failed?: number;
	skipped?: number;
};

/**
 * Work counts off the sealed terminal manifest the list payload already
 * carries (`summary.terminalManifest.work`, written by the terminal writer).
 * Live runs' counts live in the event log / kernel V2 view — not in the list
 * record — so this returns null for them: absent is not zero, and the list
 * must not fetch runViewV2 per row to invent a number.
 */
export function sealedWorkCounts(run: OptimizerRunRecord): SealedWorkCounts | null {
	const summary = run.summary && typeof run.summary === "object"
		? run.summary as Record<string, unknown>
		: null;
	const manifest = summary?.terminalManifest;
	if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) return null;
	const work = (manifest as Record<string, unknown>).work;
	if (!work || typeof work !== "object" || Array.isArray(work)) return null;
	const read = (key: string): number | undefined => {
		const value = (work as Record<string, unknown>)[key];
		return typeof value === "number" && Number.isFinite(value) ? value : undefined;
	};
	const counts: SealedWorkCounts = {
		...(read("planned") != null ? { planned: read("planned") } : {}),
		...(read("succeeded") != null ? { succeeded: read("succeeded") } : {}),
		...(read("failed") != null ? { failed: read("failed") } : {}),
		...(read("skipped") != null ? { skipped: read("skipped") } : {})
	};
	return Object.keys(counts).length > 0 ? counts : null;
}

/** "8✓ 2✕ / 10" — only the counts the manifest actually recorded. */
export function workFractionLabel(counts: SealedWorkCounts): string {
	const parts: string[] = [];
	if (counts.succeeded != null) parts.push(`${counts.succeeded}✓`);
	if (counts.failed != null && counts.failed > 0) parts.push(`${counts.failed}✕`);
	const head = parts.join(" ");
	if (counts.planned != null) return `${head || "0"} / ${counts.planned}`;
	return head;
}

export type RunFacets = {
	recipeId: string | null;
	containerId: string | null;
	model: string | null;
};

function objectish(value: unknown): Record<string, unknown> | null {
	return value && typeof value === "object" && !Array.isArray(value)
		? value as Record<string, unknown>
		: null;
}

function stringOr(value: unknown): string | null {
	return typeof value === "string" && value.length > 0 ? value : null;
}

/**
 * The recipe/container/model facets a run record actually carries, for
 * client-side filtering. Sources, in order: the worker-written summary
 * (`recipeId`/`containerId`/`model`, plus `localMlx.requestedBaseModel`),
 * `inputRefs` of kind `recipe`/`container`/`model`, and container execution
 * bindings. A run whose producer recorded none of these has that facet null —
 * it cannot match a specific filter, and pretending otherwise would be a
 * fabricated match.
 */
export function runFacets(run: OptimizerRunRecord): RunFacets {
	const summary = objectish(run.summary);
	const refs = run.inputRefs ?? [];
	const refOf = (kind: string) => refs.find((ref) => ref.kind === kind)?.id ?? null;
	const localMlx = objectish(summary?.localMlx);
	return {
		recipeId: stringOr(summary?.recipeId) ?? refOf("recipe"),
		containerId: stringOr(summary?.containerId)
			?? refOf("container")
			?? (run.executionBindings ?? []).find((binding) => binding.kind === "container_http")?.id
			?? null,
		model: stringOr(summary?.model)
			?? stringOr(localMlx?.requestedBaseModel)
			?? refOf("model")
	};
}

/** The instant a run settled, or its best-known activity time before that. */
export function runWhenMs(run: OptimizerRunRecord): number {
	const stamp = run.finishedAt ?? run.startedAt ?? run.createdAt;
	const parsed = Date.parse(stamp);
	return Number.isFinite(parsed) ? parsed : 0;
}

export function runTitle(run: OptimizerRunRecord): string {
	const objective = run.objective ?? run.id;
	const importedPath = objective.startsWith("imported from ")
		? objective.slice("imported from ".length)
		: null;
	if (!importedPath) return objective;
	const parts = importedPath.split(/[\\/]/).filter(Boolean);
	let artifactName = parts.at(-1)?.includes("events.") ? parts.at(-2) : parts.at(-1);
	if (artifactName === "artifacts") artifactName = parts.at(-3);
	const algorithmTokens = new Set([run.algorithmId, algorithmLabel(run.algorithmId), "goex"]
		.map((token) => token.toLowerCase().replace(/[^a-z0-9]/g, "")));
	return (artifactName ?? run.id)
		.split(/[_-]+/g)
		.filter((token) => !algorithmTokens.has(token.toLowerCase().replace(/[^a-z0-9]/g, "")))
		.join(" ")
		.replace(/\bmed\b/gi, "medium")
		.replace(/\b\w/g, (character) => character.toUpperCase());
}

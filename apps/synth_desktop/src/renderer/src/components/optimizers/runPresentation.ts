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

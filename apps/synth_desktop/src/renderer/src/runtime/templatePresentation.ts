/**
 * Template-id-derived presentation, in one place.
 *
 * The preview-variant classifier used to be copied between VisualHost and
 * sessionView, and surfaces it never named (harbor, the live eval stream, the
 * family-agnostic trace workstation) fell to "generic" by omission rather
 * than by decision. Here the fall-through is explicit, and a new template
 * joins a behavior by joining a set instead of editing a hardcoded equality.
 */

import type { ArtifactRef } from "../types/landing";

export type PreviewVariant = NonNullable<ArtifactRef["preview"]>["variant"];

export function previewVariantForTemplate(
	templateId: string | null | undefined
): PreviewVariant {
	const id = templateId ?? "";
	// Stream-first live surfaces and the family-agnostic workstation carry no
	// Craftax preview. Named here so "harbor" never accidentally inherits a
	// Craftax mock through a substring match added later.
	if (id.includes("harbor") || id.includes("eval_stream") || id.startsWith("trace.workbench")) {
		return "generic";
	}
	if (id.includes("scrub") || id.includes("rollout")) return "craftax_frame";
	if (id.includes("craftax") || id.includes("eval_matrix")) return "craftax_pareto";
	return "generic";
}

/**
 * Trace-workbench templates whose bound run's terminal trials may reference
 * sealed Trace V5 bundles; VisualHost resolves those digests through the
 * inventory bridge and hands the projections to the shell.
 */
export const SEALED_TRACE_WORKBENCH_TEMPLATES: ReadonlySet<string> = new Set([
	"craftax.trace_workbench.v1",
	"trace.workbench.v1"
]);

/**
 * Library-card identity.
 *
 * Suite titles are written family-first ("Banking77 · CISPO training") so they
 * group in a search, but a narrow list truncates them at exactly the
 * differentiating word: "Banking77 · CISPO tr…". The card shows the distinct
 * part as its name and carries the shared family as a badge, which reads the
 * same at any width and keeps the full title for the preview header.
 */
export type VisualCardIdentity = { name: string; badge?: string };

const TITLE_SEPARATOR = " · ";

export function visualCardIdentity(title: string): VisualCardIdentity {
	const parts = title.split(TITLE_SEPARATOR).map((part) => part.trim()).filter(Boolean);
	if (parts.length < 2) return { name: title };
	// Everything after the first segment stays together: only the leading
	// family qualifier moves into the badge.
	return { name: parts.slice(1).join(TITLE_SEPARATOR), badge: parts[0] };
}

/**
 * Whether a visual is showing evidence bound to a real run, or the examples its
 * template ships. Preview cards used to render "session — · run — · trace —",
 * which reads as broken rather than as deliberately synthetic.
 */
export type VisualEvidenceMode = "bound" | "bundled" | "unbound";

function record(value: unknown): Record<string, unknown> | null {
	return value && typeof value === "object" && !Array.isArray(value)
		? value as Record<string, unknown>
		: null;
}

function bindingRows(value: unknown): Array<Record<string, unknown>> {
	const envelope = record(value);
	if (!envelope) return [];
	const rows = Array.isArray(envelope.inputs)
		? envelope.inputs
		: Array.isArray(envelope.slots)
			? envelope.slots
			: [];
	return rows.flatMap((row) => record(row) ? [record(row)!] : []);
}

export function visualEvidenceMode(input: {
	sessionId?: string | null;
	runId?: string | null;
	traceId?: string | null;
	traceSetCount?: number | null;
	metadata?: unknown;
	bindings?: unknown;
}): VisualEvidenceMode {
	const bound = Boolean(input.sessionId?.trim() || input.runId?.trim() || input.traceId?.trim())
		|| input.traceSetCount !== undefined;
	if (bound) return "bound";
	const metadata = record(input.metadata);
	const declaredMode = String(metadata?.evidenceMode ?? metadata?.evidence_mode ?? "").toLowerCase();
	const fixtureBinding = bindingRows(input.bindings).some((binding) => binding.kind === "fixture");
	return declaredMode.includes("fixture") || declaredMode.includes("bundled") || fixtureBinding
		? "bundled"
		: "unbound";
}

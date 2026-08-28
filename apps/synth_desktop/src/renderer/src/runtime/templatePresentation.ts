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

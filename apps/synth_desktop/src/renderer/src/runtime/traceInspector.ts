// @ts-nocheck — P0-1 generated protocol is stricter than prior handwritten DTOs; UI follow-up is out of specta-cutover file ownership.
/**
 * Trace V5 inspector identity and eligibility.
 *
 * Extracted from `DataPage` so the native catalog and the agent-facing path
 * cannot drift: a trace inspector is identified by its sealed digest and by
 * nothing else. Keep this module free of React and of bridge calls so it stays
 * directly testable.
 */

import type { TraceV5Record, VisualRecord } from "@synth/runtime-protocol";

export const TRACE_INSPECTOR_TEMPLATE = "trace.rollout_inspector.v1";
export const TRACE_PROJECTION_SCHEMA = "synth.trace-projection.rollout-inspector.v1";

export type TraceInspectability = {
	eligible: boolean;
	/** Catalog row label; an unavailable trace stays visible and says why. */
	label: "Inspect" | "Quarantined" | "Archive incomplete" | "Unsupported";
};

export function traceInspectability(trace: TraceV5Record): TraceInspectability {
	const metadata = trace.metadata ?? {};
	const compatibility = typeof metadata.compatibilityLevel === "string"
		? metadata.compatibilityLevel.toLowerCase()
		: null;
	const validation = typeof metadata.validationStatus === "string"
		? metadata.validationStatus.toLowerCase()
		: null;
	if (
		metadata.quarantined === true
		|| metadata.trusted === false
		|| validation === "invalid"
		|| validation === "quarantined"
	) {
		return { eligible: false, label: "Quarantined" };
	}
	if (metadata.selfContained === false) return { eligible: false, label: "Archive incomplete" };
	if (compatibility === "invalid" || compatibility === "opaque") {
		return { eligible: false, label: "Unsupported" };
	}
	return { eligible: true, label: "Inspect" };
}

/** The digest a visual's projection slot is bound to, or null if it is not a trace inspector. */
export function traceDigestBinding(visual: VisualRecord): string | null {
	if (visual.templateId !== TRACE_INSPECTOR_TEMPLATE) return null;
	const bindings = visual.bindings as { slots?: Array<{ slot?: string; kind?: string; source?: string }> };
	const projection = bindings?.slots?.find((slot) => slot.slot === "projection" && slot.kind === "trace_v5");
	return typeof projection?.source === "string" ? projection.source : null;
}

/** Deterministic per-sealed-archive identity, stable across restarts and callers. */
export function traceInspectorVisualId(trace: TraceV5Record): string {
	const digest = trace.digest.replace(/^sha256:/, "").replace(/[^a-zA-Z0-9_.-]/g, "").slice(0, 64);
	return `vis_trace_${digest || trace.id.replace(/[^a-zA-Z0-9_.-]/g, "_").slice(0, 64)}`;
}

/**
 * Find the existing inspector for a sealed trace.
 *
 * Matches on the digest alone. A trace record id, run id, or title is not
 * archive identity: re-sealing a record produces a new digest, and reusing the
 * old visual would present the previous archive under the current name.
 */
export function findTraceInspectorVisual(
	visuals: readonly VisualRecord[],
	trace: TraceV5Record
): VisualRecord | undefined {
	return visuals.find((candidate) =>
		candidate.metadata?.traceDigest === trace.digest
		|| traceDigestBinding(candidate) === trace.digest
	);
}

/** The create request for a trace's inspector, bound to the sealed digest. */
export function traceInspectorCreateRequest(trace: TraceV5Record) {
	return {
		id: traceInspectorVisualId(trace),
		templateId: TRACE_INSPECTOR_TEMPLATE,
		title: trace.title,
		traceId: trace.id,
		bindings: {
			schemaVersion: "synth.visual-bindings.v1",
			slots: [{
				slot: "projection",
				kind: "trace_v5",
				source: trace.digest,
				schema: TRACE_PROJECTION_SCHEMA
			}]
		},
		metadata: {
			traceRecordId: trace.id,
			traceDigest: trace.digest,
			projectionSchema: TRACE_PROJECTION_SCHEMA
		}
	};
}

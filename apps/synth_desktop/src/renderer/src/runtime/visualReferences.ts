import type { TraceV5Record, VisualRecord } from "@synth/runtime-protocol";
import { bridges } from "./desktopBridge";
import { findTraceInspectorVisual, traceDigestBinding, traceInspectorCreateRequest, traceInspectorVisualId, TRACE_INSPECTOR_TEMPLATE } from "./traceInspector";

export const VISUAL_REFERENCE_OPENED_EVENT = "synth:visual-reference-opened";
export const VISUAL_REFERENCE_ERROR_EVENT = "synth:visual-reference-error";

type TraceImportResult = { inspectable?: boolean; note?: string; traces?: Array<{ traceId?: string }> };

function rolloutIdFromReference(reference: string): string | null {
	const match = reference.match(/(?:^|\/)rollouts\/([^/]+)\/trace(?:\/|$)/);
	if (match?.[1]) return decodeURIComponent(match[1]);
	return /^[a-zA-Z0-9_.:-]+$/.test(reference) && !reference.startsWith("sha256:") ? reference : null;
}

function matchesTraceReference(trace: TraceV5Record, reference: string): boolean {
	if (trace.id === reference || trace.digest === reference || trace.runId === reference || trace.path === reference) return true;
	const rolloutId = rolloutIdFromReference(reference);
	if (rolloutId && trace.runId === rolloutId) return true;
	return Object.values(trace.metadata ?? {}).some((value) => typeof value === "string" && (value === reference || Boolean(rolloutId && value.includes(rolloutId))));
}

async function ensureTrace(reference: string, containerId?: string): Promise<TraceV5Record> {
	if (!bridges.inventory) throw new Error("The local trace registry is unavailable.");
	let traces = await bridges.inventory.listTraces();
	let trace = traces.find((candidate) => matchesTraceReference(candidate, reference));
	if (trace) return trace;
	const rolloutId = rolloutIdFromReference(reference);
	if (!rolloutId || !containerId) throw new Error("This trace has not been retained in the local registry.");
	const imported: TraceImportResult = await bridges.inventory.materializeContainerTrace(containerId, rolloutId);
	if (!imported.inspectable) throw new Error(imported.note ?? "The container retained provenance, but no inspectable Trace V5 bundle.");
	traces = await bridges.inventory.listTraces();
	const importedIds = new Set((imported.traces ?? []).map((item) => item.traceId).filter(Boolean));
	trace = traces.find((candidate) => importedIds.has(candidate.id)) ?? traces.find((candidate) => matchesTraceReference(candidate, reference));
	if (!trace) throw new Error("The trace import completed, but no inspectable trace was indexed.");
	return trace;
}

export async function openTraceReference(reference: string, containerId?: string): Promise<VisualRecord> {
	if (!bridges.visuals) throw new Error("The local visual registry is unavailable.");
	const trace = await ensureTrace(reference, containerId);
	const registered = await bridges.visuals.list({ templateId: TRACE_INSPECTOR_TEMPLATE, limit: 500 });
	let visual = findTraceInspectorVisual(registered, trace);
	if (!visual) {
		try {
			visual = await bridges.visuals.create(traceInspectorCreateRequest(trace));
		} catch (createError) {
			const raced = await bridges.visuals.get(traceInspectorVisualId(trace)).catch(() => null);
			if (!raced || traceDigestBinding(raced) !== trace.digest) throw createError;
			visual = raced;
		}
	}
	return bridges.visuals.show(visual.id).catch(() => visual!);
}

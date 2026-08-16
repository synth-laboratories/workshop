/**
 * Renderer diagnostics.
 *
 * A `console.error` in a webview reaches no journal, no index, and no agent.
 * Every renderer failure worth correlating goes through here instead, which
 * forwards the same `synth.diagnostic-event.v1` envelope the Rust surfaces
 * emit — so a blank visual and the rollout that produced it end up joinable by
 * visual id, stream id, rollout id, and trace id.
 *
 * Reporting is fire-and-forget by contract. A failure to record a failure must
 * never become a second failure, so every call swallows its own errors and
 * nothing awaits it on a render path.
 */

import { COMMANDS, invokeCommand } from "../bridge";

export type DiagnosticSeverity = "debug" | "info" | "warn" | "error";

/** Components the renderer is allowed to speak for. */
export type RendererComponent = "renderer" | "visual-host";

export type DiagnosticCorrelation = {
	sessionId?: string | null;
	turnId?: string | null;
	toolCallId?: string | null;
	commandId?: string | null;
	visualId?: string | null;
	visualRevision?: number | null;
	containerId?: string | null;
	rolloutId?: string | null;
	streamId?: string | null;
	optimizerRunId?: string | null;
	traceId?: string | null;
};

export type DiagnosticReport = DiagnosticCorrelation & {
	severity: DiagnosticSeverity;
	component: RendererComponent;
	/** Dotted lowercase name, e.g. `visual.projection.rejected`. */
	event: string;
	/** Stable snake_case code the agent can filter on. */
	code: string;
	message: string;
	retryable?: boolean;
	details?: Record<string, unknown>;
};

/** Stable codes shared with `src-tauri/src/diagnostics/codes.rs`. */
export const DIAGNOSTIC_CODES = {
	unsupportedTraceProjectionSchema: "unsupported_trace_projection_schema",
	visualBindingUnresolved: "visual_binding_unresolved",
	visualTemplateUnavailable: "visual_template_unavailable",
	visualShellLoadFailed: "visual_shell_load_failed",
	visualRenderFailed: "visual_render_failed",
	streamInterrupted: "stream_interrupted",
	streamSubscribeTimeout: "stream_subscribe_timeout",
	streamReplayGap: "stream_replay_gap",
	providerDisconnected: "provider_disconnected",
	providerStalled: "provider_stalled",
} as const;

/**
 * Identical failures repeat every render. Collapse them so a re-rendering
 * error boundary cannot fill the bounded queue with one fact.
 */
const REPEAT_WINDOW_MS = 10_000;
const recentReports = new Map<string, number>();

function isRepeat(report: DiagnosticReport, now: number): boolean {
	const key = [
		report.component,
		report.code,
		report.visualId ?? "",
		report.streamId ?? "",
		report.rolloutId ?? "",
		report.message,
	].join("|");
	const last = recentReports.get(key);
	if (last !== undefined && now - last < REPEAT_WINDOW_MS) return true;
	recentReports.set(key, now);
	if (recentReports.size > 256) {
		for (const [entry, at] of recentReports) {
			if (now - at >= REPEAT_WINDOW_MS) recentReports.delete(entry);
		}
	}
	return false;
}

/** Reset the repeat window. Tests only. */
export function resetDiagnosticThrottle(): void {
	recentReports.clear();
}

/** Fields that would carry credentials or prompt text are never sent. */
const FORBIDDEN_DETAIL_KEYS = /^(authorization|api[-_]?key|token|secret|password|cookie|credential|prompt|messages)$/i;

function safeDetails(details: Record<string, unknown> | undefined): Record<string, unknown> | undefined {
	if (!details) return undefined;
	const out: Record<string, unknown> = {};
	for (const [key, value] of Object.entries(details)) {
		if (FORBIDDEN_DETAIL_KEYS.test(key)) continue;
		out[key] = typeof value === "string" && value.length > 2_000 ? `${value.slice(0, 2_000)}…` : value;
	}
	return Object.keys(out).length > 0 ? out : undefined;
}

/**
 * Record a renderer diagnostic. Never throws, never awaits on a render path.
 */
export function reportDiagnostic(report: DiagnosticReport): void {
	const now = Date.now();
	if (isRepeat(report, now)) return;
	const request = {
		severity: report.severity,
		component: report.component,
		event: report.event,
		code: report.code,
		message: report.message,
		retryable: report.retryable ?? false,
		sessionId: report.sessionId ?? null,
		turnId: report.turnId ?? null,
		toolCallId: report.toolCallId ?? null,
		commandId: report.commandId ?? null,
		visualId: report.visualId ?? null,
		visualRevision: report.visualRevision ?? null,
		containerId: report.containerId ?? null,
		rolloutId: report.rolloutId ?? null,
		streamId: report.streamId ?? null,
		optimizerRunId: report.optimizerRunId ?? null,
		traceId: report.traceId ?? null,
		details: safeDetails(report.details) ?? null,
	};
	void invokeCommand(COMMANDS.DIAGNOSTICS_REPORT, { request }).catch(() => {
		// The backend is the record. If it cannot be reached there is nowhere
		// better to put this, and retrying would only amplify the failure.
	});
}

/** Convenience for the common `catch (reason)` shape. */
export function reportDiagnosticError(
	report: Omit<DiagnosticReport, "severity" | "message"> & { message?: string },
	reason: unknown,
): void {
	const message = report.message ?? (reason instanceof Error ? reason.message : String(reason));
	reportDiagnostic({ ...report, severity: "error", message });
}

/**
 * Install the sink the visuals package emits through.
 *
 * Visual bundles cannot import this module (they run in hosts that have no
 * Workshop), so the host hands them a function instead. Called once at startup;
 * absent it, visuals report nothing rather than throwing inside a chart.
 */
export function installVisualDiagnosticSink(): void {
	const host = window as typeof window & {
		__synthDiagnosticSink?: (report: Record<string, unknown>) => void;
	};
	host.__synthDiagnosticSink = (report) => {
		reportDiagnostic({
			severity: (report.severity as DiagnosticSeverity) ?? "error",
			// The visuals package speaks for the surface it renders in.
			component: "visual-host",
			event: String(report.event ?? "visual.stream.event"),
			code: String(report.code ?? "visual_render_failed"),
			message: String(report.message ?? ""),
			retryable: Boolean(report.retryable),
			visualId: (report.visualId as string | null) ?? null,
			rolloutId: (report.rolloutId as string | null) ?? null,
			streamId: (report.streamId as string | null) ?? null,
			traceId: (report.traceId as string | null) ?? null,
			details: (report.details as Record<string, unknown> | undefined) ?? undefined,
		});
	};
}

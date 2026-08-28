/**
 * Diagnostic seam for the visuals package.
 *
 * Visual bundles run in more places than the Workshop renderer: browser
 * preview, the frozen runtime, exported artifacts. They cannot import the
 * renderer's emitter without acquiring a dependency those hosts do not have,
 * so the host installs a sink and the visuals call it.
 *
 * No host, no sink, no error: a visual rendered outside Workshop reports
 * nothing and breaks nothing. That is the whole contract.
 */

export type VisualDiagnosticSeverity = "debug" | "info" | "warn" | "error";

export type VisualDiagnostic = {
	severity: VisualDiagnosticSeverity;
	/** Dotted lowercase name, e.g. `stream.interrupted`. */
	event: string;
	/** Stable snake_case code, shared with `diagnostics/codes.rs`. */
	code: string;
	message: string;
	retryable?: boolean;
	visualId?: string | null;
	rolloutId?: string | null;
	streamId?: string | null;
	traceId?: string | null;
	details?: Record<string, unknown>;
};

export type VisualDiagnosticSink = (report: VisualDiagnostic) => void;

declare global {
	interface Window {
		__synthDiagnosticSink?: VisualDiagnosticSink;
	}
}

/** Codes the visuals package emits. Mirrors `diagnostics/codes.rs`. */
export const VISUAL_STREAM_CODES = {
	streamInterrupted: "stream_interrupted",
	streamReplayGap: "stream_replay_gap",
	streamSubscribeTimeout: "stream_subscribe_timeout"
} as const;

/**
 * Repeat suppression lives here rather than in the host: a reconnecting
 * EventSource can fire the same failure many times a second, and the queue on
 * the other side is bounded.
 */
const REPEAT_WINDOW_MS = 10_000;
const recent = new Map<string, number>();

function suppressed(report: VisualDiagnostic, now: number): boolean {
	const key = `${report.code}|${report.visualId ?? ""}|${report.streamId ?? ""}|${report.message}`;
	const last = recent.get(key);
	if (last !== undefined && now - last < REPEAT_WINDOW_MS) return true;
	recent.set(key, now);
	if (recent.size > 128) {
		for (const [entry, at] of recent) {
			if (now - at >= REPEAT_WINDOW_MS) recent.delete(entry);
		}
	}
	return false;
}

/** Reset the repeat window. Tests only. */
export function resetVisualDiagnostics(): void {
	recent.clear();
}

export function reportVisualDiagnostic(report: VisualDiagnostic): void {
	if (typeof window === "undefined") return;
	const sink = window.__synthDiagnosticSink;
	if (!sink) return;
	if (suppressed(report, Date.now())) return;
	try {
		sink(report);
	} catch {
		// A failure to report a failure is never a second failure.
	}
}

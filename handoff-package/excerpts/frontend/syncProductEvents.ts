/**
 * Pure helpers for the FE5 product surfaces: harness-bundle feature
 * detection and the compact transcript rows for the new sync event
 * kinds (score_series_point_appended, run_completed).
 */

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function asString(value: unknown): string | null {
	return typeof value === "string" && value ? value : null;
}

function asNumber(value: unknown): number | null {
	return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export type HarnessBundleAvailability = "available" | "unsupported" | "unknown";

/**
 * Interprets the HEAD probe against the harness-bundle proxy.
 * 404/501 means the in-flight backend endpoint has not shipped; 405
 * (no HEAD handler) leaves availability unknown, so the button stays
 * visible and failures surface as a graceful toast on click.
 */
export function harnessBundleAvailability(status: number): HarnessBundleAvailability {
	if (status === 404 || status === 501) return "unsupported";
	if (status >= 200 && status < 300) return "available";

	return "unknown";
}

export const SCORE_POINT_EVENT_KIND = "score_series_point_appended";

export const RUN_COMPLETED_EVENT_KIND = "run_completed";

export type ScoreSeriesPointEvent = {
	visual_id: string | null;
	kind: string | null;
	score: number | null;
	delta_vs_baseline: number | null;
};

export type RunCompletedEvent = {
	run_id: string | null;
	outcome: string | null;
	experiment_id: string | null;
	trace_id: string | null;
};

/** Parses a score_series_point_appended event detail payload. */
export function parseScoreSeriesPointEvent(detail: unknown): ScoreSeriesPointEvent | null {
	if (!isRecord(detail)) return null;
	const score = asNumber(detail.score);
	if (score === null) return null;

	return {
		visual_id: asString(detail.visual_id),
		kind: asString(detail.kind),
		score,
		delta_vs_baseline: asNumber(detail.delta_vs_baseline)
	};
}

/** Parses a run_completed event detail payload. */
export function parseRunCompletedEvent(detail: unknown): RunCompletedEvent | null {
	if (!isRecord(detail)) return null;
	const runId = asString(detail.run_id);
	if (!runId) return null;

	return {
		run_id: runId,
		outcome: asString(detail.outcome),
		experiment_id: asString(detail.experiment_id),
		trace_id: asString(detail.trace_id)
	};
}

/** Signed compact rendering for a raw score delta. */
export function formatSignedPoints(delta: number | null): string | null {
	if (delta === null) return null;

	return `${delta >= 0 ? "+" : "−"}${Math.abs(delta)
		.toFixed(3)}`;
}

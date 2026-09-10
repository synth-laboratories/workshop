/**
 * Presentation strings for the run-progress card and dialog. Pure, so the
 * copy rules — an indeterminate bar never claims a percentage, an ETA never
 * reads as a promise — are testable without a webview.
 */

import type { RunEtaProjection, RunProgressProjection, RunProgressStatus } from "./types";
import { UNAVAILABLE } from "./usage";

export function formatDurationMs(ms: number | undefined): string {
	if (ms == null || !Number.isFinite(ms) || ms < 0) return "—";
	const seconds = Math.round(ms / 1000);
	if (seconds < 60) return `${seconds}s`;
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) {
		const rest = seconds % 60;
		return rest ? `${minutes}m ${rest}s` : `${minutes}m`;
	}
	const hours = Math.floor(minutes / 60);
	const rest = minutes % 60;
	return rest ? `${hours}h ${rest}m` : `${hours}h`;
}

/** Coarse minutes for an ETA range, so "about 2–4 min" does not read as measured. */
function coarseMinutes(ms: number): number {
	return Math.max(1, Math.round(ms / 60_000));
}

/**
 * The ETA as the user reads it. Warming and unavailable states are words, not
 * numbers; a range says "about"; only a settled estimate gets the tilde form.
 */
export function formatEta(eta: RunEtaProjection | undefined): string {
	if (!eta) return UNAVAILABLE;
	if (eta.state === "estimating") return "Estimating…";
	if (eta.state === "paused") return "Paused";
	if (eta.state === "unavailable") return UNAVAILABLE;
	if (eta.state === "point" && eta.remainingMs != null) {
		return eta.remainingMs < 60_000
			? `~${Math.max(1, Math.round(eta.remainingMs / 1000))}s remaining`
			: `~${coarseMinutes(eta.remainingMs)} min remaining`;
	}
	const low = eta.lowMs ?? eta.remainingMs;
	const high = eta.highMs ?? eta.remainingMs;
	if (low == null || high == null) return "Estimating…";
	const lowMinutes = coarseMinutes(low);
	const highMinutes = coarseMinutes(high);
	if (lowMinutes === highMinutes) return `about ${lowMinutes} min remaining`;
	return `about ${lowMinutes}–${highMinutes} min remaining`;
}

const STATUS_WORDS: Record<RunProgressStatus, string> = {
	queued: "Queued",
	running: "Running",
	paused: "Paused",
	interrupted: "Interrupted",
	completed: "Completed",
	failed: "Failed",
	cancelled: "Cancelled",
	degraded: "Evidence unavailable",
	// A status word this build does not know. Say so; do not guess a state.
	unknown: "Status unavailable"
};

export function statusLabel(status: RunProgressStatus): string {
	return STATUS_WORDS[status];
}

export function statusBadgeClass(status: RunProgressStatus): string {
	if (status === "failed" || status === "degraded") return "ws-badge ws-badge-danger";
	if (status === "interrupted" || status === "unknown") return "ws-badge ws-badge-warn";
	if (status === "cancelled" || status === "paused" || status === "queued") return "ws-badge ws-badge-warn";
	if (status === "completed") return "ws-badge ws-badge-success";
	return "ws-badge ws-badge-running";
}

/**
 * What the compact card prints for ETA. Insufficient samples stay words
 * ("Estimating…"); a phase that cannot be estimated removes the line entirely
 * rather than showing a number or "Unavailable".
 */
export function etaDisplayLine(
	eta: RunEtaProjection | undefined,
	options: { terminal?: boolean; status?: RunProgressStatus } = {}
): string | null {
	if (options.terminal || options.status === "interrupted") return null;
	if (!eta || eta.state === "unavailable") return null;
	return formatEta(eta);
}

/**
 * The work line. Present only when a truthful denominator exists; otherwise the
 * caller shows the phase detail and an indeterminate bar.
 */
export function formatWork(projection: RunProgressProjection): string | null {
	const { completed, total, unit } = projection.work;
	if (completed == null) return null;
	const noun = unit ?? "units";
	if (total == null) return `${completed.toLocaleString("en-US")} ${noun}`;
	return `${completed.toLocaleString("en-US")} / ${total.toLocaleString("en-US")} ${noun}`;
}

/** The active/queued/failed tail, skipping anything the producer never reported. */
export function formatWorkBreakdown(projection: RunProgressProjection): string | null {
	const { active, queued, failed, retried } = projection.work;
	const parts = [
		active != null ? `${active} active` : null,
		queued != null ? `${queued} queued` : null,
		failed ? `${failed} failed` : null,
		retried ? `${retried} retried` : null
	].filter((part): part is string => part != null);
	return parts.length > 0 ? parts.join(" · ") : null;
}

/**
 * The line that replaces a count when nothing proves one.
 *
 * Returning words here rather than a number is the whole point: a campaign
 * whose evidence is missing reads as "Progress unavailable", with the
 * diagnostic that says where to look, and never as "0 trials".
 */
export function progressUnavailableLine(projection: RunProgressProjection): string | null {
	if (projection.evidence.state === "present") return null;
	if (projection.work.completed != null) return null;
	const reason = projection.evidence.reason;
	return reason ? `Progress unavailable — ${reason}` : "Progress unavailable";
}

/** The accessible value text for the progress bar, indeterminate included. */
export function progressAriaText(projection: RunProgressProjection): string {
	const progress = projection.progress;
	if (!progress?.determinate || progress.fraction == null) {
		return `${projection.phase.label} · progress not measurable`;
	}
	return `${Math.round(progress.fraction * 100)}% of ${progress.semantics}`;
}

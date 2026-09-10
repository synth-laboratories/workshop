/**
 * The single owner of plugin lifecycle presentation.
 *
 * Sidebar, Optimizers page, the v0.9 capability manifest, and any future
 * plugin detail surface derive their label, tone, and usability from here —
 * never from a local boolean and never from a second copy of the phase map.
 *
 * Laguna is not a plugin: its phase comes from LagunaStatus, not this map.
 */

import type { PluginStatus } from "../bridge/types";

/** Matches `PLUGIN_PHASES` in src-tauri/src/plugins/types.rs. */
const PHASE_LABELS: Record<string, string> = {
	not_installed: "Not installed",
	downloading: "Downloading",
	verifying: "Verifying",
	installed: "Installed",
	needs_permissions: "Needs permission",
	starting: "Starting",
	ready: "Ready",
	stopping: "Stopping",
	stopped: "Stopped",
	updating: "Updating",
	removing: "Removing",
	degraded: "Degraded",
	error: "Error",
	disabled: "Disabled"
};

const TRANSITIONAL = new Set([
	"downloading",
	"verifying",
	"starting",
	"stopping",
	"updating",
	"removing"
]);

export type PluginTone = "neutral" | "success" | "warning" | "danger";

export type PluginPresentation = {
	/** Right-aligned sidebar text; null when there is nothing worth saying. */
	label: string | null;
	tone: PluginTone;
	/** True when the plugin can accept work — gates recipe launch controls. */
	isUsable: boolean;
	/** True while a lifecycle action is in flight; drives the progress dot. */
	isTransitional: boolean;
	/** Runs the service still holds, surfaced even when the plugin is disabled. */
	activeRuns: number;
	/** Full sentence for assistive tech; never conveyed by colour alone. */
	a11yLabel: string | null;
	/** Registry detail, already redacted native-side. */
	detail: string | null;
};

/** Status is unknown when the plugin bridge is absent (browser and shell tests). */
const UNKNOWN: PluginPresentation = {
	label: null,
	tone: "neutral",
	isUsable: true,
	isTransitional: false,
	activeRuns: 0,
	a11yLabel: null,
	detail: null
};

export function pluginPresentation(status?: PluginStatus | null): PluginPresentation {
	if (!status) return UNKNOWN;

	const activeRuns = status.service?.activeRuns ?? 0;
	const phase = status.phase;
	const isTransitional = TRANSITIONAL.has(phase);
	const label = PHASE_LABELS[phase] ?? phase;

	// `disable` only clears the registry flag — the sidecar keeps running and
	// there is no active-run guard on it. Saying "Disabled" alone would imply
	// paid work had stopped.
	if (!status.enabled || phase === "disabled") {
		const withRuns = activeRuns > 0 ? `Disabled · ${activeRuns} running` : "Disabled";
		return {
			label: withRuns,
			tone: activeRuns > 0 ? "warning" : "neutral",
			isUsable: false,
			isTransitional: false,
			activeRuns,
			a11yLabel: activeRuns > 0
				? `Disabled, ${activeRuns} run${activeRuns === 1 ? "" : "s"} still active`
				: "Disabled",
			detail: status.detail ?? null
		};
	}

	if (phase === "error") {
		return {
			label: "Error", tone: "danger", isUsable: false, isTransitional: false, activeRuns,
			a11yLabel: "Error", detail: status.detail ?? null
		};
	}

	if (phase === "degraded") {
		return {
			label: "Needs attention", tone: "warning", isUsable: false, isTransitional: false, activeRuns,
			a11yLabel: `Needs attention: ${label.toLowerCase()}`, detail: status.detail ?? null
		};
	}

	// `stopped` with no active runs is the normal resting state of an on-demand
	// sidecar (launching work calls `ensure_ready` natively), not a fault.
	// "Needs attention" is reserved for `degraded` and `error`. A stopped
	// service that still holds runs, though, is worth a warning: work is live
	// with nothing supervising it.
	if (phase === "stopped") {
		if (activeRuns > 0) {
			return {
				label: `Stopped · ${activeRuns} running`,
				tone: "warning",
				isUsable: false,
				isTransitional: false,
				activeRuns,
				a11yLabel: `Stopped, ${activeRuns} run${activeRuns === 1 ? "" : "s"} still active`,
				detail: status.detail ?? null
			};
		}
		return {
			label: "Idle — starts on demand",
			tone: "neutral",
			isUsable: true,
			isTransitional: false,
			activeRuns,
			a11yLabel: "Idle: stopped, starts on demand",
			detail: status.detail ?? null
		};
	}

	if (phase === "not_installed") {
		return {
			label: "Not installed", tone: "neutral", isUsable: false, isTransitional: false, activeRuns,
			a11yLabel: "Not installed", detail: status.detail ?? null
		};
	}

	// Installed, but the OS has not said yes yet. Warning rather than danger:
	// nothing is broken, and the fix is one pane away in System Settings.
	if (phase === "needs_permissions") {
		const missing = (status.permissions ?? [])
			.filter((permission) => permission.state !== "granted" && permission.state !== "not_applicable")
			.map((permission) => permission.label);
		return {
			label: "Needs permission",
			tone: "warning",
			isUsable: false,
			isTransitional: false,
			activeRuns,
			// Naming the grant is what makes this actionable. "Needs permission"
			// on its own sends the operator hunting through Privacy & Security.
			a11yLabel:
				missing.length > 0 ? `Needs permission: ${missing.join(", ")}` : "Needs permission",
			detail: status.detail ?? null
		};
	}

	if (isTransitional) {
		return {
			label, tone: "neutral", isUsable: false, isTransitional: true, activeRuns,
			a11yLabel: `${label}, in progress`, detail: status.detail ?? null
		};
	}

	if (phase === "ready") {
		return {
			label: "Ready", tone: "success", isUsable: true, isTransitional: false, activeRuns,
			a11yLabel: "Ready", detail: status.detail ?? null
		};
	}

	// `installed` and any phase the native side adds later.
	return {
		label, tone: "neutral", isUsable: false, isTransitional: false, activeRuns,
		a11yLabel: label, detail: status.detail ?? null
	};
}

/** Resolve one plugin out of the registry listing. */
export function findPluginStatus(
	statuses: readonly PluginStatus[] | null | undefined,
	pluginId: string
): PluginStatus | null {
	return statuses?.find((status) => status.pluginId === pluginId) ?? null;
}

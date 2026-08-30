/**
 * v0.9 capability catalog — one owner of “what this build actually ships”.
 *
 * Plugin sidecar phases come from `pluginPresentation`. Laguna is a parallel
 * LagunaStatus local sidecar, not a PLUGIN_NAV plugin. Intern/CloudDesk stay
 * unmounted (v0.1 removal). Optimizers visual families are bundled even when
 * the `optimizers` recipe-runner sidecar is not installed — those are two nouns.
 */

import type { PluginStatus } from "../bridge/types";
import { findPluginStatus, pluginPresentation } from "./pluginPresentation";

export const OPTIMIZERS_PLUGIN_ID = "optimizers";
export const COMPUTER_USE_PLUGIN_ID = "computer-use";

export const OPTIMIZERS_VISUAL_FAMILIES_BUNDLED =
	"Optimizers visual families are bundled";

const RECIPE_NOT_READY = "GEPA/SFT recipe runner is not ready";
const RECIPE_AVAILABLE = "GEPA/SFT recipe runner is available";
const NEVER_AGENT_INSTALLABLE = "never agent-installable";
const LOCAL_SIDECAR = "Local sidecar";
const UNSUPPORTED_V09 = "Unsupported in v0.9 (v0.1 removal)";
const BUNDLED_SOURCE_FAMILIES = "Bundled (source families)";

const LAGUNA_PHASE_LABELS: Record<string, string> = {
	unknown: "Unknown",
	starting: "Starting",
	loading: "Loading",
	ready: "Ready",
	unloaded: "Unloaded",
	error: "Error",
	unavailable: "Unavailable",
	not_installed: "Not installed"
};

export type CapabilityRow = {
	id: string;
	kind: string;
	thisBuild: string;
};

export type CapabilityManifestInput = {
	pluginStatuses?: readonly PluginStatus[] | null;
	lagunaPhase?: string | null;
};

function sidecarPhase(
	statuses: readonly PluginStatus[] | null | undefined,
	pluginId: string
): { label: string; recipeReady: boolean } {
	// A missing bridge is not “not installed”: we have not observed a phase.
	if (statuses == null) {
		return { label: "Unknown", recipeReady: false };
	}
	const status = findPluginStatus(statuses, pluginId);
	if (!status) {
		return { label: "Not installed", recipeReady: false };
	}
	const view = pluginPresentation(status);
	return {
		label: view.label ?? status.phase,
		recipeReady: view.isUsable
	};
}

export function lagunaSidecarLabel(phase?: string | null): string {
	if (!phase) return "Unknown";
	return LAGUNA_PHASE_LABELS[phase] ?? phase;
}

/** Honest v0.9 rows for About and Diagnostics. Order is the acceptance table. */
export function v09CapabilityRows(input: CapabilityManifestInput = {}): CapabilityRow[] {
	const statuses = input.pluginStatuses;
	const optimizers = sidecarPhase(statuses, OPTIMIZERS_PLUGIN_ID);
	const computerUse = sidecarPhase(statuses, COMPUTER_USE_PLUGIN_ID);
	const recipeLine = optimizers.recipeReady ? RECIPE_AVAILABLE : RECIPE_NOT_READY;

	return [
		{
			id: OPTIMIZERS_PLUGIN_ID,
			kind: "plugin sidecar",
			thisBuild: `${optimizers.label} — ${recipeLine}. ${OPTIMIZERS_VISUAL_FAMILIES_BUNDLED}.`
		},
		{
			id: COMPUTER_USE_PLUGIN_ID,
			kind: "human-only plugin",
			thisBuild: `${computerUse.label} · ${NEVER_AGENT_INSTALLABLE}`
		},
		{
			id: "laguna",
			kind: "parallel LagunaStatus, not a plugin",
			thisBuild: `${LOCAL_SIDECAR} · ${lagunaSidecarLabel(input.lagunaPhase)}`
		},
		{
			id: "intern / CloudDesk",
			kind: "unmounted",
			thisBuild: UNSUPPORTED_V09
		},
		{
			id: "compose/sourced visuals",
			kind: "bundled",
			thisBuild: BUNDLED_SOURCE_FAMILIES
		}
	];
}

export function capabilityRowTestId(id: string): string {
	return `capability-manifest-row-${id.replace(/[^a-z0-9]+/gi, "-").replace(/^-|-$/g, "").toLowerCase()}`;
}

/**
 * Renderer-side view of Computer Use.
 *
 * Reads over the same loopback IPC the agent adapter uses, so the page and the
 * agent cannot disagree about whether the plugin is ready. Everything that
 * mutates — install, remove, granting permission — is human-initiated from
 * here; there is no agent path to any of it.
 */

import type { PluginPermission, PluginStatus } from "../bridge/types";

export type ComputerUseView = {
	status: PluginStatus | null;
	/** Bundle identifiers this session may drive without a fresh card. */
	allowedApps: readonly string[];
};

export const EMPTY_VIEW: ComputerUseView = { status: null, allowedApps: [] };

/** Grants that are required and not yet held. Drives the wizard's next step. */
export function missingPermissions(status: PluginStatus | null): PluginPermission[] {
	if (!status?.permissions) return [];
	return status.permissions.filter(
		(permission) => permission.state !== "granted" && permission.state !== "not_applicable"
	);
}

/** True once the helper is installed and every required grant is held. */
export function isReady(status: PluginStatus | null): boolean {
	return status?.phase === "ready" && missingPermissions(status).length === 0;
}

/**
 * The single next thing the operator should do.
 *
 * One instruction at a time rather than a checklist: a permission wizard that
 * shows five steps at once gets skimmed, and the step that gets skipped is
 * always the one that matters.
 */
export function nextStep(status: PluginStatus | null): string | null {
	if (!status || status.phase === "not_installed") return "install";
	if (status.phase === "error") return "error";
	if (status.phase === "needs_permissions") return "grant";
	if (status.phase === "ready") return null;
	return "wait";
}

/**
 * Whether a bundle identifier can ever be driven.
 *
 * Mirrors `computer_use::policy::classify_app`. Duplicated deliberately and
 * narrowly: the renderer uses it only to explain *why* an app will not appear,
 * never to permit anything. Desktop refuses regardless of what this says.
 */
export function isPermanentlyDenied(bundleId: string): boolean {
	const id = bundleId.trim().toLowerCase();
	return (
		["terminal", "iterm", "shell", "console", "tty", "keychain", "password"].some((marker) =>
			id.includes(marker)
		) || id === "com.apple.systempreferences"
	);
}

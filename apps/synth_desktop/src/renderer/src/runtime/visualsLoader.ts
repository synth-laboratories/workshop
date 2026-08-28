/** Dynamic loader for @synth/visuals genre shells. */

import type { ComponentType } from "react";
import { getShellImporter } from "@synth/visuals";

type ShellProps = {
	title?: string;
	lede?: string;
	bindings?: Record<string, unknown>;
	[key: string]: unknown;
};

type ShellModule = {
	Shell?: ComponentType<ShellProps>;
	default?: ComponentType<ShellProps>;
};

/**
 * Loads a genre shell by template id.
 * The registry stays in Vite's module graph so the desktop renderer can resolve
 * workspace-local TSX shells in dev and in the packaged renderer. A missing
 * importer returns null; a registered importer that fails is allowed to reject
 * so VisualHost reports a shell-load failure instead of claiming the template
 * was never registered.
 */
export async function loadVisualShell(
	templateId: string
): Promise<ComponentType<ShellProps> | null> {
	const importer = getShellImporter(templateId) as (() => Promise<ShellModule>) | undefined;
	if (!importer) return null;
	const mod = await importer();
	return mod.Shell ?? mod.default ?? null;
}

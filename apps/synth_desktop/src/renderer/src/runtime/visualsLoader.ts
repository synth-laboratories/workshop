/** Dynamic loader for @synth/visuals genre shells. */

import type { ComponentType } from "react";

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

type VisualsRegistry = {
	getShellImporter?: (
		id: string
	) => (() => Promise<ShellModule>) | undefined;
};

/**
 * Loads a genre shell by template id.
 * The registry import stays in Vite's module graph so the desktop renderer can resolve
 * workspace-local TSX shells in dev and in the packaged renderer.
 */
export async function loadVisualShell(
	templateId: string
): Promise<ComponentType<ShellProps> | null> {
	try {
		const registry = (await import("@synth/visuals")) as VisualsRegistry;
		const importer = registry.getShellImporter?.(templateId);
		if (!importer) return null;
		const mod = await importer();
		return mod.Shell ?? mod.default ?? null;
	} catch {
		return null;
	}
}

/**
 * Typed edge for the three workspace-document commands.
 *
 * The types are the generated ones. They were hand-declared here while the
 * commands were written but not yet in `collect_commands!`; now that they are
 * registered and `protocol.ts` has been regenerated, this file re-exports the
 * generated shapes rather than keeping a second copy of a boundary the
 * generator already owns — a hand-kept mirror is exactly what the generated
 * bindings exist to remove.
 */

import { fromGenerated } from "../bridge";
import { commands } from "../generated/protocol";
import type {
	Breadcrumb,
	DirectoryEntry,
	DocumentKind,
	DocumentShown,
	WorkspaceDirectory,
	WorkspaceDocument
} from "../generated/protocol";

export type {
	Breadcrumb,
	DirectoryEntry,
	DocumentKind,
	DocumentShown,
	WorkspaceDirectory,
	WorkspaceDocument
};

export const DOCUMENT_VIEWER_TEMPLATE = "document.viewer.v1";
export const WORKSPACE_FILE_BINDING_KIND = "workspace_file";
export const DOCUMENT_SCHEMA = "synth.workspace-document.v1";

export function readWorkspaceFile(sessionId: string, path: string): Promise<WorkspaceDocument> {
	return fromGenerated(commands.workspaceReadFile(sessionId, path));
}

export function listWorkspaceDir(sessionId: string, path: string): Promise<WorkspaceDirectory> {
	return fromGenerated(commands.workspaceListDir(sessionId, path));
}

export function showDocument(sessionId: string, path: string): Promise<DocumentShown> {
	return fromGenerated(commands.documentShow(sessionId, path));
}

/**
 * The one path a document pane may read, taken from the visual's own bindings.
 *
 * Reads the canonical `synth.visual-bindings.v1` envelope only. A pane whose
 * bindings do not declare exactly this input gets `null` and renders the reason
 * — it does not fall back to "some path in the envelope", because the whole
 * point of the declaration is that the pane cannot widen it.
 */
export function boundDocumentPath(bindings: unknown): string | null {
	if (!bindings || typeof bindings !== "object") return null;
	const inputs = (bindings as { inputs?: unknown }).inputs;
	if (!Array.isArray(inputs)) return null;
	for (const entry of inputs) {
		if (!entry || typeof entry !== "object") continue;
		const descriptor = entry as Record<string, unknown>;
		const name = descriptor.input ?? descriptor.slot;
		if (name !== "document") continue;
		if (descriptor.kind !== WORKSPACE_FILE_BINDING_KIND) continue;
		const source = descriptor.source;
		if (typeof source === "string" && source.trim()) return source;
	}
	return null;
}

/** Human byte count for the pane's status line. */
export function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes < 0) return "—";
	if (bytes < 1024) return `${bytes} B`;
	const units = ["KB", "MB", "GB", "TB"];
	let value = bytes / 1024;
	let unit = 0;
	while (value >= 1024 && unit < units.length - 1) {
		value /= 1024;
		unit += 1;
	}
	return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

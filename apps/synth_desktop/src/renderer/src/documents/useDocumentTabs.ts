/**
 * Open documents in the panel, as view state.
 *
 * The tab list is deliberately *not* durable. What is durable is the pane
 * itself — one `document.viewer.v1` visual per canonical path, created by the
 * host, addressed by a deterministic id — and every tab is re-read from
 * `workspace_read_file` when it opens. This hook holds the order they are
 * stacked in for this window, which is the same class of state as a scroll
 * position: losing it on restart costs nothing, and persisting it would be the
 * renderer inventing durable state (style guide §8).
 *
 * The first tab is the pane's own document and cannot be closed; closing it
 * would leave a document pane showing no document.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { listWorkspaceDir, readWorkspaceFile, type WorkspaceDirectory, type WorkspaceDocument } from "./bridge.ts";
import { toPublicError, type PublicError } from "../runtime/publicError.ts";

export type TabHint = "file" | "directory" | "unknown";

export type TabState =
	| { kind: "loading" }
	| { kind: "document"; document: WorkspaceDocument }
	| { kind: "directory"; listing: WorkspaceDirectory }
	| { kind: "unavailable"; error: PublicError };

export type DocumentTab = {
	/** The path as requested. The host's canonical answer lands in `state`. */
	path: string;
	label: string;
	/** The pane's own document. Always first, never closable. */
	pinned: boolean;
	state: TabState;
};

function labelFor(path: string): string {
	const segments = path.split("/").filter(Boolean);
	return segments[segments.length - 1] ?? path;
}

/**
 * Join a document-relative href onto the document's folder.
 *
 * This is a *proposal*, not a resolution: the host normalizes the result and
 * re-checks it against the session roots, so a `../../etc/passwd` composed here
 * is refused there. The renderer does the join only because a markdown link is
 * relative to the file it appears in, which is a fact only the renderer has.
 */
export function proposeRelative(documentPath: string, href: string): string {
	if (href.startsWith("/")) return href;
	const folder = documentPath.slice(0, documentPath.lastIndexOf("/"));
	const cleaned = href.split(/[?#]/)[0];
	return `${folder}/${cleaned}`;
}

export function useDocumentTabs(sessionId: string | null, rootPath: string | null) {
	const [tabs, setTabs] = useState<DocumentTab[]>([]);
	const [active, setActive] = useState(0);
	/** Mirrors `tabs` for event handlers, which must not read a stale closure. */
	const tabsRef = useRef<DocumentTab[]>([]);
	tabsRef.current = tabs;
	/** Bumped whenever the pane's subject changes, so a read that is still in
	 *  flight for the previous document cannot land in the new one. */
	const generation = useRef(0);

	const load = useCallback(
		async (path: string, hint: TabHint, token: number) => {
			if (!sessionId) return;
			// Settled by path, never by index: a tab closed while a read was in
			// flight would otherwise shift the answer onto its neighbour.
			const settle = (state: TabState) => {
				if (generation.current !== token) return;
				setTabs((current) => current.map((tab) => (tab.path === path ? { ...tab, state } : tab)));
			};
			try {
				if (hint === "directory") {
					settle({ kind: "directory", listing: await listWorkspaceDir(sessionId, path) });
					return;
				}
				settle({ kind: "document", document: await readWorkspaceFile(sessionId, path) });
			} catch (reason) {
				const error = toPublicError(reason, "This document could not be opened.");
				// A path whose kind was unknown and which is not a file is a
				// folder, not a failure — try the other command once before
				// reporting, so a link to a directory lands on its listing.
				if (hint === "unknown") {
					try {
						settle({ kind: "directory", listing: await listWorkspaceDir(sessionId, path) });
						return;
					} catch {
						/* fall through to the original, more specific reason */
					}
				}
				settle({ kind: "unavailable", error });
			}
		},
		[sessionId]
	);

	const open = useCallback(
		(path: string, hint: TabHint = "unknown") => {
			if (!sessionId) return;
			const current = tabsRef.current;
			const existing = current.findIndex((tab) => tab.path === path);
			if (existing !== -1) {
				setActive(existing);
				return;
			}
			const next = [
				...current,
				{ path, label: labelFor(path), pinned: false, state: { kind: "loading" } as TabState }
			];
			tabsRef.current = next;
			setTabs(next);
			setActive(next.length - 1);
			void load(path, hint, generation.current);
		},
		[load, sessionId]
	);

	const close = useCallback((index: number) => {
		const current = tabsRef.current;
		if (!current[index] || current[index].pinned) return;
		const next = current.filter((_, position) => position !== index);
		tabsRef.current = next;
		setTabs(next);
		setActive((position) =>
			Math.max(0, Math.min(position > index ? position - 1 : position, next.length - 1))
		);
	}, []);

	const reload = useCallback(
		(index: number) => {
			const tab = tabsRef.current[index];
			if (!tab) return;
			setTabs((current) =>
				current.map((entry) =>
					entry.path === tab.path ? { ...entry, state: { kind: "loading" } as TabState } : entry
				)
			);
			void load(tab.path, tab.state.kind === "directory" ? "directory" : "unknown", generation.current);
		},
		[load]
	);

	// The pane's own document is tab zero. Changing which visual the pane shows
	// starts a new stack rather than appending to the previous document's.
	useEffect(() => {
		generation.current += 1;
		const token = generation.current;
		if (!sessionId || !rootPath) {
			tabsRef.current = [];
			setTabs([]);
			setActive(0);
			return;
		}
		const seed: DocumentTab[] = [
			{ path: rootPath, label: labelFor(rootPath), pinned: true, state: { kind: "loading" } }
		];
		tabsRef.current = seed;
		setTabs(seed);
		setActive(0);
		void load(rootPath, "file", token);
	}, [load, rootPath, sessionId]);

	return { tabs, active, setActive, open, close, reload };
}

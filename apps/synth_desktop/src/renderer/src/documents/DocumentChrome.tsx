/**
 * Pane chrome: breadcrumbs, the Open menu, and the folder listing that the
 * breadcrumbs and the "+" affordance both land on.
 */

import { useEffect, useRef, useState } from "react";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";

import { formatBytes, type Breadcrumb, type WorkspaceDirectory } from "./bridge.ts";

/**
 * The path trail, root first.
 *
 * Segments come from the host, which computed them from the same canonical
 * path it read: the renderer never splits a path itself, because a second path
 * helper is a defect even when it is correct.
 */
export function DocumentBreadcrumbs({
	trail,
	onOpen
}: {
	trail: Breadcrumb[];
	onOpen: (path: string) => void;
}) {
	if (!trail.length) return null;
	return (
		<nav className="document-breadcrumbs" aria-label="Document path">
			{trail.map((segment, index) => {
				const last = index === trail.length - 1;
				return (
					<span key={segment.path} className="document-breadcrumb">
						{index > 0 ? <span className="document-breadcrumb-sep" aria-hidden="true">/</span> : null}
						{last ? (
							<span className="document-breadcrumb-current" aria-current="page">{segment.label}</span>
						) : (
							<button
								type="button"
								className="document-breadcrumb-link"
								onClick={() => onOpen(segment.path)}
								title={segment.path}
							>
								{segment.label}
							</button>
						)}
					</span>
				);
			})}
		</nav>
	);
}

/** Open ▾ — the external escape hatch, demoted from the primary action. */
export function OpenMenu({ path }: { path: string }) {
	const [open, setOpen] = useState(false);
	const root = useRef<HTMLDivElement | null>(null);
	const trigger = useRef<HTMLButtonElement | null>(null);

	useEffect(() => {
		if (!open) return;
		const onKeyDown = (event: KeyboardEvent) => {
			if (event.key !== "Escape") return;
			event.stopPropagation();
			setOpen(false);
			trigger.current?.focus();
		};
		const onPointerDown = (event: PointerEvent) => {
			if (root.current && event.target instanceof Node && !root.current.contains(event.target)) setOpen(false);
		};
		window.addEventListener("keydown", onKeyDown, true);
		window.addEventListener("pointerdown", onPointerDown, true);
		return () => {
			window.removeEventListener("keydown", onKeyDown, true);
			window.removeEventListener("pointerdown", onPointerDown, true);
		};
	}, [open]);

	const act = (run: () => void) => {
		run();
		setOpen(false);
		trigger.current?.focus();
	};

	return (
		<div className="document-open-menu" ref={root}>
			<button
				type="button"
				ref={trigger}
				className="document-open-trigger"
				aria-expanded={open}
				aria-haspopup="menu"
				onClick={() => setOpen((current) => !current)}
			>
				Open <span aria-hidden="true">▾</span>
			</button>
			{open ? (
				<div className="document-open-items" role="menu">
					<button type="button" role="menuitem" onClick={() => act(() => void openPath(path))}>
						Open in default app
					</button>
					<button type="button" role="menuitem" onClick={() => act(() => void revealItemInDir(path))}>
						Reveal in Finder
					</button>
					<button
						type="button"
						role="menuitem"
						onClick={() => act(() => void navigator.clipboard?.writeText(path))}
					>
						Copy path
					</button>
				</div>
			) : null}
		</div>
	);
}

/**
 * One folder's contents.
 *
 * Every child is a row. A child that cannot be opened keeps its row and shows
 * the host's reason beside it — a folder of binaries reads as a folder of
 * binaries, never as an empty folder.
 */
export function DirectoryListing({
	listing,
	onOpen
}: {
	listing: WorkspaceDirectory;
	onOpen: (path: string) => void;
}) {
	if (!listing.entries.length) {
		return (
			<p className="document-empty" role="status">
				This folder is empty.
			</p>
		);
	}
	return (
		<div className="document-listing">
			<ul>
				{listing.entries.map((entry) => (
					<li key={entry.path} data-kind={entry.kind}>
						<button
							type="button"
							className="document-listing-row"
							disabled={!entry.openable}
							onClick={() => onOpen(entry.path)}
							title={entry.path}
						>
							<span className="document-listing-icon" aria-hidden="true">
								{entry.kind === "directory" ? "▸" : "·"}
							</span>
							<span className="document-listing-name">{entry.name}</span>
							<span className="document-listing-meta">
								{entry.kind === "directory" ? "folder" : entry.language}
							</span>
							<span className="document-listing-size">
								{entry.kind === "directory" ? "" : formatBytes(entry.byteSize)}
							</span>
							{entry.reason ? <span className="document-listing-reason">{entry.reason}</span> : null}
						</button>
					</li>
				))}
			</ul>
			{listing.truncated ? (
				<p className="document-truncated" role="status">
					Showing the first {listing.entries.length} entries. Open the folder externally to see the rest.
				</p>
			) : null}
		</div>
	);
}

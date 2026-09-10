/**
 * The document pane — the right panel's second provider, rendered.
 *
 * Mounted from the `VisualHost` dispatch on `document.viewer.v1`, the same way
 * `diagram.mermaid.v1` and `analysis.chart.v1` are host-rendered rather than
 * shelled: the pane's bytes arrive through a scoped host command, not through
 * a bound payload, so there is no template shell for it to be.
 *
 * The pane owns its own tab strip and breadcrumbs rather than pushing them
 * into `VisualPane`'s header. Tabs here are *documents*, which is what the
 * requirement asked for; lifting the strip into the panel header so a trace and
 * a document can share one row is a later, larger change to `openArtifact`.
 */

import { useMemo } from "react";
import type { ArtifactRef } from "../types/landing";

import "./DocumentPane.css";
import { DirectoryListing, DocumentBreadcrumbs, OpenMenu } from "./DocumentChrome.tsx";
import { DocumentContent, DocumentOutline } from "./DocumentContent.tsx";
import { boundDocumentPath, DOCUMENT_VIEWER_TEMPLATE, formatBytes } from "./bridge.ts";
import { proposeRelative, useDocumentTabs, type DocumentTab } from "./useDocumentTabs.ts";

/** Whether the panel should render this artifact as a document. */
export function isDocumentArtifact(artifact: ArtifactRef): boolean {
	return artifact.templateId === DOCUMENT_VIEWER_TEMPLATE;
}

/** A named reason, never a blank pane. */
function Unavailable({ title, detail, remediation, code, onRetry }: {
	title: string;
	detail: string;
	remediation?: string;
	code?: string;
	onRetry?: () => void;
}) {
	return (
		<div className="document-unavailable" role="alert" data-error-code={code}>
			<strong>{title}</strong>
			<p>{detail}</p>
			{remediation ? <p className="document-unavailable-remediation">{remediation}</p> : null}
			{onRetry ? (
				<button type="button" className="document-retry" onClick={onRetry}>Retry</button>
			) : null}
			{code ? <p className="document-unavailable-code"><code>{code}</code></p> : null}
		</div>
	);
}

function TabBody({
	tab,
	onOpen,
	onRetry
}: {
	tab: DocumentTab;
	onOpen: (path: string, hint: "file" | "directory" | "unknown") => void;
	onRetry: () => void;
}) {
	switch (tab.state.kind) {
		case "loading":
			return <p className="document-loading" role="status">Reading {tab.label}…</p>;
		case "directory":
			return <DirectoryListing listing={tab.state.listing} onOpen={(path) => onOpen(path, "unknown")} />;
		case "unavailable":
			return (
				<Unavailable
					title={`${tab.label} cannot be shown`}
					detail={tab.state.error.message}
					remediation={tab.state.error.remediation}
					code={tab.state.error.code}
					onRetry={onRetry}
				/>
			);
		case "document": {
			const opened = tab.state.document;
			return (
				<DocumentContent
					document={opened}
					onOpenLink={(href) => onOpen(proposeRelative(opened.path, href), "unknown")}
				/>
			);
		}
	}
}

export function DocumentPane({ artifact, sessionId }: { artifact: ArtifactRef; sessionId?: string | null }) {
	const boundPath = useMemo(() => boundDocumentPath(artifact.bindings), [artifact.bindings]);
	// The pane reads through the conversation that owns it. A pane with no
	// conversation has no scope to read under, and says so rather than reading
	// under whichever session the window happens to be showing.
	const conversation = sessionId ?? artifact.sessionId ?? artifact.ownerSessionId ?? null;
	const { tabs, active, setActive, open, close, reload } = useDocumentTabs(conversation, boundPath);
	const current = tabs[active];

	if (!boundPath) {
		return (
			<Unavailable
				title="This pane declares no document"
				detail="A document pane reads exactly one path, declared in its visual's bindings. This visual declares none."
				remediation="Reopen the file with document_show, which writes the binding."
				code="document_binding_missing"
			/>
		);
	}
	if (!conversation) {
		return (
			<Unavailable
				title="This document is not attached to a conversation"
				detail="Workspace files are read through the conversation whose folder they belong to, and this pane is not bound to one."
				remediation="Reopen the file from the conversation that produced it."
				code="document_scope_unbound"
			/>
		);
	}

	const trail = current?.state.kind === "document"
		? current.state.document.breadcrumbs
		: current?.state.kind === "directory"
			? current.state.listing.breadcrumbs
			: [];
	const currentPath = current?.state.kind === "document"
		? current.state.document.path
		: current?.state.kind === "directory"
			? current.state.listing.path
			: current?.path ?? boundPath;
	const folder = trail.length > 1 ? trail[trail.length - 2].path : null;

	return (
		<div className="document-pane" data-testid="document-pane">
			<div className="document-tabs" role="tablist" aria-label="Open documents">
				{tabs.map((tab, index) => (
					<div key={tab.path} className="document-tab" data-active={index === active}>
						<button
							type="button"
							role="tab"
							aria-selected={index === active}
							className="document-tab-label"
							title={tab.path}
							onClick={() => setActive(index)}
						>
							{tab.label}
							{tab.state.kind === "unavailable" ? <span className="document-tab-flag" aria-label="Unavailable"> !</span> : null}
						</button>
						{tab.pinned ? null : (
							<button
								type="button"
								className="document-tab-close"
								aria-label={`Close ${tab.label}`}
								onClick={() => close(index)}
							>
								×
							</button>
						)}
					</div>
				))}
				<button
					type="button"
					className="document-tab-add"
					aria-label="Browse the containing folder"
					title="Browse the containing folder"
					disabled={!folder}
					onClick={() => folder && open(folder, "directory")}
				>
					+
				</button>
			</div>

			<div className="document-pane-head">
				<DocumentBreadcrumbs trail={trail} onOpen={(path) => open(path, "directory")} />
				<div className="document-pane-actions">
					<button type="button" className="document-reload" onClick={() => reload(active)}>Reload</button>
					<OpenMenu path={currentPath} />
				</div>
			</div>

			{current?.state.kind === "document" ? (
				<p className="document-status" role="status">
					{current.state.document.language} · {formatBytes(current.state.document.byteSize)}
					{current.state.document.modifiedAt
						? ` · modified ${new Date(current.state.document.modifiedAt).toLocaleString()}`
						: ""}
					{" · "}
					<code title="Digest of the bytes rendered">{current.state.document.contentDigest.slice(7, 15)}</code>
				</p>
			) : null}

			<div className="document-pane-body" role="tabpanel">
				{current?.state.kind === "document" && current.state.document.kind === "markdown" ? (
					<DocumentOutline
						source={current.state.document.text}
						onJump={(slug) => document.getElementById(slug)?.scrollIntoView({ behavior: "smooth", block: "start" })}
					/>
				) : null}
				{current ? (
					<TabBody tab={current} onOpen={open} onRetry={() => reload(active)} />
				) : (
					<p className="document-loading" role="status">Opening…</p>
				)}
			</div>
		</div>
	);
}

export default DocumentPane;

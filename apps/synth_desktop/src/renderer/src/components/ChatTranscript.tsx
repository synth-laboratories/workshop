import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { ArtifactRef, LocalActivityLine, LocalChat } from "../types/landing";
import type { Session } from "@synth/runtime-protocol";
import { FileTypeIcon, shortenPath } from "./FileTypeIcon";
import { ContainerIcon } from "./ContainerPane";
import { ManderPresence } from "./mander";
import {
	activityStatusAnnouncement,
	pairActivityGroupLines,
	presentActivityLines,
	type ToolActivityMode
} from "../preferences";
import { contextCompactionTokenSummary } from "../runtime/sessionView";
import "./PaidComputeApprovalModal.css";

type Props = {
	chat: LocalChat;
	openArtifactId: string | null;
	/** Pass artifact id to toggle open/closed; pass null to force close. */
	onOpenArtifact: (id: string | null) => void;
	openContainerId?: string | null;
	onOpenContainer?: (id: string | null) => void;
	onApprove?: (approvalId: string) => void;
	onAlwaysAllow?: (approvalId: string) => void;
	onReject?: (approvalId: string) => void;
	running?: boolean;
	warmingUp?: boolean;
	onStop?: () => void;
	onAdvanced?: () => void;
	activityMode?: ToolActivityMode;
	onActivityModeChange?: (mode: ToolActivityMode) => void;
	outputsOpen?: boolean;
	onToggleOutputs?: () => void;
	showMascot?: boolean;
	session?: Session;
	historyState?: TranscriptHistoryState;
	onLoadOlder?: () => void;
};

export type TranscriptHistoryState = {
	state: "idle" | "loading" | "loaded" | "error";
	hasMore: boolean;
	error?: string;
};

export function outputContainerIds(chat: LocalChat): string[] {
	return [...new Set(Object.values(chat.activityByMessageId ?? {}).flat().map((line) => line.containerId).filter((id): id is string => Boolean(id)))];
}

export function OutputsPanel({ chat, openArtifactId, onOpenArtifact, openContainerId = null, onOpenContainer }: Pick<Props, "chat" | "openArtifactId" | "onOpenArtifact" | "openContainerId" | "onOpenContainer">) {
	const artifacts = chat.artifacts ?? [];
	const subagents = artifacts.filter((artifact) => artifact.templateId === "synth.subagents.v1");
	const visuals = artifacts.filter((artifact) => artifact.templateId !== "synth.subagents.v1");
	const containerIds = outputContainerIds(chat);
	const hasResources = containerIds.length > 0 || artifacts.length > 0;
	return <div id="chat-resource-shelf" className="resource-shelf resource-shelf-docked" aria-label="Outputs" data-testid="resource-shelf">
		{!hasResources ? <div className="resource-shelf-empty" data-testid="resource-shelf-empty">
			<span className="resource-shelf-empty-icon" aria-hidden>
				<svg viewBox="0 0 24 24" fill="none"><path d="M7.5 3.75h6l3 3v13.5h-9z"/><path d="M13.5 3.75v3h3M9.75 11h4.5M9.75 14h4.5"/></svg>
			</span>
			<span className="resource-shelf-empty-copy"><strong>No outputs yet</strong><span>Files, visuals, and containers will appear here.</span></span>
		</div> : null}
		{containerIds.length > 0 ? <section className="containers-rail" data-testid="containers-rail"><h3>Containers</h3>{containerIds.map((id) => (
			<button key={id} type="button" className={`resource-shelf-row container-rail-btn${openContainerId === id ? " active" : ""}`} onClick={() => onOpenContainer?.(openContainerId === id ? null : id)} aria-pressed={openContainerId === id} aria-label={openContainerId === id ? "Hide container inspector" : "Open container inspector"} data-testid={`container-icon-${id}`}>
				<span className="resource-shelf-icon"><ContainerIcon /></span><span><strong>Container</strong><code>{id}</code></span><span aria-hidden>›</span>
			</button>
		))}</section> : null}
		{subagents.length > 0 ? <section className="subagents-rail" data-testid="subagents-rail"><h3>Subagents</h3>{subagents.map((artifact) => {
			const active = openArtifactId === artifact.id;
			return <button key={artifact.id} type="button" className={`resource-shelf-row${active ? " active" : ""}`} onClick={() => onOpenArtifact(artifact.id)} title={active ? `Hide ${artifact.title}` : `Show ${artifact.title}`} aria-pressed={active} aria-label={active ? `Hide subagents ${artifact.title}` : `Show subagents ${artifact.title}`} data-testid={`visuals-icon-${artifact.id}`}>
				<span className="resource-shelf-icon"><IconSubagents /></span><span><strong>{artifact.title}</strong><code>{artifact.summary ?? artifact.templateId}</code></span><span aria-hidden>›</span>
			</button>;
		})}</section> : null}
		{visuals.length > 0 ? <section className="visuals-rail" data-testid="visuals-rail"><h3>Visuals</h3>{visuals.map((artifact) => {
			const active = openArtifactId === artifact.id;
			return <button key={artifact.id} type="button" className={`resource-shelf-row${active ? " active" : ""}`} onClick={() => onOpenArtifact(artifact.id)} title={active ? `Hide ${artifact.title}` : `Show ${artifact.title}`} aria-pressed={active} aria-label={active ? `Hide visual ${artifact.title}` : `Show visual ${artifact.title}`} data-testid={`visuals-icon-${artifact.id}`}>
				<span className="resource-shelf-icon"><IconVisual /></span><span><strong>{artifact.title}</strong><code>{artifact.templateId ?? artifact.kind}</code></span><span aria-hidden>›</span>
			</button>;
		})}</section> : null}
	</div>;
}

function IconVisual() {
	return (
		<svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="2" y="2" width="12" height="12" rx="2.5" stroke="currentColor" strokeWidth="1.3" />
			<path
				d="M2.5 10.5l3.2-3.2 2.4 2.4 2.6-3.3 3.3 4.1"
				stroke="currentColor"
				strokeWidth="1.2"
				strokeLinejoin="round"
			/>
			<circle cx="5.4" cy="5.2" r="1.1" fill="currentColor" />
		</svg>
	);
}

function IconSubagents() {
	return (
		<svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
			<circle cx="8" cy="5" r="2.25" stroke="currentColor" strokeWidth="1.25" />
			<path d="M3.5 13c.3-2.35 1.8-3.55 4.5-3.55s4.2 1.2 4.5 3.55" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" />
			<path d="M2.2 4.25h1.55M12.25 4.25h1.55M3 7.2l1.35-.65M13 7.2l-1.35-.65" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
		</svg>
	);
}

/** Exact context-compaction glyph used by the installed Codex app. */
function IconContextCompaction() {
	return (
		<svg width="20" height="20" viewBox="0 0 20 20" fill="none" aria-hidden>
			<path d="M12.666 3.50098C13.3549 3.50098 13.9121 3.50133 14.3623 3.53809C14.8202 3.5755 15.2268 3.65483 15.6035 3.84668C16.1988 4.15007 16.6829 4.63424 16.9863 5.22949C17.1782 5.60603 17.2575 6.01205 17.2949 6.46973C17.3317 6.91983 17.3311 7.47721 17.3311 8.16602V15.1377C17.9209 15.3944 18.333 15.9827 18.333 16.667C18.3328 17.5872 17.5872 18.3328 16.667 18.333C15.7466 18.333 15.0002 17.5873 15 16.667C15 15.9832 15.4119 15.3957 16.001 15.1387V8.16602C16.001 7.45532 16.0011 6.96153 15.9697 6.57812C15.939 6.20279 15.8822 5.99093 15.8018 5.83301C15.6258 5.4879 15.3442 5.20711 14.999 5.03125C14.8411 4.95091 14.6291 4.89394 14.2539 4.86328C13.8705 4.83199 13.3767 4.83105 12.666 4.83105H7.5C7.13284 4.83092 6.83496 4.5332 6.83496 4.16602C6.8353 3.79912 7.13305 3.50111 7.5 3.50098H12.666Z" fill="currentColor" />
			<path d="M3.33301 1.66699C4.25337 1.66699 4.99981 2.41269 5 3.33301C5 4.01711 4.58759 4.60453 3.99805 4.86133V11.833C3.99805 12.5438 3.99896 13.0374 4.03027 13.4209C4.06095 13.7963 4.11783 14.008 4.19824 14.166C4.37411 14.5112 4.6549 14.7918 5 14.9678C5.15797 15.0483 5.36958 15.105 5.74512 15.1357C6.12859 15.1671 6.6221 15.168 7.33301 15.168H12.5L12.6338 15.1816C12.9367 15.2437 13.1649 15.5118 13.165 15.833C13.165 16.1543 12.9368 16.4223 12.6338 16.4844L12.5 16.498H7.33301C6.64403 16.498 6.08691 16.4987 5.63672 16.4619C5.17904 16.4245 4.77303 16.3451 4.39648 16.1533C3.8011 15.8499 3.31608 15.365 3.0127 14.7695C2.82102 14.393 2.7415 13.987 2.7041 13.5293C2.66734 13.0791 2.66797 12.5219 2.66797 11.833V4.86035C2.07898 4.60332 1.66699 4.0167 1.66699 3.33301C1.66718 2.41283 2.41284 1.66721 3.33301 1.66699Z" fill="currentColor" />
			<path d="M10.1338 11.0146C10.4366 11.0766 10.6647 11.345 10.665 11.666C10.665 11.9873 10.4367 12.2553 10.1338 12.3174L10 12.3311H7.5C7.13284 12.3309 6.83496 12.0332 6.83496 11.666C6.8353 11.2991 7.13305 11.0011 7.5 11.001H10L10.1338 11.0146Z" fill="currentColor" />
			<path d="M12.6338 7.68164C12.9367 7.74367 13.1649 8.01182 13.165 8.33301C13.165 8.65433 12.9368 8.92232 12.6338 8.98438L12.5 8.99805H7.5C7.13284 8.99791 6.83496 8.7002 6.83496 8.33301C6.83513 7.96596 7.13294 7.6681 7.5 7.66797H12.5L12.6338 7.68164Z" fill="currentColor" />
		</svg>
	);
}

function ActivityLine({
	line,
	visualOpen,
	onToggleVisual,
	containerOpen,
	onToggleContainer,
	onApprove,
	onAlwaysAllow,
	onReject,
	live: _live = false
}: {
	line: LocalActivityLine;
	visualOpen?: boolean;
	onToggleVisual?: () => void;
	containerOpen?: boolean;
	onToggleContainer?: () => void;
	onApprove?: (approvalId: string) => void;
	onAlwaysAllow?: (approvalId: string) => void;
	onReject?: (approvalId: string) => void;
	live?: boolean;
}) {
	const [open, setOpen] = useState(false);
	const isVisualCue = Boolean(onToggleVisual) || line.kind === "visual";
	const isFile =
		Boolean(line.path) || line.kind === "file_read" || line.kind === "file_write";
	const expandable = Boolean(line.detail) && !isVisualCue && !isFile;
	if (line.kind === "approval" && line.approvalId) {
		const approvalId = line.approvalId ?? line.id;
		return (
			<div className="approval-card" data-testid={`approval-${line.id}`}>
				<div className="approval-card-kicker">Permission</div>
				<strong>{line.label}</strong>
				{line.detail ? <p>{line.detail}</p> : null}
				<div className="approval-card-actions">
					<button type="button" className="approval-reject" onClick={() => onReject?.(approvalId)}>Reject</button>
					{line.alwaysAllowSupported ? <button type="button" className="approval-always" onClick={() => onAlwaysAllow?.(approvalId)}>Always allow for this session</button> : null}
					<button type="button" className="approval-approve" onClick={() => onApprove?.(approvalId)}>Approve once</button>
				</div>
			</div>
		);
	}

	if (isFile && line.path) {
		const verb = line.kind === "file_write" ? "Wrote" : line.label.replace(/^…\s*/, "") || "Read";
		const showVerb = /^(Read|Wrote|Edit)/i.test(verb) ? verb.split(/\s/)[0] : "Read";
		return (
			<div className="local-activity file-activity" data-testid={`activity-${line.id}`}>
				<FileTypeIcon path={line.path} />
				<span className="file-activity-text">
					<span className="file-activity-verb">{showVerb}</span>{" "}
					<code className="file-activity-path" title={line.path}>
						{shortenPath(line.path)}
					</code>
				</span>
			</div>
		);
	}

	if (line.kind === "run_summary") {
		return (
			<div className="run-summary" data-testid={`activity-${line.id}`}>
				<span aria-hidden>•••</span>
				<span>{line.label}</span>
			</div>
		);
	}

	if (line.kind === "visual_lifecycle" && line.visualStage) {
		const stageLabel = line.visualStage === "draft" ? "Draft"
			: line.visualStage === "review" ? "In review"
				: line.visualStage === "ready" ? "Ready" : "Needs attention";
		return (
			<div className={`visual-lifecycle stage-${line.visualStage}`} data-testid={`activity-${line.id}`}>
				<span className="visual-lifecycle-mark" aria-hidden><IconVisual /></span>
				<span className="visual-lifecycle-copy">
					<strong>{line.label}</strong>
					<span>Draft <i /> Review <i /> Ready</span>
					{line.detail ? <span className="visual-lifecycle-detail">{line.detail}</span> : null}
				</span>
				<span className="visual-lifecycle-status">{stageLabel}</span>
				{onToggleVisual ? (
					<button
						type="button"
						className="visual-lifecycle-open"
						onClick={onToggleVisual}
						aria-label={visualOpen ? "Hide visual" : "Open visual"}
						data-testid={line.artifactId ? `tool-visual-open-${line.artifactId}` : undefined}
					>
						{visualOpen ? "Hide" : "Open"}
					</button>
				) : null}
			</div>
		);
	}

	if (line.kind === "context_compaction") {
		const summary = line.tokensBefore != null && line.tokensAfter != null
			? (line.detail ?? contextCompactionTokenSummary(line.tokensBefore, line.tokensAfter))
			: line.detail;
		if (!summary) {
			return (
				<div className="context-compaction-divider" data-testid={`activity-${line.id}`}>
					<IconContextCompaction />
					<span>{line.label}</span>
					<span className="context-compaction-rule" aria-hidden />
				</div>
			);
		}
		return (
			<div className={`context-compaction-divider expandable${open ? " open" : ""}`} data-testid={`activity-${line.id}`}>
				<button
					type="button"
					className="context-compaction-toggle"
					aria-expanded={open}
					aria-controls={`activity-detail-${line.id}`}
					onClick={() => setOpen((value) => !value)}
					data-testid={`activity-toggle-${line.id}`}
				>
					<IconContextCompaction />
					<span className="context-compaction-label">{line.label}</span>
					<span className="context-compaction-chevron" aria-hidden>{open ? "▾" : "▸"}</span>
				</button>
				<span className="context-compaction-rule" aria-hidden />
				{open ? (
					<div id={`activity-detail-${line.id}`} className="context-compaction-detail" data-testid={`activity-detail-${line.id}`}>
						{summary}
					</div>
				) : null}
			</div>
		);
	}

	if (line.kind === "command") {
		return (
			<div className="local-activity tool-activity command-activity" data-testid={`activity-${line.id}`}>
				<span className="tool-activity-icon" aria-hidden>&gt;_</span>
				<span className="tool-activity-body">
					<span className="tool-activity-label">{line.label}</span>
					{line.detail ? <code title={line.detail}>{line.detail}</code> : null}
				</span>
			</div>
		);
	}

	if (line.kind === "search") {
		return (
			<div className="local-activity tool-activity search-activity" data-testid={`activity-${line.id}`}>
				<span className="tool-activity-icon" aria-hidden>⌕</span>
				<span className="tool-activity-body">
					<span className="tool-activity-label">{line.label}</span>
					{line.detail ? <span className="tool-activity-detail">{line.detail}</span> : null}
				</span>
			</div>
		);
	}

	if (line.toolStatus) {
		return (
			<div className="local-activity tool-activity mcp-activity" data-testid={`activity-${line.id}`}>
				<span className="tool-activity-icon" aria-hidden>◆</span>
				<span className="tool-activity-body">
					<code className="mcp-activity-name">{line.label}</code>
					{line.detail ? <span className="tool-activity-detail">{line.detail}</span> : null}
					<span className={`tool-status tool-status-${line.toolStatus}`}>{line.toolStatus === "running" ? "Running" : line.toolStatus === "completed" ? "Completed" : "Failed"}</span>
				</span>
				{onToggleVisual ? (
					<button
						type="button"
						className={`tool-visual-open${visualOpen ? " active" : ""}`}
						onClick={onToggleVisual}
						aria-pressed={visualOpen}
						aria-label={visualOpen ? "Hide visual in side panel" : "Open visual in side panel"}
						title={visualOpen ? "Hide visual" : "Open visual"}
						data-testid={`tool-visual-open-${line.artifactId}`}
					>
						<IconVisual />
					</button>
				) : null}
				{onToggleContainer ? (
					<button
						type="button"
						className={`tool-container-open${containerOpen ? " active" : ""}`}
						onClick={onToggleContainer}
						aria-pressed={containerOpen}
						aria-label={containerOpen ? "Hide container inspector" : "Open container inspector"}
						title={containerOpen ? "Hide container" : "Inspect container"}
						data-testid={`tool-container-open-${line.containerId}`}
					>
						<ContainerIcon />
					</button>
				) : null}
			</div>
		);
	}

	if (isVisualCue && onToggleVisual) {
		return (
			<button
				type="button"
				className={`local-activity visual-cue${visualOpen ? " active" : ""}`}
				onClick={onToggleVisual}
				aria-pressed={visualOpen}
				data-testid={`activity-${line.id}`}
			>
				<span className="local-activity-label">{line.label}</span>
				<span className="visual-cue-hint">{visualOpen ? "Hide" : "Show"}</span>
			</button>
		);
	}

	if (!expandable) {
		return (
			<div className="local-activity" data-testid={`activity-${line.id}`}>
				<span className="local-activity-label">{line.label}</span>
			</div>
		);
	}

	const isReasoning = line.kind === "thought";
	return (
		<div className={`local-activity expandable${isReasoning ? " reasoning-disclosure" : ""}${open ? " open" : ""}`}>
			<button
				type="button"
				className="local-activity-toggle"
				aria-expanded={open}
				aria-controls={`activity-detail-${line.id}`}
				aria-label={isReasoning ? `${open ? "Hide" : "Show"} ${line.reasoningDisplay === "summary" ? "Reasoning summary" : "Thought"}` : undefined}
				onClick={() => setOpen((v) => !v)}
				data-testid={`activity-${line.id}`}
			>
				<span className="local-activity-label">{line.label}</span>
				{isReasoning ? (
					<svg className="reasoning-disclosure-chevron" viewBox="0 0 12 12" fill="none" aria-hidden>
						<path d="m3 4.75 3 3 3-3" />
					</svg>
				) : <span className="local-activity-hint">{open ? "Hide" : "Show"}</span>}
			</button>
			{open ? (
				<pre id={`activity-detail-${line.id}`} className="local-activity-detail" data-testid={`activity-detail-${line.id}`}>
					{line.detail}
				</pre>
			) : !isReasoning ? (
				<div className="local-activity-wave" aria-hidden />
			) : null}
		</div>
	);
}

function PaidComputeApprovalModal({ line, onApprove, onReject }: {
	line: LocalActivityLine;
	onApprove?: (approvalId: string) => void;
	onReject?: (approvalId: string) => void;
}) {
	const payload = line.approvalPayload;
	const cap = payload?.requestedCap;
	const estimated = payload?.estimatedCostUsdMicros;
	const formatUsd = (micros: number) => `$${(micros / 1_000_000).toFixed(2)}`;
	const approveRef = useRef<HTMLButtonElement>(null);
	useEffect(() => {
		approveRef.current?.focus();
		const onKey = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				event.preventDefault();
				onReject?.(line.approvalId!);
			}
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [line.approvalId, onReject]);
	return <div className="paid-compute-modal-backdrop" data-testid="paid-compute-approval-modal">
		<section className="paid-compute-modal" role="dialog" aria-modal="true" aria-labelledby="paid-compute-title">
			<div className="approval-card-kicker">Paid compute</div>
			<h2 id="paid-compute-title">Approve this bounded run?</h2>
			<dl>
				<div><dt>Requesting agent</dt><dd>{payload?.requestingAgent ?? "Unknown agent"}</dd></div>
				<div><dt>Operation</dt><dd><code>{payload?.operation ?? "optimizer recipe"}</code></dd></div>
				{estimated != null ? <div><dt>Predicted spend</dt><dd>{formatUsd(estimated)}</dd></div> : <div><dt>Predicted spend</dt><dd>Not available</dd></div>}
				{cap?.maxCostUsdMicros != null ? <div><dt>Cost cap</dt><dd>{formatUsd(cap.maxCostUsdMicros)}</dd></div> : null}
				{cap?.maxRollouts != null ? <div><dt>Rollout cap</dt><dd>{cap.maxRollouts.toLocaleString()}</dd></div> : null}
			</dl>
			{payload?.parameters ? <details><summary>Run parameters</summary><pre>{JSON.stringify(payload.parameters, null, 2)}</pre></details> : null}
			<p className="paid-compute-consent">I approve this run only within the cap shown above.</p>
			<div className="paid-compute-modal-actions">
				<button type="button" className="approval-reject" onClick={() => onReject?.(line.approvalId!)}>Reject</button>
				<button ref={approveRef} type="button" className="approval-approve" onClick={() => onApprove?.(line.approvalId!)}>Approve with cap</button>
			</div>
		</section>
	</div>;
}

function VisualCard({
	artifact,
	active,
	onToggle
}: {
	artifact: ArtifactRef;
	active: boolean;
	onToggle: () => void;
}) {
	const lifecycle = artifact.status === "review" ? "In review"
		: artifact.status === "ready" ? "Ready"
			: artifact.status === "failed" ? "Needs attention" : "Draft";
	return (
		<button
			type="button"
			className={`visual-card${active ? " active" : ""}`}
			onClick={(e) => {
				e.preventDefault();
				e.stopPropagation();
				onToggle();
			}}
			aria-pressed={active}
			aria-label={active ? `Hide ${artifact.title}` : `Show ${artifact.title}`}
			data-testid={`artifact-chip-${artifact.id}`}
		>
			<span className="visual-card-icon">
				{artifact.templateId === "synth.subagents.v1" ? <IconSubagents /> : <IconVisual />}
			</span>
			<span className="visual-card-body">
				<span className="visual-card-title">{artifact.title}</span>
				<span className="visual-card-meta">
					<span className={`visual-card-status status-${artifact.status ?? "draft"}`}>{lifecycle}</span>
					{active ? " · open" : " · click to open"}
				</span>
			</span>
			<span className="visual-card-action">{active ? "Hide" : "Open"}</span>
		</button>
	);
}

function ActivityGroup({
	id,
	label,
	summary,
	status,
	count,
	lines,
	expanded,
	onToggle,
	renderLine
}: {
	id: string;
	label: string;
	summary: string;
	status: string;
	count: number;
	lines: LocalActivityLine[];
	expanded: boolean;
	onToggle: () => void;
	renderLine: (line: LocalActivityLine) => ReactNode;
}) {
	return (
		<div className={`activity-group status-${status}`} data-testid={`activity-group-${id}`} data-status={status}>
			<button
				type="button"
				className="activity-group-toggle"
				aria-expanded={expanded}
				aria-controls={`activity-group-body-${id}`}
				onClick={onToggle}
				data-testid={`activity-group-toggle-${id}`}
			>
				<span className="activity-group-chevron" aria-hidden="true">›</span>
				<span className="activity-group-label">{label}</span>
				<span className="activity-group-summary">{summary}</span>
				<span className="sr-only">{count} tool {count === 1 ? "call" : "calls"}</span>
			</button>
			{expanded ? (
				<div id={`activity-group-body-${id}`} className="activity-group-body" data-testid={`activity-group-body-${id}`}>
					{pairActivityGroupLines(lines).map((row) => (
						<div
							key={row.id}
							className={`activity-group-step${row.context.length ? " has-context" : ""}${row.action ? "" : " context-only"}`}
						>
							{row.context.length ? <div className="activity-group-context">{row.context.map((line) => renderLine(line))}</div> : null}
							{row.action ? <div className="activity-group-action">{renderLine(row.action)}</div> : null}
						</div>
					))}
				</div>
			) : null}
		</div>
	);
}

const COLLAPSE_USER_MESSAGE_AT = 1_000;
const COLLAPSE_USER_MESSAGE_LINES = 12;

function IconCopy() {
	return (
		<svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="5.25" y="2.25" width="8.5" height="9.5" rx="1.5" stroke="currentColor" strokeWidth="1.25" />
			<path d="M10.75 12.25v.5A1.5 1.5 0 019.25 14.25h-6a1.5 1.5 0 01-1.5-1.5v-6a1.5 1.5 0 011.5-1.5h.5" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" />
		</svg>
	);
}

function copyText(value: string): Promise<void> {
	if (navigator.clipboard?.writeText) return navigator.clipboard.writeText(value);
	return new Promise((resolve, reject) => {
		const textarea = document.createElement("textarea");
		textarea.value = value;
		textarea.style.position = "fixed";
		textarea.style.opacity = "0";
		document.body.append(textarea);
		textarea.select();
		const copied = document.execCommand("copy");
		textarea.remove();
		if (copied) resolve();
		else reject(new Error("Clipboard access is unavailable"));
	});
}

function CopyMessageButton({ body }: { body: string }) {
	const [copied, setCopied] = useState(false);
	const [failed, setFailed] = useState(false);
	return (
		<button
			type="button"
			className="message-copy"
			aria-label="Copy message"
			title={failed ? "Clipboard unavailable" : copied ? "Copied" : "Copy message"}
			onClick={() => {
				void copyText(body).then(() => {
					setFailed(false);
					setCopied(true);
					window.setTimeout(() => setCopied(false), 1_600);
				}).catch(() => setFailed(true));
			}}
		>
			<IconCopy />
			<span>{copied ? "Copied" : "Copy"}</span>
		</button>
	);
}

/**
 * A pasted brief can be important, but it must not turn the current turn into
 * a screen-height blue wall. Keep the full value in the DOM and make expansion
 * an explicit, reversible choice.
 */
function UserMessage({ id, body, images, onExpansionChange }: { id: string; body: string; images?: Array<{ path: string; name: string; previewUrl: string }>; onExpansionChange: () => void }) {
	const [expanded, setExpanded] = useState(false);
	const collapsible = body.length > COLLAPSE_USER_MESSAGE_AT || body.split(/\r?\n/).length > COLLAPSE_USER_MESSAGE_LINES;
	const bodyId = `user-message-body-${id}`;
	return (
		<div className="local-user-message">
			<div
				className={`local-bubble local-bubble-user${collapsible && !expanded ? " is-collapsed" : ""}`}
				data-testid={`user-message-${id}`}
			>
				{body ? <p id={bodyId}>{body}</p> : null}
				{images?.length ? (
					<div className="local-user-images" aria-label="Attached screenshots">
						{images.map((image) => (
							<figure key={image.path}>
								<img src={image.previewUrl} alt={image.name} />
								<figcaption>{image.name}</figcaption>
							</figure>
						))}
					</div>
				) : null}
				{collapsible ? (
					<button
						type="button"
						className="local-bubble-expand"
						aria-expanded={expanded}
						aria-controls={bodyId}
						onClick={() => {
							setExpanded((value) => !value);
							onExpansionChange();
						}}
					>
						{expanded ? "Show less" : "Show full message"}
					</button>
				) : null}
			</div>
			<div className="message-actions message-actions-user"><CopyMessageButton body={body} /></div>
		</div>
	);
}

export function ChatTranscript({
	chat,
	openArtifactId,
	onOpenArtifact,
	openContainerId = null,
	onOpenContainer,
	onApprove,
	onAlwaysAllow,
	onReject,
	running = false,
	warmingUp = false,
	onStop,
	onAdvanced,
	activityMode = "grouped",
	onActivityModeChange,
	outputsOpen = false,
	onToggleOutputs,
	showMascot = false,
	session,
	historyState,
	onLoadOlder
}: Props) {
	const [expandedGroupIds, setExpandedGroupIds] = useState<Set<string>>(() => new Set());
	const [modeMenuOpen, setModeMenuOpen] = useState(false);
	const modeMenuRef = useRef<HTMLDivElement>(null);
	const scrollRef = useRef<HTMLDivElement>(null);
	const followsTailRef = useRef(true);
	const historyAnchorRef = useRef<{ chatId: string; scrollHeight: number } | null>(null);
	const historyRequestPendingRef = useRef(false);
	const previousChatIdRef = useRef(chat.id);
	const previousActiveRef = useRef<LocalActivityLine[] | undefined>(undefined);
	const [liveAnnouncement, setLiveAnnouncement] = useState("");
	const activityByMessageId = chat.activityByMessageId ?? {};
	const artifacts = chat.artifacts ?? [];
	const containerIds = outputContainerIds(chat);
	const hasResources = containerIds.length > 0 || artifacts.length > 0;
	const finalAssistantMessageId = useMemo(() => {
		for (let index = chat.messages.length - 1; index >= 0; index -= 1) {
			if (chat.messages[index]?.role === "assistant") return chat.messages[index]!.id;
		}
		return null;
	}, [chat.messages]);
	const activeLines = activityByMessageId.__active__ ?? [];
	// A pending approval is a turn-level blocking condition, not ordinary
	// message activity. Message-id rotation and replay ordering can attach it to
	// a historical/user key that is not rendered as assistant activity. Pin all
	// unresolved approvals at the live tail so a turn can never look like an
	// inert "Working…" state while it is actually waiting for the operator.
	const pendingApprovals = useMemo(() => {
		const byId = new Map<string, LocalActivityLine>();
		for (const line of Object.values(activityByMessageId).flat()) {
			if (line.kind === "approval" && line.approvalId) byId.set(line.approvalId, line);
		}
		return [...byId.values()];
	}, [activityByMessageId]);
	const paidComputeApproval = pendingApprovals.find((line) => line.approvalKind === "paid_compute");
	const inlineApprovals = pendingApprovals.filter((line) => line.approvalKind !== "paid_compute");
	const withoutPendingApproval = (line: LocalActivityLine) =>
		!(line.kind === "approval" && line.approvalId);
	const presentedActive = useMemo(
		() => presentActivityLines(activeLines.filter(withoutPendingApproval), activityMode, { running, expandedGroupIds }),
		[activeLines, activityMode, running, expandedGroupIds]
	);
	const transcriptContentKey = useMemo(() => [
		chat.id,
		running ? "running" : "idle",
		chat.messages.map((message) => `${message.id}:${message.role}:${message.body}`).join("\u001f"),
		activeLines.map((line) => `${line.id}:${line.toolStatus ?? ""}:${line.label}:${line.detail ?? ""}`).join("\u001f")
	].join("\u001e"), [activeLines, chat.id, chat.messages, running]);

	useEffect(() => {
		if (previousChatIdRef.current !== chat.id) {
			followsTailRef.current = true;
			historyAnchorRef.current = null;
			historyRequestPendingRef.current = false;
		}
		previousChatIdRef.current = chat.id;
	}, [chat.id]);

	useEffect(() => {
		if (historyState?.state !== "loading") historyRequestPendingRef.current = false;
	}, [historyState?.state]);

	/*
	 * --composer-clearance is published by Composer.tsx onto .main-pane and
	 * inherited here. A second writer used to set it on this scroller, which won
	 * on specificity while only observing the dock's *size*: moving the dock
	 * (terminal open/close, pane changes) left a stale value and the last turn
	 * scrolled under the composer. One owner, measured from the dock itself.
	 */

	useLayoutEffect(() => {
		const anchor = historyAnchorRef.current;
		const scroller = scrollRef.current;
		if (!anchor || !scroller || anchor.chatId !== chat.id || historyState?.state === "loading") return;
		scroller.scrollTop += scroller.scrollHeight - anchor.scrollHeight;
		historyAnchorRef.current = null;
	}, [chat.id, historyState?.state, transcriptContentKey]);

	useLayoutEffect(() => {
		if (!followsTailRef.current) return;
		const scroller = scrollRef.current;
		if (!scroller) return;
		const frame = requestAnimationFrame(() => {
			scroller.scrollTop = scroller.scrollHeight;
		});
		return () => cancelAnimationFrame(frame);
	}, [transcriptContentKey]);

	const requestOlderHistory = () => {
		const scroller = scrollRef.current;
		if (!scroller || !onLoadOlder || !historyState?.hasMore || historyState.state === "loading" || historyRequestPendingRef.current) return;
		historyRequestPendingRef.current = true;
		historyAnchorRef.current = { chatId: chat.id, scrollHeight: scroller.scrollHeight };
		onLoadOlder();
	};

	useEffect(() => {
		const announcement = activityStatusAnnouncement(previousActiveRef.current, activeLines, running);
		previousActiveRef.current = activeLines;
		if (announcement) setLiveAnnouncement(announcement);
	}, [activeLines, running]);

	useEffect(() => {
		if (!modeMenuOpen) return;
		const close = (event: MouseEvent) => {
			if (!modeMenuRef.current?.contains(event.target as Node)) setModeMenuOpen(false);
		};
		document.addEventListener("mousedown", close);
		return () => document.removeEventListener("mousedown", close);
	}, [modeMenuOpen]);

	const toggleGroup = (id: string) => {
		setExpandedGroupIds((current) => {
			const next = new Set(current);
			if (next.has(id)) next.delete(id);
			else next.add(id);
			return next;
		});
	};

	const keepTailVisible = () => {
		followsTailRef.current = true;
		requestAnimationFrame(() => {
			const scroller = scrollRef.current;
			if (scroller) scroller.scrollTop = scroller.scrollHeight;
		});
	};

	const renderActivityLine = (line: LocalActivityLine, messageArtifacts: ArtifactRef[] = [], primaryOpen = false, live = false) => {
		const primaryArtifact = messageArtifacts[0];
		const linkedArtifact = line.artifactId
			? artifacts.find((artifact) => artifact.id === line.artifactId)
			: primaryArtifact;
		const opensVisual = line.kind === "visual" || line.kind === "subagent" || /visual|artifact/i.test(line.label);
		return (
			<ActivityLine
				key={line.id}
				line={line}
				visualOpen={linkedArtifact ? openArtifactId === linkedArtifact.id : primaryOpen}
				onToggleVisual={opensVisual && linkedArtifact ? () => onOpenArtifact(linkedArtifact.id) : undefined}
				containerOpen={Boolean(line.containerId && openContainerId === line.containerId)}
				onToggleContainer={line.containerId && onOpenContainer ? () => onOpenContainer(openContainerId === line.containerId ? null : line.containerId!) : undefined}
				onApprove={onApprove}
				onAlwaysAllow={onAlwaysAllow}
				onReject={onReject}
				live={live && line.kind === "thought"}
			/>
		);
	};

	const renderPresented = (
		items: ReturnType<typeof presentActivityLines>,
		messageArtifacts: ArtifactRef[] = [],
		primaryOpen = false,
		live = false
	) => items.map((item) => {
		if (item.kind === "line") return renderActivityLine(item.line, messageArtifacts, primaryOpen, live);
		return (
			<ActivityGroup
				key={item.id}
				id={item.id}
				label={item.label}
				summary={item.summary}
				status={item.status}
				count={item.count}
				lines={item.lines}
				expanded={item.expanded}
				onToggle={() => toggleGroup(item.id)}
				renderLine={(line) => renderActivityLine(line, messageArtifacts, primaryOpen, live)}
			/>
		);
	});

	return (
		<div className={`chat-transcript${outputsOpen ? " resources-open" : ""}`} data-testid="chat-transcript" data-activity-mode={activityMode}>
			{showMascot ? <ManderPresence session={session} chat={chat} running={running} /> : null}
			<div className="transcript-toolbar" data-testid="transcript-toolbar">
			<div className="activity-mode-bar" ref={modeMenuRef}>
				<button
					type="button"
					className="activity-mode-trigger"
					aria-expanded={modeMenuOpen}
					aria-controls="activity-mode-menu"
					aria-haspopup="menu"
					data-testid="activity-mode-menu-trigger"
					onClick={() => setModeMenuOpen((open) => !open)}
				>
					Activity · {activityMode}
				</button>
				{modeMenuOpen ? (
					<div id="activity-mode-menu" className="activity-mode-menu" role="menu" data-testid="activity-mode-menu">
						{(["detailed", "grouped", "compact"] as ToolActivityMode[]).map((mode) => (
							<button
								key={mode}
								type="button"
								role="menuitemradio"
								aria-checked={activityMode === mode}
								className={activityMode === mode ? "selected" : ""}
								data-testid={`activity-mode-option-${mode}`}
								onClick={() => {
									onActivityModeChange?.(mode);
									setModeMenuOpen(false);
								}}
							>
								{mode[0]!.toUpperCase() + mode.slice(1)}
							</button>
						))}
					</div>
				) : null}
			</div>
			<button type="button" className={`resource-shelf-trigger${outputsOpen ? " active" : ""}`} onClick={onToggleOutputs} aria-expanded={outputsOpen} aria-controls="workbench-side-panel" data-testid="resource-shelf-trigger"><span aria-hidden>☷</span> Outputs {hasResources ? <strong>{containerIds.length + artifacts.length}</strong> : null}</button>
			</div>
			<div className="sr-only" role="status" aria-live="polite" data-testid="activity-live-region">{liveAnnouncement}</div>

			<div
				className="chat-transcript-scroll"
				ref={scrollRef}
				onScroll={(event) => {
					const node = event.currentTarget;
					const followsTail = node.scrollHeight - node.scrollTop - node.clientHeight <= 96;
					followsTailRef.current = followsTail;
					if (node.scrollTop <= 96 && !followsTail) requestOlderHistory();
				}}
			>
				<div className="chat-transcript-inner">
					{historyState?.state === "loading" ? (
						<div className="transcript-history-status" role="status">Loading conversation history…</div>
					) : historyState?.state === "error" ? (
						<div className="transcript-history-status transcript-history-error" role="alert">History unavailable: {historyState.error ?? "Unknown error"}</div>
					) : historyState?.hasMore ? (
						<button type="button" className="transcript-history-load" onClick={requestOlderHistory}>Load earlier history</button>
					) : null}
						{chat.messages.map((m) => {
						const showAdvancedAtMessage = !running && onAdvanced && m.id === finalAssistantMessageId;
						const messageArtifacts = artifacts.filter((a) => a.messageId === m.id);
						const primaryArtifact = messageArtifacts[0];
						const primaryOpen = primaryArtifact
							? openArtifactId === primaryArtifact.id
							: false;
						const messageActivity = activityByMessageId[m.id] ?? [];
						const presented = presentActivityLines(messageActivity.filter((line) => line.placement !== "after" && withoutPendingApproval(line)), activityMode, {
							running: false,
							expandedGroupIds
						});
						const presentedAfter = presentActivityLines(messageActivity.filter((line) => line.placement === "after" && withoutPendingApproval(line)), activityMode, {
							running: false,
							expandedGroupIds
						});
						return (
							<div key={m.id} className={`local-turn local-turn-${m.role}`}>
								{m.role === "assistant" ? renderPresented(presented, messageArtifacts, primaryOpen, running) : null}
								{m.role === "user" ? (
									<UserMessage id={m.id} body={m.body} images={m.images} onExpansionChange={keepTailVisible} />
								) : m.role === "system" ? (
									<div className="local-system"><p>{m.body}</p><div className="message-actions"><CopyMessageButton body={m.body} /></div></div>
								) : (
									<div className={`local-assistant${showAdvancedAtMessage ? " local-assistant-with-advanced" : ""}`}>
										<div className="local-assistant-content">
											<p>{m.body}</p>
											{showAdvancedAtMessage ? <button type="button" className="message-advanced" onClick={onAdvanced} aria-label="Open advanced trace">Advanced</button> : null}
										</div>
										<div className="message-actions"><CopyMessageButton body={m.body} /></div>
									</div>
								)}
								{m.role === "assistant" ? renderPresented(presentedAfter, [], false, running) : null}
								{messageArtifacts.map((a) => (
									<VisualCard
										key={a.id}
										artifact={a}
										active={openArtifactId === a.id}
										onToggle={() => onOpenArtifact(a.id)}
									/>
								))}
							</div>
						);
						})}
						{renderPresented(presentedActive, [], false, running)}
						{inlineApprovals.map((line) => renderActivityLine(line, [], false, false))}
						{running ? (
							<div className="model-working" role="status" aria-live="polite" data-testid="model-working">
								<span className="model-working-dots" aria-hidden><i /><i /><i /></span>
								<span>{warmingUp ? "Warming up…" : "Working…"}</span>
								{onStop ? <button type="button" onClick={onStop} aria-label="Stop generating">Stop</button> : null}
								{onAdvanced ? <button type="button" onClick={onAdvanced} aria-label="Open advanced trace">Advanced</button> : null}
							</div>
						) : null}
					</div>
			</div>
			{paidComputeApproval ? <PaidComputeApprovalModal line={paidComputeApproval} onApprove={onApprove} onReject={onReject} /> : null}
		</div>
	);
}

import { useEffect, useState } from "react";
import type { ArtifactRef, LocalActivityLine, LocalChat } from "../types/landing";
import { FileTypeIcon, shortenPath } from "./FileTypeIcon";
import { ContainerIcon } from "./ContainerPane";

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
	onStop?: () => void;
};

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

function ActivityLine({
	line,
	visualOpen,
	onToggleVisual,
	containerOpen,
	onToggleContainer,
	onApprove,
	onAlwaysAllow,
	onReject
}: {
	line: LocalActivityLine;
	visualOpen?: boolean;
	onToggleVisual?: () => void;
	containerOpen?: boolean;
	onToggleContainer?: () => void;
	onApprove?: (approvalId: string) => void;
	onAlwaysAllow?: (approvalId: string) => void;
	onReject?: (approvalId: string) => void;
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

	return (
		<div className={`local-activity expandable${open ? " open" : ""}`}>
			<button
				type="button"
				className="local-activity-toggle"
				aria-expanded={open}
				onClick={() => setOpen((v) => !v)}
				data-testid={`activity-${line.id}`}
			>
				<span className="local-activity-label">{line.label}</span>
				<span className="local-activity-hint">{open ? "Hide" : "Open"}</span>
			</button>
			{open ? (
				<pre className="local-activity-detail" data-testid={`activity-detail-${line.id}`}>
					{line.detail}
				</pre>
			) : (
				<div className="local-activity-wave" aria-hidden />
			)}
		</div>
	);
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
					{artifact.kind.replace(/_/g, " ")}
					{active ? " · open" : " · click to open"}
				</span>
			</span>
			<span className="visual-card-action">{active ? "Hide" : "Open"}</span>
		</button>
	);
}

export function ChatTranscript({ chat, openArtifactId, onOpenArtifact, openContainerId = null, onOpenContainer, onApprove, onAlwaysAllow, onReject, running = false, onStop }: Props) {
	const [resourcesOpen, setResourcesOpen] = useState(true);
	const activityByMessageId = chat.activityByMessageId ?? {};
	const artifacts = chat.artifacts ?? [];
	const containerIds = [...new Set(Object.values(activityByMessageId).flat().map((line) => line.containerId).filter((id): id is string => Boolean(id)))];
	const hasResources = containerIds.length > 0 || artifacts.length > 0;
	useEffect(() => {
		if (openArtifactId || openContainerId) setResourcesOpen(false);
	}, [openArtifactId, openContainerId]);

	return (
		<div className="chat-transcript" data-testid="chat-transcript">
			{hasResources ? <button type="button" className={`resource-shelf-trigger${resourcesOpen ? " active" : ""}`} onClick={() => setResourcesOpen((open) => !open)} aria-expanded={resourcesOpen} aria-controls="chat-resource-shelf" data-testid="resource-shelf-trigger"><span aria-hidden>☷</span> Outputs <strong>{containerIds.length + artifacts.length}</strong></button> : null}
			{hasResources && resourcesOpen ? <aside id="chat-resource-shelf" className="resource-shelf" aria-label="Outputs" data-testid="resource-shelf">
				<header><span>Outputs</span><button type="button" onClick={() => setResourcesOpen(false)} aria-label="Close outputs panel">×</button></header>
				{containerIds.length > 0 ? <section className="containers-rail" data-testid="containers-rail"><h3>Containers</h3>{containerIds.map((id) => (
					<button key={id} type="button" className={`resource-shelf-row container-rail-btn${openContainerId === id ? " active" : ""}`} onClick={() => { setResourcesOpen(false); onOpenContainer?.(openContainerId === id ? null : id); }} aria-pressed={openContainerId === id} aria-label={openContainerId === id ? "Hide container inspector" : "Open container inspector"} data-testid={`container-icon-${id}`}>
						<span className="resource-shelf-icon"><ContainerIcon /></span><span><strong>Container</strong><code>{id}</code></span><span aria-hidden>›</span>
					</button>
				))}</section> : null}
				{artifacts.length > 0 ? <section className="visuals-rail" data-testid="visuals-rail"><h3>Visuals</h3>{artifacts.map((a) => {
						const active = openArtifactId === a.id;
						return (
							<button
								key={a.id}
								type="button"
								className={`resource-shelf-row${active ? " active" : ""}`}
								onClick={() => { setResourcesOpen(false); onOpenArtifact(a.id); }}
								title={active ? `Hide ${a.title}` : `Show ${a.title}`}
								aria-pressed={active}
								aria-label={active ? `Hide visual ${a.title}` : `Show visual ${a.title}`}
								data-testid={`visuals-icon-${a.id}`}
							>
								<span className="resource-shelf-icon">{a.templateId === "synth.subagents.v1" ? <IconSubagents /> : <IconVisual />}</span><span><strong>{a.title}</strong><code>{a.templateId ?? a.kind}</code></span><span aria-hidden>›</span>
							</button>
						);
						})}</section> : null}
			</aside> : null}

			<div className="chat-transcript-scroll">
				<div className="chat-transcript-inner">
						{chat.messages.map((m) => {
						const messageArtifacts = artifacts.filter((a) => a.messageId === m.id);
						const primaryArtifact = messageArtifacts[0];
						const primaryOpen = primaryArtifact
							? openArtifactId === primaryArtifact.id
							: false;
						return (
							<div key={m.id} className={`local-turn local-turn-${m.role}`}>
								{m.role === "assistant"
									? (activityByMessageId[m.id] ?? []).map((line) => {
										const linkedArtifact = line.artifactId
											? artifacts.find((artifact) => artifact.id === line.artifactId)
											: primaryArtifact;
										const opensVisual = line.kind === "visual" || line.kind === "subagent" || /visual|artifact/i.test(line.label);
										return <ActivityLine
											key={line.id}
											line={line}
											visualOpen={linkedArtifact ? openArtifactId === linkedArtifact.id : primaryOpen}
											onToggleVisual={opensVisual && linkedArtifact ? () => onOpenArtifact(linkedArtifact.id) : undefined}
											containerOpen={Boolean(line.containerId && openContainerId === line.containerId)}
											onToggleContainer={line.containerId && onOpenContainer ? () => onOpenContainer(openContainerId === line.containerId ? null : line.containerId!) : undefined}
												onApprove={onApprove}
												onAlwaysAllow={onAlwaysAllow}
											onReject={onReject}
										/>;
									})
									: null}
								{m.role === "user" ? (
									<div className="local-bubble local-bubble-user">
										<p>{m.body}</p>
									</div>
								) : m.role === "system" ? (
									<p className="local-system">{m.body}</p>
								) : (
									<div className="local-assistant">
										<p>{m.body}</p>
									</div>
								)}
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
						{(activityByMessageId.__active__ ?? []).map((line) => {
							const linkedArtifact = line.artifactId
								? artifacts.find((artifact) => artifact.id === line.artifactId)
								: undefined;
							return <ActivityLine
								key={line.id}
								line={line}
								visualOpen={linkedArtifact ? openArtifactId === linkedArtifact.id : false}
								onToggleVisual={linkedArtifact ? () => onOpenArtifact(linkedArtifact.id) : undefined}
								containerOpen={Boolean(line.containerId && openContainerId === line.containerId)}
								onToggleContainer={line.containerId && onOpenContainer ? () => onOpenContainer(openContainerId === line.containerId ? null : line.containerId!) : undefined}
								onApprove={onApprove}
								onAlwaysAllow={onAlwaysAllow}
								onReject={onReject}
							/>;
						})}
						{running ? (
							<div className="model-working" role="status" aria-live="polite" data-testid="model-working">
								<span className="model-working-dots" aria-hidden><i /><i /><i /></span>
								<span>Working…</span>
								<button type="button" onClick={onStop} aria-label="Stop generating">Stop</button>
							</div>
						) : null}
					</div>
			</div>
		</div>
	);
}

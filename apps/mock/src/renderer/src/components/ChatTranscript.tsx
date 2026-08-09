import { useState } from "react";
import type { ArtifactRef, LocalActivityLine, LocalChat } from "../types/landing";
import { FileTypeIcon, shortenPath } from "./FileTypeIcon";

type Props = {
	chat: LocalChat;
	openArtifactId: string | null;
	/** Pass artifact id to toggle open/closed; pass null to force close. */
	onOpenArtifact: (id: string | null) => void;
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

function ActivityLine({
	line,
	visualOpen,
	onToggleVisual
}: {
	line: LocalActivityLine;
	visualOpen?: boolean;
	onToggleVisual?: () => void;
}) {
	const [open, setOpen] = useState(false);
	const isVisualCue = Boolean(onToggleVisual) || line.kind === "visual";
	const isFile =
		Boolean(line.path) || line.kind === "file_read" || line.kind === "file_write";
	const expandable = Boolean(line.detail) && !isVisualCue && !isFile;

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
				<IconVisual />
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

export function ChatTranscript({ chat, openArtifactId, onOpenArtifact }: Props) {
	const activityByMessageId = chat.activityByMessageId ?? {};
	const artifacts = chat.artifacts ?? [];

	return (
		<div className="chat-transcript" data-testid="chat-transcript">
			{artifacts.length > 0 ? (
				<div className="visuals-rail" data-testid="visuals-rail">
					<span className="visuals-rail-label">Visuals</span>
					{artifacts.map((a) => {
						const active = openArtifactId === a.id;
						return (
							<button
								key={a.id}
								type="button"
								className={`visuals-rail-btn${active ? " active" : ""}`}
								onClick={() => onOpenArtifact(a.id)}
								title={active ? `Hide ${a.title}` : `Show ${a.title}`}
								aria-pressed={active}
								aria-label={active ? `Hide visual ${a.title}` : `Show visual ${a.title}`}
								data-testid={`visuals-icon-${a.id}`}
							>
								<IconVisual />
							</button>
						);
					})}
				</div>
			) : null}

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
									? (activityByMessageId[m.id] ?? []).map((line) => (
											<ActivityLine
												key={line.id}
												line={line}
												visualOpen={primaryOpen}
												onToggleVisual={
													(line.kind === "visual" || /visual|artifact/i.test(line.label)) &&
													primaryArtifact
														? () => onOpenArtifact(primaryArtifact.id)
														: undefined
												}
											/>
										))
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
				</div>
			</div>
		</div>
	);
}

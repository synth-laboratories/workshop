import { useState } from "react";
import type { ActivityEvent, AsyncInternPin, ChatMessage, SyncSession } from "../types/landing";
import { ASYNC_PHASE_LABEL, SYNC_STATUS_LABEL } from "../types/landing";
import { VisualPane } from "./VisualPane";

type SyncProps = {
	kind: "sync";
	session: SyncSession;
	openArtifactId?: string | null;
	onOpenArtifact?: (id: string | null) => void;
	onBack: () => void;
	onAction: (label: string) => void;
	onSendMessage?: (text: string) => void;
};

type AsyncProps = {
	kind: "async";
	intern: AsyncInternPin;
	onBack: () => void;
	onAction: (label: string) => void;
	onSendMessage?: (text: string) => void;
};

type Props = SyncProps | AsyncProps;

type ActivityFilter = "all" | "mailbox";

function MessagePanel({
	messages,
	artifacts = [],
	openArtifactId = null,
	onOpenArtifact,
	onSend
}: {
	messages: ChatMessage[];
	artifacts?: SyncSession["artifacts"];
	openArtifactId?: string | null;
	onOpenArtifact?: (id: string | null) => void;
	onSend: (text: string) => void;
}) {
	const [draft, setDraft] = useState("");

	const submit = () => {
		const text = draft.trim();
		if (!text) return;
		onSend(text);
		setDraft("");
	};

	return (
		<section className="desk-message-panel" aria-label="Messaging" data-testid="cloud-messaging">
			<div className="desk-pane-head">
				<span className="desk-pane-title">Messages</span>
			</div>

			<div className="desk-messages-scroll">
				{messages.length === 0 ? (
					<p className="desk-empty">No messages yet.</p>
				) : (
					<div className="desk-messages">
						{messages.map((m) => {
							const messageArtifacts = (artifacts ?? []).filter((a) => a.messageId === m.id);
							return (
								<article key={m.id} className={`desk-msg desk-msg-${m.role}`}>
									{m.role === "user" ? (
										<div className="desk-bubble">{m.body}</div>
									) : (
										<div className="desk-assistant-block">
											<span className="desk-msg-meta">
												{m.role === "assistant" ? "Intern" : "System"} · {m.at}
											</span>
											<p>{m.body}</p>
										</div>
									)}
									{messageArtifacts.length > 0 && onOpenArtifact ? (
										<div className="artifact-chips">
											{messageArtifacts.map((a) => (
												<button
													key={a.id}
													type="button"
													className={`artifact-chip${openArtifactId === a.id ? " active" : ""} shown`}
													onClick={() => onOpenArtifact(a.id)}
													data-testid={`artifact-chip-${a.id}`}
												>
													<span className="artifact-chip-icon" aria-hidden>
														◈
													</span>
													<span className="artifact-chip-text">
														<span className="artifact-chip-kind">{a.kind}</span>
														<span className="artifact-chip-title">{a.title}</span>
													</span>
												</button>
											))}
										</div>
									) : null}
								</article>
							);
						})}
					</div>
				)}
			</div>

			<div className="desk-message-compose">
				<input
					type="text"
					placeholder="Message Intern…"
					value={draft}
					onChange={(e) => setDraft(e.target.value)}
					onKeyDown={(e) => {
						if (e.key === "Enter") {
							e.preventDefault();
							submit();
						}
					}}
					aria-label="Session message"
					data-testid="cloud-message-input"
				/>
				<button
					type="button"
					className="desk-send"
					disabled={!draft.trim()}
					onClick={submit}
					data-testid="cloud-message-send"
				>
					Send
				</button>
			</div>
		</section>
	);
}

function ActivityStream({
	activity,
	filter,
	onFilterChange
}: {
	activity: ActivityEvent[];
	filter: ActivityFilter;
	onFilterChange: (next: ActivityFilter) => void;
}) {
	const visible =
		filter === "mailbox" ? activity.filter((ev) => ev.lane === "intern") : activity;

	return (
		<section className="desk-activity-panel" aria-label="Activity" data-testid="cloud-activity-panel">
			<div className="desk-pane-head desk-pane-head-row">
				<span className="desk-pane-title">
					{filter === "mailbox" ? "Mailbox" : "Activity"}
				</span>
				<div className="filter-seg" role="group" aria-label="Activity filter">
					<button
						type="button"
						className={filter === "all" ? "active" : ""}
						onClick={() => onFilterChange("all")}
					>
						All
					</button>
					<button
						type="button"
						className={filter === "mailbox" ? "active" : ""}
						onClick={() => onFilterChange("mailbox")}
						data-testid="mailbox-only-toggle"
					>
						Mailbox
					</button>
				</div>
			</div>

			<div className="desk-activity-scroll">
				{visible.length === 0 ? (
					<p className="desk-empty">No events.</p>
				) : (
					<div className="desk-activity" data-testid="cloud-activity">
						{visible
							.slice()
							.reverse()
							.map((ev) => (
								<article
									key={`${ev.lane}-${ev.sequence}`}
									className={`activity-row lane-${ev.lane}`}
									data-event-kind={ev.eventKind}
								>
									<div className="activity-row-top">
										<span className={`lane-dot lane-${ev.lane}`} aria-hidden />
										<span className="activity-kind">{ev.eventKind}</span>
										<span className="activity-when">{ev.at}</span>
									</div>
									<p className="activity-summary">{ev.summary}</p>
									{ev.detail ? <pre className="activity-detail">{ev.detail}</pre> : null}
								</article>
							))}
					</div>
				)}
			</div>
		</section>
	);
}

export function CloudDesk(props: Props) {
	const [filter, setFilter] = useState<ActivityFilter>("all");
	const [interventionOpen, setInterventionOpen] = useState(false);
	const [interventionDraft, setInterventionDraft] = useState("");
	const isSync = props.kind === "sync";
	const title = isSync ? props.session.title : "Async Intern";
	const status = isSync
		? SYNC_STATUS_LABEL[props.session.status]
		: ASYNC_PHASE_LABEL[props.intern.phase];
	const remoteId = isSync ? props.session.remoteId : props.intern.remoteId;
	const cursor = isSync ? props.session.cursor : props.intern.cursor;
	const messages = isSync ? props.session.messages : props.intern.messages;
	const activity = isSync ? props.session.activity : props.intern.activity;
	const leaveSafe = isSync ? false : props.intern.leaveSafe === true;
	const shortId = remoteId
		? remoteId.replace(/^smr\.(intern-sync-session|intern-async-runtime)\.v1\//, "")
		: null;
	const openArtifact =
		isSync && props.openArtifactId
			? (props.session.artifacts?.find((a) => a.id === props.openArtifactId) ?? null)
			: null;

	return (
		<div className="cloud-desk" data-testid="cloud-desk">
			<header className="desk-header">
				<div className="desk-heading">
					<button type="button" className="desk-back" onClick={props.onBack}>
						← Cloud
					</button>
					<div className="desk-title-row">
						<h1>{title}</h1>
						<span className={`status-chip status-${isSync ? props.session.status : `async-${props.intern.phase}`}`}>
							{status}
						</span>
					</div>
					<div className="desk-meta">
						{shortId ? <span className="desk-id">{shortId}</span> : null}
						{cursor != null ? <span>cursor {cursor}</span> : null}
						{!isSync && props.intern.cycle != null ? <span>cycle {props.intern.cycle}</span> : null}
					</div>
				</div>
				<div className="desk-actions">
					{isSync ? (
						<>
							<button type="button" onClick={() => props.onAction(props.session.status === "paused" ? "Resume" : "Pause")}>
								{props.session.status === "paused" ? "Resume" : "Pause"}
							</button>
							<button type="button" className="danger-text" onClick={() => props.onAction("Close")}>
								Close
							</button>
						</>
					) : (
						<>
							<button
								type="button"
								onClick={() =>
									props.onAction(
										props.intern.phase === "sleeping" || props.intern.phase === "waiting_for_input"
											? "Resume"
											: "Pause"
									)
								}
							>
								{props.intern.phase === "sleeping" || props.intern.phase === "waiting_for_input"
									? "Resume"
									: "Pause"}
							</button>
							<button type="button" onClick={() => props.onAction("Checkpoint")}>
								Checkpoint
							</button>
							<button type="button" className="danger-text" onClick={() => props.onAction("Cancel")}>
								Cancel
							</button>
						</>
					)}
				</div>
			</header>

			{leaveSafe ? (
				<div className="leave-safe-banner" data-testid="async-leave-safe" role="note">
					Leave-safe · closing the window does not pause this job
				</div>
			) : null}

			{!isSync && props.intern.needsInput ? (
				<div className="needs-input-banner" role="status">
					<strong>Needs input</strong>
					<span>{props.intern.summary}</span>
					{interventionOpen ? (
						<form
							className="intern-intervention"
							onSubmit={(event) => {
								event.preventDefault();
								const text = interventionDraft.trim();
								if (!text) return;
								if (props.onSendMessage) props.onSendMessage(text);
								else props.onAction(`Send: ${text.slice(0, 48)}`);
								setInterventionDraft("");
								setInterventionOpen(false);
							}}
						>
							<textarea
								data-testid="intern-intervention-input"
								aria-label="Operator intervention"
								value={interventionDraft}
								onChange={(event) => setInterventionDraft(event.target.value)}
								rows={3}
							/>
							<button type="submit" disabled={!interventionDraft.trim()}>
								Send response
							</button>
						</form>
					) : (
						<button type="button" onClick={() => setInterventionOpen(true)}>
							Respond
						</button>
					)}
				</div>
			) : null}

			<div className={`desk-split${openArtifact ? " with-visual" : ""}`}>
				<MessagePanel
					messages={messages}
					artifacts={isSync ? props.session.artifacts : undefined}
					openArtifactId={isSync ? props.openArtifactId : null}
					onOpenArtifact={isSync ? props.onOpenArtifact : undefined}
					onSend={(text) => {
						if (props.onSendMessage) {
							props.onSendMessage(text);
							return;
						}
						props.onAction(`Send: ${text.slice(0, 48)}`);
					}}
				/>
				{openArtifact && isSync ? (
					<VisualPane
						artifact={openArtifact}
						onClose={() => props.onOpenArtifact?.(null)}
					/>
				) : (
					<ActivityStream activity={activity} filter={filter} onFilterChange={setFilter} />
				)}
			</div>
		</div>
	);
}

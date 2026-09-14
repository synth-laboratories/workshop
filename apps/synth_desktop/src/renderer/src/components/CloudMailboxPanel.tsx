import { useCallback, useEffect, useState } from "react";
import type { MailboxConnectionView, MailboxStatusView } from "../generated/protocol";
import { cloudMailbox } from "../runtime/cloudMailbox";

function message(error: unknown): string {
	if (error && typeof error === "object" && "message" in error) return String((error as { message: unknown }).message);
	return String(error);
}

const OUTBOX_STATUS: Record<string, string> = {
	queued: "Queued on this device",
	unknown: "Outcome unknown — not resent",
	accepted: "Accepted",
	answered: "Answered",
	refused: "Refused",
	conflict: "Conflict",
	fenced: "Fenced"
};

/**
 * Shared-thread mailbox: connection and grant state, inbox requests that wait
 * for an operator, the outbox (including unresolved outcomes) and device
 * sign-out. Hidden entirely while the cloud host is qualification-gated.
 */
export function CloudMailboxPanel() {
	const [gated, setGated] = useState(true);
	const [connections, setConnections] = useState<MailboxConnectionView[]>([]);
	const [selected, setSelected] = useState<string | null>(null);
	const [status, setStatus] = useState<MailboxStatusView | null>(null);
	const [drafts, setDrafts] = useState<Record<string, string>>({});
	const [notice, setNotice] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);

	const refresh = useCallback(async (thread: string | null) => {
		try {
			const view = await cloudMailbox.view();
			if (view.availability === "qualification_required") {
				setGated(true);
				return;
			}
			setGated(false);
			const list = await cloudMailbox.connections();
			setConnections(list);
			const next = thread ?? list[0]?.threadId ?? null;
			setSelected(next);
			setStatus(next ? await cloudMailbox.status(next) : null);
		} catch (error) {
			setNotice(message(error));
		}
	}, []);

	useEffect(() => {
		void refresh(null);
	}, [refresh]);

	if (gated) return null;

	const act = async (work: () => Promise<unknown>, done: string) => {
		setBusy(true);
		try {
			await work();
			setNotice(done);
			await refresh(selected);
		} catch (error) {
			setNotice(message(error));
		} finally {
			setBusy(false);
		}
	};

	const connection = status?.connection ?? null;

	return (
		<section className="settings-finetunes cloud-mailbox" data-testid="cloud-mailbox">
			<header className="settings-section-head">
				<h3>Shared thread mailbox</h3>
				<button
					type="button"
					className="settings-secondary-btn"
					data-testid="cloud-mailbox-sign-out"
					disabled={busy}
					onClick={() => {
						if (!window.confirm("Sign this device out of shared threads? Queued messages will not be sent.")) return;
						void act(async () => {
							const result = await cloudMailbox.signOut();
							if (result.revocationError) setNotice(`Signed out locally; server revocation unconfirmed: ${result.revocationError}`);
						}, "Signed out of shared threads.");
					}}
				>
					Sign out device
				</button>
			</header>
			{notice ? <p role="status" className="finetune-meta">{notice}</p> : null}
			{connections.length === 0 ? (
				<p className="finetune-meta" data-testid="cloud-mailbox-empty">No local session is connected to a shared thread.</p>
			) : (
				<label className="finetune-meta">
					Thread{" "}
					<select value={selected ?? ""} onChange={event => void refresh(event.target.value)} data-testid="cloud-mailbox-thread">
						{connections.map(row => (
							<option key={row.threadId} value={row.threadId}>{row.threadId}</option>
						))}
					</select>
				</label>
			)}
			{connection ? (
				<p className="finetune-meta" data-testid="cloud-mailbox-connection">
					{connection.preset} · grant {connection.state}
					{connection.stateReason ? ` (${connection.stateReason})` : ""}
					{connection.grantOperations.length ? ` · ${connection.grantOperations.join(", ")}` : ""}
					{connection.grantExpiresAt ? ` · expires ${connection.grantExpiresAt}` : ""}
				</p>
			) : null}
			{status ? (
				<>
					<h4>Inbox</h4>
					{status.inbox.length === 0 ? <p className="finetune-meta">Nothing delivered yet.</p> : null}
					<ul data-testid="cloud-mailbox-inbox">
						{status.inbox.map(row => (
							<li key={row.messageId}>
								<span className="finetune-meta">{row.sender} · {row.kind} · {row.stage}</span>
								<p>{row.body}</p>
								{row.awaitingOperator ? (
									<div>
										<textarea
											aria-label="Reply"
											value={drafts[row.messageId] ?? ""}
											onChange={event => setDrafts({ ...drafts, [row.messageId]: event.target.value })}
										/>
										<button
											type="button"
											className="settings-secondary-btn"
											disabled={busy || !(drafts[row.messageId] ?? "").trim()}
											onClick={() => void act(() => cloudMailbox.answer(status.threadId, row.messageId, drafts[row.messageId] ?? ""), "Answer queued.")}
										>
											Answer
										</button>
										<button
											type="button"
											className="settings-secondary-btn"
											disabled={busy}
											onClick={() => void act(() => cloudMailbox.decline(status.threadId, row.messageId, (drafts[row.messageId] ?? "").trim() || "Declined by operator"), "Decline queued.")}
										>
											Decline
										</button>
									</div>
								) : null}
							</li>
						))}
					</ul>
					<h4>Outbox</h4>
					{status.unknownOutcomes > 0 ? (
						<p className="finetune-meta" data-testid="cloud-mailbox-unknown">
							{status.unknownOutcomes} message(s) have an unknown outcome. They are never resent automatically.
						</p>
					) : null}
					<ul data-testid="cloud-mailbox-outbox">
						{status.outbox.map(row => (
							<li key={row.commandId}>
								<span className="finetune-meta">{row.disposition} · {OUTBOX_STATUS[row.status] ?? row.status}{row.fencedReason ? ` (${row.fencedReason})` : ""}</span>
							</li>
						))}
					</ul>
				</>
			) : null}
		</section>
	);
}

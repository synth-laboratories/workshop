import { useEffect, useMemo, useState } from "react";
import type { Session } from "@synth/runtime-protocol";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";
import { responseTraceStore } from "../runtime/responseTraceStore";
import { ResponsesTracePanel } from "./ResponsesTracePanel";

function isWorkshopCodexSession(session: Session): boolean {
	return session.metadata?.runtime === "codex-app-server";
}

function isProviderEvent(event: { source?: string; kind?: string }): boolean {
	if (event.source !== "codex") return false;
	const kind = event.kind ?? "";
	return !(kind.startsWith("approval.")
		|| kind.startsWith("session.")
		|| kind.startsWith("run.")
		|| kind === "session/unhealthy"
		|| kind === "app-server/stderr"
		|| kind === "message.created");
}

export function CodexTracesPanel({ sessions, activeSessionId }: { sessions: Session[]; activeSessionId?: string | null }) {
	const codexSessions = useMemo(
		() => sessions.filter(isWorkshopCodexSession).sort((a, b) => Date.parse(b.updatedAt) - Date.parse(a.updatedAt)),
		[sessions]
	);
	const [selectedId, setSelectedId] = useState("");
	const [error, setError] = useState<string | null>(null);
	const effectiveId = codexSessions.some((session) => session.id === selectedId)
		? selectedId
		: codexSessions.some((session) => session.id === activeSessionId)
			? activeSessionId!
			: codexSessions[0]?.id ?? "";
	const selected = codexSessions.find((session) => session.id === effectiveId) ?? null;

	useEffect(() => {
		if (!effectiveId || !bridges.core) return;
		let disposed = false;
		setError(null);
		responseTraceStore.setLoading(effectiveId);
		void bridges.core.sessionEventsTail(effectiveId, 250)
			.then((events) => {
				if (!disposed) responseTraceStore.setJournal(effectiveId, events.filter(isProviderEvent));
			})
			.catch((reason) => {
				if (disposed) return;
				const message = publicError(reason);
				setError(message);
				responseTraceStore.setError(effectiveId, message);
			});
		return () => { disposed = true; };
	}, [effectiveId]);

	return <div className="codex-traces-panel ws-stack" data-testid="codex-traces-panel">
		<div className="codex-traces-head">
			<div>
				<h2>Workshop agent traces</h2>
				<p>Raw provider events from the Codex app-server sessions powering this Workshop.</p>
			</div>
			{codexSessions.length ? <label className="ws-field codex-session-picker">
				<span>Conversation</span>
				<select className="ws-select" value={effectiveId} onChange={(event) => setSelectedId(event.target.value)} data-testid="codex-trace-session">
					{codexSessions.map((session) => <option key={session.id} value={session.id}>{session.title}</option>)}
				</select>
			</label> : null}
		</div>
		{error ? <div className="ws-note ws-note-danger" role="alert">{error}</div> : null}
		{selected ? <>
			<div className="codex-trace-meta"><strong>{selected.title}</strong><span>{selected.status} · updated {new Date(selected.updatedAt).toLocaleString()}</span></div>
			<ResponsesTracePanel sessionId={selected.id} running={selected.status === "running"} />
		</> : <div className="ws-empty"><p>No Workshop Codex app-server sessions yet.</p></div>}
	</div>;
}

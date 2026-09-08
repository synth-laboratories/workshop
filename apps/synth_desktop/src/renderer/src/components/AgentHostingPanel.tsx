import { useEffect, useState } from "react";
import { commands, type AppEvent } from "../generated/protocol";
import { SettingsCard, SettingsRow } from "./SettingsCard";
import { publicError } from "../runtime/publicError";

type Session = { sessionId: string; title: string; backendId: string; status: string; attached: boolean };
function object(value: unknown): Record<string, unknown> {
	return value && typeof value === "object" ? value as Record<string, unknown> : {};
}
async function unwrap<T>(result: Promise<{ status: "ok"; data: T } | { status: "error"; error: unknown }>): Promise<T> {
	const value = await result;
	if (value.status === "error") throw value.error;
	return value.data;
}

/** ACP transport state is journaled by the host. This panel only renders it. */
export function AgentHostingPanel() {
	const [backends, setBackends] = useState<string[]>([]);
	const [backend, setBackend] = useState("");
	const [sessions, setSessions] = useState<Session[]>([]);
	const [selected, setSelected] = useState("");
	const [events, setEvents] = useState<AppEvent[]>([]);
	const [prompt, setPrompt] = useState("");
	const [error, setError] = useState("");
	const [busy, setBusy] = useState(false);
	useEffect(() => {
		let stopped = false;
		let timer: ReturnType<typeof setTimeout>;
		async function refresh() {
			try {
				const [configured, listing, history] = await Promise.all([
					unwrap(commands.agentBackendsList()), unwrap(commands.agentSessionsList()),
					selected ? unwrap(commands.coreSessionEventsTail(selected, 500)) : Promise.resolve([])
				]);
				if (stopped) return;
				setBackends(configured.map(item => item.id));
				const rows = object(listing).sessions;
				setSessions(Array.isArray(rows) ? rows.filter((item): item is Session => typeof object(item).sessionId === "string") : []);
				setEvents(history);
			} catch (reason) { if (!stopped) setError(publicError(reason)); }
			finally { if (!stopped) timer = setTimeout(() => void refresh(), 1500); }
		}
		void refresh();
		return () => { stopped = true; clearTimeout(timer); };
	}, [selected]);
	async function act(action: () => Promise<unknown>) {
		setBusy(true); setError("");
		try { await action(); } catch (reason) { setError(publicError(reason)); }
		finally { setBusy(false); }
	}
	const session = sessions.find(item => item.sessionId === selected);
	const settled = new Set(events.filter(event => ["approval.granted", "approval.rejected", "approval.expired"].includes(event.kind)).map(event => object(event.payload).approvalId));
	const approvals = events.filter(event => event.kind === "approval.requested" && !settled.has(object(event.payload).approvalId));
	return <SettingsCard title="Hosted agents" description="Run a locally configured ACP agent with access to this Workshop instance." testId="agent-hosting-panel">
		<p className="finetune-meta">Configure agent-backends.json in this instance’s data directory using the repository README. Backend workspaces and time limits are checked before launch.</p>
		<SettingsRow label="Agent" htmlFor="hosted-agent-backend">
			<select id="hosted-agent-backend" value={backend} onChange={event => setBackend(event.target.value)}>
				<option value="">Choose a configured agent</option>{backends.map(id => <option key={id}>{id}</option>)}
			</select>
			<button type="button" disabled={busy || !backend} onClick={() => void act(async () => {
				const result = object(await unwrap(commands.agentSessionStart({ backendId: backend, title: `${backend} task`, parentSessionId: null })));
				if (typeof result.sessionId === "string") setSelected(result.sessionId);
			})}>Start task</button>
		</SettingsRow>
		<SettingsRow label="Task" htmlFor="hosted-agent-task">
			<select id="hosted-agent-task" value={selected} onChange={event => setSelected(event.target.value)}>
				<option value="">Choose a task</option>{sessions.map(item => <option key={item.sessionId} value={item.sessionId}>{item.title} · {item.status}{item.attached ? "" : " · detached"}</option>)}
			</select>
			<button type="button" disabled={busy || !selected || session?.attached} onClick={() => void act(() => unwrap(commands.agentSessionResume(selected)))}>Resume</button>
			<button type="button" disabled={busy || !session?.attached} onClick={() => void act(() => unwrap(commands.agentSessionCancel(selected)))}>Cancel turn</button>
			<button type="button" disabled={busy || !selected || session?.status === "closed"} onClick={() => void act(() => unwrap(commands.agentSessionClose(selected)))}>Close task</button>
		</SettingsRow>
		{selected ? <>
			<SettingsRow label="Prompt" htmlFor="hosted-agent-prompt">
				<textarea id="hosted-agent-prompt" value={prompt} onChange={event => setPrompt(event.target.value)} />
				<button type="button" disabled={busy || !session?.attached || session.status === "running" || !prompt.trim()} onClick={() => void act(async () => { await unwrap(commands.agentSessionSend(selected, prompt)); setPrompt(""); })}>Send</button>
			</SettingsRow>
			{approvals.map(event => {
				const payload = object(event.payload);
				return <SettingsRow key={event.eventId} label="Agent requests permission" description={String(payload.detail ?? "Review this request before allowing it.")}>
					{["once", "reject"].map(decision => <button type="button" key={decision} disabled={busy} onClick={() => void act(() => unwrap(commands.codexApprovalResolve({ sessionId: selected, approvalId: String(payload.approvalId), decision })))}>{decision === "once" ? "Allow once" : "Reject"}</button>)}
				</SettingsRow>;
			})}
			<details><summary>Task history (latest 500 events)</summary><pre className="agent-hosting-history">{events.map(event => {
				const payload = object(event.payload);
				const content = object(payload.content);
				return `${event.kind}: ${typeof payload.text === "string" ? payload.text : typeof content.text === "string" ? content.text : JSON.stringify(payload)}`;
			}).join("\n")}</pre></details>
		</> : null}
		{error ? <p role="alert">{error}</p> : null}
	</SettingsCard>;
}

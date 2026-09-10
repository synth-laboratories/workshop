import {useEffect,useState,type ReactNode} from "react";
import {useVisualState,useVisualSessionSnapshot,usePublishVisualScene} from "@synth/visuals-react";
export type WorkshopSubagent={id:string;title:string;summary?:string|null;status:"starting"|"working"|"completed"|"interrupted"|"failed"|"stopped"|"unavailable";startedAt:string;updatedAt:string};

function elapsedLabel(value: string, now: number): string {
	const seconds = Math.max(0, Math.floor((now - Date.parse(value)) / 1000));
	if (seconds < 60) return `${seconds}s`;
	if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
	return `${Math.floor(seconds / 3600)}h`;
}

function subagentStatusLabel(status: WorkshopSubagent["status"]): string {
	return ({
		starting: "Starting",
		working: "Working",
		completed: "Completed",
		interrupted: "Interrupted",
		failed: "Failed",
		stopped: "Stopped",
		unavailable: "Unavailable"
	})[status];
}

function subagentMarker(id: string): string {
	let value = 0;
	for (let index = 0; index < id.length; index += 1) value = (value + id.charCodeAt(index)) % 2;
	return value ? "✣" : "✺";
}

export function WorkshopSubagents({agents,sessionId,readThread,formatError,renderConversation}:{agents:WorkshopSubagent[];sessionId?:string;renderConversation?:(agent:WorkshopSubagent)=>ReactNode;readThread?:(sessionId:string,agentId:string,includeTurns:boolean)=>Promise<unknown>;formatError:(error:unknown)=>string}) {
	const [now, setNow] = useState(Date.now());
	const [selectedId, setSelectedId] = useVisualState<string | null>("subagents.selected",null);
	const [detail, setDetail] = useState<unknown>(null);
	const [detailError, setDetailError] = useState<string | null>(null);
	useEffect(() => {
		const timer = window.setInterval(() => setNow(Date.now()), 1_000);
		return () => window.clearInterval(timer);
	}, []);
	useEffect(() => {
		if (!selectedId || !sessionId || !readThread) {
			setDetail(null);
			setDetailError(null);
			return;
		}
		let cancelled = false;
		void readThread(sessionId, selectedId, true).then(
			(payload) => {
				if (!cancelled) {
					setDetail(payload);
					setDetailError(null);
				}
			},
			(reason) => {
				if (!cancelled) {
					setDetail(null);
					setDetailError(formatError(reason));
				}
			}
		);
		return () => {
			cancelled = true;
		};
	}, [selectedId, sessionId,readThread,formatError]);
	const groups = [
		{ label: "Working", agents: agents.filter((agent) => agent.status === "starting" || agent.status === "working") },
		{ label: "Needs attention", agents: agents.filter((agent) => agent.status === "interrupted" || agent.status === "failed" || agent.status === "stopped" || agent.status === "unavailable") },
		{ label: "Completed", agents: agents.filter((agent) => agent.status === "completed") }
	];
	const selected = agents.find((agent) => agent.id === selectedId) ?? null;
	const working = groups[0].agents.length;
	const attention = groups[1].agents.length;
	const completed = groups[2].agents.length;
  const shared=useVisualSessionSnapshot();
  usePublishVisualScene(shared?.ready?{visualId:shared.state.visualId,revision:shared.state.revision,stateVersion:shared.state.stateVersion,clocks:{},selection:{members:selected?[{kind:"agent",id:selected.id}]:[]},landmarks:agents.map(agent=>({ref:{kind:"agent",id:agent.id},role:"button",label:`${agent.title}: ${agent.status}`,actions:["presentation.set"]})),truth:{working:{state:"observed",value:working},attention:{state:"observed",value:attention},completed:{state:"observed",value:completed}},diagnostics:[]}:undefined);
	if (selected) {
		return (
			<div className="subagents-visual" data-testid="visual-subagents">
				<button type="button" className="subagents-back" data-testid="subagents-back" onClick={() => setSelectedId(null)}>
					← {selected.title}
				</button>
				<p className="subagents-workspace-summary" data-testid="subagents-workspace-summary">
					{subagentStatusLabel(selected.status)} · {selected.status === "starting" || selected.status === "working" ? elapsedLabel(selected.startedAt, now) : elapsedLabel(selected.updatedAt, now)}
				</p>
				<div className="subagents-detail" data-testid="subagents-detail">
					{renderConversation ? renderConversation(selected) : selected.summary ? <p>{selected.summary}</p> : <p>No result yet</p>}
					{detailError ? <p className="subagents-empty">{detailError}</p> : null}
					{detail ? <details><summary>Retained thread details</summary><pre>{JSON.stringify(detail, null, 2)}</pre></details> : null}
				</div>
			</div>
		);
	}
	return (
		<div className="subagents-visual" data-testid="visual-subagents">
			<p className="subagents-workspace-summary" data-testid="subagents-workspace-summary">
				{working} working · {attention} need attention · {completed} completed
			</p>
			{groups.map((group) => (
				<section key={group.label} className="subagents-group">
					<h3>{group.label} · {group.agents.length}</h3>
					{group.agents.length === 0 ? <p className="subagents-empty">No {group.label.toLowerCase()} subagents</p> : null}
					{group.agents.map((agent) => (
						<button
							type="button"
							className="subagent-row"
							key={agent.id}
							data-status={agent.status}
							data-testid={`subagent-row-${agent.id}`}
							onClick={() => setSelectedId(agent.id)}
						>
							<span className={`subagent-mark mark-${agent.status}`} aria-hidden>{subagentMarker(agent.id)}</span>
							<div className="subagent-copy">
								<div className="subagent-title-row"><strong>{agent.title}</strong><span className={`subagent-state state-${agent.status}`}>{subagentStatusLabel(agent.status)}</span></div>
								{agent.summary ? <p>{agent.summary}</p> : null}
							</div>
							<time dateTime={agent.updatedAt}>{agent.status === "starting" || agent.status === "working" ? elapsedLabel(agent.startedAt, now) : elapsedLabel(agent.updatedAt, now) + " ago"}</time>
						</button>
					))}
				</section>
			))}
		</div>
	);
}

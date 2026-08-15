import type { CodexEvent } from "../bridge";

export type ReceivedResponseEvent = CodexEvent & { receivedAt: string };
export type ResponseTraceLoadState = { state: "loading" | "loaded" | "error"; message?: string };

export function ResponsesTracePanel({ events, running, loadState }: { events: ReceivedResponseEvent[]; running: boolean; loadState?: ResponseTraceLoadState }) {
	return <section className="responses-trace" aria-label="Raw Responses API trace" data-testid="responses-trace">
		<header><div><strong>Responses API v5</strong><span>{running ? "Live stream" : "Latest receipt"}</span></div><span>{events.length} events</span></header>
		{events.length === 0 ? <p className="responses-trace-empty">{loadState?.state === "loading"
			? "Loading recorded events…"
			: loadState?.state === "error"
				? `Trace unavailable: ${loadState.message ?? "Unknown error"}`
				: "No provider events recorded for this conversation."}</p> : <ol>
			{events.map((event, index) => <li key={`${event.sessionId}-${event.method}-${event.receivedAt}-${index}`}>
				<div><code>{event.method}</code><time dateTime={event.receivedAt}>received {new Date(event.receivedAt).toLocaleTimeString()}</time></div>
				<pre>{JSON.stringify(event.params, null, 2)}</pre>
			</li>)}
		</ol>}
	</section>;
}

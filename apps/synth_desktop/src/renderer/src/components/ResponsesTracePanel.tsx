import { useEffect, useState } from "react";
import type { CodexEvent } from "../bridge";

export type ReceivedResponseEvent = CodexEvent & { receivedAt: string };
export type ResponseTraceLoadState = { state: "loading" | "loaded" | "error"; message?: string };

const INITIAL_VISIBLE_EVENTS = 50;

function ResponseTraceEvent({ event, index }: { event: ReceivedResponseEvent; index: number }) {
	const [expanded, setExpanded] = useState(false);
	return <li>
		<details open={expanded} onToggle={(event) => setExpanded(event.currentTarget.open)}>
			<summary>
				<code>{event.method}</code>
				<time dateTime={event.receivedAt}>received {new Date(event.receivedAt).toLocaleTimeString()}</time>
				<span className="sr-only">Event {index + 1}</span>
			</summary>
			{expanded ? <pre>{JSON.stringify(event.params, null, 2)}</pre> : null}
		</details>
	</li>;
}

export function ResponsesTracePanel({ events, running, loadState }: { events: ReceivedResponseEvent[]; running: boolean; loadState?: ResponseTraceLoadState }) {
	const sessionId = events.at(-1)?.sessionId ?? "empty";
	const [visibleCount, setVisibleCount] = useState(INITIAL_VISIBLE_EVENTS);
	useEffect(() => setVisibleCount(INITIAL_VISIBLE_EVENTS), [sessionId]);
	const visibleEvents = events.slice(-visibleCount);
	const hiddenCount = events.length - visibleEvents.length;
	return <section className="responses-trace" aria-label="Raw Responses API trace" data-testid="responses-trace">
		<header><div><strong>Responses API v5</strong><span>{running ? "Live stream" : "Latest receipt"}</span></div><span>{events.length} events</span></header>
		{events.length === 0 ? <p className="responses-trace-empty">{loadState?.state === "loading"
			? "Loading recorded events…"
			: loadState?.state === "error"
				? `Trace unavailable: ${loadState.message ?? "Unknown error"}`
				: "No provider events recorded for this conversation."}</p> : <>
			{hiddenCount > 0 ? <button type="button" className="responses-trace-earlier" onClick={() => setVisibleCount((count) => Math.min(events.length, count + INITIAL_VISIBLE_EVENTS))}>
				Show {Math.min(INITIAL_VISIBLE_EVENTS, hiddenCount)} earlier events
			</button> : null}
			<ol>
				{visibleEvents.map((event, index) => <ResponseTraceEvent key={`${event.sessionId}-${event.method}-${event.receivedAt}-${events.length - visibleEvents.length + index}`} event={event} index={events.length - visibleEvents.length + index} />)}
			</ol>
		</>}
	</section>;
}

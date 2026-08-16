import { useEffect, useMemo, useRef, useState } from "react";
import { useResponseTrace, type ReceivedResponseEvent } from "../runtime/responseTraceStore";

const ROW_HEIGHT = 42;
const VIEWPORT_HEIGHT = 360;
const OVERSCAN = 4;

function eventKey(event: ReceivedResponseEvent, index: number): string {
	return `${event.sessionId}-${event.method}-${event.receivedAt}-${index}`;
}

export function ResponsesTracePanel({ sessionId, running }: { sessionId: string; running: boolean }) {
	const { events, loadState } = useResponseTrace(sessionId);
	const viewportRef = useRef<HTMLDivElement>(null);
	const followTailRef = useRef(true);
	const [scrollTop, setScrollTop] = useState(0);
	const [selected, setSelected] = useState<ReceivedResponseEvent | null>(null);
	useEffect(() => {
		setSelected(null);
		followTailRef.current = true;
	}, [sessionId]);
	useEffect(() => {
		if (!followTailRef.current) return;
		const next = Math.max(0, events.length * ROW_HEIGHT - VIEWPORT_HEIGHT);
		setScrollTop(next);
		if (viewportRef.current) viewportRef.current.scrollTop = next;
	}, [events.length, sessionId]);
	const windowed = useMemo(() => {
		const first = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
		const count = Math.ceil(VIEWPORT_HEIGHT / ROW_HEIGHT) + OVERSCAN * 2;
		return { first, events: events.slice(first, first + count) };
	}, [events, scrollTop]);

	return <section className="responses-trace" aria-label="Raw Responses API trace" data-testid="responses-trace">
		<header><div><strong>Responses API v5</strong><span>{running ? "Receiving events" : "Latest receipt"}</span></div><span>{events.length} events</span></header>
		{events.length === 0 ? <p className="responses-trace-empty">{loadState.state === "loading"
			? "Loading recorded events…"
			: loadState.state === "error"
				? `Trace unavailable: ${loadState.message ?? "Unknown error"}`
				: "No provider events recorded for this conversation."}</p> : <div className="responses-trace-workspace">
			<div
				className="responses-trace-viewport"
				ref={viewportRef}
				role="region"
				aria-label={`${events.length} recorded provider events`}
				onScroll={(event) => {
					const viewport = event.currentTarget;
					followTailRef.current = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight <= ROW_HEIGHT;
					setScrollTop(viewport.scrollTop);
				}}
			>
				<div className="responses-trace-spacer" style={{ height: events.length * ROW_HEIGHT }}>
					{windowed.events.map((event, offset) => {
						const index = windowed.first + offset;
						return <button
							type="button"
							aria-pressed={selected === event}
							aria-label={`${event.method}, event ${index + 1} of ${events.length}, received ${new Date(event.receivedAt).toLocaleTimeString()}`}
							className="responses-trace-row"
							key={eventKey(event, index)}
							style={{ height: ROW_HEIGHT, transform: `translateY(${index * ROW_HEIGHT}px)` }}
							onClick={() => setSelected(event)}
						>
							<code>{event.method}</code>
							<time dateTime={event.receivedAt}>{new Date(event.receivedAt).toLocaleTimeString()}</time>
						</button>;
					})}
				</div>
			</div>
			<aside className="responses-trace-inspector" aria-label="Selected event payload">
				{selected ? <>
					<div><code>{selected.method}</code><time dateTime={selected.receivedAt}>received {new Date(selected.receivedAt).toLocaleTimeString()}</time></div>
					<pre>{JSON.stringify(selected.params, null, 2)}</pre>
				</> : <p>Select an event to inspect its payload.</p>}
			</aside>
		</div>}
	</section>;
}

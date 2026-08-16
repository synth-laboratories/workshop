import { useCallback, useSyncExternalStore } from "react";
import type { AppEvent } from "@synth/runtime-protocol";
import type { CodexEvent } from "../bridge";

export type ReceivedResponseEvent = CodexEvent & { receivedAt: string };
export type ResponseTraceLoadState = { state: "loading" | "loaded" | "error"; message?: string };
export type ResponseTraceSnapshot = {
	events: readonly ReceivedResponseEvent[];
	loadState: ResponseTraceLoadState;
};

type TraceSession = {
	journal: AppEvent[];
	live: ReceivedResponseEvent[];
	loadState: ResponseTraceLoadState;
	listeners: Set<() => void>;
	snapshot: ResponseTraceSnapshot | null;
	notifyFrame: number | null;
};

const TRACE_LIMIT = 250;
const EMPTY_SNAPSHOT: ResponseTraceSnapshot = Object.freeze({
	events: Object.freeze([]),
	loadState: Object.freeze({ state: "loaded" })
});
const sessions = new Map<string, TraceSession>();

function traceSession(sessionId: string): TraceSession {
	let session = sessions.get(sessionId);
	if (!session) {
			session = {
				journal: [],
				live: [],
				loadState: { state: "loading" },
			listeners: new Set(),
			snapshot: null,
			notifyFrame: null
		};
		sessions.set(sessionId, session);
	}
	return session;
}

function journalEvent(sessionId: string, event: AppEvent): ReceivedResponseEvent {
	const payload = event.payload;
	return {
		sessionId,
		method: event.kind,
		params: payload && typeof payload === "object" && !Array.isArray(payload)
			? payload as Record<string, unknown>
			: { payload },
		receivedAt: event.createdAt
	};
}

function notify(session: TraceSession): void {
	session.snapshot = null;
	if (session.listeners.size === 0 || session.notifyFrame !== null) return;
	session.notifyFrame = window.requestAnimationFrame(() => {
		session.notifyFrame = null;
		for (const listener of session.listeners) listener();
	});
}

function snapshot(sessionId: string): ResponseTraceSnapshot {
	if (!sessionId) return EMPTY_SNAPSHOT;
	const session = traceSession(sessionId);
	if (session.snapshot) return session.snapshot;
	// Materialization is deliberately subscription-driven. Closed Advanced panes
	// retain raw event references without mapping payloads or touching React.
	const journal = session.journal.map((event) => journalEvent(sessionId, event));
	const events = [...journal, ...session.live]
		.sort((left, right) => Date.parse(left.receivedAt) - Date.parse(right.receivedAt))
		.slice(-TRACE_LIMIT);
	session.snapshot = { events, loadState: session.loadState };
	return session.snapshot;
}

export const responseTraceStore = {
	setLoading(sessionId: string): void {
		const session = traceSession(sessionId);
		session.loadState = { state: "loading" };
		notify(session);
	},
	setJournal(sessionId: string, rows: AppEvent[]): void {
		const session = traceSession(sessionId);
		session.journal = rows.slice(-TRACE_LIMIT);
		session.loadState = { state: "loaded" };
		notify(session);
	},
	setError(sessionId: string, message: string): void {
		const session = traceSession(sessionId);
		session.loadState = { state: "error", message };
		notify(session);
	},
	markLoaded(sessionId: string): void {
		const session = traceSession(sessionId);
		session.loadState = { state: "loaded" };
		notify(session);
	},
	appendLive(event: CodexEvent): void {
		const session = traceSession(event.sessionId);
		session.live.push({ ...event, receivedAt: new Date().toISOString() });
		if (session.live.length > TRACE_LIMIT) session.live.splice(0, session.live.length - TRACE_LIMIT);
		notify(session);
	},
	evict(sessionId: string): void {
		const session = sessions.get(sessionId);
		if (session && session.notifyFrame !== null) window.cancelAnimationFrame(session.notifyFrame);
		sessions.delete(sessionId);
	},
	subscribe(sessionId: string, listener: () => void): () => void {
		const session = traceSession(sessionId);
		session.listeners.add(listener);
		return () => session.listeners.delete(listener);
	},
	getSnapshot(sessionId: string): ResponseTraceSnapshot {
		return snapshot(sessionId);
	}
};

export function useResponseTrace(sessionId: string): ResponseTraceSnapshot {
	const subscribe = useCallback((listener: () => void) => responseTraceStore.subscribe(sessionId, listener), [sessionId]);
	const getSnapshot = useCallback(() => responseTraceStore.getSnapshot(sessionId), [sessionId]);
	return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

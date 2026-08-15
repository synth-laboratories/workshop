/**
 * Observable Session store (preferences/store.ts patterns + useSyncExternalStore).
 *
 * Session.status has one writer: applyRuntimeEvent (and the sibling helpers in
 * that module). This store owns the mutable snapshot and notifies subscribers.
 */

import { useSyncExternalStore } from "react";
import type {
	ExecutionTarget,
	RuntimeEvent,
	Session,
	SessionStatus
} from "@synth/runtime-protocol";
import {
	applyLocalSessionStatus,
	applyRuntimeEvent,
	applyTurnAccepted,
	mergeReplayedRuntimeEvents,
	selectSessionRunning,
	selectWorkingChatIds,
	type ApplyRuntimeEventOptions,
	type SessionStoreState
} from "./applyRuntimeEvent";

export type { SessionStoreState, ApplyRuntimeEventOptions };
export {
	applyRuntimeEvent,
	applyLocalSessionStatus,
	applyTurnAccepted,
	appendRuntimeEvent,
	mergeReplayedRuntimeEvents,
	selectSessionRunning,
	selectWorkingChatIds,
	statusFromRuntimeEvent
} from "./applyRuntimeEvent";

type Listener = () => void;

const EMPTY_EVENTS: RuntimeEvent[] = [];

let cached: SessionStoreState = {
	sessions: [],
	eventsBySession: {}
};
const listeners = new Set<Listener>();

function commit(next: SessionStoreState): SessionStoreState {
	if (next === cached) return cached;
	cached = next;
	for (const listener of listeners) listener();
	return cached;
}

export function subscribeSessionStore(listener: Listener): () => void {
	listeners.add(listener);
	return () => {
		listeners.delete(listener);
	};
}

export function getSessionStoreSnapshot(): SessionStoreState {
	return cached;
}

export function getSessions(): Session[] {
	return cached.sessions;
}

export function getEventsBySession(): Record<string, RuntimeEvent[]> {
	return cached.eventsBySession;
}

export function resetSessionStore(
	next: SessionStoreState = { sessions: [], eventsBySession: {} }
): SessionStoreState {
	return commit(next);
}

export function replaceSessions(sessions: Session[]): SessionStoreState {
	return commit({ ...cached, sessions });
}

export function mergeInternSessions(internSessions: Session[]): SessionStoreState {
	return commit({
		...cached,
		sessions: [
			...cached.sessions.filter((session) => session.target.kind !== "intern"),
			...internSessions
		]
	});
}

export function upsertSession(session: Session): SessionStoreState {
	const without = cached.sessions.filter((item) => item.id !== session.id);
	return commit({ ...cached, sessions: [session, ...without] });
}

export function patchSessionMetadata(
	sessionId: string,
	metadata: Record<string, unknown>
): SessionStoreState {
	const sessions = cached.sessions.map((session) =>
		session.id === sessionId
			? { ...session, metadata: { ...session.metadata, ...metadata } }
			: session
	);
	return commit({ ...cached, sessions });
}

/** THE event-driven Session.status writer. */
export function dispatchRuntimeEvent(
	event: RuntimeEvent,
	options: ApplyRuntimeEventOptions = {}
): SessionStoreState {
	return commit(applyRuntimeEvent(cached, event, options));
}

export function dispatchLocalSessionStatus(
	sessionId: string,
	status: SessionStatus,
	options: { onlyIf?: SessionStatus; updatedAt?: string } = {}
): SessionStoreState {
	return commit(applyLocalSessionStatus(cached, sessionId, status, options));
}

export function dispatchTurnAccepted(
	sessionId: string,
	patch: { target: ExecutionTarget; turnId?: string | null; updatedAt?: string }
): SessionStoreState {
	return commit(applyTurnAccepted(cached, sessionId, patch));
}

export function replaceSessionEvents(
	sessionId: string,
	events: RuntimeEvent[]
): SessionStoreState {
	return commit({
		...cached,
		eventsBySession: { ...cached.eventsBySession, [sessionId]: events }
	});
}

export function evictSessionEvents(sessionIds: Iterable<string>): SessionStoreState {
	const evicted = new Set(sessionIds);
	if (evicted.size === 0 || !Object.keys(cached.eventsBySession).some((id) => evicted.has(id))) return cached;
	const eventsBySession = Object.fromEntries(
		Object.entries(cached.eventsBySession).filter(([id]) => !evicted.has(id))
	);
	return commit({ ...cached, eventsBySession });
}

export function mergeSessionReplay(
	replay: ReadonlyArray<readonly [string, RuntimeEvent[]]>
): SessionStoreState {
	const eventsBySession = { ...cached.eventsBySession };
	for (const [sessionId, events] of replay) {
		eventsBySession[sessionId] = mergeReplayedRuntimeEvents(
			eventsBySession[sessionId] ?? [],
			events
		);
	}
	return commit({ ...cached, eventsBySession });
}

export function useSessionStore<T>(selector: (state: SessionStoreState) => T): T {
	return useSyncExternalStore(
		subscribeSessionStore,
		() => selector(getSessionStoreSnapshot()),
		() => selector(getSessionStoreSnapshot())
	);
}

export function useSessions(): Session[] {
	return useSessionStore((state) => state.sessions);
}

export function useEventsBySession(): Record<string, RuntimeEvent[]> {
	return useSessionStore((state) => state.eventsBySession);
}

export function useSessionRunning(sessionId: string | null | undefined): boolean {
	return useSessionStore((state) => {
		if (!sessionId) return false;
		return selectSessionRunning(
			state.sessions.find((session) => session.id === sessionId),
			state.eventsBySession[sessionId] ?? EMPTY_EVENTS
		);
	});
}

export function useWorkingChatIds(): Set<string> {
	return useSessionStore((state) => selectWorkingChatIds(state.sessions));
}

/**
 * Subscribe to one session's event list. Token events for other sessions do
 * not change this selector's return value (same array reference stays cached
 * in the store), so transcript views can rememoize per session.
 */
export function useSessionEvents(sessionId: string | null | undefined): RuntimeEvent[] {
	return useSessionStore((state) => {
		if (!sessionId) return EMPTY_EVENTS;
		return state.eventsBySession[sessionId] ?? EMPTY_EVENTS;
	});
}

export function useSession(sessionId: string | null | undefined): Session | null {
	return useSessionStore((state) => {
		if (!sessionId) return null;
		return state.sessions.find((session) => session.id === sessionId) ?? null;
	});
}

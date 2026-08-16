/**
 * Single writer for Session.status on the renderer side.
 *
 * Runtime events (and the local status/turn helpers in this module) are the only
 * paths that may change `session.status`. App.tsx and other UI code must go
 * through these functions — never patch status inline.
 */

import type {
	ExecutionTarget,
	RuntimeEvent,
	Session,
	SessionStatus
} from "@synth/runtime-protocol";

export type SessionStoreState = {
	sessions: Session[];
	eventsBySession: Record<string, RuntimeEvent[]>;
};

export type ApplyRuntimeEventOptions = {
	/** When true, a fenced run.started must not resurrect Working. */
	fenced?: boolean;
	/** Optional thread title from thread/name/updated. */
	title?: string | null;
	/**
	 * When false, append the event but leave Session.status untouched.
	 * Used when a local status write already ran (failTurnStart onlyIf: running).
	 */
	updateStatus?: boolean;
};

const TERMINAL_RUN_KINDS = new Set([
	"run.completed",
	"run.failed",
	"run.cancelled"
]);

function runIdentity(event: RuntimeEvent): string | undefined {
	const payload = event.payload ?? {};
	for (const value of [payload.runId, payload.turnId, payload.run_id, payload.turn_id]) {
		if (typeof value === "string" && value) return value;
	}
	for (const nested of [payload.turn, payload.run]) {
		if (!nested || typeof nested !== "object") continue;
		const id = (nested as Record<string, unknown>).id;
		if (typeof id === "string" && id) return id;
	}
	return undefined;
}

/**
 * `sendTurn` resolves only after Rust has persisted the run. A fast provider
 * can publish its terminal while that promise is still pending, so the later
 * acceptance callback must not resurrect Working. Exact turn identity is the
 * primary fence; old envelopes without an id are fenced only when their
 * terminal follows the latest operator message.
 */
function acceptedTurnAlreadyTerminal(
	state: SessionStoreState,
	sessionId: string,
	turnId: string
): boolean {
	const events = state.eventsBySession[sessionId] ?? [];
	const latestUserSequence = [...events]
		.reverse()
		.find((event) =>
			event.eventKind === "message.created" && event.payload?.role === "user"
		)?.sequence;
	return events.some((event) => {
		if (!TERMINAL_RUN_KINDS.has(event.eventKind)) return false;
		const terminalTurnId = runIdentity(event);
		if (terminalTurnId) return terminalTurnId === turnId;
		return latestUserSequence !== undefined && event.sequence > latestUserSequence;
	});
}

function payloadId(value: RuntimeEvent): string {
	const payload = value.payload ?? {};
	return typeof payload.messageId === "string"
		? payload.messageId
		: typeof payload.eventId === "string"
			? payload.eventId
			: typeof payload.id === "string"
				? payload.id
				: "";
}

/** Deduping append used by the reducer and replay merge. */
export function appendRuntimeEvent(
	events: RuntimeEvent[],
	event: RuntimeEvent
): RuntimeEvent[] {
	if (
		events.some(
			(candidate) =>
				candidate.sequence === event.sequence &&
				candidate.eventKind === event.eventKind &&
				candidate.source === event.source &&
				payloadId(candidate) === payloadId(event)
		)
	) {
		return events;
	}
	return [...events, event].sort((left, right) => left.sequence - right.sequence);
}

export function mergeReplayedRuntimeEvents(
	current: RuntimeEvent[],
	replayed: RuntimeEvent[]
): RuntimeEvent[] {
	return [...replayed, ...current].reduce<RuntimeEvent[]>(
		(events, event) => appendRuntimeEvent(events, event),
		[]
	);
}

/** Map a runtime event kind onto the durable session status. */
export function statusFromRuntimeEvent(
	current: SessionStatus,
	eventKind: string,
	options: ApplyRuntimeEventOptions = {}
): SessionStatus {
	if (options.fenced && eventKind === "run.started") return current;
	if (eventKind === "run.started") return "running";
	if (eventKind === "run.completed") return "ready";
	if (eventKind === "run.failed") return "failed";
	if (eventKind === "run.cancelled") return "cancelled";
	if (eventKind === "session/unhealthy") return "interrupted";
	return current;
}

function patchSession(
	sessions: Session[],
	sessionId: string,
	mutator: (session: Session) => Session
): Session[] {
	let changed = false;
	const next = sessions.map((session) => {
		if (session.id !== sessionId) return session;
		const updated = mutator(session);
		if (updated !== session) changed = true;
		return updated;
	});
	return changed ? next : sessions;
}

function patchPresentationMetadata(
	metadata: Record<string, unknown>,
	payload: Record<string, unknown> | undefined
): Record<string, unknown> {
	const next = { ...metadata };
	if (payload && "emotion" in payload) {
		if (payload.emotion == null || payload.emotion === "") delete next.presentationEmotion;
		else if (typeof payload.emotion === "string") next.presentationEmotion = payload.emotion;
	}
	if (payload && "summary" in payload) {
		if (payload.summary == null || payload.summary === "") delete next.presentationSummary;
		else if (typeof payload.summary === "string") next.presentationSummary = payload.summary;
	}
	return next;
}

/**
 * Append a runtime event and update Session.status when the event kind owns it.
 * This is the sole event-driven status writer.
 */
export function applyRuntimeEvent(
	state: SessionStoreState,
	event: RuntimeEvent,
	options: ApplyRuntimeEventOptions = {}
): SessionStoreState {
	const priorEvents = state.eventsBySession[event.sessionId] ?? [];
	const nextEvents = appendRuntimeEvent(priorEvents, event);
	const eventsChanged = nextEvents !== priorEvents;

	const nextSessions = patchSession(state.sessions, event.sessionId, (session) => {
		const nextStatus =
			options.updateStatus === false
				? session.status
				: statusFromRuntimeEvent(session.status, event.eventKind, options);
		const nextTitle =
			typeof options.title === "string" && options.title.trim()
				? options.title.trim()
				: event.eventKind === "session.title_changed" && typeof event.payload?.to === "string" && event.payload.to.trim()
					? event.payload.to.trim()
					: event.eventKind === "session.presented" && typeof event.payload?.title === "string" && event.payload.title.trim()
						? event.payload.title.trim()
						: session.title;
		const nextMetadata = event.eventKind === "session.presented"
			? patchPresentationMetadata(session.metadata, event.payload)
			: session.metadata;
		const statusChanged = nextStatus !== session.status;
		const titleChanged = nextTitle !== session.title;
		const metadataChanged = nextMetadata !== session.metadata;
		if (!statusChanged && !titleChanged && !metadataChanged && !eventsChanged) return session;
		return {
			...session,
			...(titleChanged ? { title: nextTitle } : {}),
			...(statusChanged ? { status: nextStatus } : {}),
			...(metadataChanged ? { metadata: nextMetadata } : {}),
			updatedAt: event.createdAt,
			latestCursor: Math.max(session.latestCursor, event.sequence)
		};
	});

	if (!eventsChanged && nextSessions === state.sessions) return state;

	return {
		sessions: nextSessions,
		eventsBySession: eventsChanged
			? { ...state.eventsBySession, [event.sessionId]: nextEvents }
			: state.eventsBySession
	};
}

/**
 * Optimistic local status write (turn-start fence, interrupt, close).
 * Lives beside applyRuntimeEvent so App.tsx never patches status inline.
 */
export function applyLocalSessionStatus(
	state: SessionStoreState,
	sessionId: string,
	status: SessionStatus,
	options: { onlyIf?: SessionStatus; updatedAt?: string } = {}
): SessionStoreState {
	const nextSessions = patchSession(state.sessions, sessionId, (session) => {
		if (options.onlyIf !== undefined && session.status !== options.onlyIf) return session;
		if (session.status === status) return session;
		return {
			...session,
			status,
			updatedAt: options.updatedAt ?? new Date().toISOString()
		};
	});
	if (nextSessions === state.sessions) return state;
	return { ...state, sessions: nextSessions };
}

/**
 * Working appears once a real turn id exists; otherwise run.started promotes it.
 */
export function applyTurnAccepted(
	state: SessionStoreState,
	sessionId: string,
	patch: { target: ExecutionTarget; turnId?: string | null; updatedAt?: string }
): SessionStoreState {
	const terminalAlreadyApplied = patch.turnId
		? acceptedTurnAlreadyTerminal(state, sessionId, patch.turnId)
		: false;
	const nextSessions = patchSession(state.sessions, sessionId, (session) => ({
		...session,
		target: patch.target,
		status: patch.turnId && !terminalAlreadyApplied ? "running" : session.status,
		updatedAt: terminalAlreadyApplied
			? session.updatedAt
			: patch.updatedAt ?? new Date().toISOString()
	}));
	if (nextSessions === state.sessions) return state;
	return { ...state, sessions: nextSessions };
}

/**
 * UI "running" / Stop arbitration formerly inlined as an IIFE in App.tsx.
 *
 * A restored session record is authoritative: a stale run.started must not
 * resurrect Working after the owning app-server exited. A terminal run event
 * clears a lagging running record unless a newer user turn was already accepted.
 */
export function selectSessionRunning(
	session: Session | undefined,
	events: RuntimeEvent[]
): boolean {
	const latestRunEvent = [...events]
		.reverse()
		.find((event) => event.eventKind.startsWith("run."));
	if (latestRunEvent && TERMINAL_RUN_KINDS.has(latestRunEvent.eventKind)) {
		const hasNewerUserTurn = events.some(
			(event) =>
				event.sequence > latestRunEvent.sequence &&
				event.eventKind === "message.created" &&
				event.payload?.role === "user"
		);
		if (session?.status !== "running" || !hasNewerUserTurn) return false;
	}
	if (session) return session.status === "running";
	return latestRunEvent?.eventKind === "run.started";
}

export function selectWorkingChatIds(sessions: Session[]): Set<string> {
	return new Set(
		sessions
			.filter((session) => session.target.kind !== "intern" && session.status === "running")
			.map((session) => session.id)
	);
}

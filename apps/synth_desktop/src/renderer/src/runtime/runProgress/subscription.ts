/**
 * `RunProgressSubscription` — one durable, cursor-based subscription per run,
 * shared by every surface that shows it.
 *
 * This is the transport half of the feature, lifted out of `VisualHost` so the
 * transcript card, the expanded dialog, and the full visual read the same
 * bytes. A run that appears in three places must not triple upstream traffic,
 * and closing a dialog must not reset a run or replay its history — both fall
 * out of a reference-counted store keyed by run id.
 *
 * The rules it enforces:
 *
 *   · The run record is terminal-state authority. Events can suggest a run
 *     finished; only `run.status` ends a subscription.
 *   · Events are consumed by durable sequence cursor. Notifications are
 *     wakeups, not truth — every wakeup re-reads the persisted pages.
 *   · A sequence hole is never patched over. A gap, a shrinking run cursor, or
 *     a replaced local import forces a full snapshot reload; if history is
 *     still short of the run's own cursor, the state is `stale` and consumers
 *     must present counts as a floor.
 *   · Unsubscribing parks a subscription; it does not destroy what was read.
 *     Re-subscribing resumes from the retained cursor.
 */

import { publicError } from "../publicError";
import { mergeOptimizerEventPage, type OptimizerEventCursorState } from "../optimizerEventCursor";
import { isTerminalRunStatus } from "./types";

export type RunProgressConnectionState =
	| "loading"
	| "replaying"
	| "subscribed"
	| "reconnecting"
	| "stale"
	| "terminal"
	| "failed"
	| "unavailable";

/** The minimum of an `optimizer_run.v1` record this layer needs. */
export type RunRecord = {
	id: string;
	algorithmId: string;
	status: string;
	source?: string;
	objective?: string;
	sessionRef?: string | null;
	createdAt?: string;
	startedAt?: string | null;
	finishedAt?: string | null;
	cursorSeq?: number;
	capabilities?: Record<string, boolean>;
	summary?: Record<string, unknown>;
	usage?: Record<string, unknown>;
	visualRefs?: Array<Record<string, unknown>>;
	outputRefs?: Array<Record<string, unknown>>;
	executionBindings?: Array<Record<string, unknown>>;
	error?: unknown;
};

export type RunProgressSnapshot = {
	runId: string;
	state: RunProgressConnectionState;
	run: RunRecord | null;
	events: unknown[];
	cursor: number;
	/** History is known-incomplete at this cursor. */
	gap: boolean;
	error?: string;
	/** Increments on every published change, so consumers can memoize cheaply. */
	revision: number;
};

/** The transport the store reads through. Injectable so the rules are testable. */
export type RunProgressTransport = {
	get(runId: string): Promise<RunRecord>;
	eventsAfter(runId: string, afterSeq?: number, limit?: number): Promise<unknown[]>;
	refresh(runId: string): Promise<unknown>;
	onEvent(listener: (event: { payload?: Record<string, unknown> }) => void): () => void;
};

const PAGE_SIZE = 500;
const POLL_INTERVAL_MS = 750;
/** Parked entries retained so a reopened dialog resumes instead of replaying. */
const MAX_PARKED_ENTRIES = 32;

type Entry = {
	runId: string;
	snapshot: RunProgressSnapshot;
	listeners: Set<(snapshot: RunProgressSnapshot) => void>;
	cursorState: OptimizerEventCursorState;
	pending: Promise<void>;
	/** Node and DOM disagree on the handle type; take whatever the platform returns. */
	poll: ReturnType<typeof globalThis.setInterval> | null;
	unlisten: (() => void) | null;
	stopPolling: boolean;
	/**
	 * A hole was seen. The next read must start from zero: reading forward from a
	 * holed cursor would leave the hole in place forever, and the missing events
	 * would never be noticed again.
	 */
	needsSnapshot: boolean;
	lastTouchedAt: number;
	disposed: boolean;
};

/**
 * A stream diagnostic. The real sink is installed by the renderer entry point;
 * this module must stay importable outside a webview, so the default is a
 * no-op rather than a static dependency on the Tauri bridge.
 */
export type RunProgressDiagnostic = {
	runId: string;
	severity: "warn" | "error";
	event: string;
	code: "stream_replay_gap" | "stream_interrupted";
	message: string;
	details?: Record<string, unknown>;
};

const entries = new Map<string, Entry>();
let injectedTransport: RunProgressTransport | null = null;
let pollIntervalMs = POLL_INTERVAL_MS;
let diagnosticSink: ((report: RunProgressDiagnostic) => void) | null = null;

/** Install the renderer's diagnostic reporter. Called once, from the entry point. */
export function installRunProgressDiagnostics(
	sink: (report: RunProgressDiagnostic) => void
): void {
	diagnosticSink = sink;
}

function report(diagnostic: RunProgressDiagnostic): void {
	try {
		diagnosticSink?.(diagnostic);
	} catch {
		// Failing to record a failure must never become a second failure.
	}
}

/** Tests inject a transport; the app resolves the desktop bridge lazily. */
export function setRunProgressTransport(transport: RunProgressTransport | null): void {
	injectedTransport = transport;
}

/** Tests shorten the wakeup interval. */
export function setRunProgressPollInterval(ms: number): void {
	pollIntervalMs = ms;
}

/** Tests only: drop every entry and every timer. */
export function resetRunProgressStore(): void {
	for (const entry of entries.values()) park(entry);
	entries.clear();
}

/**
 * The injected transport, or the desktop bridge the host installs on the global
 * object. Read lazily and without importing the bridge module, so this file is
 * importable in a plain Node test.
 */
function transport(): RunProgressTransport | null {
	if (injectedTransport) return injectedTransport;
	const bridge = (globalThis as { synthOptimizers?: RunProgressTransport }).synthOptimizers;
	if (!bridge) return null;
	return {
		get: (runId) => bridge.get(runId),
		eventsAfter: (runId, afterSeq, limit) => bridge.eventsAfter(runId, afterSeq, limit),
		refresh: (runId) => bridge.refresh(runId),
		onEvent: (listener) => bridge.onEvent(listener)
	};
}

/**
 * Deliver one snapshot to one listener, absorbing its failure. Used for both the
 * immediate delivery on subscribe and the fan-out on every change.
 */
function deliver(entry: Entry, listener: (snapshot: RunProgressSnapshot) => void): void {
	try {
		listener(entry.snapshot);
	} catch (reason) {
		report({
			runId: entry.runId,
			severity: "error",
			event: "run_progress.consumer.failed",
			code: "stream_interrupted",
			message: publicError(reason),
			details: { revision: entry.snapshot.revision }
		});
	}
}

/**
 * Publish to every subscriber, each isolated from the others.
 *
 * One surface crashing must not change what another surface shows: a renderer
 * failure in the full visual cannot break the transcript card's state, and a
 * throwing listener must neither abort the loop nor escape into `load`, where it
 * would be recorded as a stream failure for the whole run.
 */
function publish(entry: Entry, next: Partial<RunProgressSnapshot>): void {
	entry.snapshot = { ...entry.snapshot, ...next, revision: entry.snapshot.revision + 1 };
	for (const listener of [...entry.listeners]) deliver(entry, listener);
}

function eventRunId(event: { payload?: Record<string, unknown> }): string | null {
	const payload = event.payload ?? {};
	const camel = payload.optimizerRunId;
	const snake = payload.optimizer_run_id;
	if (typeof camel === "string") return camel;
	if (typeof snake === "string") return snake;
	return null;
}

/**
 * Read every persisted page after `after`. Stops on a gap so the caller can
 * decide to reload from zero rather than reduce over a hole.
 */
async function readPersisted(
	entry: Entry,
	api: RunProgressTransport,
	after: number
): Promise<OptimizerEventCursorState> {
	let state: OptimizerEventCursorState = {
		events: after === 0 ? [] : entry.cursorState.events,
		cursor: after,
		gap: false
	};
	for (;;) {
		const page = await api.eventsAfter(entry.runId, state.cursor, PAGE_SIZE);
		if (!Array.isArray(page) || page.length === 0) return state;
		const before = state.cursor;
		state = mergeOptimizerEventPage(state, page);
		if (state.gap || state.cursor === before || page.length < PAGE_SIZE) return state;
	}
}

async function load(entry: Entry, api: RunProgressTransport, requestSnapshot: boolean): Promise<void> {
	if (entry.disposed) return;
	// A previously seen hole is only healed by a full reload, so it upgrades an
	// incremental wakeup into a snapshot read.
	const snapshot = requestSnapshot || entry.needsSnapshot;
	try {
		publish(entry, {
			state: snapshot
				? "replaying"
				: entry.snapshot.state === "subscribed"
					? "reconnecting"
					: entry.snapshot.state
		});
		const run = await api.get(entry.runId);
		let next = await readPersisted(entry, api, snapshot ? 0 : entry.cursorState.cursor);
		const runCursor = typeof run.cursorSeq === "number" ? run.cursorSeq : next.cursor;
		if (!snapshot && (next.gap || runCursor < entry.cursorState.cursor || next.cursor < runCursor)) {
			// A missed notification, a truncated page, or a replaced local import.
			// Reload from the durable start; never patch over a sequence hole.
			next = await readPersisted(entry, api, 0);
		}
		if (entry.disposed) return;
		entry.cursorState = next;
		const terminal = isTerminalRunStatus(run.status);
		if (terminal) entry.stopPolling = true;
		entry.needsSnapshot = next.gap || next.cursor < runCursor;

		if (next.gap || next.cursor < runCursor) {
			report({
				runId: entry.runId,
				severity: "warn",
				event: "run_progress.replay.gap",
				code: "stream_replay_gap",
				message: `Optimizer event history is incomplete at ${next.cursor}/${runCursor}`,
				details: { cursor: next.cursor, runCursor, gap: next.gap }
			});
			publish(entry, {
				state: "stale",
				run,
				events: next.events,
				cursor: next.cursor,
				gap: true,
				error: undefined
			});
			return;
		}

		publish(entry, {
			state: terminal ? "terminal" : "subscribed",
			run,
			events: next.events,
			cursor: next.cursor,
			gap: false,
			error: undefined
		});
	} catch (reason) {
		if (entry.disposed) return;
		const message = publicError(reason);
		report({
			runId: entry.runId,
			severity: "error",
			event: "run_progress.stream.interrupted",
			code: "stream_interrupted",
			message
		});
		// A failed read never discards what was already replayed: a card that
		// showed 68 rollouts must not blank because one page timed out.
		publish(entry, { state: "failed", error: message });
	}
}

function enqueue(entry: Entry, api: RunProgressTransport, snapshot: boolean): void {
	entry.pending = entry.pending.then(() => load(entry, api, snapshot));
}

function activate(entry: Entry, api: RunProgressTransport): void {
	entry.disposed = false;
	entry.unlisten = api.onEvent((event) => {
		const id = eventRunId(event);
		if (!id || id === entry.runId) enqueue(entry, api, false);
	});
	entry.poll = globalThis.setInterval(() => {
		if (entry.stopPolling) return;
		void api.refresh(entry.runId).catch(() => undefined);
	}, pollIntervalMs);
	// Resume from the retained cursor; a re-subscribe is not a replay.
	enqueue(entry, api, entry.cursorState.cursor === 0);
}

function park(entry: Entry): void {
	entry.disposed = true;
	if (entry.poll != null) globalThis.clearInterval(entry.poll);
	entry.poll = null;
	entry.unlisten?.();
	entry.unlisten = null;
}

function evictParked(): void {
	const parked = [...entries.values()]
		.filter((entry) => entry.listeners.size === 0)
		.sort((left, right) => left.lastTouchedAt - right.lastTouchedAt);
	while (entries.size > MAX_PARKED_ENTRIES && parked.length > 0) {
		const victim = parked.shift()!;
		park(victim);
		entries.delete(victim.runId);
	}
}

function emptySnapshot(runId: string, state: RunProgressConnectionState): RunProgressSnapshot {
	return { runId, state, run: null, events: [], cursor: 0, gap: false, revision: 0 };
}

/**
 * Ownership gate. Returns the durable record when this session may watch the
 * run, and `null` when the run is owned by another conversation or has been
 * removed — the caller then renders an unavailable state instead of
 * subscribing. A run with no `sessionRef` is a workspace-level run and is
 * readable from any conversation.
 */
export async function resolveOwnedRun(runId: string, sessionRef?: string): Promise<RunRecord | null> {
	const api = transport();
	if (!api) return null;
	try {
		const run = await api.get(runId);
		if (!run || typeof run.id !== "string") return null;
		if (sessionRef && run.sessionRef && run.sessionRef !== sessionRef) return null;
		return run;
	} catch {
		return null;
	}
}

/**
 * Subscribe to a run. The listener is called immediately with the current
 * snapshot — retained from an earlier subscription when there is one — and on
 * every change after. The returned function parks the subscription.
 */
export function subscribeToRun(
	runId: string,
	listener: (snapshot: RunProgressSnapshot) => void
): () => void {
	const api = transport();
	if (!api) {
		listener({ ...emptySnapshot(runId, "unavailable"), error: "Optimizer bridge is unavailable" });
		return () => undefined;
	}
	let entry = entries.get(runId);
	if (!entry) {
		entry = {
			runId,
			snapshot: emptySnapshot(runId, "loading"),
			listeners: new Set(),
			cursorState: { events: [], cursor: 0, gap: false },
			pending: Promise.resolve(),
			poll: null,
			unlisten: null,
			stopPolling: false,
			needsSnapshot: false,
			lastTouchedAt: Date.now(),
			disposed: true
		};
		entries.set(runId, entry);
	}
	const wasIdle = entry.listeners.size === 0;
	entry.listeners.add(listener);
	entry.lastTouchedAt = Date.now();
	deliver(entry, listener);
	if (wasIdle) {
		// A terminal run needs no live wakeups, but it still needs one read to
		// restore its card after an application restart.
		activate(entry, api);
	}
	evictParked();
	return () => {
		const current = entries.get(runId);
		if (!current) return;
		current.listeners.delete(listener);
		current.lastTouchedAt = Date.now();
		if (current.listeners.size === 0) park(current);
	};
}

export function runSnapshot(runId: string): RunProgressSnapshot | undefined {
	return entries.get(runId)?.snapshot;
}

/** Live subscriber count for a run. Tests assert the sharing contract with it. */
export function runSubscriberCount(runId: string): number {
	return entries.get(runId)?.listeners.size ?? 0;
}

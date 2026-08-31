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
 *   · The kernel V2 view is terminal-state authority for optimizer runs. The
 *     old run record is a compatibility fallback only when a transport does
 *     not implement V2 (environment cards and injected legacy tests).
 *   · Events are consumed by durable sequence cursor. Notifications are
 *     wakeups, not truth — every wakeup re-reads the persisted pages.
 *   · A sequence hole is never patched over. A gap, a shrinking run cursor, or
 *     a replaced local import forces a full snapshot reload; if history is
 *     still short of the run's own cursor, the state is `stale` and consumers
 *     must present counts as a floor.
 *   · Unsubscribing parks a subscription; it does not destroy what was read.
 *     Re-subscribing resumes from the retained cursor.
 */

import { publicError, toPublicError } from "../publicError";
import {
	emptyOptimizerEventCursor,
	mergeOptimizerEventPage,
	publishedOptimizerEvents,
	type OptimizerEventCursorState
} from "../optimizerEventCursor";
import {
	flushReadPathTelemetry,
	recordEvidencePage,
	recordFirstPaint,
	recordInteractive,
	recordProbe,
	recordReadFailure,
	recordReadStarted,
	recordReplay,
	recordStage
} from "./readPathTelemetry";
import { isTerminalRunStatus } from "./types";
import type { OptimizerRunViewEnvelope, OptimizerRunViewV2 } from "../../generated/protocol";

export type RunProgressConnectionState =
	| "loading"
	| "replaying"
	| "subscribed"
	| "reconnecting"
	| "stale"
	| "terminal"
	| "interrupted"
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
	/** Product truth. Raw events below are retained for diagnostics and rich evidence browsing. */
	viewV2?: OptimizerRunViewV2;
	events: unknown[];
	cursor: number;
	/** History is known-incomplete at this cursor. */
	gap: boolean;
	/**
	 * Where raw-evidence hydration has got to, independent of `state`.
	 *
	 * Aggregate surfaces are decided by `viewV2` and must mount as soon as it
	 * arrives; Replay, the agent transcript, and frame drill-down are decided
	 * by `events` and legitimately arrive later. Collapsing both into one
	 * connection state is what made a visual wait for the whole journal before
	 * it would render a number the projection already knew.
	 */
	evidence: "pending" | "loading" | "ready" | "partial" | "unavailable";
	error?: string;
	/** Increments on every published change, so consumers can memoize cheaply. */
	revision: number;
};

/** The transport the store reads through. Injectable so the rules are testable. */
export type RunProgressTransport = {
	get(runId: string): Promise<RunRecord>;
	/** Optional only for legacy/injected tests. The desktop bridge always exposes it. */
	runViewV2?(runId: string): Promise<OptimizerRunViewV2>;
	/**
	 * Projection, run record, and durable journal tail in one conditional
	 * read. Preferred over `runViewV2` + `get`: it is one IPC hop instead of
	 * two, and passing the revision already held returns `unchanged` rather
	 * than the same bytes again.
	 *
	 * Optional for the same reason as `runViewV2` — injected legacy transports
	 * in tests do not implement it.
	 */
	runView?(runId: string, ifNewerThan?: number | null): Promise<OptimizerRunViewEnvelope>;
	eventsAfter(runId: string, afterSeq?: number, limit?: number): Promise<unknown[]>;
	refresh(runId: string): Promise<unknown>;
	onEvent(listener: (event: { payload?: Record<string, unknown> }) => void): () => void;
};

const PAGE_SIZE = 500;
const POLL_INTERVAL_MS = 750;
/** How long a hung get/eventsAfter may sit before the UI leaves Running. */
const STALL_TIMEOUT_MS = 15_000;
/** A broken producer gets a bounded automatic recovery budget. */
const MAX_CONSECUTIVE_FAILURES = 5;
const RETRY_BASE_MS = 250;
/** Parked entries retained so a reopened dialog resumes instead of replaying. */
const MAX_PARKED_ENTRIES = 32;
/**
 * Total events the store may retain across *parked* runs.
 *
 * `MAX_PARKED_ENTRIES` bounds how many runs are remembered; it does not bound
 * how much is remembered. A five-event smoke run and a fifty-thousand-event
 * eval count the same against it, so the store could hold thirty-two complete
 * journals — on the reference machine, ~9MB each — long after the user closed
 * every visual. That is not a cycle or a lost reference; it is a retention
 * policy written to preserve a *cursor* that ended up preserving the whole
 * history with it.
 *
 * Parking still resumes from the cursor for the runs a user is actually moving
 * between, which is what the policy exists for. Beyond this budget the oldest
 * parked journals are released and their cursors reset, so reopening one
 * re-reads it — which is now cheap, because the aggregate mounts from the
 * durable projection and evidence hydrates behind it rather than blocking it.
 */
const MAX_PARKED_EVENTS = 20_000;

type Entry = {
	runId: string;
	snapshot: RunProgressSnapshot;
	listeners: Set<(snapshot: RunProgressSnapshot) => void>;
	cursorState: OptimizerEventCursorState;
	pending: Promise<void>;
	/** Invalidates work still attached to an abandoned promise chain. */
	queueEpoch: number;
	/** Invalidates a transport result after its watchdog has unwedged the queue. */
	loadEpoch: number;
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
	stallTimer: ReturnType<typeof globalThis.setTimeout> | null;
	retryTimer: ReturnType<typeof globalThis.setTimeout> | null;
	consecutiveFailures: number;
	pollBusy: boolean;
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
	code: "stream_replay_gap" | "stream_interrupted" | "stream_stalled";
	message: string;
	details?: Record<string, unknown>;
};

const entries = new Map<string, Entry>();
let injectedTransport: RunProgressTransport | null = null;
let pollIntervalMs = POLL_INTERVAL_MS;
let stallTimeoutMs = STALL_TIMEOUT_MS;
let retryBaseMs = RETRY_BASE_MS;
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

/** Tests shorten the hung-read watchdog. */
export function setRunProgressStallTimeout(ms: number): void {
	stallTimeoutMs = ms;
}

/** Tests shorten the bounded reconnect ladder. */
export function setRunProgressRetryBase(ms: number): void {
	retryBaseMs = ms;
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
		...(typeof bridge.runViewV2 === "function"
			? { runViewV2: (runId: string) => bridge.runViewV2!(runId) }
			: {}),
		...(typeof bridge.runView === "function"
			? {
				runView: (runId: string, ifNewerThan?: number | null) =>
					bridge.runView!(runId, ifNewerThan)
			}
			: {}),
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
	// A full reload starts from a fresh index; an incremental one continues
	// mutating the retained state. Both keep the sequence index alive across
	// pages, which is what makes a long walk linear rather than quadratic.
	//
	// The retained array must never be the one a consumer is holding: `publish`
	// hands out a copy for exactly this reason.
	const state = after === 0 ? emptyOptimizerEventCursor() : entry.cursorState;
	state.cursor = after;
	state.gap = false;
	for (;;) {
		const pageStartedAt = Date.now();
		const page = await withDeadline(
			() => api.eventsAfter(entry.runId, state.cursor, PAGE_SIZE),
			`eventsAfter(${state.cursor})`
		);
		timedStage(entry.runId, "eventsAfter", pageStartedAt);
		if (!Array.isArray(page) || page.length === 0) return state;
		recordEvidencePage(entry.runId, page.length, Date.now());
		const before = state.cursor;
		mergeOptimizerEventPage(state, page);
		if (state.gap || state.cursor === before || page.length < PAGE_SIZE) return state;
	}
}

function deadlineMs(): number {
	return stallTimeoutMs * 2;
}

/**
 * Every renderer/host boundary has a deadline. The underlying invoke may not
 * be cancellable, so the load epoch below also prevents a late result from
 * overwriting the recovery load that replaced it.
 */
/**
 * Stage attribution for one timed read.
 *
 * `withDeadline` already wraps every renderer/host boundary, so timing it here
 * measures each stage — projection, metadata, evidence page — separately
 * rather than collapsing a slow open into one "subscription stalled".
 */
function timedStage(runId: string, operation: string, startedAt: number): void {
	const elapsed = Date.now() - startedAt;
	if (operation.startsWith("projection")) {
		recordStage(runId, "projection", elapsed, Date.now());
	} else if (operation === "metadata") {
		recordStage(runId, "metadata", elapsed, Date.now());
	} else if (operation.startsWith("eventsAfter")) {
		recordStage(runId, "evidence_page", elapsed, Date.now());
	}
}

function withDeadline<T>(read: () => Promise<T>, operation: string): Promise<T> {
	return new Promise<T>((resolve, reject) => {
		let settled = false;
		const timer = globalThis.setTimeout(() => {
			if (settled) return;
			settled = true;
			reject(new Error(`Optimizer subscription stalled — ${operation} exceeded the ${deadlineMs()}ms deadline`));
		}, deadlineMs());
		void Promise.resolve()
			.then(read)
			.then(
				(value) => {
					if (settled) return;
					settled = true;
					globalThis.clearTimeout(timer);
					resolve(value);
				},
				(reason) => {
					if (settled) return;
					settled = true;
					globalThis.clearTimeout(timer);
					reject(reason);
				}
			);
	});
}

function disarmStall(entry: Entry, epoch?: number): void {
	if (epoch != null && epoch !== entry.loadEpoch) return;
	if (entry.stallTimer != null) globalThis.clearTimeout(entry.stallTimer);
	entry.stallTimer = null;
}

function clearRetry(entry: Entry): void {
	if (entry.retryTimer != null) globalThis.clearTimeout(entry.retryTimer);
	entry.retryTimer = null;
}

function enqueue(entry: Entry, api: RunProgressTransport, snapshot: boolean): void {
	const queueEpoch = entry.queueEpoch;
	entry.pending = entry.pending.then(async () => {
		if (entry.disposed || queueEpoch !== entry.queueEpoch) return;
		await load(entry, api, snapshot);
	});
}

function scheduleRetry(entry: Entry, api: RunProgressTransport): void {
	if (entry.disposed || entry.consecutiveFailures >= MAX_CONSECUTIVE_FAILURES || entry.retryTimer != null) return;
	const exponent = Math.max(0, entry.consecutiveFailures - 1);
	const backoffMs = Math.min(4_000, retryBaseMs * (2 ** exponent));
	entry.retryTimer = globalThis.setTimeout(() => {
		entry.retryTimer = null;
		if (entry.disposed || entry.consecutiveFailures >= MAX_CONSECUTIVE_FAILURES) return;
		enqueue(entry, api, false);
	}, backoffMs);
}

/**
 * Whether a failure is worth trying again.
 *
 * A backend that says `retryable: false` is reporting a structural fact about
 * the row, not a transport hiccup — a run with no admitted spec can never
 * acquire one by being asked five more times. Each of those attempts replays
 * the whole journal before failing identically, so honouring the flag is both
 * a latency fix and the difference between a legible message and a stalled
 * subscription.
 */
function isRetryable(reason: unknown): boolean {
	const { retryable } = toPublicError(reason);
	return retryable !== false;
}

function recordFailure(entry: Entry, api: RunProgressTransport, reason: unknown): void {
	if (entry.disposed) return;
	const message = publicError(reason);
	recordReadFailure(entry.runId, Date.now());
	entry.consecutiveFailures += 1;
	const permanent = !isRetryable(reason);
	if (permanent) {
		// Spend the whole budget at once: the state is `failed` now, and no
		// wakeup or poll will attempt this read again.
		entry.consecutiveFailures = MAX_CONSECUTIVE_FAILURES;
		entry.stopPolling = true;
	}
	const exhausted = entry.consecutiveFailures >= MAX_CONSECUTIVE_FAILURES;
	report({
		runId: entry.runId,
		severity: "error",
		event: exhausted ? "run_progress.stream.failed" : "run_progress.stream.interrupted",
		code: "stream_interrupted",
		message,
		details: {
			attempt: entry.consecutiveFailures,
			maxAttempts: MAX_CONSECUTIVE_FAILURES,
			permanent
		}
	});
	publish(entry, { state: exhausted ? "failed" : "interrupted", error: message });
	if (!exhausted) scheduleRetry(entry, api);
}

function armStall(entry: Entry, api: RunProgressTransport, epoch: number): void {
	disarmStall(entry);
	entry.stallTimer = globalThis.setTimeout(() => {
		if (epoch !== entry.loadEpoch) return;
		entry.stallTimer = null;
		if (entry.disposed) return;
		// Abandon both the active load and anything chained behind it. The
		// transport promise can still settle later, but its epoch is now stale.
		entry.loadEpoch += 1;
		entry.queueEpoch += 1;
		entry.pending = Promise.resolve();
		report({
			runId: entry.runId,
			severity: "error",
			event: "run_progress.stream.stalled",
			code: "stream_stalled",
			message: "Optimizer subscription stalled; the run is interrupted, not still working"
		});
		recordFailure(
			entry,
			api,
			new Error("subscription stalled — the producer stopped answering; reconnecting resumes from the retained cursor")
		);
	}, stallTimeoutMs);
}

/**
 * Read the durable projection and the run record for one load.
 *
 * Prefers the single conditional `runView`. When the caller already holds a
 * projection revision it is sent along, and an `unchanged` answer costs one
 * indexed column read and carries no payload — the retained view and run are
 * reused instead. `runViewV2` + `get` remains as the fallback for injected
 * legacy transports in tests, which do not implement the envelope.
 */
async function readProjection(
	entry: Entry,
	api: RunProgressTransport,
	cachedRevision: number | undefined
): Promise<{ viewV2: OptimizerRunViewV2 | undefined; run: RunRecord }> {
	if (api.runView) {
		const startedAt = Date.now();
		const envelope = await withDeadline(
			() => api.runView!(entry.runId, cachedRevision ?? null),
			"projection"
		);
		timedStage(entry.runId, "projection", startedAt);
		if (envelope.unchanged) {
			// Nothing moved. Reuse what is already published rather than
			// re-deriving it — and if this entry somehow has no retained run,
			// fall through to a full read rather than inventing one.
			if (entry.snapshot.run) {
				return {
					viewV2: entry.snapshot.viewV2 as OptimizerRunViewV2 | undefined,
					run: entry.snapshot.run
				};
			}
			const full = await withDeadline(
				() => api.runView!(entry.runId, null),
				"projection"
			);
			return {
				viewV2: full.view ?? undefined,
				run: (full.run ?? undefined) as RunRecord
			};
		}
		if (envelope.run) {
			return { viewV2: envelope.view ?? undefined, run: envelope.run as RunRecord };
		}
	}
	const projectionStartedAt = Date.now();
	const viewV2 = api.runViewV2
		? await withDeadline(() => api.runViewV2!(entry.runId), "projection")
		: undefined;
	timedStage(entry.runId, "projection", projectionStartedAt);
	const metadataStartedAt = Date.now();
	const run = await withDeadline(() => api.get(entry.runId), "metadata");
	timedStage(entry.runId, "metadata", metadataStartedAt);
	return { viewV2, run };
}

async function load(entry: Entry, api: RunProgressTransport, requestSnapshot: boolean): Promise<void> {
	if (entry.disposed) return;
	const epoch = ++entry.loadEpoch;
	// A previously seen hole is only healed by a full reload, so it upgrades an
	// incremental wakeup into a snapshot read.
	let snapshot = requestSnapshot || entry.needsSnapshot;
	if (snapshot) {
		recordReplay(entry.runId, entry.needsSnapshot ? "healing_hole" : "cold_open", Date.now());
	}
	armStall(entry, api, epoch);
	try {
		publish(entry, {
			state: snapshot || entry.snapshot.state === "interrupted" || entry.snapshot.state === "failed"
				? "replaying"
				: entry.snapshot.state === "subscribed"
					? "reconnecting"
					: entry.snapshot.state
		});
		// Projection first: a reconnect must never converge below the durable
		// reducer revision even when its event wakeup was missed.
		//
		// One read, not two. `runView` returns the projection and the run
		// record from the same deferred transaction — the backend had already
		// loaded the run row to build the view's context, so the separate
		// `get` was a second IPC hop for bytes that were already in hand.
		const cachedRevision = entry.snapshot.viewV2?.header.projectionRevision;
		const { viewV2, run } = await readProjection(entry, api, cachedRevision);
		if (entry.disposed || epoch !== entry.loadEpoch) return;
		const durableRevision = viewV2?.header.projectionRevision;
		const reconnecting = entry.consecutiveFailures > 0;
		if (
			cachedRevision != null &&
			durableRevision != null &&
			(
				durableRevision < cachedRevision ||
				durableRevision > cachedRevision + 1 ||
				(reconnecting && durableRevision > cachedRevision)
			)
		) {
			snapshot = true;
		}

		// FIRST PAINT. Everything an aggregate surface needs is now in hand, so
		// publish before walking the journal rather than after.
		//
		// This is the whole point of the split. Raw events feed Replay, the
		// transcript, and frame drill-down; they do not decide lifecycle,
		// progress, usage, or result — the projection does. Holding the mount
		// until the last page landed made optional detail a blocking
		// dependency, and turned any slow page into "Restoring run evidence…".
		const terminal = viewV2
			? viewV2.header.lifecycle === "terminal"
			: isTerminalRunStatus(run.status);
		if (terminal) entry.stopPolling = true;
		const retainedCursor = snapshot ? 0 : entry.cursorState.cursor;
		publish(entry, {
			state: terminal ? "terminal" : "subscribed",
			run,
			viewV2,
			cursor: retainedCursor,
			evidence: "loading",
			error: undefined
		});
		recordFirstPaint(entry.runId, Date.now());
		disarmStall(entry, epoch);
		clearRetry(entry);
		entry.consecutiveFailures = 0;

		// Evidence hydration continues under its own deadline. A failure from
		// here degrades the evidence lane; it never blanks the aggregate view
		// that has already been published above.
		armStall(entry, api, epoch);
		let next = await readPersisted(entry, api, retainedCursor);
		const runCursor = typeof run.cursorSeq === "number" ? run.cursorSeq : next.cursor;
		if (!snapshot && (next.gap || runCursor < entry.cursorState.cursor || next.cursor < runCursor)) {
			// A missed notification, a truncated page, or a replaced local import.
			// Reload from the durable start; never patch over a sequence hole.
			recordReplay(
				entry.runId,
				next.gap ? "sequence_gap" : runCursor < entry.cursorState.cursor ? "cursor_rewound" : "short_history",
				Date.now()
			);
			next = await readPersisted(entry, api, 0);
		}
		if (entry.disposed || epoch !== entry.loadEpoch) return;
		disarmStall(entry, epoch);
		clearRetry(entry);
		entry.consecutiveFailures = 0;
		entry.cursorState = next;
		entry.needsSnapshot = next.gap || next.cursor < runCursor;

		const incomplete = next.gap || next.cursor < runCursor;
		if (incomplete && !viewV2) {
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
				viewV2,
				events: publishedOptimizerEvents(next),
				cursor: next.cursor,
				gap: true,
				evidence: "partial",
				error: undefined
			});
			return;
		}

		// With a durable projection in hand an incomplete journal is an
		// evidence-lane fact, not a lifecycle fact: the aggregate stays
		// authoritative and only the detail surfaces report a shortfall.
		publish(entry, {
			state: terminal ? "terminal" : "subscribed",
			run,
			viewV2,
			events: publishedOptimizerEvents(next),
			cursor: next.cursor,
			gap: incomplete,
			evidence: incomplete ? "partial" : "ready",
			error: undefined
		});
		if (!incomplete) recordInteractive(entry.runId, Date.now());
		// A terminal run has nothing further to measure, so it reports now
		// rather than waiting for a park that may never come.
		if (terminal) flushReadPathTelemetry(entry.runId);
	} catch (reason) {
		disarmStall(entry, epoch);
		if (entry.disposed || epoch !== entry.loadEpoch) return;
		// A failed read never discards what was already replayed: a card that
		// showed 68 rollouts must not blank because one page timed out. The
		// first four failures are interrupted; the bounded fifth is failed.
		recordFailure(entry, api, reason);
	}
}

async function pollDurableRevision(entry: Entry, api: RunProgressTransport): Promise<void> {
	if (entry.disposed || entry.stopPolling || entry.pollBusy || entry.consecutiveFailures >= MAX_CONSECUTIVE_FAILURES) return;
	entry.pollBusy = true;
	try {
		const cachedRevision = entry.snapshot.viewV2?.header.projectionRevision;
		if (api.runView) {
			// A freshness probe should cost one indexed column read, not a
			// projection load. Sending the revision we hold means the common
			// answer — nothing changed — carries no payload at all.
			const envelope = await withDeadline(
				() => api.runView!(entry.runId, cachedRevision ?? null),
				"projection probe"
			);
			if (entry.disposed) return;
			recordProbe(entry.runId, envelope.unchanged, Date.now());
			if (envelope.unchanged) return;
			const durableRevision = envelope.projectionRevision;
			if (cachedRevision != null && (durableRevision < cachedRevision || durableRevision > cachedRevision + 1)) {
				entry.needsSnapshot = true;
			}
			enqueue(entry, api, cachedRevision == null);
			return;
		}
		if (!api.runViewV2) {
			await withDeadline(() => api.refresh(entry.runId), "refresh");
			return;
		}
		const durable = await withDeadline(() => api.runViewV2!(entry.runId), "projection probe");
		if (entry.disposed) return;
		const durableRevision = durable.header.projectionRevision;
		if (cachedRevision == null || durableRevision !== cachedRevision) {
			if (cachedRevision != null && (durableRevision < cachedRevision || durableRevision > cachedRevision + 1)) {
				entry.needsSnapshot = true;
			}
			enqueue(entry, api, cachedRevision == null);
		}
	} catch (reason) {
		recordFailure(entry, api, reason);
	} finally {
		entry.pollBusy = false;
	}
}

function activate(entry: Entry, api: RunProgressTransport): void {
	entry.disposed = false;
	entry.unlisten = api.onEvent((event) => {
		const id = eventRunId(event);
		// Producer notifications are never gated by the retry budget. A producer
		// returning after five failed reads is the strongest recovery signal.
		if (!id || id === entry.runId) enqueue(entry, api, false);
	});
	// A terminal run has no further revisions to discover, so it gets no
	// interval at all. Arming one and returning early from every tick still
	// cost a timer per mounted card, and `stopPolling` is only known after the
	// first read — so re-check it once that read has landed.
	if (!entry.stopPolling) {
		entry.poll = globalThis.setInterval(() => {
			if (entry.stopPolling) {
				if (entry.poll != null) globalThis.clearInterval(entry.poll);
				entry.poll = null;
				return;
			}
			void pollDurableRevision(entry, api);
		}, pollIntervalMs);
	}
	recordReadStarted(entry.runId, Date.now());
	// Resume from the retained cursor; a re-subscribe is not a replay.
	enqueue(entry, api, entry.cursorState.cursor === 0);
}

function park(entry: Entry): void {
	// A run the user closed before it settled still reports what its open cost.
	flushReadPathTelemetry(entry.runId);
	// The sequence index only earns its keep during a paged walk. It is
	// rebuilt from `events` on the next merge, so an idle entry should not
	// carry a second copy of every key.
	entry.cursorState.index = undefined;
	entry.disposed = true;
	entry.queueEpoch += 1;
	entry.loadEpoch += 1;
	entry.pending = Promise.resolve();
	disarmStall(entry);
	clearRetry(entry);
	if (entry.poll != null) globalThis.clearInterval(entry.poll);
	entry.poll = null;
	entry.unlisten?.();
	entry.unlisten = null;
}

/**
 * Release a parked entry's journal while keeping the entry.
 *
 * The projection and run record stay — they are small, and they are what
 * mounts a visual — so a reopen still paints immediately. Only the raw
 * evidence goes, and the cursor resets with it so the next read replays from
 * the durable start rather than resuming over a hole it no longer holds.
 */
function releaseJournal(entry: Entry): void {
	if (entry.snapshot.events.length === 0 && entry.cursorState.cursor === 0) return;
	entry.cursorState = emptyOptimizerEventCursor();
	entry.needsSnapshot = false;
	entry.snapshot = {
		...entry.snapshot,
		events: [],
		cursor: 0,
		gap: false,
		evidence: "pending",
		revision: entry.snapshot.revision + 1
	};
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
	enforceRetentionBudget();
}

/**
 * Hold parked journals inside [`MAX_PARKED_EVENTS`], newest-touched first.
 *
 * Runs when an entry parks rather than only when one is created: a journal
 * does not exist until its run has loaded, so a budget evaluated on subscribe
 * would always be measuring the state before the newest history arrived.
 */
function enforceRetentionBudget(): void {
	let budget = MAX_PARKED_EVENTS;
	for (const entry of [...entries.values()]
		.filter((candidate) => candidate.listeners.size === 0)
		.sort((left, right) => right.lastTouchedAt - left.lastTouchedAt)) {
		const size = entry.snapshot.events.length;
		if (size <= budget) {
			budget -= size;
			continue;
		}
		releaseJournal(entry);
	}
}

function emptySnapshot(runId: string, state: RunProgressConnectionState): RunProgressSnapshot {
	return {
		runId,
		state,
		run: null,
		events: [],
		cursor: 0,
		gap: false,
		evidence: "pending",
		revision: 0
	};
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
		const run = await withDeadline(() => api.get(runId), "ownership get");
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
			cursorState: emptyOptimizerEventCursor(),
			pending: Promise.resolve(),
			queueEpoch: 0,
			loadEpoch: 0,
			poll: null,
			unlisten: null,
			stopPolling: false,
			needsSnapshot: false,
			lastTouchedAt: Date.now(),
			disposed: true,
			stallTimer: null,
			retryTimer: null,
			consecutiveFailures: 0,
			pollBusy: false
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
		if (current.listeners.size === 0) {
			park(current);
			// The journal only exists once the run has loaded, so the budget is
			// meaningful here rather than at subscribe time.
			enforceRetentionBudget();
		}
	};
}

export function runSnapshot(runId: string): RunProgressSnapshot | undefined {
	return entries.get(runId)?.snapshot;
}

/** Live subscriber count for a run. Tests assert the sharing contract with it. */
export function runSubscriberCount(runId: string): number {
	return entries.get(runId)?.listeners.size ?? 0;
}

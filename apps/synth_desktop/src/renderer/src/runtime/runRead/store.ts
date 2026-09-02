/**
 * `RunReadStore` — the shared renderer query/cache layer over the backend
 * optimizer read model.
 *
 * Every live surface reads a run through this one store:
 *
 *   · the **summary** is the only thing an initial mount requests. It is
 *     small, byte-budgeted by the backend, and revalidated conditionally —
 *     the common answer to "anything new?" carries no payload;
 *   · **collection pages** (candidates, rollouts, evaluations, metric points,
 *     proposer calls, artifacts, evidence refs) are fetched on intent with an
 *     explicit limit and cached under a byte budget, not just an entry count;
 *   · **historical projections** come from the backend's checkpoint fold and
 *     are cached the same way;
 *   · notifications are coalesced to one newest-revision refresh per run, and
 *     only mounted pages refetch — parked ones are marked stale and refreshed
 *     when someone looks again.
 *
 * The transport is injectable for the same reason the run-progress store's
 * is: the rules here are testable in plain Node, without a webview.
 *
 * Cancellation: an invoke cannot be aborted, so "abortable" means a result
 * that lands after its reader left is dropped from the reader's view and
 * never published. It may still be cached if its key is still resident, which
 * is a cache-warming side effect, not a leak — the byte budget bounds it.
 */

import type {
	HistoricalProjection,
	OptimizerRunSummary,
	OptimizerRunSummaryEnvelope,
	RunCollection,
	RunCollectionPage,
	RunCollectionQuery,
	RunCollectionRow
} from "../../generated/protocol";
import { publicError } from "../publicError";

export type RunReadTransport = {
	runSummary(runId: string, ifNewerThan?: number | null): Promise<OptimizerRunSummaryEnvelope>;
	runCollection(runId: string, collection: RunCollection, query: RunCollectionQuery): Promise<RunCollectionPage>;
	runCollectionItem(runId: string, collection: RunCollection, itemId: string): Promise<RunCollectionRow | null>;
	projectionAt(runId: string, sequence: number): Promise<HistoricalProjection>;
	onEvent(listener: (event: { payload?: Record<string, unknown> }) => void): () => void;
};

export type RunReadStatus = "loading" | "ready" | "stale" | "error" | "unavailable";

export type RunSummaryState = {
	runId: string;
	status: RunReadStatus;
	summary: OptimizerRunSummary | null;
	/** Projection revision the summary describes; 0 before the first read. */
	revision: number;
	tailCursor: number;
	error?: string;
	/** Increments on every published change, for cheap memoization. */
	version: number;
};

export type RunCollectionState = {
	runId: string;
	collection: RunCollection;
	status: RunReadStatus;
	page: RunCollectionPage | null;
	/** The page is being shown from an older revision while a refresh runs. */
	stale: boolean;
	error?: string;
	version: number;
};

export type RunItemState = {
	status: RunReadStatus;
	row: RunCollectionRow | null;
	stale: boolean;
	error?: string;
	version: number;
};

/** Default budget for parked pages, items, and historical projections. */
export const DEFAULT_CACHE_BUDGET_BYTES = 8 * 1024 * 1024;
/** Summary revalidation cadence while a run is live and someone is mounted. */
const DEFAULT_POLL_INTERVAL_MS = 2_000;
/** Notification bursts collapse into one refresh per tick. */
const COALESCE_MS = 25;

type Listener<T> = (state: T) => void;

type SummaryEntry = {
	state: RunSummaryState;
	listeners: Set<Listener<RunSummaryState>>;
	unlisten: (() => void) | null;
	poll: ReturnType<typeof globalThis.setInterval> | null;
	coalesce: ReturnType<typeof globalThis.setTimeout> | null;
	inFlight: boolean;
	queued: boolean;
	epoch: number;
	terminal: boolean;
	lastTouchedAt: number;
};

type CacheEntry<T> = {
	key: string;
	runId: string;
	bytes: number;
	state: T;
	listeners: Set<Listener<T>>;
	epoch: number;
	lastTouchedAt: number;
	/** Revision the cached value was read at; used to decide staleness. */
	revision: number;
};

const summaries = new Map<string, SummaryEntry>();
const pages = new Map<string, CacheEntry<RunCollectionState>>();
const items = new Map<string, CacheEntry<RunItemState>>();
const histories = new Map<string, CacheEntry<{ status: RunReadStatus; projection: HistoricalProjection | null; error?: string; version: number }>>();

let injectedTransport: RunReadTransport | null = null;
let cacheBudgetBytes = DEFAULT_CACHE_BUDGET_BYTES;
let pollIntervalMs = DEFAULT_POLL_INTERVAL_MS;
let stats = { summaryReads: 0, unchangedProbes: 0, pageReads: 0, itemReads: 0, historyReads: 0, evictions: 0 };

/** Tests inject a transport; the app resolves the desktop bridge lazily. */
export function setRunReadTransport(transport: RunReadTransport | null): void {
	injectedTransport = transport;
}

export function setRunReadCacheBudget(bytes: number): void {
	cacheBudgetBytes = Math.max(0, bytes);
	enforceBudget();
}

export function setRunReadPollInterval(ms: number): void {
	pollIntervalMs = ms;
}

/** Tests only: drop everything and every timer. */
export function resetRunReadStore(): void {
	for (const entry of summaries.values()) deactivate(entry);
	summaries.clear();
	pages.clear();
	items.clear();
	histories.clear();
	stats = { summaryReads: 0, unchangedProbes: 0, pageReads: 0, itemReads: 0, historyReads: 0, evictions: 0 };
}

export function runReadStats(): {
	summaryReads: number;
	unchangedProbes: number;
	pageReads: number;
	itemReads: number;
	historyReads: number;
	evictions: number;
	residentBytes: number;
	residentEntries: number;
} {
	let residentBytes = 0;
	let residentEntries = 0;
	for (const entry of [...pages.values(), ...items.values(), ...histories.values()]) {
		residentBytes += entry.bytes;
		residentEntries += 1;
	}
	return { ...stats, residentBytes, residentEntries };
}

type BridgeShape = {
	runSummary?: RunReadTransport["runSummary"];
	runCollection?: RunReadTransport["runCollection"];
	runCollectionItem?: RunReadTransport["runCollectionItem"];
	projectionAt?: RunReadTransport["projectionAt"];
	onEvent?: RunReadTransport["onEvent"];
};

function transport(): RunReadTransport | null {
	if (injectedTransport) return injectedTransport;
	const bridge = (globalThis as { synthOptimizers?: BridgeShape }).synthOptimizers;
	if (!bridge || typeof bridge.runSummary !== "function" || typeof bridge.runCollection !== "function") return null;
	return {
		runSummary: (runId, ifNewerThan) => bridge.runSummary!(runId, ifNewerThan),
		runCollection: (runId, collection, query) => bridge.runCollection!(runId, collection, query),
		runCollectionItem: (runId, collection, itemId) =>
			typeof bridge.runCollectionItem === "function"
				? bridge.runCollectionItem(runId, collection, itemId)
				: Promise.reject(new Error("Collection item reads are unavailable")),
		projectionAt: (runId, sequence) =>
			typeof bridge.projectionAt === "function"
				? bridge.projectionAt(runId, sequence)
				: Promise.reject(new Error("Historical projections are unavailable")),
		onEvent: (listener) => (typeof bridge.onEvent === "function" ? bridge.onEvent(listener) : () => undefined)
	};
}

function byteLength(value: unknown): number {
	try {
		return JSON.stringify(value)?.length ?? 0;
	} catch {
		return 0;
	}
}

function deliver<T>(listeners: Set<Listener<T>>, state: T): void {
	for (const listener of [...listeners]) {
		try {
			listener(state);
		} catch {
			// One surface failing must not change what another shows.
		}
	}
}

function eventRunId(event: { payload?: Record<string, unknown> }): string | null {
	const payload = event.payload ?? {};
	const camel = payload.optimizerRunId;
	const snake = payload.optimizer_run_id;
	if (typeof camel === "string") return camel;
	if (typeof snake === "string") return snake;
	return null;
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

function emptySummary(runId: string, status: RunReadStatus): RunSummaryState {
	return { runId, status, summary: null, revision: 0, tailCursor: 0, version: 0 };
}

function publishSummary(entry: SummaryEntry, next: Partial<RunSummaryState>): void {
	entry.state = { ...entry.state, ...next, version: entry.state.version + 1 };
	deliver(entry.listeners, entry.state);
}

async function refreshSummary(entry: SummaryEntry, api: RunReadTransport): Promise<void> {
	if (entry.inFlight) {
		entry.queued = true;
		return;
	}
	entry.inFlight = true;
	const epoch = entry.epoch;
	try {
		const cached = entry.state.revision > 0 ? entry.state.revision : null;
		stats.summaryReads += 1;
		const envelope = await api.runSummary(entry.state.runId, cached);
		if (epoch !== entry.epoch) return;
		if (envelope.unchanged) {
			stats.unchangedProbes += 1;
			if (entry.state.status === "stale" || entry.state.status === "error") {
				publishSummary(entry, { status: "ready", error: undefined, tailCursor: envelope.tailCursor });
			} else if (envelope.tailCursor !== entry.state.tailCursor) {
				publishSummary(entry, { tailCursor: envelope.tailCursor });
			}
			return;
		}
		const summary = envelope.summary ?? null;
		const terminal = summary?.lifecycle === "terminal";
		entry.terminal = terminal;
		if (terminal) stopPolling(entry);
		const previousRevision = entry.state.revision;
		publishSummary(entry, {
			status: "ready",
			summary,
			revision: envelope.projectionRevision,
			tailCursor: envelope.tailCursor,
			error: undefined
		});
		if (envelope.projectionRevision !== previousRevision) {
			invalidateRun(entry.state.runId, envelope.projectionRevision, summary);
		}
	} catch (reason) {
		if (epoch !== entry.epoch) return;
		// A failed refresh keeps the last summary visible with an explicit
		// marker; it never blanks a card that already showed a number.
		publishSummary(entry, {
			status: entry.state.summary ? "stale" : "error",
			error: publicError(reason)
		});
	} finally {
		if (epoch === entry.epoch) {
			entry.inFlight = false;
			if (entry.queued) {
				entry.queued = false;
				void refreshSummary(entry, api);
			}
		}
	}
}

function scheduleRefresh(entry: SummaryEntry, api: RunReadTransport): void {
	// Bursts collapse into one newest-state read.
	if (entry.coalesce != null) return;
	entry.coalesce = globalThis.setTimeout(() => {
		entry.coalesce = null;
		void refreshSummary(entry, api);
	}, COALESCE_MS);
}

function stopPolling(entry: SummaryEntry): void {
	if (entry.poll != null) globalThis.clearInterval(entry.poll);
	entry.poll = null;
}

function activate(entry: SummaryEntry, api: RunReadTransport): void {
	entry.epoch += 1;
	entry.unlisten = api.onEvent((event) => {
		const id = eventRunId(event);
		if (!id || id === entry.state.runId) scheduleRefresh(entry, api);
	});
	if (!entry.terminal && pollIntervalMs > 0) {
		entry.poll = globalThis.setInterval(() => {
			if (entry.terminal) {
				stopPolling(entry);
				return;
			}
			void refreshSummary(entry, api);
		}, pollIntervalMs);
	}
	void refreshSummary(entry, api);
}

function deactivate(entry: SummaryEntry): void {
	entry.epoch += 1;
	entry.inFlight = false;
	entry.queued = false;
	entry.unlisten?.();
	entry.unlisten = null;
	stopPolling(entry);
	if (entry.coalesce != null) globalThis.clearTimeout(entry.coalesce);
	entry.coalesce = null;
}

/**
 * Subscribe to a run's bounded summary. The listener is called immediately
 * with the retained state and on every change after.
 */
export function subscribeRunSummary(runId: string, listener: Listener<RunSummaryState>): () => void {
	const api = transport();
	if (!api) {
		listener({ ...emptySummary(runId, "unavailable"), error: "Optimizer read model is unavailable" });
		return () => undefined;
	}
	let entry = summaries.get(runId);
	if (!entry) {
		entry = {
			state: emptySummary(runId, "loading"),
			listeners: new Set(),
			unlisten: null,
			poll: null,
			coalesce: null,
			inFlight: false,
			queued: false,
			epoch: 0,
			terminal: false,
			lastTouchedAt: Date.now()
		};
		summaries.set(runId, entry);
	}
	const wasIdle = entry.listeners.size === 0;
	entry.listeners.add(listener);
	entry.lastTouchedAt = Date.now();
	listener(entry.state);
	if (wasIdle) activate(entry, api);
	return () => {
		const current = summaries.get(runId);
		if (!current) return;
		current.listeners.delete(listener);
		current.lastTouchedAt = Date.now();
		if (current.listeners.size === 0) deactivate(current);
	};
}

export function runSummaryState(runId: string): RunSummaryState | undefined {
	return summaries.get(runId)?.state;
}

/** Ask for the newest summary now (after a control action, say). */
export function refreshRunSummary(runId: string): void {
	const api = transport();
	const entry = summaries.get(runId);
	if (api && entry && entry.listeners.size > 0) scheduleRefresh(entry, api);
}

// ---------------------------------------------------------------------------
// Invalidation
// ---------------------------------------------------------------------------

function changedCollections(summary: OptimizerRunSummary | null, sinceRevision: number): Set<RunCollection> | null {
	if (!summary) return null;
	const changed = new Set<RunCollection>();
	for (const item of summary.collections) {
		if (item.latestRevision > sinceRevision) changed.add(item.collection);
	}
	return changed;
}

/**
 * A new projection revision landed. Mounted pages of collections that
 * actually changed refetch; parked ones are marked stale so the next reader
 * refreshes them. A collection the summary says did not move keeps its page
 * and its status — nothing is refetched for no news.
 */
function invalidateRun(runId: string, revision: number, summary: OptimizerRunSummary | null): void {
	const api = transport();
	for (const entry of pages.values()) {
		if (entry.runId !== runId) continue;
		const changed = changedCollections(summary, entry.revision);
		if (changed && !changed.has(entry.state.collection)) continue;
		if (entry.listeners.size > 0 && api) {
			void loadPage(entry, api);
		} else {
			entry.state = { ...entry.state, stale: true, status: entry.state.page ? "stale" : entry.state.status, version: entry.state.version + 1 };
		}
	}
	for (const entry of items.values()) {
		if (entry.runId !== runId) continue;
		const changed = changedCollections(summary, entry.revision);
		const [, collection] = JSON.parse(entry.key) as [string, RunCollection, string];
		if (changed && !changed.has(collection)) continue;
		if (entry.listeners.size > 0 && api) {
			void loadItem(entry, api);
		} else {
			entry.state = { ...entry.state, stale: true, status: entry.state.row ? "stale" : entry.state.status, version: entry.state.version + 1 };
		}
	}
	void revision;
}

// ---------------------------------------------------------------------------
// Byte-bounded cache
// ---------------------------------------------------------------------------

function enforceBudget(): void {
	const resident = [...pages.values(), ...items.values(), ...histories.values()]
		.filter((entry) => entry.listeners.size === 0)
		.sort((left, right) => left.lastTouchedAt - right.lastTouchedAt);
	let total = 0;
	for (const entry of [...pages.values(), ...items.values(), ...histories.values()]) total += entry.bytes;
	for (const victim of resident) {
		if (total <= cacheBudgetBytes) break;
		total -= victim.bytes;
		stats.evictions += 1;
		pages.delete(victim.key);
		items.delete(victim.key);
		histories.delete(victim.key);
	}
}

function canonicalQuery(query: RunCollectionQuery): string {
	const filter = query.filter ?? {};
	return JSON.stringify({
		cursor: query.cursor ?? null,
		limit: query.limit ?? null,
		descending: query.descending === true,
		filter: {
			parentId: filter.parentId ?? null,
			label: filter.label ?? null,
			status: filter.status ?? null,
			kind: filter.kind ?? null,
			changedAfterRevision: filter.changedAfterRevision ?? null
		}
	});
}

function pageKey(runId: string, collection: RunCollection, query: RunCollectionQuery): string {
	return JSON.stringify([runId, collection, canonicalQuery(query)]);
}

/** Pure cache snapshot for React's external-store contract. Never starts I/O. */
export function runCollectionState(
	runId: string,
	collection: RunCollection,
	query: RunCollectionQuery
): RunCollectionState | undefined {
	return pages.get(pageKey(runId, collection, query))?.state;
}

async function loadPage(entry: CacheEntry<RunCollectionState>, api: RunReadTransport): Promise<void> {
	const epoch = ++entry.epoch;
	const [, , queryJson] = JSON.parse(entry.key) as [string, RunCollection, string];
	const query = JSON.parse(queryJson) as ReturnType<typeof JSON.parse>;
	const request: RunCollectionQuery = {
		cursor: query.cursor,
		limit: query.limit,
		descending: query.descending,
		filter: {
			parentId: query.filter.parentId,
			label: query.filter.label,
			status: query.filter.status,
			kind: query.filter.kind,
			changedAfterRevision: query.filter.changedAfterRevision
		}
	};
	if (entry.state.page) {
		entry.state = { ...entry.state, stale: true, status: "stale", version: entry.state.version + 1 };
		deliver(entry.listeners, entry.state);
	}
	try {
		stats.pageReads += 1;
		const page = await api.runCollection(entry.runId, entry.state.collection, request);
		// The reader left, or a newer load superseded this one: drop it.
		if (epoch !== entry.epoch || !pages.has(entry.key)) return;
		entry.bytes = byteLength(page);
		entry.revision = page.projectionRevision;
		entry.state = { ...entry.state, status: "ready", page, stale: false, error: undefined, version: entry.state.version + 1 };
		deliver(entry.listeners, entry.state);
		enforceBudget();
	} catch (reason) {
		if (epoch !== entry.epoch || !pages.has(entry.key)) return;
		entry.state = {
			...entry.state,
			status: entry.state.page ? "stale" : "error",
			stale: entry.state.page != null,
			error: publicError(reason),
			version: entry.state.version + 1
		};
		deliver(entry.listeners, entry.state);
	}
}

/**
 * Subscribe to one page of a collection. Identical queries from a card, a
 * dialog, and a visual share one cached page and one read.
 */
export function subscribeRunCollection(
	runId: string,
	collection: RunCollection,
	query: RunCollectionQuery,
	listener: Listener<RunCollectionState>
): () => void {
	const api = transport();
	const key = pageKey(runId, collection, query);
	if (!api) {
		listener({ runId, collection, status: "unavailable", page: null, stale: false, error: "Optimizer read model is unavailable", version: 0 });
		return () => undefined;
	}
	let entry = pages.get(key);
	const fresh = !entry;
	if (!entry) {
		entry = {
			key,
			runId,
			bytes: 0,
			state: { runId, collection, status: "loading", page: null, stale: false, version: 0 },
			listeners: new Set(),
			epoch: 0,
			lastTouchedAt: Date.now(),
			revision: 0
		};
		pages.set(key, entry);
	}
	entry.listeners.add(listener);
	entry.lastTouchedAt = Date.now();
	listener(entry.state);
	const summaryRevision = summaries.get(runId)?.state.revision ?? 0;
	if (fresh || entry.state.stale || entry.state.status === "error" || (summaryRevision > entry.revision && entry.revision > 0)) {
		void loadPage(entry, api);
	}
	return () => {
		const current = pages.get(key);
		if (!current) return;
		current.listeners.delete(listener);
		current.lastTouchedAt = Date.now();
		if (current.listeners.size === 0) {
			// Abandon the in-flight read's publication; the entry stays parked
			// under the byte budget for a quick reopen.
			current.epoch += 1;
			enforceBudget();
		}
	};
}

function itemKey(runId: string, collection: RunCollection, itemId: string): string {
	return JSON.stringify([runId, collection, itemId]);
}

/** Pure cache snapshot for React's external-store contract. Never starts I/O. */
export function runCollectionItemState(
	runId: string,
	collection: RunCollection,
	itemId: string
): RunItemState | undefined {
	return items.get(itemKey(runId, collection, itemId))?.state;
}

async function loadItem(entry: CacheEntry<RunItemState>, api: RunReadTransport): Promise<void> {
	const epoch = ++entry.epoch;
	const [runId, collection, itemId] = JSON.parse(entry.key) as [string, RunCollection, string];
	if (entry.state.row) {
		entry.state = { ...entry.state, stale: true, status: "stale", version: entry.state.version + 1 };
		deliver(entry.listeners, entry.state);
	}
	try {
		stats.itemReads += 1;
		const row = await api.runCollectionItem(runId, collection, itemId);
		if (epoch !== entry.epoch || !items.has(entry.key)) return;
		entry.bytes = byteLength(row);
		entry.revision = row?.revision ?? summaries.get(runId)?.state.revision ?? 0;
		entry.state = { status: "ready", row, stale: false, error: undefined, version: entry.state.version + 1 };
		deliver(entry.listeners, entry.state);
		enforceBudget();
	} catch (reason) {
		if (epoch !== entry.epoch || !items.has(entry.key)) return;
		entry.state = {
			...entry.state,
			status: entry.state.row ? "stale" : "error",
			stale: entry.state.row != null,
			error: publicError(reason),
			version: entry.state.version + 1
		};
		deliver(entry.listeners, entry.state);
	}
}

export function subscribeRunCollectionItem(
	runId: string,
	collection: RunCollection,
	itemId: string,
	listener: Listener<RunItemState>
): () => void {
	const api = transport();
	const key = itemKey(runId, collection, itemId);
	if (!api) {
		listener({ status: "unavailable", row: null, stale: false, error: "Optimizer read model is unavailable", version: 0 });
		return () => undefined;
	}
	let entry = items.get(key);
	const fresh = !entry;
	if (!entry) {
		entry = {
			key,
			runId,
			bytes: 0,
			state: { status: "loading", row: null, stale: false, version: 0 },
			listeners: new Set(),
			epoch: 0,
			lastTouchedAt: Date.now(),
			revision: 0
		};
		items.set(key, entry);
	}
	entry.listeners.add(listener);
	entry.lastTouchedAt = Date.now();
	listener(entry.state);
	if (fresh || entry.state.stale || entry.state.status === "error") void loadItem(entry, api);
	return () => {
		const current = items.get(key);
		if (!current) return;
		current.listeners.delete(listener);
		current.lastTouchedAt = Date.now();
		if (current.listeners.size === 0) {
			current.epoch += 1;
			enforceBudget();
		}
	};
}

export type HistoryState = { status: RunReadStatus; projection: HistoricalProjection | null; error?: string; version: number };

function historyKey(runId: string, sequence: number): string {
	return JSON.stringify([runId, "history", Math.max(0, Math.floor(sequence))]);
}

/** Pure cache snapshot for React's external-store contract. Never starts I/O. */
export function projectionAtState(runId: string, sequence: number): HistoryState | undefined {
	return histories.get(historyKey(runId, sequence))?.state;
}

/**
 * The projection at `sequence`, from the backend checkpoint fold. Terminal
 * history never changes, so a cached answer is reused as long as the budget
 * keeps it; a live run's answer at a sequence already durable is equally
 * immutable, which is what makes scrubbing back and forth cheap.
 */
export function subscribeProjectionAt(runId: string, sequence: number, listener: Listener<HistoryState>): () => void {
	const api = transport();
	const key = historyKey(runId, sequence);
	if (!api) {
		listener({ status: "unavailable", projection: null, error: "Historical projections are unavailable", version: 0 });
		return () => undefined;
	}
	let entry = histories.get(key);
	const fresh = !entry;
	if (!entry) {
		entry = {
			key,
			runId,
			bytes: 0,
			state: { status: "loading", projection: null, version: 0 },
			listeners: new Set(),
			epoch: 0,
			lastTouchedAt: Date.now(),
			revision: 0
		};
		histories.set(key, entry);
	}
	const current = entry;
	current.listeners.add(listener);
	current.lastTouchedAt = Date.now();
	listener(current.state);
	if (fresh || current.state.status === "error") {
		const epoch = ++current.epoch;
		stats.historyReads += 1;
		void api
			.projectionAt(runId, Math.max(0, Math.floor(sequence)))
			.then((projection) => {
				if (epoch !== current.epoch || !histories.has(key)) return;
				current.bytes = byteLength(projection);
				current.state = { status: "ready", projection, error: undefined, version: current.state.version + 1 };
				deliver(current.listeners, current.state);
				enforceBudget();
			})
			.catch((reason) => {
				if (epoch !== current.epoch || !histories.has(key)) return;
				current.state = { status: "error", projection: null, error: publicError(reason), version: current.state.version + 1 };
				deliver(current.listeners, current.state);
			});
	}
	return () => {
		const parked = histories.get(key);
		if (!parked) return;
		parked.listeners.delete(listener);
		parked.lastTouchedAt = Date.now();
		if (parked.listeners.size === 0) {
			parked.epoch += 1;
			enforceBudget();
		}
	};
}

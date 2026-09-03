/**
 * The shared optimizer read hooks. Every surface that shows a run — card,
 * dialog, training workspace, visual host — reads through these, and none of
 * them mounts with more than the bounded summary.
 *
 *   useOptimizerRun(runId)                    → bounded summary, live
 *   useRunCollection(runId, "candidates", …)  → one explicit page
 *   useRunCollectionItem(runId, coll, itemId) → one row's durable detail
 *   useRunMetricSeries(runId, …)              → the downsampled curve page
 *   useProjectionAt(runId, sequence)          → backend historical projection
 *
 * Selectors keep unrelated summary changes from re-rendering every detail
 * panel: a hook given `select` only re-renders when the selected value
 * changes by `Object.is`.
 */

import { useCallback, useMemo, useRef, useSyncExternalStore } from "react";
import type {
	OptimizerRunSummary,
	RunCollection,
	RunCollectionQuery
} from "../generated/protocol";
import {
	refreshRunSummary,
	projectionAtState,
	runCollectionItemState,
	runCollectionState,
	runSummaryState,
	subscribeProjectionAt,
	subscribeRunCollection,
	subscribeRunCollectionItem,
	subscribeRunSummary,
	type HistoryState,
	type RunCollectionState,
	type RunItemState,
	type RunSummaryState
} from "../runtime/runRead/store";

export type { HistoryState, RunCollectionState, RunItemState, RunSummaryState };

/** Subscribe with a selector; re-render only when the selection changes. */
function useSelected<S, T>(
	subscribe: (listener: (state: S) => void) => () => void,
	getSnapshot: () => S,
	select: (state: S) => T,
	deps: readonly unknown[]
): T {
	const selectRef = useRef(select);
	selectRef.current = select;
	const subscribeExternal = useCallback(
		(notify: () => void) => subscribe(() => notify()),
		// eslint-disable-next-line react-hooks/exhaustive-deps
		deps
	);
	const selectedSnapshot = useCallback(() => selectRef.current(getSnapshot()),
		// eslint-disable-next-line react-hooks/exhaustive-deps
		deps
	);
	return useSyncExternalStore(subscribeExternal, selectedSnapshot, selectedSnapshot);
}

const identity = <T,>(value: T): T => value;

/**
 * The run's bounded summary, kept live. This is the only read an ordinary
 * mount performs; nothing here touches the raw journal.
 */
export function useOptimizerRun<T = RunSummaryState>(
	runId: string | null | undefined,
	select: (state: RunSummaryState) => T = identity as (state: RunSummaryState) => T
): T {
	const fallback = useMemo<RunSummaryState>(
		() => ({ runId: runId ?? "", status: runId ? "loading" : "unavailable", summary: null, revision: 0, tailCursor: 0, version: 0 }),
		[runId]
	);
	return useSelected<RunSummaryState, T>(
		(listener) => {
			if (!runId) {
				listener({ runId: "", status: "unavailable", summary: null, revision: 0, tailCursor: 0, version: 0 });
				return () => undefined;
			}
			return subscribeRunSummary(runId, listener);
		},
		() => (runId ? runSummaryState(runId) ?? fallback : fallback),
		select,
		[runId]
	);
}

/** Just the summary, or null. Convenience over `useOptimizerRun`. */
export function useRunSummary(runId: string | null | undefined): OptimizerRunSummary | null {
	return useOptimizerRun(runId, (state) => state.summary);
}

export type UseRunCollectionOptions = RunCollectionQuery & {
	/** Skip the subscription entirely (a collapsed panel, say). */
	enabled?: boolean;
};

function canonical(query: RunCollectionQuery): string {
	const filter = query.filter ?? {};
	return [
		query.cursor ?? "",
		query.limit ?? "",
		query.descending ? "d" : "a",
		filter.parentId ?? "",
		filter.label ?? "",
		filter.status ?? "",
		filter.kind ?? "",
		filter.changedAfterRevision ?? ""
	].join("|");
}

/**
 * One explicit page of a collection. The limit is always sent; the backend
 * clamps it to its ceiling and there is no unbounded form.
 */
export function useRunCollection(
	runId: string | null | undefined,
	collection: RunCollection,
	options: UseRunCollectionOptions = {}
): RunCollectionState {
	const { enabled = true, ...query } = options;
	const key = canonical(query);
	const request = useMemo<RunCollectionQuery>(
		() => ({
			cursor: query.cursor ?? null,
			limit: query.limit ?? null,
			descending: query.descending ?? false,
			filter: query.filter ?? null
		}),
		// eslint-disable-next-line react-hooks/exhaustive-deps
		[key]
	);
	const fallback = useMemo<RunCollectionState>(
		() => ({ runId: runId ?? "", collection, status: enabled && runId ? "loading" : "unavailable", page: null, stale: false, version: 0 }),
		[runId, collection, enabled]
	);
	return useSelected<RunCollectionState, RunCollectionState>(
		(listener) => {
			if (!runId || !enabled) {
				listener({ runId: runId ?? "", collection, status: enabled ? "unavailable" : "loading", page: null, stale: false, version: 0 });
				return () => undefined;
			}
			return subscribeRunCollection(runId, collection, request, listener);
		},
		() => (runId && enabled ? runCollectionState(runId, collection, request) ?? fallback : fallback),
		identity,
		[runId, collection, key, enabled]
	);
}

export function useRunCollectionItem(
	runId: string | null | undefined,
	collection: RunCollection,
	itemId: string | null | undefined
): RunItemState {
	const fallback = useMemo<RunItemState>(
		() => ({ status: runId && itemId ? "loading" : "unavailable", row: null, stale: false, version: 0 }),
		[runId, itemId]
	);
	return useSelected<RunItemState, RunItemState>(
		(listener) => {
			if (!runId || !itemId) {
				listener({ status: "loading", row: null, stale: false, version: 0 });
				return () => undefined;
			}
			return subscribeRunCollectionItem(runId, collection, itemId, listener);
		},
		() => (runId && itemId ? runCollectionItemState(runId, collection, itemId) ?? fallback : fallback),
		identity,
		[runId, collection, itemId]
	);
}

/**
 * The downsampled metric curve, newest first by default so a live chart's
 * first page is the part that is moving. Full resolution is an explicit
 * further fetch by cursor, never the default.
 */
export function useRunMetricSeries(
	runId: string | null | undefined,
	options: UseRunCollectionOptions = {}
): RunCollectionState {
	return useRunCollection(runId, "metric_points", { descending: true, limit: 100, ...options });
}

/** The backend's projection at `sequence`; `null` sequence means "not scrubbing". */
export function useProjectionAt(runId: string | null | undefined, sequence: number | null | undefined): HistoryState {
	const fallback = useMemo<HistoryState>(
		() => ({ status: runId && sequence != null ? "loading" : "unavailable", projection: null, version: 0 }),
		[runId, sequence]
	);
	return useSelected<HistoryState, HistoryState>(
		(listener) => {
			if (!runId || sequence == null) {
				listener({ status: "loading", projection: null, version: 0 });
				return () => undefined;
			}
			return subscribeProjectionAt(runId, sequence, listener);
		},
		() => (runId && sequence != null ? projectionAtState(runId, sequence) ?? fallback : fallback),
		identity,
		[runId, sequence]
	);
}

/** Ask the store to revalidate a run's summary now. */
export function useRefreshRun(runId: string | null | undefined): () => void {
	return useMemo(() => () => { if (runId) refreshRunSummary(runId); }, [runId]);
}

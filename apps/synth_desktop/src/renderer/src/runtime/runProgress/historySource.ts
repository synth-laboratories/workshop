/**
 * Fetching the recipe history a run is estimated against.
 *
 * Each finished run carries its own traced shape in its summary, so this needs
 * one `list()` call per recipe rather than any event replay: the curves ride
 * along with the run records that already come back. The result is cached per
 * recipe for the session, because a completed run's shape never changes.
 *
 * A cache miss returns null rather than blocking. The card renders its honest
 * "no comparable run yet" state, and the next projection picks the shape up once
 * the fetch lands — which is the same behaviour as any other late-arriving fact.
 */

import { historicalShape, recipeKeyOf, type HistoricalShape } from "./history";
import type { RunRecord } from "./subscription";

type Entry = {
	shape: HistoricalShape | null;
	/** Units the shape was pooled for, so a differently-sized run re-pools. */
	units?: number;
};

const cache = new Map<string, Entry>();
const inFlight = new Set<string>();

/** Tests only. */
export function resetRunHistoryCache(): void {
	cache.clear();
	inFlight.clear();
}

function cacheKey(recipe: string, units?: number): string {
	return `${recipe}::${units ?? "any"}`;
}

/** The transport this reads through, injectable for tests. */
export type HistorySource = {
	list(query: { algorithmId?: string; status?: string }): Promise<RunRecord[]>;
};

let injected: HistorySource | null = null;

export function setRunHistorySource(source: HistorySource | null): void {
	injected = source;
}

function source(): HistorySource | null {
	if (injected) return injected;
	const bridge = (globalThis as { synthOptimizers?: HistorySource }).synthOptimizers;
	return bridge ?? null;
}

/**
 * The pooled shape for this run, if it is already known. Synchronous by design:
 * a projection must never await, so this reads the cache and returns null while
 * a fetch is outstanding.
 */
export function cachedShapeFor(run: RunRecord, expectedUnits?: number): HistoricalShape | null {
	return cache.get(cacheKey(recipeKeyOf(run), expectedUnits))?.shape ?? null;
}

/**
 * Ensure the shape for this run's recipe is fetched. Resolves to the shape, and
 * de-duplicates concurrent callers so five cards on one recipe make one request.
 */
export async function ensureShapeFor(
	run: RunRecord,
	expectedUnits?: number
): Promise<HistoricalShape | null> {
	const key = cacheKey(recipeKeyOf(run), expectedUnits);
	const known = cache.get(key);
	if (known) return known.shape;
	if (inFlight.has(key)) return null;
	const api = source();
	if (!api) return null;
	inFlight.add(key);
	try {
		// Only completed runs seal a curve, so only completed runs are worth asking
		// for; a failed run stopped early for reasons that teach nothing.
		const peers = await api.list({ algorithmId: run.algorithmId, status: "completed" });
		const shape = historicalShape(run, Array.isArray(peers) ? peers : [], expectedUnits);
		cache.set(key, { shape, units: expectedUnits });
		return shape;
	} catch {
		// A history lookup that fails costs an estimate, never a card.
		cache.set(key, { shape: null, units: expectedUnits });
		return null;
	} finally {
		inFlight.delete(key);
	}
}

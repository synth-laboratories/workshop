/**
 * `RunReadStore` — the shared renderer read-model contract.
 *
 * Initial mount requests only the bounded summary; pages are explicit and
 * bounded; identical queries share one read; notifications coalesce to one
 * newest-revision refresh; the cache is bounded by bytes; a reader that
 * leaves never sees a late answer; stale data stays visible with a marker.
 */
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const outfile = join(compiledDir, "runReadStore.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/runRead/store.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile
});
const {
	resetRunReadStore,
	runReadStats,
	setRunReadCacheBudget,
	setRunReadPollInterval,
	setRunReadTransport,
	subscribeProjectionAt,
	subscribeRunCollection,
	subscribeRunCollectionItem,
	subscribeRunSummary
} = await import(pathToFileURL(outfile).href);

setRunReadPollInterval(0);

const tick = (ms = 60) => new Promise((resolve) => setTimeout(resolve, ms));

function summaryOf(runId, revision, counts = {}) {
	return {
		schemaVersion: "optimizer_run_summary.v1",
		runId,
		algorithm: "gepa",
		status: "running",
		lifecycle: "running",
		projectionRevision: revision,
		asOfSequence: revision * 10,
		tailCursor: revision * 10,
		collections: Object.entries({ candidates: 0, rollouts: 0, evaluations: 0, metric_points: 0, proposer_calls: 0, artifacts: 0, evidence_refs: 0, ...counts })
			.map(([collection, latestRevision]) => ({ collection, count: 1, latestRevision })),
		work: {},
		usage: {},
		budget: { bytes: 512, limit: 65536, within: true }
	};
}

function fakeTransport() {
	const calls = { summary: [], collection: [], item: [], history: [] };
	const listeners = new Set();
	let revision = 1;
	let counts = {};
	let rowsByCollection = { candidates: Array.from({ length: 12 }, (_, index) => ({ itemId: `cand_${index}`, ordinal: index, revision: 1, details: { prompt: "x".repeat(200) } })) };
	const transport = {
		calls,
		listeners,
		setRevision(next, changed = {}) { revision = next; counts = changed; },
		setRows(collection, rows) { rowsByCollection[collection] = rows; },
		emit(runId) { for (const listener of [...listeners]) listener({ payload: { optimizerRunId: runId } }); },
		async runSummary(runId, ifNewerThan) {
			calls.summary.push([runId, ifNewerThan ?? null]);
			if (ifNewerThan != null && ifNewerThan === revision) {
				return { unchanged: true, projectionRevision: revision, tailCursor: revision * 10 };
			}
			return { unchanged: false, projectionRevision: revision, tailCursor: revision * 10, summary: summaryOf(runId, revision, counts) };
		},
		async runCollection(runId, collection, query) {
			calls.collection.push([runId, collection, query]);
			const limit = Math.min(query.limit ?? 50, 100);
			const rows = (rowsByCollection[collection] ?? []).map((row) => ({ ...row, runId, collection }));
			const after = query.cursor ? Number(query.cursor.slice(4)) : -1;
			const remaining = rows.filter((row) => row.ordinal > after);
			const page = remaining.slice(0, limit);
			return {
				runId, collection, rows: page, total: rows.length,
				nextCursor: remaining.length > limit ? `ord:${page[page.length - 1].ordinal}` : undefined,
				projectionRevision: revision, asOfSequence: revision * 10, truncatedByBytes: false, limit
			};
		},
		async runCollectionItem(runId, collection, itemId) {
			calls.item.push([runId, collection, itemId]);
			return (rowsByCollection[collection] ?? []).find((row) => row.itemId === itemId) ?? null;
		},
		async projectionAt(runId, sequence) {
			calls.history.push([runId, sequence]);
			return { schemaVersion: "optimizer_historical_projection.v1", runId, requestedSequence: sequence, asOfSequence: sequence, replayedEvents: 3, view: { algorithm: "gepa", header: {}, projection: { big: "y".repeat(4000) } } };
		},
		onEvent(listener) { listeners.add(listener); return () => listeners.delete(listener); }
	};
	return transport;
}

test.beforeEach(() => {
	resetRunReadStore();
	setRunReadCacheBudget(8 * 1024 * 1024);
});

test("an initial mount reads the bounded summary and nothing else", async () => {
	const transport = fakeTransport();
	setRunReadTransport(transport);
	const states = [];
	const unsubscribe = subscribeRunSummary("run-a", (state) => states.push(state));
	await tick();
	assert.equal(states[0].status, "loading");
	assert.equal(states.at(-1).status, "ready");
	assert.equal(states.at(-1).revision, 1);
	assert.deepEqual(transport.calls.summary, [["run-a", null]]);
	assert.equal(transport.calls.collection.length, 0, "no collection is paged without intent");
	assert.equal(transport.calls.history.length, 0);
	unsubscribe();
});

test("a notification storm coalesces to one conditional revalidation", async () => {
	const transport = fakeTransport();
	setRunReadTransport(transport);
	const unsubscribe = subscribeRunSummary("run-a", () => undefined);
	await tick();
	transport.calls.summary.length = 0;
	for (let index = 0; index < 25; index += 1) transport.emit("run-a");
	await tick();
	assert.equal(transport.calls.summary.length, 1, "twenty-five wakeups became one read");
	assert.deepEqual(transport.calls.summary[0], ["run-a", 1], "the held revision travels with the probe");
	assert.equal(runReadStats().unchangedProbes, 1);
	unsubscribe();
});

test("collection pages are explicit, bounded, shared, and refreshed only when their collection moved", async () => {
	const transport = fakeTransport();
	setRunReadTransport(transport);
	const summary = subscribeRunSummary("run-a", () => undefined);
	await tick();
	const seenA = [];
	const seenB = [];
	const query = { limit: 10, filter: { parentId: "cand_0" } };
	const a = subscribeRunCollection("run-a", "candidates", query, (state) => seenA.push(state));
	const b = subscribeRunCollection("run-a", "candidates", { ...query }, (state) => seenB.push(state));
	await tick();
	assert.equal(transport.calls.collection.length, 1, "two identical readers share one page read");
	assert.equal(seenA.at(-1).page.rows.length, 10);
	assert.equal(seenA.at(-1).page.nextCursor, "ord:9");
	assert.equal(seenB.at(-1).page, seenA.at(-1).page);

	// A revision that changed only rollouts leaves the candidates page alone.
	transport.setRevision(2, { rollouts: 2 });
	transport.emit("run-a");
	await tick();
	assert.equal(transport.calls.collection.length, 1, "an unrelated revision does not refetch the page");
	assert.equal(seenA.at(-1).status, "ready");

	// A revision that changed candidates refetches the mounted page, keeping
	// the old rows visible with the stale marker in between.
	transport.setRevision(3, { candidates: 3 });
	transport.emit("run-a");
	await tick();
	assert.equal(transport.calls.collection.length, 2);
	assert.ok(seenA.some((state) => state.stale && state.page?.rows.length === 10), "stale rows stayed visible during the refresh");
	assert.equal(seenA.at(-1).stale, false);
	assert.equal(seenA.at(-1).page.projectionRevision, 3);
	a();
	b();
	summary();
});

test("a reader that leaves never sees a late answer, and parked pages stay under the byte budget", async () => {
	const transport = fakeTransport();
	let release;
	transport.runCollection = (...args) => new Promise((resolve) => { release = () => resolve({ runId: args[0], collection: args[1], rows: [], total: 0, projectionRevision: 1, asOfSequence: 1, truncatedByBytes: false, limit: 5 }); });
	setRunReadTransport(transport);
	const seen = [];
	const unsubscribe = subscribeRunCollection("run-a", "rollouts", { limit: 5 }, (state) => seen.push(state));
	unsubscribe();
	release();
	await tick();
	assert.equal(seen.length, 1, "only the synchronous loading state was delivered");

	// Byte budget: parked pages beyond it are evicted oldest-first.
	setRunReadCacheBudget(6_000);
	const transport2 = fakeTransport();
	setRunReadTransport(transport2);
	for (let index = 0; index < 6; index += 1) {
		const off = subscribeRunCollection("run-b", "candidates", { limit: 12, filter: { label: `page-${index}` } }, () => undefined);
		await tick();
		off();
	}
	const stats = runReadStats();
	assert.ok(stats.residentBytes <= 6_000, `resident ${stats.residentBytes} bytes exceeds the budget`);
	assert.ok(stats.evictions > 0, "eviction actually happened");
});

test("items and historical projections load on intent and are cached", async () => {
	const transport = fakeTransport();
	setRunReadTransport(transport);
	const item = [];
	const offItem = subscribeRunCollectionItem("run-a", "candidates", "cand_3", (state) => item.push(state));
	await tick();
	assert.equal(item.at(-1).status, "ready");
	assert.equal(item.at(-1).row.itemId, "cand_3");
	offItem();
	const again = [];
	const offAgain = subscribeRunCollectionItem("run-a", "candidates", "cand_3", (state) => again.push(state));
	assert.equal(again[0].row.itemId, "cand_3", "a parked item answers synchronously from cache");
	assert.equal(transport.calls.item.length, 1);
	offAgain();

	const history = [];
	const offHistory = subscribeProjectionAt("run-a", 4242, (state) => history.push(state));
	await tick();
	assert.equal(history.at(-1).status, "ready");
	assert.equal(history.at(-1).projection.asOfSequence, 4242);
	offHistory();
	const cached = [];
	subscribeProjectionAt("run-a", 4242, (state) => cached.push(state))();
	assert.equal(cached[0].status, "ready");
	assert.deepEqual(transport.calls.history, [["run-a", 4242]], "a scrub back to a seen sequence costs nothing");
});

test("without a read-model bridge the state is unavailable, not a crash", () => {
	setRunReadTransport(null);
	const seen = [];
	subscribeRunSummary("run-x", (state) => seen.push(state))();
	assert.equal(seen[0].status, "unavailable");
	const page = [];
	subscribeRunCollection("run-x", "rollouts", { limit: 5 }, (state) => page.push(state))();
	assert.equal(page[0].status, "unavailable");
});

test("a failed refresh keeps the last summary visible with a stale marker", async () => {
	const transport = fakeTransport();
	setRunReadTransport(transport);
	const seen = [];
	const unsubscribe = subscribeRunSummary("run-a", (state) => seen.push(state));
	await tick();
	assert.equal(seen.at(-1).status, "ready");
	transport.runSummary = async () => { throw new Error("bridge went away"); };
	transport.emit("run-a");
	await tick();
	assert.equal(seen.at(-1).status, "stale");
	assert.equal(seen.at(-1).summary.runId, "run-a", "the number already shown is not blanked");
	assert.match(seen.at(-1).error, /bridge went away/);
	unsubscribe();
});

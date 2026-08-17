/**
 * `RunProgressSubscription` — the transport contract.
 *
 * These are the acceptance tests that do not need a webview: one subscription
 * per run however many surfaces read it, terminal authority from the run
 * record, gap recovery instead of a silently wrong count, restart recovery from
 * the durable pages, and cross-run isolation.
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

const outfile = join(compiledDir, "runProgressSubscription.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/runProgress/subscription.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile
});
const {
	installRunProgressDiagnostics,
	resetRunProgressStore,
	resolveOwnedRun,
	runSubscriberCount,
	setRunProgressPollInterval,
	setRunProgressStallTimeout,
	setRunProgressTransport,
	subscribeToRun
} = await import(pathToFileURL(outfile).href);

setRunProgressPollInterval(3_600_000);

/** A fake durable store: a run record plus persisted event pages by sequence. */
function fakeTransport({ runs, pages, onEventsAfter } = {}) {
	const calls = { get: 0, eventsAfter: 0, refresh: 0, listeners: 0 };
	// The real bridge broadcasts every optimizer notification to every listener;
	// a fake that keeps only the newest one would hide cross-run misrouting.
	const listeners = new Set();
	const transport = {
		calls,
		emit: (runId) => {
			for (const listener of [...listeners]) listener({ payload: { optimizerRunId: runId } });
		},
		setRun: (runId, next) => { runs[runId] = next; },
		setPages: (runId, next) => { pages[runId] = next; },
		async get(runId) {
			calls.get += 1;
			const run = runs[runId];
			if (!run) throw new Error(`no run ${runId}`);
			return run;
		},
		async eventsAfter(runId, afterSeq = 0) {
			calls.eventsAfter += 1;
			onEventsAfter?.(runId, afterSeq);
			return (pages[runId] ?? []).filter((event) => event.sequenceNumber > afterSeq);
		},
		async refresh(runId) {
			calls.refresh += 1;
			return runs[runId];
		},
		onEvent(listener) {
			calls.listeners += 1;
			listeners.add(listener);
			return () => { calls.listeners -= 1; listeners.delete(listener); };
		}
	};
	return transport;
}

function event(sequence, type = "optimizer.evaluation_result.received") {
	return {
		schemaVersion: "optimizer_event.v1",
		eventId: `e${sequence}`,
		type,
		sequenceNumber: sequence,
		occurredAt: new Date(Date.UTC(2026, 7, 17, 12, 0, sequence)).toISOString(),
		optimizerRunId: "run-a",
		algorithmId: "gepa"
	};
}

function runRecord(overrides = {}) {
	return {
		schemaVersion: "optimizer_run.v1",
		id: "run-a",
		algorithmId: "gepa",
		status: "running",
		sessionRef: "sess-1",
		cursorSeq: 3,
		capabilities: { cancel: true },
		...overrides
	};
}

/** Resolve after the store's internal promise chain drains. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 5));

test.beforeEach(() => {
	resetRunProgressStore();
	installRunProgressDiagnostics(() => undefined);
});

test.after(() => {
	resetRunProgressStore();
	setRunProgressTransport(null);
	setRunProgressStallTimeout(15_000);
});

test("a run history replays once and reaches subscribed with the run's cursor", async () => {
	const transport = fakeTransport({
		runs: { "run-a": runRecord() },
		pages: { "run-a": [event(1), event(2), event(3)] }
	});
	setRunProgressTransport(transport);
	const seen = [];
	subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	const last = seen.at(-1);
	assert.equal(last.state, "subscribed");
	assert.equal(last.cursor, 3);
	assert.equal(last.events.length, 3);
	assert.equal(last.gap, false);
});

test("three surfaces on one run share a single subscription and one read pass", async () => {
	const transport = fakeTransport({
		runs: { "run-a": runRecord() },
		pages: { "run-a": [event(1), event(2), event(3)] }
	});
	setRunProgressTransport(transport);
	subscribeToRun("run-a", () => undefined);
	await settle();
	const readsAfterFirst = transport.calls.eventsAfter;
	// The card's dialog and the full visual join the same run.
	subscribeToRun("run-a", () => undefined);
	subscribeToRun("run-a", () => undefined);
	await settle();
	assert.equal(runSubscriberCount("run-a"), 3);
	assert.equal(transport.calls.listeners, 1, "one upstream event listener, not three");
	assert.equal(
		transport.calls.eventsAfter,
		readsAfterFirst,
		"joining an active subscription must not re-read the history"
	);
});

test("a dialog joining a live run adds no read at all", async () => {
	const reads = [];
	const transport = fakeTransport({
		runs: { "run-a": runRecord() },
		pages: { "run-a": [event(1), event(2), event(3)] },
		onEventsAfter: (_runId, afterSeq) => reads.push(afterSeq)
	});
	setRunProgressTransport(transport);
	const card = subscribeToRun("run-a", () => undefined);
	await settle();
	const before = reads.length;
	const dialog = subscribeToRun("run-a", () => undefined);
	await settle();
	assert.equal(reads.length, before, "opening a dialog over a live card must not re-read");
	dialog();
	await settle();
	assert.equal(reads.length, before, "closing it must not re-read either");
	card();
});

test("a fully parked run resumes from its cursor rather than replaying from zero", async () => {
	const reads = [];
	const transport = fakeTransport({
		runs: { "run-a": runRecord() },
		pages: { "run-a": [event(1), event(2), event(3)] },
		onEventsAfter: (_runId, afterSeq) => reads.push(afterSeq)
	});
	setRunProgressTransport(transport);
	const card = subscribeToRun("run-a", () => undefined);
	await settle();
	card();
	reads.length = 0;
	// Reopening the conversation re-mounts the card over the retained cursor.
	const seen = [];
	const reopened = subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	assert.ok(reads.length > 0, "a re-subscribe does read");
	assert.ok(
		reads.every((afterSeq) => afterSeq >= 3),
		`a parked run must resume from the cursor, not from 0 (saw ${reads})`
	);
	assert.equal(seen.at(-1).events.length, 3, "the retained history is still there");
	reopened();
});

test("the last unsubscribe parks the subscription and its upstream listener", async () => {
	const transport = fakeTransport({
		runs: { "run-a": runRecord() },
		pages: { "run-a": [event(1)] }
	});
	setRunProgressTransport(transport);
	const stop = subscribeToRun("run-a", () => undefined);
	await settle();
	assert.equal(transport.calls.listeners, 1);
	stop();
	assert.equal(runSubscriberCount("run-a"), 0);
	assert.equal(transport.calls.listeners, 0, "a parked run holds no upstream listener");
});

test("a notification is a wakeup, not truth: the persisted page is what lands", async () => {
	const transport = fakeTransport({
		runs: { "run-a": runRecord({ cursorSeq: 3 }) },
		pages: { "run-a": [event(1), event(2), event(3)] }
	});
	setRunProgressTransport(transport);
	const seen = [];
	subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	assert.equal(seen.at(-1).events.length, 3);
	// The producer wrote a fourth event and pinged; the store re-reads.
	transport.setPages("run-a", [event(1), event(2), event(3), event(4)]);
	transport.setRun("run-a", runRecord({ cursorSeq: 4 }));
	transport.emit("run-a");
	await settle();
	assert.equal(seen.at(-1).cursor, 4);
	assert.equal(seen.at(-1).events.length, 4);
});

test("the run record is terminal authority, and a terminal run stops polling", async () => {
	const transport = fakeTransport({
		runs: { "run-a": runRecord({ status: "completed", cursorSeq: 3, finishedAt: "2026-08-17T12:05:00Z" }) },
		pages: { "run-a": [event(1), event(2), event(3)] }
	});
	setRunProgressTransport(transport);
	const seen = [];
	subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	assert.equal(seen.at(-1).state, "terminal");
});

test("a run that stopped emitting is still terminal when its record says so", async () => {
	// The stream's last event is a mid-run rollout; only the record knows.
	const transport = fakeTransport({
		runs: { "run-a": runRecord({ status: "failed", cursorSeq: 2 }) },
		pages: { "run-a": [event(1), event(2)] }
	});
	setRunProgressTransport(transport);
	const seen = [];
	subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	assert.equal(seen.at(-1).state, "terminal");
	assert.equal(seen.at(-1).run.status, "failed");
});

test("a sequence hole becomes a stale state, never a silently short count", async () => {
	const gaps = [];
	installRunProgressDiagnostics((report) => gaps.push(report));
	const transport = fakeTransport({
		// The record claims five events; only three of the first four are readable.
		runs: { "run-a": runRecord({ cursorSeq: 5 }) },
		pages: { "run-a": [event(1), event(2), event(4)] }
	});
	setRunProgressTransport(transport);
	const seen = [];
	subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	const last = seen.at(-1);
	assert.equal(last.state, "stale");
	assert.equal(last.gap, true);
	assert.equal(gaps.length, 1);
	assert.equal(gaps[0].code, "stream_replay_gap");
	assert.match(gaps[0].message, /history is incomplete at 4\/5/);
});

test("history that catches up clears the stale state", async () => {
	const transport = fakeTransport({
		runs: { "run-a": runRecord({ cursorSeq: 5 }) },
		pages: { "run-a": [event(1), event(2), event(4)] }
	});
	setRunProgressTransport(transport);
	const seen = [];
	subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	assert.equal(seen.at(-1).state, "stale");
	transport.setPages("run-a", [event(1), event(2), event(3), event(4), event(5)]);
	transport.emit("run-a");
	await settle();
	assert.equal(seen.at(-1).state, "subscribed");
	assert.equal(seen.at(-1).gap, false);
	assert.equal(seen.at(-1).events.length, 5);
});

test("a shrinking run cursor forces a full reload rather than a patch", async () => {
	const reads = [];
	const transport = fakeTransport({
		runs: { "run-a": runRecord({ cursorSeq: 3 }) },
		pages: { "run-a": [event(1), event(2), event(3)] },
		onEventsAfter: (_runId, afterSeq) => reads.push(afterSeq)
	});
	setRunProgressTransport(transport);
	const seen = [];
	subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	reads.length = 0;
	// A replaced local import: the same run id, a shorter history.
	transport.setRun("run-a", runRecord({ cursorSeq: 1 }));
	transport.setPages("run-a", [event(1)]);
	transport.emit("run-a");
	await settle();
	assert.ok(reads.includes(0), `a shrinking cursor must reload from 0 (saw ${reads})`);
	assert.equal(seen.at(-1).events.length, 1);
});

test("a read failure keeps what was already replayed and reports the interruption", async () => {
	const failures = [];
	installRunProgressDiagnostics((report) => failures.push(report));
	const runs = { "run-a": runRecord() };
	const pages = { "run-a": [event(1), event(2), event(3)] };
	let failNext = false;
	const transport = {
		async get(runId) {
			if (failNext) throw new Error("bridge closed");
			return runs[runId];
		},
		async eventsAfter(runId, afterSeq = 0) {
			return pages[runId].filter((entry) => entry.sequenceNumber > afterSeq);
		},
		async refresh() { return undefined; },
		onEvent(listener) { transport.notify = listener; return () => undefined; }
	};
	setRunProgressTransport(transport);
	const seen = [];
	subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	assert.equal(seen.at(-1).events.length, 3);
	failNext = true;
	transport.notify({ payload: { optimizerRunId: "run-a" } });
	await settle();
	const last = seen.at(-1);
	assert.equal(last.state, "interrupted");
	assert.ok(last.error);
	assert.equal(last.events.length, 3, "a failed read must not blank a card that had counts");
	assert.equal(failures.at(-1).code, "stream_interrupted");
});

test("a hung read stalls into a recoverable interrupted state, not endless Running", async () => {
	const failures = [];
	installRunProgressDiagnostics((report) => failures.push(report));
	setRunProgressStallTimeout(20);
	const transport = {
		get() {
			return new Promise(() => undefined);
		},
		async eventsAfter() { return []; },
		async refresh() { return undefined; },
		onEvent() { return () => undefined; }
	};
	setRunProgressTransport(transport);
	const seen = [];
	subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await new Promise((resolve) => setTimeout(resolve, 50));
	assert.equal(seen.at(-1).state, "interrupted");
	assert.match(seen.at(-1).error, /stalled/);
	assert.ok(failures.some((report) => report.code === "stream_stalled"));
	setRunProgressStallTimeout(15_000);
});

test("an interrupted subscription recovers to subscribed on the next successful read", async () => {
	const runs = { "run-a": runRecord() };
	const pages = { "run-a": [event(1), event(2), event(3)] };
	let failNext = false;
	const transport = {
		async get(runId) {
			if (failNext) throw new Error("bridge closed");
			return runs[runId];
		},
		async eventsAfter(runId, afterSeq = 0) {
			return pages[runId].filter((entry) => entry.sequenceNumber > afterSeq);
		},
		async refresh() { return undefined; },
		onEvent(listener) { transport.notify = listener; return () => undefined; }
	};
	setRunProgressTransport(transport);
	const seen = [];
	subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	failNext = true;
	transport.notify({ payload: { optimizerRunId: "run-a" } });
	await settle();
	assert.equal(seen.at(-1).state, "interrupted");
	failNext = false;
	transport.notify({ payload: { optimizerRunId: "run-a" } });
	await settle();
	assert.equal(seen.at(-1).state, "subscribed");
	assert.equal(seen.at(-1).events.length, 3);
	assert.equal(seen.at(-1).error, undefined);
});

test("five concurrent runs stay independently scoped", async () => {
	const runs = {};
	const pages = {};
	for (const id of ["r1", "r2", "r3", "r4", "r5"]) {
		runs[id] = runRecord({ id, cursorSeq: 2, sessionRef: `sess-${id}` });
		pages[id] = [{ ...event(1), optimizerRunId: id }, { ...event(2), optimizerRunId: id }];
	}
	const transport = fakeTransport({ runs, pages });
	setRunProgressTransport(transport);
	const snapshots = {};
	for (const id of Object.keys(runs)) {
		subscribeToRun(id, (snapshot) => { snapshots[id] = snapshot; });
	}
	await settle();
	for (const id of Object.keys(runs)) {
		assert.equal(snapshots[id].runId, id);
		assert.equal(snapshots[id].run.id, id);
		assert.equal(snapshots[id].cursor, 2);
		assert.equal(runSubscriberCount(id), 1);
	}
	// A wakeup for one run must not be attributed to another.
	pages.r3 = [...pages.r3, { ...event(3), optimizerRunId: "r3" }];
	runs.r3 = runRecord({ id: "r3", cursorSeq: 3, sessionRef: "sess-r3" });
	transport.emit("r3");
	await settle();
	assert.equal(snapshots.r3.cursor, 3);
	assert.equal(snapshots.r1.cursor, 2);
});

test("restart recovery: a terminal card rebuilds from the durable record and pages", async () => {
	// Nothing is carried over from a previous session; this is a cold read.
	const transport = fakeTransport({
		runs: { "run-a": runRecord({ status: "completed", cursorSeq: 3, finishedAt: "2026-08-17T12:09:00Z" }) },
		pages: { "run-a": [event(1), event(2), event(3)] }
	});
	setRunProgressTransport(transport);
	resetRunProgressStore();
	const seen = [];
	subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	assert.equal(seen.at(-1).state, "terminal");
	assert.equal(seen.at(-1).events.length, 3);
	assert.equal(seen.at(-1).run.finishedAt, "2026-08-17T12:09:00Z");
});

test("ownership: a run from another conversation is not this chat's to watch", async () => {
	const transport = fakeTransport({
		runs: { "run-a": runRecord({ sessionRef: "sess-other" }) },
		pages: { "run-a": [event(1)] }
	});
	setRunProgressTransport(transport);
	assert.equal(await resolveOwnedRun("run-a", "sess-other") !== null, true);
	assert.equal(await resolveOwnedRun("run-a", "sess-mine"), null);
	// A workspace-level run with no session is readable from any conversation.
	transport.setRun("run-a", runRecord({ sessionRef: null }));
	assert.ok(await resolveOwnedRun("run-a", "sess-mine"));
	// A removed run is unavailable rather than an exception.
	transport.setRun("run-a", undefined);
	assert.equal(await resolveOwnedRun("run-a", "sess-1"), null);
});

test("without a bridge the state is unavailable, not a crash", async () => {
	setRunProgressTransport(null);
	const seen = [];
	const stop = subscribeToRun("run-a", (snapshot) => seen.push(snapshot));
	await settle();
	assert.equal(seen.at(-1).state, "unavailable");
	assert.match(seen.at(-1).error, /bridge is unavailable/);
	stop();
	assert.equal(await resolveOwnedRun("run-a", "sess-1"), null);
});

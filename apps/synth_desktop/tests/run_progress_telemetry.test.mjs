/**
 * Experience-budget telemetry, and the consumer isolation that acceptance
 * test 8 depends on: a renderer crash in the full visual must not change or
 * stall the chat card's terminal state.
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

function bundle(relative, outName) {
	const outfile = join(compiledDir, outName);
	buildSync({
		entryPoints: [join(appRoot, relative)],
		bundle: true,
		format: "esm",
		target: "es2022",
		platform: "node",
		alias: { "@synth/visual-templates": join(appRoot, "../../visuals/families") },
		outfile
	});
	return pathToFileURL(outfile).href;
}

const {
	flushRunTelemetry,
	installRunProgressTelemetry,
	recordSample,
	recordSubscribed,
	resetRunProgressTelemetry,
	runTelemetrySnapshot
} = await import(bundle("src/renderer/src/runtime/runProgress/telemetry.ts", "runProgressTelemetry.mjs"));

const {
	installRunProgressDiagnostics,
	resetRunProgressStore,
	setRunProgressPollInterval,
	setRunProgressTransport,
	subscribeToRun
} = await import(bundle("src/renderer/src/runtime/runProgress/subscription.ts", "runProgressSubscription2.mjs"));

setRunProgressPollInterval(3_600_000);
const settle = () => new Promise((resolve) => setTimeout(resolve, 5));

test.beforeEach(() => {
	resetRunProgressTelemetry();
	resetRunProgressStore();
	installRunProgressDiagnostics(() => undefined);
	installRunProgressTelemetry(() => undefined);
});

test.after(() => {
	resetRunProgressStore();
	setRunProgressTransport(null);
});

test("time to first progress is measured from subscribe to the first projection", () => {
	recordSubscribed("run-a", "gepa", 1_000);
	recordSample("run-a", "gepa", { etaState: "estimating", stale: false, now: 1_420 });
	assert.equal(runTelemetrySnapshot("run-a").timeToFirstProgressMs, 420);
	// A later sample does not move the first-progress measurement.
	recordSample("run-a", "gepa", { etaState: "point", stale: false, now: 9_000 });
	assert.equal(runTelemetrySnapshot("run-a").timeToFirstProgressMs, 420);
});

test("estimate coverage counts only samples that offered a usable estimate", () => {
	recordSubscribed("run-a", "gepa", 0);
	for (const state of ["estimating", "estimating", "range", "point", "unavailable"]) {
		recordSample("run-a", "gepa", { etaState: state, stale: false, now: 1_000 });
	}
	const snapshot = runTelemetrySnapshot("run-a");
	assert.equal(snapshot.samples, 5);
	assert.equal(snapshot.estimateCoverage, 2 / 5);
});

test("a run that never produced an estimate reports zero coverage, not no coverage", () => {
	recordSubscribed("run-b", "sft", 0);
	recordSample("run-b", "sft", { etaState: "unavailable", stale: false, now: 100 });
	assert.equal(runTelemetrySnapshot("run-b").estimateCoverage, 0);
});

test("update latency keeps the worst observed delay and stale samples are counted", () => {
	recordSubscribed("run-a", "eval", 0);
	recordSample("run-a", "eval", { etaState: "point", stale: false, latencyMs: 120, now: 10 });
	recordSample("run-a", "eval", { etaState: "point", stale: true, latencyMs: 4_800, now: 20 });
	recordSample("run-a", "eval", { etaState: "point", stale: false, latencyMs: 90, now: 30 });
	const snapshot = runTelemetrySnapshot("run-a");
	assert.equal(snapshot.worstUpdateLatencyMs, 4_800);
	assert.equal(snapshot.staleSamples, 1);
});

test("a run flushes exactly one record however often the card re-renders", () => {
	const records = [];
	installRunProgressTelemetry((record) => records.push(record));
	recordSubscribed("run-a", "gepa", 0);
	recordSample("run-a", "gepa", { etaState: "point", stale: false, latencyMs: 40, now: 50 });
	assert.ok(flushRunTelemetry("run-a"));
	assert.equal(flushRunTelemetry("run-a"), null);
	assert.equal(flushRunTelemetry("run-a"), null);
	assert.equal(records.length, 1);
	assert.equal(records[0].runId, "run-a");
	assert.equal(records[0].runKind, "gepa");
});

test("a crashing consumer cannot change what another surface sees", async () => {
	const failures = [];
	installRunProgressDiagnostics((report) => failures.push(report));
	const run = {
		schemaVersion: "optimizer_run.v1",
		id: "run-a",
		algorithmId: "gepa",
		status: "completed",
		sessionRef: "sess-1",
		cursorSeq: 2,
		capabilities: {}
	};
	const events = [1, 2].map((sequence) => ({
		schemaVersion: "optimizer_event.v1",
		eventId: `e${sequence}`,
		type: "optimizer.evaluation_result.received",
		sequenceNumber: sequence,
		occurredAt: `2026-08-17T12:00:0${sequence}Z`,
		optimizerRunId: "run-a",
		algorithmId: "gepa"
	}));
	setRunProgressTransport({
		get: async () => run,
		eventsAfter: async (_id, afterSeq = 0) => events.filter((entry) => entry.sequenceNumber > afterSeq),
		refresh: async () => run,
		onEvent: () => () => undefined
	});

	// The full visual's shell throws on every snapshot; the chat card does not.
	const card = [];
	subscribeToRun("run-a", () => {
		throw new Error("visual shell crashed");
	});
	subscribeToRun("run-a", (snapshot) => card.push(snapshot));
	await settle();

	const last = card.at(-1);
	assert.equal(last.state, "terminal", "the card still reaches its terminal state");
	assert.equal(last.events.length, 2);
	assert.equal(last.error, undefined, "a consumer crash is not a stream failure");
	assert.ok(
		failures.some((report) => report.event === "run_progress.consumer.failed"),
		"the crash is still recorded rather than swallowed"
	);
});

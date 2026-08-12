/**
 * Unit tests for the Wave 3 session status reducer (applyRuntimeEvent).
 *
 * Session.status has one writer on the TS side: the helpers in
 * stores/applyRuntimeEvent.ts. These tests lock the transitions and the
 * running-selector arbitration formerly inlined in App.tsx.
 */
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

function compile(relative, outName) {
	const source = join(appRoot, relative);
	const compiled = join(compiledDir, outName);
	writeFileSync(
		compiled,
		transformSync(readFileSync(source, "utf8"), {
			loader: "ts",
			format: "esm",
			target: "es2022",
			sourcefile: source
		}).code
	);
	return pathToFileURL(compiled).href;
}

const {
	applyLocalSessionStatus,
	applyRuntimeEvent,
	applyTurnAccepted,
	selectSessionRunning,
	statusFromRuntimeEvent
} = await import(compile("src/renderer/src/stores/applyRuntimeEvent.ts", "applyRuntimeEvent.mjs"));

function session(overrides = {}) {
	return {
		id: "sess-1",
		title: "Chat",
		target: { kind: "local", model: "laguna-xs" },
		createdAt: "2026-08-11T00:00:00.000Z",
		updatedAt: "2026-08-11T00:00:00.000Z",
		status: "ready",
		latestCursor: 0,
		metadata: {},
		...overrides
	};
}

function event(overrides = {}) {
	return {
		schemaVersion: "synth.desktop-runtime-event.v1",
		sessionId: "sess-1",
		sequence: 1,
		eventKind: "run.started",
		payload: {},
		createdAt: "2026-08-11T00:00:01.000Z",
		source: "local",
		...overrides
	};
}

function emptyState(sessions = [session()]) {
	return { sessions, eventsBySession: {} };
}

test("statusFromRuntimeEvent maps run lifecycle kinds", () => {
	assert.equal(statusFromRuntimeEvent("ready", "run.started"), "running");
	assert.equal(statusFromRuntimeEvent("running", "run.completed"), "ready");
	assert.equal(statusFromRuntimeEvent("running", "run.failed"), "failed");
	assert.equal(statusFromRuntimeEvent("running", "run.cancelled"), "cancelled");
	assert.equal(statusFromRuntimeEvent("running", "session/unhealthy"), "interrupted");
	assert.equal(statusFromRuntimeEvent("running", "message.created"), "running");
});

test("fenced run.started does not resurrect Working", () => {
	assert.equal(statusFromRuntimeEvent("interrupted", "run.started", { fenced: true }), "interrupted");
	assert.equal(statusFromRuntimeEvent("ready", "run.started", { fenced: true }), "ready");
});

test("applyRuntimeEvent appends the event and promotes status to running", () => {
	const next = applyRuntimeEvent(emptyState(), event({ sequence: 3, eventKind: "run.started" }));
	assert.equal(next.sessions[0].status, "running");
	assert.equal(next.sessions[0].latestCursor, 3);
	assert.equal(next.eventsBySession["sess-1"].length, 1);
	assert.equal(next.eventsBySession["sess-1"][0].eventKind, "run.started");
});

test("applyRuntimeEvent clears running on terminal run events", () => {
	const running = emptyState([session({ status: "running", latestCursor: 1 })]);
	const started = applyRuntimeEvent(running, event({ sequence: 1, eventKind: "run.started" }));
	const completed = applyRuntimeEvent(started, event({ sequence: 2, eventKind: "run.completed" }));
	assert.equal(completed.sessions[0].status, "ready");
	assert.equal(completed.eventsBySession["sess-1"].length, 2);
});

test("applyRuntimeEvent dedupes identical events", () => {
	const first = applyRuntimeEvent(emptyState(), event({ sequence: 5 }));
	const second = applyRuntimeEvent(first, event({ sequence: 5 }));
	assert.equal(second.eventsBySession["sess-1"].length, 1);
	assert.equal(second, first);
});

test("applyRuntimeEvent can append without touching status", () => {
	const interrupted = applyLocalSessionStatus(emptyState([session({ status: "running" })]), "sess-1", "interrupted", {
		onlyIf: "running"
	});
	assert.equal(interrupted.sessions[0].status, "interrupted");
	const withEvent = applyRuntimeEvent(
		interrupted,
		event({ sequence: 9, eventKind: "session/unhealthy" }),
		{ updateStatus: false }
	);
	assert.equal(withEvent.sessions[0].status, "interrupted");
	assert.equal(withEvent.eventsBySession["sess-1"].length, 1);
});

test("applyLocalSessionStatus respects onlyIf", () => {
	const ready = emptyState([session({ status: "ready" })]);
	const unchanged = applyLocalSessionStatus(ready, "sess-1", "interrupted", { onlyIf: "running" });
	assert.equal(unchanged, ready);
	const running = emptyState([session({ status: "running" })]);
	const next = applyLocalSessionStatus(running, "sess-1", "interrupted", { onlyIf: "running" });
	assert.equal(next.sessions[0].status, "interrupted");
});

test("applyTurnAccepted sets running only when a turn id exists", () => {
	const target = { kind: "remote", provider: "openrouter", model: "luna" };
	const withoutTurn = applyTurnAccepted(emptyState(), "sess-1", { target, turnId: null });
	assert.equal(withoutTurn.sessions[0].status, "ready");
	assert.deepEqual(withoutTurn.sessions[0].target, target);
	const withTurn = applyTurnAccepted(emptyState(), "sess-1", { target, turnId: "turn-1" });
	assert.equal(withTurn.sessions[0].status, "running");
});

test("selectSessionRunning trusts restored session status over stale run.started", () => {
	const events = [event({ sequence: 1, eventKind: "run.started" })];
	assert.equal(selectSessionRunning(session({ status: "ready" }), events), false);
	assert.equal(selectSessionRunning(session({ status: "running" }), events), true);
	assert.equal(selectSessionRunning(undefined, events), true);
});

test("selectSessionRunning keeps Stop when a newer user turn follows a terminal run", () => {
	const events = [
		event({ sequence: 1, eventKind: "run.started" }),
		event({ sequence: 2, eventKind: "run.completed" }),
		event({
			sequence: 3,
			eventKind: "message.created",
			payload: { messageId: "m1", role: "user", content: "next" }
		})
	];
	assert.equal(selectSessionRunning(session({ status: "running" }), events), true);
	assert.equal(selectSessionRunning(session({ status: "ready" }), events), false);
});

test("selectSessionRunning clears Stop on a terminal run without a newer user turn", () => {
	const events = [
		event({ sequence: 1, eventKind: "run.started" }),
		event({ sequence: 2, eventKind: "run.failed" })
	];
	assert.equal(selectSessionRunning(session({ status: "running" }), events), false);
	assert.equal(selectSessionRunning(session({ status: "failed" }), events), false);
});

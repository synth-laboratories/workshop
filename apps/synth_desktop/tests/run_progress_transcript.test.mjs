/**
 * Where a run-progress card attaches, and what the transcript projection reads
 * to decide.
 *
 * Two rules: a card comes from the durable run reference a tool *result*
 * carried, never from prose; and one run gets one card, anchored to the turn
 * that first referenced it however many times it is polled afterwards.
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
		// The renderer resolves family internals through the same alias Vite and
		// tsconfig use; esbuild needs it spelled out.
		alias: { "@synth/visual-templates": join(appRoot, "../../visuals/families") },
		outfile
	});
	return pathToFileURL(outfile).href;
}

const { chatActivityLines, runProgressItemsByMessage, runProgressItemsForLines, supersededRunActivity } = await import(
	bundle("src/renderer/src/runtime/runProgress/transcript.ts", "runProgressTranscript.mjs")
);
const { eventsToLocalActivity } = await import(
	bundle("src/renderer/src/runtime/sessionView.ts", "runProgressSessionView.mjs")
);

const SESSION = "sess-1";

function toolEvent(sequence, { tool, result, args }) {
	return {
		schemaVersion: "synth.desktop-runtime-event.v1",
		sessionId: SESSION,
		source: "local",
		createdAt: "2026-08-17T12:00:00Z",
		sequence,
		eventKind: "item/completed",
		payload: {
			item: {
				type: "mcpToolCall",
				id: `call-${sequence}`,
				server: "synth_optimizers",
				tool,
				status: "completed",
				arguments: args ?? {},
				result: { isError: false, structuredContent: result }
			}
		}
	};
}

const ASSISTANT = [{ id: "assistant-1", role: "assistant", body: "Started the run.", at: "2026-08-17T12:00:05Z" }];

const RUN_RECORD = {
	schemaVersion: "optimizer_run.v1",
	id: "banking77_gepa_sol_med_45856f25",
	algorithmId: "gepa",
	status: "queued",
	sessionRef: SESSION
};

test("a run reference comes from the tool result, not from assistant prose", () => {
	const activity = eventsToLocalActivity(
		[toolEvent(3, { tool: "optimizer_start_recipe", args: { recipe_id: "gepa.banking77.sol.v1" }, result: RUN_RECORD })],
		ASSISTANT
	);
	const withRun = Object.values(activity).flat().filter((line) => line.optimizerRunId);
	assert.equal(withRun.length, 1);
	assert.equal(withRun[0].optimizerRunId, "banking77_gepa_sol_med_45856f25");
	assert.equal(withRun[0].runKind, "gepa");
});

test("a nested run record in the result is read the same way", () => {
	const activity = eventsToLocalActivity(
		[toolEvent(3, { tool: "optimizer_get_run", result: { run: RUN_RECORD } })],
		ASSISTANT
	);
	const withRun = Object.values(activity).flat().find((line) => line.optimizerRunId);
	assert.equal(withRun.optimizerRunId, "banking77_gepa_sol_med_45856f25");
	assert.equal(withRun.runKind, "gepa");
});

test("a run id mentioned only in prose produces no card", () => {
	const chat = {
		id: SESSION,
		title: "Chat",
		messages: [{
			id: "assistant-9",
			role: "assistant",
			body: "I started banking77_gepa_sol_med_45856f25 for you.",
			at: "2026-08-17T12:00:05Z"
		}],
		activityByMessageId: {}
	};
	assert.deepEqual(runProgressItemsByMessage(chat), {});
});

test("a non-optimizer tool result carrying an id is not read as a run", () => {
	const containerCall = toolEvent(3, { tool: "container_register", result: { id: "ctr_9" } });
	containerCall.payload.item.server = "synth_containers";
	const activity = eventsToLocalActivity([containerCall], ASSISTANT);
	assert.equal(
		Object.values(activity).flat().some((line) => line.optimizerRunId),
		false
	);
});

test("a workflow chat has no card for is referenced but declares no run kind", () => {
	const activity = eventsToLocalActivity(
		[toolEvent(3, {
			tool: "optimizer_get_run",
			args: { optimizer_run_id: "goex_craftax_1" },
			result: { ...RUN_RECORD, id: "goex_craftax_1", algorithmId: "go-ex" }
		})],
		ASSISTANT
	);
	const withRun = Object.values(activity).flat().find((line) => line.optimizerRunId);
	assert.equal(withRun.optimizerRunId, "goex_craftax_1");
	assert.equal(withRun.runKind, undefined, "an uncarded algorithm declares no run kind");
});

test("polling one run four times still yields one card", () => {
	const items = runProgressItemsForLines([
		{ id: "a", label: "start", optimizerRunId: "run-a", runKind: "gepa" },
		{ id: "b", label: "get", optimizerRunId: "run-a", runKind: "gepa" },
		{ id: "c", label: "get", optimizerRunId: "run-a", runKind: "gepa" },
		{ id: "d", label: "cancel", optimizerRunId: "run-a", runKind: "gepa" }
	]);
	assert.equal(items.length, 1);
	assert.deepEqual(items[0], {
		kind: "run_progress",
		runId: "run-a",
		runKind: "gepa",
		createdAt: ""
	});
});

test("a card anchors to the turn that first referenced its run", () => {
	const chat = {
		id: SESSION,
		title: "Chat",
		messages: [
			{ id: "assistant-1", role: "assistant", body: "started", at: "2026-08-17T12:00:00Z" },
			{ id: "assistant-2", role: "assistant", body: "checked", at: "2026-08-17T12:05:00Z" }
		],
		activityByMessageId: {
			"assistant-1": [{ id: "a", label: "start", optimizerRunId: "run-a", runKind: "gepa" }],
			"assistant-2": [
				{ id: "b", label: "get", optimizerRunId: "run-a", runKind: "gepa" },
				{ id: "c", label: "start", optimizerRunId: "run-b", runKind: "sft" }
			]
		}
	};
	const items = runProgressItemsByMessage(chat);
	assert.deepEqual(items["assistant-1"].map((item) => item.runId), ["run-a"]);
	assert.deepEqual(items["assistant-2"].map((item) => item.runId), ["run-b"]);
});

test("a run referenced only by the live turn gets a card before the turn has a message", () => {
	const chat = {
		id: SESSION,
		title: "Chat",
		messages: [{ id: "user-1", role: "user", body: "go", at: "2026-08-17T12:00:00Z" }],
		activityByMessageId: {
			__active__: [{ id: "a", label: "start", optimizerRunId: "run-a", runKind: "eval" }]
		}
	};
	const items = runProgressItemsByMessage(chat);
	assert.deepEqual(items.__active__.map((item) => item.runId), ["run-a"]);
});

test("several concurrent runs in one turn each get their own card", () => {
	const items = runProgressItemsForLines([
		{ id: "a", label: "start", optimizerRunId: "run-a", runKind: "gepa" },
		{ id: "b", label: "start", optimizerRunId: "run-b", runKind: "eval" },
		{ id: "c", label: "start", optimizerRunId: "run-c", runKind: "sft" }
	]);
	assert.deepEqual(items.map((item) => item.runId), ["run-a", "run-b", "run-c"]);
	assert.deepEqual(items.map((item) => item.runKind), ["gepa", "eval", "sft"]);
});


/**
 * A tool line records what was true when the call returned; it is never
 * rewritten. What has to change is whether the reader can still tell that it
 * holds. The v9 rollout was cancelled and the transcript went on describing an
 * active rollout, four polls deep.
 */
test("every line reporting on a run is marked once that run has stopped", () => {
	const lines = [
		{ id: "activity-3", optimizerRunId: "run-a", toolStatus: "completed" },
		{ id: "activity-5", optimizerRunId: "run-a", toolStatus: "completed" },
		{ id: "activity-7", optimizerRunId: "run-b", toolStatus: "completed" },
		{ id: "activity-9", toolStatus: "completed" }
	];
	const superseded = supersededRunActivity(lines, [
		{ id: "run-a", status: "cancelled" },
		{ id: "run-b", status: "running" }
	]);
	assert.equal(superseded.get("activity-3"), "cancelled");
	assert.equal(superseded.get("activity-5"), "cancelled");
	assert.equal(superseded.has("activity-7"), false, "a live run supersedes nothing");
	assert.equal(superseded.has("activity-9"), false, "a line bound to no run is untouched");
});

test("a completed or failed run marks its lines the same way a cancelled one does", () => {
	const lines = [{ id: "activity-1", optimizerRunId: "run-a" }];
	for (const [status, expected] of [["completed", "completed"], ["failed", "failed"]]) {
		const superseded = supersededRunActivity(lines, [{ id: "run-a", status }]);
		assert.equal(superseded.get("activity-1"), expected);
	}
});

/**
 * The mirror of the run card's own rule: a word this build cannot read is not
 * the durable record saying "finished". Marking it as stopped would retire a
 * live run's history out from under the reader.
 */
test("an unrecognised run status supersedes nothing", () => {
	const superseded = supersededRunActivity(
		[{ id: "activity-1", optimizerRunId: "run-a" }],
		[{ id: "run-a", status: "reticulating" }]
	);
	assert.equal(superseded.size, 0);
});

test("no runs at all is cheap and marks nothing", () => {
	assert.equal(supersededRunActivity([{ id: "activity-1", optimizerRunId: "run-a" }], []).size, 0);
});

test("chat activity lines are collected across every message", () => {
	const chat = {
		id: "sess-1",
		title: "t",
		messages: ASSISTANT,
		activityByMessageId: {
			"assistant-1": [{ id: "activity-1", optimizerRunId: "run-a" }],
			__active__: [{ id: "activity-2", optimizerRunId: "run-a" }]
		}
	};
	const lines = chatActivityLines(chat);
	assert.deepEqual(lines.map((line) => line.id).sort(), ["activity-1", "activity-2"]);
	const superseded = supersededRunActivity(lines, [{ id: "run-a", status: "cancelled" }]);
	assert.equal(superseded.size, 2);
});

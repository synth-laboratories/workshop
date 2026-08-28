/**
 * Transcript projection: optimistic renderer `message.created` plus the host
 * journal event must collapse to one user bubble when they share messageId.
 * Divergent ids were the CUA P1 (every submitted prompt rendered twice).
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

const source = join(appRoot, "src/renderer/src/runtime/sessionView.ts");
const compiled = join(compiledDir, "sessionView.mjs");
buildSync({
	entryPoints: [source],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile: compiled
});

const { eventsToLocalActivity, eventsToMessages } = await import(pathToFileURL(compiled).href);

function event(overrides = {}) {
	return {
		schemaVersion: "synth.desktop-runtime-event.v1",
		sessionId: "sess-1",
		sequence: 1,
		eventKind: "message.created",
		payload: {},
		createdAt: "2026-08-12T00:00:00.000Z",
		source: "local",
		...overrides
	};
}

test("optimistic and host user prompts with the same messageId yield one bubble", () => {
	const messages = eventsToMessages([
		event({
			sequence: 1,
			payload: { messageId: "user-7", role: "user", content: "hello from composer" }
		}),
		event({
			sequence: 40,
			payload: { messageId: "user-7", role: "user", content: "hello from composer" }
		})
	]);
	const user = messages.filter((message) => message.role === "user");
	assert.equal(user.length, 1);
	assert.equal(user[0].id, "user-7");
	assert.equal(user[0].body, "hello from composer");
});

test("divergent messageIds for the same prompt still yield two bubbles (ownership bug)", () => {
	// Documents the pre-fix failure mode: renderer `user-${sequence}` vs Rust
	// `user-${uuid}`. The host path must reuse the client id; this test keeps the
	// projection honest so a regression cannot hide behind silent content merge.
	const messages = eventsToMessages([
		event({
			sequence: 1,
			payload: { messageId: "user-7", role: "user", content: "hello from composer" }
		}),
		event({
			sequence: 40,
			payload: {
				messageId: "user-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
				role: "user",
				content: "hello from composer"
			}
		})
	]);
	const user = messages.filter((message) => message.role === "user");
	assert.equal(user.length, 2);
});

test("approval policy state and unknown approval events stay out of conversation activity", () => {
	const activity = eventsToLocalActivity([
		event({
			sequence: 1,
			eventKind: "approval.policy.effective",
			payload: { approvalPolicy: "never", sandbox: "danger-full-access" }
		}),
		event({
			sequence: 2,
			eventKind: "approval.future-state",
			payload: { detail: "new backend state" }
		})
	], []);

	assert.deepEqual(activity, {});
});

test("recognized approval lifecycle events retain explicit Synth labels", () => {
	const activity = eventsToLocalActivity([
		event({
			sequence: 1,
			eventKind: "approval.granted",
			payload: { approvalId: "approval-1" }
		})
	], []);

	assert.equal(activity.__active__?.[0]?.label, "Permission granted");

	const paid = eventsToLocalActivity([
		event({ sequence: 2, eventKind: "approval.granted", payload: { approvalId: "approval-2", kind: "paid_compute" } })
	], []);
	assert.equal(paid.__active__?.[0]?.label, "Paid compute granted");
});

test("duplicate provider failures render once and hide raw provider payloads", () => {
	const rawFailure = JSON.stringify({
		error: {
			message: "Provider returned error",
			metadata: { raw: "Requests ending with a model turn are not supported" }
		}
	});
	const messages = eventsToMessages([
		event({ sequence: 1, eventKind: "run.started" }),
		event({ sequence: 2, eventKind: "run.failed", payload: { error: { message: rawFailure } } }),
		event({ sequence: 3, eventKind: "run.failed", payload: { error: { message: rawFailure } } })
	]);
	const system = messages.filter((message) => message.role === "system");
	assert.equal(system.length, 1);
	assert.equal(
		system[0].body,
		"The provider could not produce a response: The provider rejected a request ending with a model turn. Try again."
	);
	assert.equal(system[0].body.includes("metadata"), false);
});

test("duplicate turn terminals after an assistant answer do not synthesize a false empty response", () => {
	const messages = eventsToMessages([
		event({ sequence: 1, eventKind: "run.started" }),
		event({
			sequence: 2,
			eventKind: "message.created",
			payload: { messageId: "user-8", role: "user", content: "reply with the nonce" }
		}),
		event({
			sequence: 3,
			eventKind: "message.completed",
			payload: { messageId: "answer-8", role: "assistant", content: "WORKSHOP_CUA_LAGUNA_OK" }
		}),
		event({ sequence: 4, eventKind: "run.completed", payload: { runId: "turn-8" } }),
		event({ sequence: 5, eventKind: "run.completed", payload: { runId: "turn-8" } })
	]);
	assert.deepEqual(
		messages.map(({ role, body }) => ({ role, body })),
		[
			{ role: "user", body: "reply with the nonce" },
			{ role: "assistant", body: "WORKSHOP_CUA_LAGUNA_OK" }
		]
	);
});

test("operator Stop is explicit, deduped, and a follow-up resumes normally", () => {
	const events = [
		event({ sequence: 1, eventKind: "message.created", payload: { messageId: "user-stop", role: "user", content: "start the long tool" } }),
		event({ sequence: 2, eventKind: "run.started", payload: { runId: "turn-stop" } }),
		event({ sequence: 3, eventKind: "message.completed", payload: { messageId: "partial-stop", role: "assistant", content: "I started the tool." } }),
		event({ sequence: 4, eventKind: "run.cancelled", payload: { runId: "turn-stop", reason: "operator_cancelled", cancelledBy: "user" } }),
		// Provider and durable projections may both deliver the same terminal.
		event({ sequence: 5, eventKind: "run.cancelled", payload: { runId: "turn-stop", reason: "operator_cancelled", cancelledBy: "user" } }),
		event({ sequence: 6, eventKind: "message.created", payload: { messageId: "user-resume", role: "user", content: "continue without that tool" } }),
		event({ sequence: 7, eventKind: "run.started", payload: { runId: "turn-resume" } }),
		event({ sequence: 8, eventKind: "message.completed", payload: { messageId: "answer-resume", role: "assistant", content: "Continued cleanly." } }),
		event({ sequence: 9, eventKind: "run.completed", payload: { runId: "turn-resume" } })
	];
	assert.deepEqual(
		eventsToMessages(events).map(({ role, body }) => ({ role, body })),
		[
			{ role: "user", body: "start the long tool" },
			{ role: "assistant", body: "I started the tool." },
			{ role: "system", body: "You stopped this response." },
			{ role: "user", body: "continue without that tool" },
			{ role: "assistant", body: "Continued cleanly." }
		]
	);
});

test("operator Stop marks an in-flight transcript tool cancelled", () => {
	const activity = eventsToLocalActivity([
		event({ sequence: 1, eventKind: "run.started", payload: { runId: "turn-stop" } }),
		event({
			sequence: 2,
			eventKind: "item/started",
			payload: { item: { type: "commandExecution", id: "cmd-stop", command: "long-running-command" } }
		}),
		event({ sequence: 3, eventKind: "run.cancelled", payload: { runId: "turn-stop", reason: "operator_cancelled", cancelledBy: "user" } })
	], []);
	const command = Object.values(activity).flat().find((line) => line.kind === "command");
	assert.equal(command.toolStatus, "cancelled");
	assert.equal(Object.values(activity).flat().filter((line) => line.kind === "run_summary").length, 1);
});

test("replayed terminal envelopes do not duplicate or age a run summary", () => {
	const activity = eventsToLocalActivity([
		event({ sequence: 1, eventKind: "run.started", payload: { runId: "turn-1" }, createdAt: "2026-08-12T00:00:00.000Z" }),
		event({ sequence: 2, eventKind: "run.failed", payload: { runId: "turn-1" }, createdAt: "2026-08-12T00:00:20.000Z" }),
		event({ sequence: 3, eventKind: "run.started", payload: { runId: "turn-1" }, createdAt: "2026-08-12T00:10:00.000Z" }),
		event({ sequence: 4, eventKind: "run.failed", payload: { runId: "turn-1" }, createdAt: "2026-08-12T00:15:00.000Z" })
	], []);
	const summaries = Object.values(activity).flat().filter((line) => line.kind === "run_summary");
	assert.equal(summaries.length, 1);
	assert.match(summaries[0].label, /20s/);
});

test("provider duration wins over a synthesized terminal timestamp", () => {
	const activity = eventsToLocalActivity([
		event({ sequence: 1, eventKind: "run.started", payload: { runId: "turn-2" }, createdAt: "2026-08-12T00:00:00.000Z" }),
		event({
			sequence: 2,
			eventKind: "run.failed",
			payload: { turn: { id: "turn-2", durationMs: 19_979 } },
			createdAt: "2026-08-12T00:19:10.000Z"
		})
	], []);
	const summary = Object.values(activity).flat().find((line) => line.kind === "run_summary");
	assert.match(summary.label, /20s/);
	assert.doesNotMatch(summary.label, /19m/);
});

test("completed tool lifecycle preserves provider duration and terminal status", () => {
	const activity = eventsToLocalActivity([
		event({
			sequence: 1,
			eventKind: "item/started",
			payload: { item: { type: "commandExecution", id: "cmd-slow", command: "npm test" } }
		}),
		event({
			sequence: 2,
			eventKind: "item/completed",
			payload: { item: { type: "commandExecution", id: "cmd-slow", command: "npm test", durationMs: 16_400 } }
		})
	], []);
	const commands = Object.values(activity).flat().filter((line) => line.kind === "command");
	assert.equal(commands.length, 1);
	assert.equal(commands[0].toolStatus, "completed");
	assert.equal(commands[0].durationMs, 16_400);
});

test("visual tool operations project as lifecycle milestones", () => {
	const activity = eventsToLocalActivity([
		event({
			sequence: 1,
			eventKind: "item/completed",
			payload: { item: {
				type: "mcpToolCall", id: "visual-create", server: "synth_visuals", tool: "visual_manage",
				status: "completed", arguments: { operation: "create", arguments: { title: "Flow" } },
				result: { structuredContent: { visual: { id: "vis-1", templateId: "diagram.mermaid.v1", title: "Flow" } } }
			} }
		})
	], []);
	const line = Object.values(activity).flat()[0];
	assert.equal(line.kind, "visual_lifecycle");
	assert.equal(line.visualStage, "draft");
	assert.equal(line.label, "Visual draft created");
});

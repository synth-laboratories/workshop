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

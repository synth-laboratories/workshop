import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
const compiled = join(compiledDir, "sessionView.mjs");
mkdirSync(compiledDir, { recursive: true });
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/sessionView.ts")],
	bundle: true,
	platform: "node",
	format: "esm",
	target: "es2022",
	outfile: compiled
});

const { eventsToLocalActivity } = await import(pathToFileURL(compiled).href);

const at = (second) => `2026-08-10T16:00:0${second}.000Z`;

test("tool calls following an assistant preamble render below that message", () => {
	const messages = [
		{ id: "user-1", role: "user", body: "Run the checks", at: at(0) },
		{ id: "assistant-1", role: "assistant", body: "I’ll run the remaining checks.", at: at(2) }
	];
	const events = [
		{ sequence: 1, eventKind: "message.created", createdAt: at(0), payload: { messageId: "user-1", role: "user" } },
		{ sequence: 2, eventKind: "command.execution", createdAt: at(1), payload: { id: "before", command: "pwd" } },
		{ sequence: 3, eventKind: "message.created", createdAt: at(2), payload: { messageId: "assistant-1", role: "assistant" } },
		{ sequence: 4, eventKind: "command.execution", createdAt: at(3), payload: { id: "after", command: "npm test" } }
	];

	const activity = eventsToLocalActivity(events, messages);

	assert.deepEqual(activity["assistant-1"].map((line) => ({ detail: line.detail, placement: line.placement })), [
		{ detail: "pwd", placement: "before" },
		{ detail: "npm test", placement: "after" }
	]);
});

test("recorded timestamps win when activity events are replayed out of order", () => {
	const messages = [
		{ id: "assistant-1", role: "assistant", body: "I’ll check that now.", at: at(2) }
	];
	const events = [
		{ sequence: 1, eventKind: "message.created", createdAt: at(2), payload: { messageId: "assistant-1", role: "assistant" } },
		{ sequence: 2, eventKind: "command.execution", createdAt: at(3), payload: { id: "later", command: "npm test" } },
		{ sequence: 3, eventKind: "command.execution", createdAt: at(1), payload: { id: "earlier", command: "pwd" } }
	];

	const activity = eventsToLocalActivity(events, messages)["assistant-1"];

	assert.equal(activity.find((line) => line.detail === "pwd").placement, "before");
	assert.equal(activity.find((line) => line.detail === "npm test").placement, "after");
});

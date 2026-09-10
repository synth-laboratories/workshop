import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const compiled = join(compiledDir, "subagent-sessionView.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/sessionView.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile: compiled
});

const {
	conversationThreadEvents,
	eventsToLocalActivity,
	eventsToMessages
} = await import(pathToFileURL(compiled).href);

const session = {
	id: "parent-session",
	title: "Parent conversation",
	target: { kind: "remote", provider: "openrouter", model: "openai/gpt-5.6-luna", adapter: null },
	createdAt: "2026-08-25T00:00:00.000Z",
	updatedAt: "2026-08-25T00:00:00.000Z",
	status: "running",
	latestCursor: 12,
	metadata: { runtime: "codex-app-server", threadId: "parent-thread" }
};

function event(sequence, eventKind, payload = {}) {
	return {
		schemaVersion: "synth.desktop-runtime-event.v1",
		sessionId: session.id,
		sequence,
		eventKind,
		payload,
		createdAt: `2026-08-25T00:00:${String(sequence).padStart(2, "0")}.000Z`,
		source: "local"
	};
}

function fixture() {
	return [
		event(1, "message.created", { threadId: "parent-thread", messageId: "parent-user", role: "user", content: "Audit the migration." }),
		event(2, "message.completed", { threadId: "parent-thread", messageId: "parent-plan", content: "I will delegate the audit." }),
		event(3, "item/started", { threadId: "parent-thread", item: { id: "spawn-a", type: "collabAgentToolCall", tool: "spawnAgent", prompt: "Inspect the migration boundary." } }),
		event(4, "item/completed", { threadId: "parent-thread", item: { id: "spawn-a", type: "collabAgentToolCall", tool: "spawnAgent", prompt: "Inspect the migration boundary.", receiverThreadIds: ["child-a"], agentsStates: { "child-a": { status: "running" } } } }),
		event(5, "run.started", { threadId: "child-a", turn: { id: "child-turn" } }),
		event(6, "message.delta", { threadId: "child-a", messageId: "child-answer", delta: "Boundary " }),
		event(7, "item/started", { threadId: "child-a", item: { id: "tool-a", type: "mcpToolCall", server: "synth_visuals", tool: "visual_manage", arguments: { operation: "create", arguments: { count: 0, nested: { safe: true } } }, status: "running" } }),
		event(8, "item/completed", { threadId: "child-a", item: { id: "tool-a", type: "mcpToolCall", server: "synth_visuals", tool: "visual_manage", arguments: { operation: "create", arguments: { count: 0, nested: { safe: true } } }, status: "completed", result: { structuredContent: { visual: { id: "visual-a", templateId: "diagram.mermaid.v1", title: "Audit map" } }, content: [{ type: "text", text: "{malformed" }] } } }),
		event(9, "message.completed", { threadId: "child-a", messageId: "child-answer", content: "Boundary is safe." }),
		event(10, "run.completed", { threadId: "child-a", turn: { status: "completed", lastAgentMessage: "Boundary is safe." } }),
		event(11, "message.completed", { threadId: "child-b", messageId: "sibling-answer", content: "Sibling output must not leak." })
	];
}

test("parent and child normalize from the same ordered event record without sibling leakage", () => {
 const events = fixture();
 const parentEvents = conversationThreadEvents(events, "parent-thread", true);
 const childEvents = conversationThreadEvents(events, "child-a");
 const parentMessages = eventsToMessages(parentEvents);
 const childMessages = eventsToMessages(childEvents);
 assert.deepEqual(parentMessages.map(message => message.body), ["Audit the migration.", "I will delegate the audit."]);
 assert.deepEqual(childMessages.map(message => message.body), ["Boundary is safe."]);
 assert.equal(JSON.stringify(eventsToLocalActivity(events, parentMessages)).includes("visual-a"), false);
 assert.equal(childMessages.some(message => message.body.includes("Sibling")), false);
 assert.deepEqual(conversationThreadEvents(events, null), []);
});

test("child tool activity uses the same safe projection and a bounded event window", () => {
 const events = conversationThreadEvents(fixture(), "child-a");
 const messages = eventsToMessages(events);
 const lines = Object.values(eventsToLocalActivity(events, messages)).flat();
 const tool = lines.find(line => line.label === "Visual draft created");
 assert.ok(tool);
 assert.equal(tool.artifactId, "visual-a");
 assert.match(tool.detail, /operation create/);
 assert.equal(JSON.stringify(tool).includes("{malformed"), false);
 assert.equal(JSON.stringify(tool).includes("[object Object]"), false);
 const many = Array.from({length: 1500}, (_, index) => event(index, "message.delta", {thread_id: index % 2 ? "child-a" : "child-b", delta: "."}));
 const bounded = conversationThreadEvents(many, "child-a");
 assert.equal(bounded.length, 600);
 assert.equal(bounded[0].sequence, 301);
 assert.equal(bounded.at(-1).sequence, 1499);
});

test("reasoning remains hidden unless the runtime explicitly permits it", () => {
	const events = [
		event(1, "message.completed", { messageId: "assistant-1", content: "Visible answer." }),
		event(2, "agent.reasoning", { content: "Private provider reasoning." })
	];
	const messages = eventsToMessages(events);
	const activity = eventsToLocalActivity(events, messages, "none");
	assert.equal(Object.values(activity).flat().some((line) => line.kind === "thought"), false);
});

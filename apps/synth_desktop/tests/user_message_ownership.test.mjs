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

const { eventsToMessages } = await import(pathToFileURL(compiled).href);

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

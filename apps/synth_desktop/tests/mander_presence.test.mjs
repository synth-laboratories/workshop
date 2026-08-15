/**
 * Host vs overlay emotion resolution for the optional chat mascot.
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
const compiled = join(compiledDir, "manderPresence.mjs");

buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/components/mander/Mander.presence.ts")],
	bundle: true,
	format: "esm",
	platform: "neutral",
	outfile: compiled,
	logLevel: "silent"
});

const { resolveManderEmotion, sessionHasOpenTools } = await import(pathToFileURL(compiled).href);

test("running turns use the host default, not the overlay", () => {
	assert.equal(resolveManderEmotion({ running: true, overlay: "success" }), "thinking");
	assert.equal(resolveManderEmotion({ running: true, toolsOpen: true, overlay: "idle" }), "working");
});

test("idle turns keep a success overlay and otherwise rest", () => {
	assert.equal(resolveManderEmotion({ running: false, overlay: "success" }), "success");
	assert.equal(resolveManderEmotion({ running: false, overlay: "thinking" }), "thinking");
	assert.equal(resolveManderEmotion({ running: false }), "idle");
	assert.equal(resolveManderEmotion({ running: false, overlay: "confused" }), "idle");
});

test("open tool lines count as working", () => {
	assert.equal(sessionHasOpenTools({
		id: "chat-1",
		title: "Chat",
		messages: [],
		activityByMessageId: {
			__active__: [{ id: "cmd-1", label: "cargo test", kind: "command", toolStatus: "running" }]
		}
	}), true);
	assert.equal(sessionHasOpenTools({
		id: "chat-1",
		title: "Chat",
		messages: [],
		activityByMessageId: {
			m1: [{ id: "cmd-1", label: "cargo test", kind: "command", toolStatus: "completed" }]
		}
	}), false);
});

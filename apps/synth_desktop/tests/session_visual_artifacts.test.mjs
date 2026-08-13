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
const compiled = join(compiledDir, "sessionView.visuals.mjs");
buildSync({
	entryPoints: [source],
	outfile: compiled,
	bundle: true,
	format: "esm",
	platform: "node",
	target: "es2022"
});

const { eventsToArtifacts } = await import(pathToFileURL(compiled).href);

const runtimeEvent = (sequence, eventKind, payload) => ({
	schemaVersion: "synth.desktop-runtime-event.v1",
	sessionId: "session_current",
	sequence,
	eventKind,
	payload,
	createdAt: `2026-08-13T00:00:0${sequence}.000Z`,
	source: "visual"
});

test("a visual.show event makes a historical optimizer visual a chat artifact", () => {
	const [artifact] = eventsToArtifacts([
		runtimeEvent(1, "visual.show", {
			visualId: "visual_gepa_1",
			title: "GEPA · Banking77",
			templateId: "optimizer.gepa.live.v1"
		})
	]);

	assert.equal(artifact.id, "visual_gepa_1");
	assert.equal(artifact.visualId, "visual_gepa_1");
	assert.equal(artifact.title, "GEPA · Banking77");
	assert.equal(artifact.shownByAgent, true);
});

test("showing an already-created visual preserves its durable bindings", () => {
	const [artifact] = eventsToArtifacts([
		runtimeEvent(1, "visual.created", {
			visualId: "visual_gepa_1",
			title: "GEPA draft",
			templateId: "optimizer.gepa.live.v1",
			bindings: { slots: [{ slot: "optimizer_run", source: "opt_1" }] }
		}),
		runtimeEvent(2, "visual.show", {
			visualId: "visual_gepa_1",
			title: "GEPA · Banking77",
			templateId: "optimizer.gepa.live.v1"
		})
	]);

	assert.equal(artifact.title, "GEPA · Banking77");
	assert.deepEqual(artifact.bindings, {
		slots: [{ slot: "optimizer_run", source: "opt_1" }]
	});
});

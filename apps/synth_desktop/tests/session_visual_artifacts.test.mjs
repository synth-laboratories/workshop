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

const { eventsToArtifacts, ownedChatArtifacts, openArtifactIdForChat } = await import(pathToFileURL(compiled).href);

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

const toolEvent = (sequence, tool, args, visual, sessionId = "session_current") => ({
	schemaVersion: "synth.desktop-runtime-event.v1",
	sessionId,
	sequence,
	eventKind: "codex.tool_result",
	payload: {
		item: {
			server: "synth_visuals",
			tool,
			arguments: JSON.stringify(args),
			result: { structuredContent: { visual } }
		}
	},
	createdAt: `2026-08-13T00:01:0${sequence}.000Z`,
	source: "intern"
});

test("looking at another chat's visual does not put it in this chat's outputs", () => {
	// Five parallel Craftax chats shared one instance-global visual registry.
	// Any call that happened to return a visual record made it this chat's
	// output, so inspection contaminated ownership.
	const foreign = {
		id: "visual_seed_204",
		templateId: "live.craftax.v1",
		title: "Craftax seed 204",
		sessionId: "session_other"
	};
	assert.deepEqual(
		eventsToArtifacts([
			toolEvent(1, "visual_manage", { operation: "get", arguments: { visual_id: "visual_seed_204" } }, foreign),
			toolEvent(2, "visual_manage", { operation: "show", arguments: { visual_id: "visual_seed_204" } }, foreign),
			toolEvent(3, "visual_manage", { operation: "capture_review", arguments: { visual_id: "visual_seed_204" } }, foreign),
			toolEvent(4, "visual_manage", { operation: "review", arguments: { visual_id: "visual_seed_204" } }, foreign)
		]),
		[]
	);
});

test("creating a visual in this chat makes it this chat's output", () => {
	const [artifact] = eventsToArtifacts([
		toolEvent(1, "visual_manage", { operation: "create", arguments: { template_id: "live.craftax.v1" } }, {
			id: "visual_seed_201",
			templateId: "live.craftax.v1",
			title: "Craftax seed 201",
			sessionId: "session_current"
		})
	]);
	assert.equal(artifact.id, "visual_seed_201");
});

test("a visual authored by another session is never adopted, whatever the operation", () => {
	assert.deepEqual(
		eventsToArtifacts([
			toolEvent(1, "visual_manage", { operation: "update", arguments: { visual_id: "visual_seed_205" } }, {
				id: "visual_seed_205",
				templateId: "live.craftax.v1",
				title: "Craftax seed 205",
				sessionId: "session_other"
			})
		]),
		[]
	);
});

test("showing a foreign visual displays it without claiming it", () => {
	assert.deepEqual(
		eventsToArtifacts([
			runtimeEvent(1, "visual.show", {
				visualId: "visual_seed_202",
				title: "Craftax seed 202",
				templateId: "live.craftax.v1",
				ownerSessionId: "session_other"
			})
		]),
		[]
	);
});

test("showing this chat's own visual still lists it", () => {
	const [artifact] = eventsToArtifacts([
		runtimeEvent(1, "visual.show", {
			visualId: "visual_seed_203",
			title: "Craftax seed 203",
			templateId: "live.craftax.v1",
			ownerSessionId: "session_current"
		})
	]);
	assert.equal(artifact.id, "visual_seed_203");
});

test("five concurrent task-scoped visuals never leak into another chat's outputs", () => {
	const chats = [1, 2, 3, 4, 5].map((index) => ({
		sessionId: `session_${index}`,
		visual: {
			id: `visual_task_${index}`,
			templateId: "live.craftax.v1",
			title: `Craftax ${index}`,
			sessionId: `session_${index}`
		}
	}));
	for (const chat of chats) {
		const ownCreate = toolEvent(
			1,
			"visual_manage",
			{ operation: "create", arguments: { template_id: "live.craftax.v1" } },
			chat.visual,
			chat.sessionId
		);
		const foreignLooks = chats
			.filter((other) => other.sessionId !== chat.sessionId)
			.flatMap((other, offset) => [
				toolEvent(
					10 + offset,
					"visual_manage",
					{ operation: "get", arguments: { visual_id: other.visual.id } },
					other.visual,
					chat.sessionId
				),
				toolEvent(
					20 + offset,
					"visual_manage",
					{ operation: "show", arguments: { visual_id: other.visual.id } },
					other.visual,
					chat.sessionId
				),
				toolEvent(
					30 + offset,
					"visual_manage",
					{ operation: "capture_review", arguments: { visual_id: other.visual.id } },
					other.visual,
					chat.sessionId
				)
			]);
		const artifacts = eventsToArtifacts([ownCreate, ...foreignLooks]);
		assert.equal(artifacts.length, 1, `${chat.sessionId} must keep one output`);
		assert.equal(artifacts[0].id, chat.visual.id);
	}
});

test("Outputs lists only this chat's owned visuals", () => {
	const artifacts = [
		{ id: "visual_own", title: "Own", templateId: "live.craftax.v1", ownerSessionId: "session_1" },
		{ id: "visual_other", title: "Other", templateId: "live.craftax.v1", ownerSessionId: "session_2" },
		{ id: "subagents", title: "Subagents", templateId: "synth.subagents.v1" }
	];
	const owned = ownedChatArtifacts("session_1", artifacts);
	assert.deepEqual(owned.map((artifact) => artifact.id), ["visual_own", "subagents"]);
});

test("an open pane clears when the artifact is not in this chat", () => {
	const artifacts = [{ id: "visual_own", title: "Own", templateId: "live.craftax.v1" }];
	assert.equal(openArtifactIdForChat("visual_own", artifacts), "visual_own");
	assert.equal(openArtifactIdForChat("visual_foreign", artifacts), null);
});

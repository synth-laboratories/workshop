import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const source = join(appRoot, "src/renderer/src/runtime/modelSwitchPlan.ts");
const compiled = join(compiledDir, "modelSwitchPlan.mjs");
writeFileSync(compiled, transformSync(readFileSync(source, "utf8"), {
	loader: "ts",
	format: "esm",
	target: "es2022",
	sourcefile: source
}).code);

const {
	planModelChipChange,
	planEffortChipChange,
	planComposerSend,
	threadHasHistoryFromEvents
} = await import(pathToFileURL(compiled).href);

test("model chip change only updates pending target — no landing, compact, or rebind", () => {
	const plan = planModelChipChange({ nextTargetId: "openrouter-luna" });
	assert.deepEqual(plan, {
		pendingTargetId: "openrouter-luna",
		kickToLanding: false,
		compact: false,
		rebind: false
	});
});

test("effort chip change never compact or rebinds", () => {
	assert.deepEqual(planEffortChipChange(), {
		persistKnob: true,
		compact: false,
		rebind: false
	});
});

test("send with matching pending and session targets is a plain turn/start", () => {
	const plan = planComposerSend({
		pendingTargetId: "local-laguna",
		sessionTargetId: "local-laguna",
		threadHasHistory: true,
		turnRunning: false,
		hasPendingImages: false,
		destinationSupportsImages: false
	});
	assert.deepEqual(plan, {
		kind: "turn_start",
		targetId: "local-laguna",
		compact: false,
		rebind: false
	});
});

test("send after model chip change compact+rebinds when the thread has history", () => {
	const plan = planComposerSend({
		pendingTargetId: "openrouter-luna",
		sessionTargetId: "local-laguna",
		threadHasHistory: true,
		turnRunning: false,
		hasPendingImages: false,
		destinationSupportsImages: true
	});
	assert.deepEqual(plan, {
		kind: "model_switch_then_turn",
		sourceTargetId: "local-laguna",
		destinationTargetId: "openrouter-luna",
		compact: true,
		rebind: true
	});
});

test("empty-thread model switch skips compact but still rebinds", () => {
	const plan = planComposerSend({
		pendingTargetId: "openrouter-luna",
		sessionTargetId: "local-laguna",
		threadHasHistory: false,
		turnRunning: false,
		hasPendingImages: false,
		destinationSupportsImages: true
	});
	assert.equal(plan.kind, "model_switch_then_turn");
	assert.equal(plan.compact, false);
	assert.equal(plan.rebind, true);
});

test("fiddle A→B→A without sending is just a matching turn_start on A", () => {
	const chip = planModelChipChange({ nextTargetId: "openrouter-luna" });
	assert.equal(chip.pendingTargetId, "openrouter-luna");
	const back = planModelChipChange({ nextTargetId: "local-laguna" });
	const plan = planComposerSend({
		pendingTargetId: back.pendingTargetId,
		sessionTargetId: "local-laguna",
		threadHasHistory: true,
		turnRunning: false,
		hasPendingImages: false,
		destinationSupportsImages: false
	});
	assert.equal(plan.kind, "turn_start");
	assert.equal(plan.compact, false);
});

test("running turn blocks model switch but not same-model sends", () => {
	const switching = planComposerSend({
		pendingTargetId: "openrouter-luna",
		sessionTargetId: "local-laguna",
		threadHasHistory: true,
		turnRunning: true,
		hasPendingImages: false,
		destinationSupportsImages: true
	});
	assert.equal(switching.kind, "block");
	assert.equal(switching.reason, "turn_running");

	const sameModel = planComposerSend({
		pendingTargetId: "local-laguna",
		sessionTargetId: "local-laguna",
		threadHasHistory: true,
		turnRunning: true,
		hasPendingImages: false,
		destinationSupportsImages: false
	});
	assert.equal(sameModel.kind, "turn_start");
});

test("pending images to a text-only destination are blocked on send", () => {
	const plan = planComposerSend({
		pendingTargetId: "local-laguna",
		sessionTargetId: "openrouter-luna",
		threadHasHistory: true,
		turnRunning: false,
		hasPendingImages: true,
		destinationSupportsImages: false
	});
	assert.equal(plan.kind, "block");
	assert.equal(plan.reason, "images_unsupported_on_destination");
});

test("threadHasHistoryFromEvents treats prior user turns as history", () => {
	assert.equal(threadHasHistoryFromEvents([]), false);
	assert.equal(threadHasHistoryFromEvents([
		{ eventKind: "message.created", payload: { role: "user", content: "hi" } }
	]), true);
	assert.equal(threadHasHistoryFromEvents([
		{ eventKind: "run.started", payload: {} }
	]), true);
});

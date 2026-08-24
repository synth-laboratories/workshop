import assert from "node:assert/strict";
import test from "node:test";
import { chatIsWarmingUp } from "../src/renderer/src/runtime/chatWarmingState.ts";

const base = {
	running: true,
	targetKind: "cloud",
	targetModel: "synth_internal/laguna-xs-2.1-nvfp4",
	lastMessageRole: "user",
	localPhase: null,
	localLoadedModel: null
};

test("hosted Laguna shows warmup before the first model output", () => {
	assert.equal(chatIsWarmingUp(base), true);
});

test("hosted Laguna leaves warmup when model output begins", () => {
	assert.equal(chatIsWarmingUp({ ...base, lastMessageRole: "assistant" }), false);
});

test("unrelated cloud models never claim a Laguna warmup", () => {
	assert.equal(chatIsWarmingUp({ ...base, targetModel: "gpt-5.6-luna" }), false);
});

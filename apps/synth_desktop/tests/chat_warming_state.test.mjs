import assert from "node:assert/strict";
import test from "node:test";
import { chatInferencePhase, chatIsWarmingUp } from "../src/renderer/src/runtime/chatWarmingState.ts";

const base = {
	running: true,
	targetKind: "cloud",
	targetModel: "synth_internal/laguna-xs-2.1-nvfp4",
	lastMessageRole: "user",
	localPhase: null,
	localLoadedModel: null
};

test("hosted Laguna shows warmup before the first model output", () => {
	assert.equal(chatInferencePhase(base), "warming");
	assert.equal(chatIsWarmingUp(base), true);
});

test("hosted Laguna leaves warmup when model output begins", () => {
	assert.equal(chatInferencePhase({ ...base, lastMessageRole: "assistant" }), "working");
	assert.equal(chatIsWarmingUp({ ...base, lastMessageRole: "assistant" }), false);
});

test("authoritative Shoal lifecycle replaces first-token inference", () => {
	assert.equal(chatInferencePhase({ ...base, lastMessageRole: "assistant", hostedPhase: "provisioning" }), "warming");
	assert.equal(chatInferencePhase({ ...base, lastMessageRole: "user", hostedPhase: "warming" }), "warming");
	assert.equal(chatInferencePhase({ ...base, lastMessageRole: "user", hostedPhase: "ready" }), "working");
	assert.equal(chatInferencePhase({ ...base, lastMessageRole: "user", hostedPhase: "running" }), "working");
});

test("unrelated cloud models never claim a Laguna warmup", () => {
	assert.equal(chatIsWarmingUp({ ...base, targetModel: "gpt-5.6-luna" }), false);
});

test("local and hosted Laguna share the same lifecycle phases", () => {
	const local = {
		...base,
		targetKind: "local",
		targetModel: null,
		localPhase: "loading",
		localLoadedModel: null
	};
	assert.equal(chatInferencePhase(local), chatInferencePhase(base));
	assert.equal(
		chatInferencePhase({ ...local, localPhase: "ready", localLoadedModel: "laguna" }),
		chatInferencePhase({ ...base, lastMessageRole: "assistant" })
	);
	assert.equal(chatInferencePhase({ ...local, running: false }), "idle");
});

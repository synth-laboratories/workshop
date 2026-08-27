import assert from "node:assert/strict";
import test from "node:test";
import { turnPerformanceLabels } from "../src/renderer/src/hooks/useTurnPerformanceLabels.ts";
import { codexEventToRuntime } from "../src/renderer/src/runtime/nativeCodex.ts";

const at = (seconds) => new Date(Date.UTC(2026, 0, 1, 0, 0, seconds)).toISOString();
const event = (sequence, seconds, eventKind, payload = {}) => ({ sequence, createdAt: at(seconds), eventKind, payload });

/** One backend measurement, as the pump publishes it onto the journal. */
const measured = (sequence, seconds, itemId, overrides = {}) => event(sequence, seconds, "turn/generationSpeed", {
	schemaVersion: "synth.generation-speed.v1",
	measurementKind: "observed_stream_segment",
	itemId,
	responseId: null,
	outputIndex: 0,
	contentIndex: 0,
	phase: "final_answer",
	status: "completed",
	tps: 50,
	exactTokensAfterFirstSample: 60,
	durationMs: 1_200,
	sampleCount: 4,
	tokenCountSource: "provider_item_usage",
	clockSource: "workshop_monotonic_receive",
	unavailableReason: null,
	qualityFlags: [],
	...overrides
});

/** A turn that answered, called a tool, then answered again. */
function fixture(extra = []) {
	return {
		chat: { id: "s", title: "history", messages: [
			{ id: "u", role: "user", body: "go", at: at(0) },
			{ id: "msg_1", role: "assistant", body: "first", at: at(2) },
			{ id: "msg_2", role: "assistant", body: "final", at: at(8) }
		] },
		events: [
			event(1, 1, "turn/accepted", { turnId: "t" }),
			event(2, 2, "message.delta", { delta: "f", messageId: "msg_1" }),
			measured(3, 4, "msg_1", { phase: "commentary", tps: 71 }),
			event(4, 5, "item/started", { item: { id: "exec-1", type: "commandExecution" } }),
			event(5, 7, "item/completed", { item: { id: "exec-1", type: "commandExecution" } }),
			event(6, 8, "message.delta", { delta: "final", messageId: "msg_2" }),
			measured(7, 10, "msg_2", { tps: 84 }),
			...extra,
			event(20, 11, "run.completed")
		]
	};
}

test("each assistant message shows its own segment measurement, never a turn-wide blend", () => {
	const { chat, events } = fixture();
	const labels = turnPerformanceLabels(chat, events);
	// 18 seconds of tool execution sit between these two segments and are in
	// neither of them; a turn-wide figure could not say either number.
	assert.equal(labels.byMessageId.msg_1.generation, "Observed generation: 71.0 tok/s");
	assert.equal(labels.byMessageId.msg_2.generation, "Observed generation: 84.0 tok/s");
	assert.equal(labels.byMessageId.msg_2.worked, "Worked 10s");
});

test("a measurement without a rate says so and names nothing else", () => {
	const { chat, events } = fixture();
	const unavailable = events.map((entry) => entry.sequence === 7
		? measured(7, 10, "msg_2", { tps: null, status: "unavailable", unavailableReason: "missing_exact_token_source", exactTokensAfterFirstSample: 0 })
		: entry);
	const labels = turnPerformanceLabels(chat, unavailable);
	assert.equal(labels.byMessageId.msg_2.generation, "Generation speed unavailable");
	assert.match(labels.byMessageId.msg_2.detail, /reason missing_exact_token_source/);
	// The earlier segment's rate is never borrowed to fill the gap.
	assert.doesNotMatch(labels.byMessageId.msg_2.generation, /71|tok\/s/);
});

test("late exact response usage is labelled as a generation estimate, not request throughput", () => {
	const { chat, events } = fixture();
	const estimated = events.map((entry) => entry.sequence === 7
		? measured(7, 10, "msg_2", {
			tokenCountSource: "provider_response_visible_usage",
			tps: 71.08,
			exactTokensAfterFirstSample: 283,
			durationMs: 3_981.5
		})
		: entry);
	const label = turnPerformanceLabels(chat, estimated).byMessageId.msg_2;
	assert.equal(label.generation, "Observed generation estimate: 71.1 tok/s");
	assert.match(label.detail, /Exact response output minus exact reasoning output/);
	assert.match(label.detail, /excludes TTFT, tools, and reasoning time/);
});

test("provider token totals never become acceptance-to-completion TPS", () => {
	const chat = { id: "s", title: "none", messages: [
		{ id: "u", role: "user", body: "go", at: at(0) },
		{ id: "msg_a", role: "assistant", body: "answer", at: at(2) }
	] };
	const events = [
		event(1, 1, "turn/accepted"),
		event(2, 2, "message.delta", { delta: "a", messageId: "msg_a" }),
		event(3, 3, "message.delta", { delta: "b", messageId: "msg_a" }),
		event(4, 4, "thread/tokenUsage/updated", { tokenUsage: { last: { outputTokens: 322 } } }),
		event(5, 5, "run.completed")
	];
	const labels = turnPerformanceLabels(chat, events);
	assert.equal(labels.byMessageId.msg_a.generation, "Generation speed unavailable");
	assert.equal(labels.byMessageId.msg_a.detail, null);
});

test("Core event timestamps survive Codex adaptation", () => {
	const createdAt = at(7);
	const runtime = codexEventToRuntime({ sessionId: "s", method: "turn/completed", params: {}, createdAt }, 3);
	assert.equal(runtime.createdAt, createdAt);
});

test("a hosted model never falls back to a persisted end-to-end rate", () => {
	const chat = { id: "hosted", title: "hosted", messages: [
		{ id: "u", role: "user", body: "go", at: at(0) },
		{ id: "msg_hosted", role: "assistant", body: "answer", at: at(2) }
	] };
	const events = [event(1, 1, "turn/accepted"), event(2, 4, "turn/completed")];
	const samples = [{
		runId: "turn-hosted",
		measurementKind: "end_to_end",
		startedAtMs: Date.parse(at(1)),
		completedAtMs: Date.parse(at(4)),
		outputTps: 24.75
	}];
	const label = turnPerformanceLabels(chat, events, false, samples).byMessageId.msg_hosted;
	assert.equal(label.generation, "Generation speed unavailable");
	assert.equal(label.detail, null);
});

test("a persisted end-to-end rate stays hidden while UI activity lingers", () => {
	const chat = { id: "hosted", title: "hosted", messages: [
		{ id: "u", role: "user", body: "go", at: at(0) },
		{ id: "msg_hosted", role: "assistant", body: "answer", at: at(2) }
	] };
	const events = [event(1, 1, "turn/accepted"), event(2, 4, "turn/completed")];
	const samples = [{
		runId: "turn-hosted",
		measurementKind: "end_to_end",
		startedAtMs: Date.parse(at(1)),
		completedAtMs: Date.parse(at(4)),
		outputTps: 24.75
	}];
	const label = turnPerformanceLabels(chat, events, true, samples).byMessageId.msg_hosted;
	assert.equal(label.generation, "Generation speed unavailable");
});

test("an interrupted segment is labelled partial rather than presented as a headline", () => {
	const { chat, events } = fixture();
	const cut = events.map((entry) => entry.sequence === 7
		? measured(7, 10, "msg_2", { status: "partial", tps: 84 })
		: entry);
	const labels = turnPerformanceLabels(chat, cut);
	assert.equal(labels.byMessageId.msg_2.generation, "Observed generation: 84.0 tok/s (partial)");
	assert.match(labels.byMessageId.msg_2.detail, /partial/);
});

test("a measurement from an older schema is never rendered as a measurement", () => {
	const { chat, events } = fixture();
	const legacy = events.map((entry) => entry.sequence === 7
		? event(7, 10, "turn/generationSpeed", { schemaVersion: "synth.generation-speed.v0", itemId: "msg_2", tps: 643 })
		: entry);
	const labels = turnPerformanceLabels(chat, legacy);
	assert.equal(labels.byMessageId.msg_2.generation, "Generation speed unavailable");
});

test("no live figure is shown while a segment is still streaming", () => {
	const { chat, events } = fixture();
	const streaming = events.filter((entry) => entry.eventKind !== "run.completed" && entry.sequence !== 7);
	assert.equal(turnPerformanceLabels(chat, streaming, true).live, null);
	assert.equal(turnPerformanceLabels(chat, events).live, null);
});

test("late, duplicate, and out-of-order journal traffic never rewrites an earlier segment", () => {
	const before = turnPerformanceLabels(fixture().chat, fixture().events).byMessageId.msg_1;
	const later = fixture([
		measured(8, 10, "msg_2", { tps: 999 }),
		event(30, 30, "thread/tokenUsage/updated", { tokenUsage: { last: { outputTokens: 999 } } })
	]);
	assert.deepEqual(turnPerformanceLabels(later.chat, later.events).byMessageId.msg_1, before);
});

test("the advanced detail exposes the audit fields behind a displayed value", () => {
	const { chat, events } = fixture();
	const detail = turnPerformanceLabels(chat, events).byMessageId.msg_2.detail;
	for (const field of [
		/Client-observed text delivery; excludes TTFT, tools, and reasoning time/,
		/kind observed_stream_segment/,
		/tokens 60/,
		/duration 1\.20s/,
		/samples 4/,
		/token source provider_item_usage/,
		/clock workshop_monotonic_receive/,
		/segment msg_2:0:0/
	]) {
		assert.match(detail, field);
	}
});

test("reload, pagination, and compaction preserve snapshots without startup polling", () => {
	const { chat, events } = fixture();
	const expected = turnPerformanceLabels(chat, events);
	const restored = [
		event(-2, -2, "run.completed"),
		event(-1, -1, "thread/compacted", { source: "automatic" }),
		...structuredClone(events)
	];
	assert.deepEqual(turnPerformanceLabels(structuredClone(chat), restored), expected);
	const source = turnPerformanceLabels.toString();
	assert.doesNotMatch(source, /setInterval|Date\.now/);
});

test("the renderer never renders NaN, infinity, a negative rate, or the word median", () => {
	const { chat } = fixture();
	for (const tps of [Number.NaN, Number.POSITIVE_INFINITY, -12, 0]) {
		const labels = turnPerformanceLabels(chat, [measured(1, 4, "msg_2", { tps })]);
		assert.equal(labels.byMessageId.msg_2.generation, "Generation speed unavailable");
	}
	assert.doesNotMatch(turnPerformanceLabels.toString(), /median/i);
});

test("a long transcript projection remains bounded for startup and scrolling", () => {
	const messages = [{ id: "u", role: "user", body: "go", at: at(0) }];
	const events = [event(1, 1, "turn/accepted")];
	for (let index = 0; index < 200; index += 1) {
		messages.push({ id: `msg_${index}`, role: "assistant", body: "x", at: at(index + 2) });
		events.push(event(index * 3 + 2, index + 2, "message.delta", { delta: "x", messageId: `msg_${index}` }));
		events.push(measured(index * 3 + 3, index + 2.5, `msg_${index}`));
	}
	events.push(event(10_000, 203, "run.completed"));
	const started = performance.now();
	turnPerformanceLabels({ id: "large", title: "large", messages }, events);
	assert.ok(performance.now() - started < 500, "temporal projection regressed transcript startup");
});

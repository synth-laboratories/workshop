import assert from "node:assert/strict";
import test from "node:test";
import { turnPerformanceLabels } from "../src/renderer/src/hooks/useTurnPerformanceLabels.ts";

const at = (seconds) => new Date(Date.UTC(2026, 0, 1, 0, 0, seconds)).toISOString();
const event = (sequence, seconds, eventKind, payload = {}) => ({ sequence, createdAt: at(seconds), eventKind, payload });

function fixture(extra = []) {
	return {
		chat: { id: "s", title: "history", messages: [
			{ id: "u", role: "user", body: "go", at: at(0) },
			{ id: "a1", role: "assistant", body: "first", at: at(2) },
			{ id: "a2", role: "assistant", body: "final", at: at(8) }
		] },
		events: [
			event(1, 1, "turn/accepted", { turnId: "t" }),
			event(2, 2, "message.delta", { delta: "f" }),
			event(3, 3, "message.delta", { delta: "irst" }),
			event(4, 4, "thread/tokenUsage/updated", { tokenUsage: { last: { outputTokens: 10 } } }),
			event(5, 8, "message.delta", { delta: "final" }),
			event(6, 9, "message.delta", { delta: "." }),
			event(7, 10, "thread/tokenUsage/updated", { tokenUsage: { last: { outputTokens: 30 } } }),
			...extra,
			event(20, 11, "run.completed")
		]
	};
}

test("multiple telemetry updates freeze distinct historical snapshots", () => {
	const { chat, events } = fixture();
	const labels = turnPerformanceLabels(chat, events);
	assert.equal(labels.byMessageId.a1.generation, "10.0 tok/s generation speed");
	assert.equal(labels.byMessageId.a2.generation, "15.0 tok/s generation speed");
	assert.equal(labels.byMessageId.a2.worked, "Worked 10s");
});

test("late, duplicate, and out-of-order telemetry never rewrites an earlier segment", () => {
	const base = fixture();
	const before = turnPerformanceLabels(base.chat, base.events).byMessageId.a1;
	const later = fixture([
		event(8, 10, "thread/tokenUsage/updated", { tokenUsage: { last: { outputTokens: 30 } } }),
		event(9, 10, "thread/tokenUsage/updated", { tokenUsage: { last: { outputTokens: 12 } } }),
		event(30, 30, "thread/tokenUsage/updated", { tokenUsage: { last: { outputTokens: 999 } } })
	]);
	assert.deepEqual(turnPerformanceLabels(later.chat, later.events).byMessageId.a1, before);
});

test("tool-only gaps are excluded and missing usage/timing stays unavailable", () => {
	const chat = { id: "s", title: "gaps", messages: [
		{ id: "u", role: "user", body: "go", at: at(0) },
		{ id: "a", role: "assistant", body: "answer", at: at(2) }
	] };
	const noUsage = [event(1, 1, "turn/accepted"), event(2, 2, "message.delta", { delta: "a" }), event(3, 9, "message.delta", { delta: "b" }), event(4, 10, "run.completed")];
	assert.equal(turnPerformanceLabels(chat, noUsage).byMessageId.a.generation, "Generation speed unavailable");
	const withUsage = [...noUsage.slice(0, -1), event(4, 9, "usage.updated", { outputTokens: 10 }), event(5, 10, "run.completed")];
	assert.equal(turnPerformanceLabels(chat, withUsage).byMessageId.a.generation, "Generation speed unavailable");
});

test("only accepted steering resets elapsed duration; reconnect replay is stable", () => {
	const value = fixture([event(8, 6, "message.created", { role: "user", content: "queued" })]);
	value.chat.messages.splice(2, 0, { id: "queued", role: "user", body: "queued", at: at(6) });
	const { chat, events } = value;
	assert.equal(turnPerformanceLabels(chat, events).byMessageId.a2.worked, "Worked 10s");
	const accepted = [...events, event(19, 7, "turn/accepted", { turnId: "t" })].sort((a, b) => a.sequence - b.sequence);
	const once = turnPerformanceLabels(chat, accepted);
	const replayed = turnPerformanceLabels(chat, [...accepted]);
	assert.equal(once.byMessageId.a2.worked, "Worked 4s");
	assert.deepEqual(replayed, once);
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

test("a long transcript projection remains bounded for startup and scrolling", () => {
	const messages = [{ id: "u", role: "user", body: "go", at: at(0) }];
	const events = [event(1, 1, "turn/accepted")];
	for (let index = 0; index < 200; index += 1) {
		messages.push({ id: `a${index}`, role: "assistant", body: "x", at: at(index + 2) });
		events.push(event(index * 3 + 2, index + 2, "message.delta", { delta: "x" }));
		events.push(event(index * 3 + 3, index + 2.5, "message.delta", { delta: "y" }));
		events.push(event(index * 3 + 4, index + 2.75, "usage.updated", { outputTokens: index + 2 }));
	}
	events.push(event(10_000, 203, "run.completed"));
	const started = performance.now();
	turnPerformanceLabels({ id: "large", title: "large", messages }, events);
	assert.ok(performance.now() - started < 500, "temporal projection regressed transcript startup");
});

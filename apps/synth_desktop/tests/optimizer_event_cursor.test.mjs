import assert from "node:assert/strict";
import test from "node:test";
import {
  mergeOptimizerEventPage,
  optimizerEventSequence,
} from "../src/renderer/src/runtime/optimizerEventCursor.ts";

const event = (sequence) => ({ eventId: `e${sequence}`, sequenceNumber: sequence });

test("optimizer event pages append incrementally and de-duplicate replay", () => {
  let state = { events: [], cursor: 0, gap: false };
  state = mergeOptimizerEventPage(state, [event(1), event(2)]);
  state = mergeOptimizerEventPage(state, [event(2), event(3)]);
  assert.equal(state.cursor, 3);
  assert.deepEqual(state.events.map(optimizerEventSequence), [1, 2, 3]);
  assert.equal(state.gap, false);
});

test("optimizer event cursor detects a missing durable page", () => {
  const state = mergeOptimizerEventPage(
    { events: [event(1)], cursor: 1, gap: false },
    [event(3)],
  );
  assert.equal(state.cursor, 3);
  assert.equal(state.gap, true);
});

test("optimizer event cursor rejects missing sequence", () => {
  assert.throws(
    () => mergeOptimizerEventPage({ events: [], cursor: 0, gap: false }, [{ eventId: "bad" }]),
    /missing a valid sequence number/,
  );
});

test("optimizer event replay is idempotent but sequence equivocation fails closed", () => {
  const original = { sequenceNumber: 1, eventId: "e1", delta: { scored: 9, failed: 1 } };
  const replay = { delta: { failed: 1, scored: 9 }, eventId: "e1", sequenceNumber: 1 };
  const state = mergeOptimizerEventPage({ events: [], cursor: 0, gap: false }, [original]);
  assert.doesNotThrow(() => mergeOptimizerEventPage(state, [replay]));
  assert.throws(
    () => mergeOptimizerEventPage(state, [{ ...original, delta: { scored: 10, failed: 0 } }]),
    /replayed with different content/,
  );
});

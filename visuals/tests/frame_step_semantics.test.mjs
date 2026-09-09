import assert from "node:assert/strict";
import test from "node:test";
import { craftaxTraceFromSealedTrace } from "../runtime/craftaxTraceView.ts";

// Fixtures. Each document is hand-written to the Trace V5 shape and named for
// the producer behaviour it stands for; none is a captured trace. The one
// exception is labelled where it appears.
function sealed(events) {
  return {
    schema_version: "synth.trace.v5",
    trace_kind: "agent_rollout",
    trace_id: "roll_fixture",
    content_digest: "sha256:fixture",
    completeness: {},
    sessions: [],
    usage: {},
    events: events.map((event, index) => ({
      order: { ordinal: index + 1 },
      payload: { source_event_type: event.kind, ...event.payload }
    }))
  };
}

const identity = { traceId: "roll_fixture", scenario: "fixture", seed: null, status: "completed" };

test("a frame ordinal filed under step is refused, not displayed as a step", () => {
  // The RuneBench shape: one frame per capture tick, numbered sequentially,
  // while the episode itself only ever reaches step 2.
  const events = [
    { kind: "observation", payload: { step: 1 } },
    { kind: "action", payload: { step: 1, action: "chop" } },
    { kind: "observation", payload: { step: 2 } },
    { kind: "action", payload: { step: 2, action: "chop" } }
  ];
  for (let ordinal = 0; ordinal < 8; ordinal += 1) {
    events.push({ kind: "frame", payload: { step: ordinal, frame_index: ordinal, format: "png" } });
  }

  const view = craftaxTraceFromSealedTrace(sealed(events), identity);

  assert.equal(view.frames.length, 8);
  assert.equal(view.coverage.frameSteps, "frame_ordinal");
  assert.deepEqual(view.frames.map((frame) => frame.position), [1, 2, 3, 4, 5, 6, 7, 8]);
  // Position is always known; the refuted claim leaves no environment step.
  assert.deepEqual(view.frames.map((frame) => frame.step), new Array(8).fill(null));
  assert.equal(view.coverage.frameStepReasons.length, 1);
  assert.match(view.coverage.frameStepReasons[0], /environment events only reach step 2/);
});

test("a producer whose frame steps stay inside the episode keeps them", () => {
  const view = craftaxTraceFromSealedTrace(sealed([
    { kind: "observation", payload: { step: 1 } },
    { kind: "frame", payload: { step: 1, format: "png" } },
    { kind: "action", payload: { step: 1, action: "chop" } },
    { kind: "observation", payload: { step: 2 } },
    { kind: "frame", payload: { step: 2, format: "png" } },
    { kind: "frame", payload: { step: 2, format: "png" } }
  ]), identity);

  assert.equal(view.coverage.frameSteps, "producer");
  assert.deepEqual(view.frames.map((frame) => frame.step), [1, 2, 2]);
  assert.deepEqual(view.frames.map((frame) => frame.position), [1, 2, 3]);
  assert.deepEqual(view.coverage.frameStepReasons, []);
});

test("one frame per step is not mistaken for an ordinal", () => {
  // step === position here, legitimately. The check must not fire: the claim
  // never leaves the range the episode establishes.
  const view = craftaxTraceFromSealedTrace(sealed([
    { kind: "observation", payload: { step: 1 } },
    { kind: "frame", payload: { step: 1, format: "png" } },
    { kind: "observation", payload: { step: 2 } },
    { kind: "frame", payload: { step: 2, format: "png" } },
    { kind: "observation", payload: { step: 3 } },
    { kind: "frame", payload: { step: 3, format: "png" } }
  ]), identity);

  assert.equal(view.coverage.frameSteps, "producer");
  assert.deepEqual(view.frames.map((frame) => frame.step), [1, 2, 3]);
});

test("a trace with no environment step evidence cannot refute the producer", () => {
  // Nothing here establishes a ceiling, so the claim stands. Refusing it would
  // be guessing in the other direction.
  const view = craftaxTraceFromSealedTrace(sealed([
    { kind: "frame", payload: { step: 40, format: "png" } },
    { kind: "frame", payload: { step: 41, format: "png" } }
  ]), identity);

  assert.equal(view.coverage.frameSteps, "producer");
  assert.deepEqual(view.frames.map((frame) => frame.step), [40, 41]);
});

test("frames that declare no step at all report absent, never a position", () => {
  const view = craftaxTraceFromSealedTrace(sealed([
    { kind: "observation", payload: { step: 1 } },
    { kind: "frame", payload: { format: "png" } },
    { kind: "frame", payload: { format: "png" } }
  ]), identity);

  assert.equal(view.coverage.frameSteps, "absent");
  assert.deepEqual(view.frames.map((frame) => frame.step), [null, null]);
  assert.deepEqual(view.frames.map((frame) => frame.position), [1, 2]);
});

test("a refused frame step does not become a call's turn range", () => {
  // turn_start/turn_end fall back to the frames a call owns. With the claim
  // refused there is no turn to report, and reporting one would reintroduce
  // the ordinal through a second door.
  const events = [
    { kind: "observation", payload: { step: 1 } },
    { kind: "span.policy.opened", payload: {} }
  ];
  for (let ordinal = 0; ordinal < 5; ordinal += 1) {
    events.push({ kind: "frame", payload: { step: ordinal, format: "png" } });
  }

  const view = craftaxTraceFromSealedTrace(sealed(events), identity);

  const call = view.steps.find((step) => step.kind === "policy_call");
  assert.ok(call, "the fixture opens one policy call");
  assert.equal(call.turn_start, null);
  assert.equal(call.turn_end, null);
});

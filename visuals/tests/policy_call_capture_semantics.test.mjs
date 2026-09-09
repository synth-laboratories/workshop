import assert from "node:assert/strict";
import test from "node:test";
import {
  craftaxTraceFromSealedTrace,
  policyCallCoverageFromSealedTrace
} from "../runtime/craftaxTraceView.ts";

// Fixtures. Each document is hand-written to the Trace V5 shape and named for
// the producer behaviour it stands for; none is a captured trace.
function sealed({ events, completeness = {}, sessions = [], usage = {} }) {
  return {
    schema_version: "synth.trace.v5",
    trace_kind: "agent_rollout",
    trace_id: "roll_fixture",
    content_digest: "sha256:fixture",
    completeness,
    sessions,
    usage,
    events: events.map((event, index) => ({
      order: { ordinal: index + 1 },
      payload: { source_event_type: event.kind, ...event.payload }
    }))
  };
}

const identity = { traceId: "roll_fixture", scenario: "fixture", seed: null, status: "completed" };

test("a producer that records no call boundaries yields segments, not aborted calls", () => {
  // Shaped after the retained RuneBench promotion: application events only,
  // with the producer declaring partial model-call capture.
  const document = sealed({
    completeness: {
      model_calls: "partial",
      raw_provider: "unavailable",
      reasons: ["promoted from the durable application-event journal"]
    },
    usage: { provenance: "unavailable" },
    events: [
      { kind: "frame", payload: { step: 1, format: "png" } },
      { kind: "observation", payload: { step: 1, readout: { observation_text: "you see a tree" } } },
      { kind: "action", payload: { step: 1, action: "chop" } },
      { kind: "frame", payload: { step: 2, format: "png" } },
      { kind: "action", payload: { step: 2, action: "chop" } }
    ]
  });

  const view = craftaxTraceFromSealedTrace(document, { ...identity, parentTerminal: true });

  assert.equal(view.steps.length, 1);
  const [segment] = view.steps;
  assert.equal(segment.kind, "uncaptured_segment");
  assert.equal(segment.status, "not_captured");
  assert.equal(segment.closure, null, "a segment has no call outcome to close");
  // The whole point: a terminal parent must not turn "no call was recorded"
  // into "a call was aborted".
  assert.notEqual(segment.status, "aborted");

  assert.equal(view.coverage.modelCalls, "partial");
  assert.equal(view.coverage.rawProvider, "unavailable");
  assert.deepEqual(view.coverage.modelCallReasons, [
    "promoted from the durable application-event journal"
  ]);
});

test("call counts are never reconstructed from frames or actions", () => {
  const document = sealed({
    completeness: { model_calls: "unavailable", raw_provider: "unavailable" },
    events: [
      ...Array.from({ length: 20 }, (_, index) => ({ kind: "frame", payload: { step: index + 1, format: "png" } })),
      ...Array.from({ length: 10 }, (_, index) => ({ kind: "action", payload: { step: index + 1, action: "chop" } }))
    ]
  });

  const view = craftaxTraceFromSealedTrace(document, { ...identity, parentTerminal: true });

  assert.equal(view.run.usage.calls, null, "an uncounted call is null, never a frame or action count");
  assert.equal(view.steps.length, 1);
  assert.equal(view.steps[0].kind, "uncaptured_segment");
  // Frames that precede everything still belong to the run's single segment
  // rather than being retained under no step at all.
  assert.equal(view.frames.length, 20);
  assert.equal(view.steps[0].frames.length, 20);
});

test("a genuinely unclosed call is still aborted by its terminal parent", () => {
  const document = sealed({
    completeness: { model_calls: "complete", raw_provider: "complete" },
    events: [
      { kind: "observation", payload: { step: 1, readout: { observation_text: "you see a tree" } } },
      { kind: "span.policy.opened", payload: { call_number: 1 } },
      { kind: "action", payload: { step: 1, action: "chop" } }
    ]
  });

  const view = craftaxTraceFromSealedTrace(document, { ...identity, parentTerminal: true });

  assert.equal(view.steps.length, 1);
  const [call] = view.steps;
  assert.equal(call.kind, "policy_call");
  assert.equal(call.status, "aborted");
  assert.equal(call.closure?.reason, "parent_terminal_before_policy_close");
  assert.equal(view.run.usage.calls, 1);
});

test("a closed call keeps the producer's own outcome", () => {
  const document = sealed({
    completeness: { model_calls: "complete", raw_provider: "complete" },
    events: [
      { kind: "span.policy.opened", payload: { call_number: 1 } },
      { kind: "action", payload: { step: 1, action: "chop" } },
      { kind: "span.policy.closed", payload: { outcome: "completed" } }
    ]
  });

  const view = craftaxTraceFromSealedTrace(document, { ...identity, parentTerminal: true });

  assert.equal(view.steps[0].kind, "policy_call");
  assert.equal(view.steps[0].status, "completed");
  assert.equal(view.steps[0].closure?.reason, "producer_completed");
  assert.equal(view.coverage.modelCalls, "complete");
});

test("policy data without an opening envelope is still a call, not a segment", () => {
  const document = sealed({
    completeness: { model_calls: "complete", raw_provider: "complete" },
    events: [
      { kind: "span.policy.data", payload: { assistant: { content: "chop the tree" } } },
      { kind: "action", payload: { step: 1, action: "chop" } }
    ]
  });

  const view = craftaxTraceFromSealedTrace(document, { ...identity, parentTerminal: true });

  assert.equal(view.steps[0].kind, "policy_call");
  assert.equal(view.steps[0].status, "aborted", "a malformed envelope is still a call the parent aborted");
  assert.equal(view.run.usage.calls, 1);
});

test("the weakest declaration wins across the document and its sessions", () => {
  const coverage = policyCallCoverageFromSealedTrace({
    completeness: { model_calls: "complete", raw_provider: "complete", reasons: ["document"] },
    sessions: [
      { coverage: { model_calls: "complete", raw_provider: "complete" } },
      { coverage: { model_calls: "partial", raw_provider: "unavailable", reasons: ["agent session"] } }
    ],
    usage: {}
  });

  assert.equal(coverage.modelCalls, "partial");
  assert.equal(coverage.rawProvider, "unavailable");
  assert.deepEqual(coverage.reasons, ["document", "agent session"]);
});

test("a source that declares nothing is unknown, never assumed complete", () => {
  const coverage = policyCallCoverageFromSealedTrace({ events: [] });
  assert.equal(coverage.modelCalls, "unknown");
  assert.equal(coverage.rawProvider, "unknown");

  const document = sealed({ events: [{ kind: "action", payload: { step: 1, action: "chop" } }] });
  const view = craftaxTraceFromSealedTrace(document, identity);
  assert.equal(view.coverage.modelCalls, "unknown");
});

test("unavailable usage provenance downgrades a complete raw-provider claim", () => {
  const coverage = policyCallCoverageFromSealedTrace({
    completeness: { model_calls: "complete", raw_provider: "complete" },
    usage: { provenance: "unavailable" }
  });
  assert.equal(coverage.rawProvider, "partial");
});

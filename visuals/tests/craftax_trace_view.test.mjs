import assert from "node:assert/strict";
import test from "node:test";

import {
  EVAL_TRACE_VIEW_SCHEMA,
  containerEventsFromOptimizerEvents,
  containerEventsFromSealedTrace,
  craftaxTraceFromOptimizerEvents,
  foldCraftaxTrace,
  localMapRows,
  reconcileCraftaxTrace
} from "../runtime/craftaxTraceView.ts";

const CAS = (seed) => `ab${String(seed).padStart(62, "c")}`;

let sequence = 0;
const producer = (kind, payload) => ({ sequence: (sequence += 1), kind, payload });

/** The inspected rollout's shape: two calls, one of which commits two actions. */
function craftaxEvents() {
  sequence = 0;
  return [
    producer("observation", {
      step: 0,
      readout: {
        observation_text: "step 0\nlocal_map:\n....\n.@..\n....\n\ninventory: wood 1",
        local_map: ["....", ".@..", "...."]
      }
    }),
    producer("frame", {
      step: 0,
      format: "png",
      digest: "4e27ac3b1f0a9d55",
      media: { casDigest: CAS(0), mediaType: "image/png", width: 768, height: 768 }
    }),
    producer("span.policy.opened", { call: 0 }),
    producer("span.policy.data", {
      messages: [
        { role: "system", content: "You are playing Craftax." },
        { role: "user", content: "step 0 observation" }
      ],
      assistant: {
        reasoning_content: "wood first",
        content: "Collecting wood.",
        tool_calls: [
          {
            id: "call_0",
            function: { name: "craftax_act", arguments: '{"actions":["do","do","move_up"]}' }
          }
        ]
      },
      usage: { prompt_tokens: 100, completion_tokens: 20 }
    }),
    producer("span.policy.closed", { length: 3 }),
    producer("action_applied", { step: 1, action: "do" }),
    producer("achievement_unlocked", { step: 1, achievement: "collect_wood" }),
    producer("reward_delta", { step: 1, delta: 1 }),
    producer("resource_delta", { step: 1, resource: "wood", before: 0, after: 1 }),
    producer("frame", {
      step: 1,
      format: "png",
      media: { casDigest: CAS(1), mediaType: "image/png", width: 768, height: 768 }
    }),
    producer("action_applied", { step: 2, action: "do" }),
    // The model proposed three actions; the environment refused the third.
    producer("action_rejected", { step: 2, action: "move_up", reason: "blocked by water" }),
    producer("frame", { step: 2, format: "png", mediaError: { detail: "PNG signature mismatch" } }),
    // A second call that is still open: the pane must show it as running.
    producer("span.policy.opened", { call: 1 }),
    producer("span.policy.data", {
      messages: [{ role: "user", content: "step 2 observation" }],
      assistant: { reasoning_content: "stone next" },
      usage: { prompt_tokens: 140, completion_tokens: 5 }
    })
  ];
}

const identity = {
  traceId: "roll_craftax_train_780003_4e27ac3b",
  scenario: "craftax",
  seed: 780003,
  status: "running",
  model: "gpt-5.6-luna",
  relay: { framesDeclared: 3, framesRetained: 2, journalClosed: false, degradations: [] }
};

test("the fold names its schema and keeps every producer event for disclosure", () => {
  const view = foldCraftaxTrace(craftaxEvents(), identity);
  assert.equal(view.schema, EVAL_TRACE_VIEW_SCHEMA);
  assert.equal(view.source_schema, "optimizer_events");
  assert.equal(view.integrity.status, "live");
  assert.equal(view.events.length, 15);
  assert.equal(view.trace_id, identity.traceId);
});

test("applied actions are authoritative and are never mixed with proposals or rejections", () => {
  const view = foldCraftaxTrace(craftaxEvents(), identity);
  const [first] = view.steps;
  // The model asked for three; the environment did two and refused one.
  assert.deepEqual(first.action.proposed, ["do", "do", "move_up"]);
  assert.deepEqual(
    first.action.applied.map((row) => `${row.name}@${row.turn}`),
    ["do@1", "do@2"]
  );
  assert.deepEqual(first.action.rejected, [
    { turn: 2, name: "move_up", reason: "blocked by water" }
  ]);
  // The refused action is not silently omitted and not counted as executed.
  assert.ok(!first.action.applied.some((row) => row.name === "move_up"));
  assert.deepEqual(first.action.noop, []);
});

test("a call that commits a batch owns a range of frames, not one", () => {
  const view = foldCraftaxTrace(craftaxEvents(), identity);
  const [first] = view.steps;
  assert.equal(first.turn_start, 1);
  assert.equal(first.turn_end, 2);
  // The frame before the call, plus the two its actions produced.
  assert.deepEqual(first.frames, [0, 1, 2]);
  assert.equal(view.frames.length, 3);
  assert.equal(view.frames[0].media.casDigest, CAS(0));
});

test("a refused frame keeps its step and says why there are no bytes", () => {
  const view = foldCraftaxTrace(craftaxEvents(), identity);
  const refused = view.frames[2];
  assert.equal(refused.step, 2);
  assert.equal(refused.media, null);
  assert.match(refused.unavailable, /signature mismatch/);
  // The relay receipt travels verbatim: 3 declared, 2 retained.
  assert.equal(view.coverage.framesDeclared, 3);
  assert.equal(view.coverage.framesRetained, 2);
});

test("the producer's 16-character digest is provenance, never the media address", () => {
  const view = foldCraftaxTrace(craftaxEvents(), identity);
  const frame = view.frames[0];
  assert.equal(frame.producerDigest, "4e27ac3b1f0a9d55");
  assert.equal(frame.media.casDigest.length, 64);
  assert.notEqual(frame.producerDigest, frame.media.casDigest);
});

test("reasoning, final content, tool calls and messages each come from their own field", () => {
  const view = foldCraftaxTrace(craftaxEvents(), identity);
  const [first] = view.steps;
  assert.equal(first.content.reasoning, "wood first");
  assert.equal(first.content.message, "Collecting wood.");
  assert.equal(view.system_prompt, "You are playing Craftax.");
  assert.equal(first.content.input_messages.length, 2);
  assert.equal(first.tool_calls[0].name, "craftax_act");
  assert.deepEqual(first.tool_calls[0].arguments, { actions: ["do", "do", "move_up"] });
  assert.deepEqual(first.tokens, { input: 100, output: 20 });
  // The observation standing when the policy was asked, not a later one.
  assert.match(first.content.observation, /^step 0/);
});

test("reward, achievements and state deltas attach to the call whose steps produced them", () => {
  const view = foldCraftaxTrace(craftaxEvents(), identity);
  const [first] = view.steps;
  assert.equal(first.reward, 1);
  assert.deepEqual(first.achievements, ["collect_wood"]);
  assert.deepEqual(first.state_delta, [
    {
      field: "wood",
      before: 0,
      after: 1,
      delta: 1,
      turn: 1,
      source: "resource_delta"
    }
  ]);
  assert.deepEqual(view.achievements, ["collect_wood"]);
  assert.equal(view.total_reward, 1);
});

test("a policy call still in flight stays in the trajectory, marked running", () => {
  const view = foldCraftaxTrace(craftaxEvents(), identity);
  assert.equal(view.steps.length, 2);
  assert.equal(view.steps[0].status, "complete");
  const open = view.steps[1];
  assert.equal(open.status, "running");
  assert.equal(open.content.reasoning, "stone next");
  // It has decided nothing yet, and nothing is invented for it.
  assert.deepEqual(open.action.applied, []);
  assert.equal(open.reward, null);
  assert.equal(view.run.usage.calls, 2);
  assert.equal(view.run.usage.input_tokens, 240);
});

test("a run with no reward evidence reports nothing rather than zero", () => {
  const view = foldCraftaxTrace(
    [producer("span.policy.opened", { call: 0 })],
    { traceId: "t", scenario: "craftax", seed: 0, status: "running" }
  );
  assert.equal(view.total_reward, null);
  assert.equal(view.steps[0].reward, null);
  assert.equal(view.run.cost_usd, null);
});

test("relayed optimizer envelopes are read by trial and deduplicated by sequence", () => {
  const envelope = (sequenceNumber, kind, payload, trialId = "trial:craftax:780003") => ({
    type: "eval.trial.event",
    delta: {
      trial_id: trialId,
      containerEvent: { rollout_id: "roll_x", sequence: sequenceNumber, kind, payload }
    },
    raw: { trial_id: trialId }
  });
  const rows = containerEventsFromOptimizerEvents(
    [
      envelope(1, "observation", { step: 0 }),
      envelope(2, "span.policy.opened", { call: 0 }),
      // The same producer sequence re-offered by a retried page.
      envelope(2, "span.policy.opened", { call: 0 }),
      envelope(3, "action_applied", { step: 1, action: "do" }, "trial:craftax:999"),
      { type: "eval.trial.terminal", item: {} }
    ],
    "trial:craftax:780003"
  );
  assert.deepEqual(
    rows.map((row) => `${row.sequence}:${row.kind}`),
    ["1:observation", "2:span.policy.opened"]
  );
});

test("a sealed Trace V5 document folds through the same rules as the live relay", () => {
  const live = foldCraftaxTrace(craftaxEvents(), identity);
  const sealedDocument = {
    content_digest: "sha256:deadbeef",
    events: craftaxEvents().map((event, index) => ({
      occurred_at: "2026-08-26T00:00:00Z",
      order: { ordinal: index + 1 },
      content_digest: `evt-${index}`,
      payload: { ...event.payload, source_event_type: event.kind, source_event_digest: "d" }
    }))
  };
  const rows = containerEventsFromSealedTrace(sealedDocument);
  const sealed = foldCraftaxTrace(rows, { ...identity, sealed: true, status: "completed" });
  assert.equal(sealed.source_schema, "trace_v5");
  assert.equal(sealed.integrity.status, "sealed");
  // The same trajectory, from a different source. This is the property the one
  // shared fold exists to guarantee.
  assert.equal(sealed.steps.length, live.steps.length);
  assert.deepEqual(
    sealed.steps[0].action.applied.map((row) => row.name),
    live.steps[0].action.applied.map((row) => row.name)
  );
  assert.deepEqual(sealed.achievements, live.achievements);
  assert.equal(sealed.frames.length, live.frames.length);
});

test("RuneBench Trace V5 timelines retain agent messages and actions as inspectable items", () => {
  const rows = containerEventsFromSealedTrace({
    schema: "synth.trace.v5",
    version: 5,
    timeline: [
      { sequence: 1, kind: "frame", payload: { step: 0, format: "png" } },
      { sequence: 2, kind: "agent.message", payload: { frame_index: 0, text: "I will chop oak." } },
      { sequence: 3, kind: "agent.action", payload: { frame_index: 0, item_id: "item_1", tool: "execute_code", status: "completed", arguments_preview: '{"code":"chop"}' } }
    ]
  });
  assert.deepEqual(rows.map((row) => row.kind), ["frame", "agent.message", "agent.action"]);
  const view = foldCraftaxTrace(rows, { ...identity, sealed: true, status: "completed" });
  assert.equal(view.steps.length, 2);
  assert.equal(view.steps[0].content.message, "I will chop oak.");
  assert.equal(view.steps[1].tool_calls[0].name, "execute_code");
  assert.deepEqual(view.steps[1].tool_calls[0].arguments, { code: "chop" });
  assert.deepEqual(view.steps[1].action.applied, [{ turn: 0, name: "execute_code" }]);
});

test("the registered rollout-inspector projection is accepted as sealed authority", () => {
  const events = craftaxEvents();
  const projection = {
    schema_version: "synth.trace-projection.rollout-inspector.v1",
    trace_id: identity.traceId,
    visual: {
      items: events.map((event) => ({
        kind: event.kind,
        sequence: event.sequence,
        occurred_at: "2026-08-26T00:00:00Z",
        detail: {
          ...event.payload,
          ...(event.kind === "frame" ? { media: undefined } : {}),
          source_event_type: event.kind,
          source_event_digest: "producer-digest",
          ...(event.kind === "frame" ? {
            artifacts: [{
              artifact_id: `frame-${event.sequence}`,
              digest: `sha256:${CAS(event.sequence)}`,
              media_type: "image/png",
              size_bytes: 123,
              metadata: { width: 768, height: 768 }
            }]
          } : {})
        }
      }))
    }
  };
  const rows = containerEventsFromSealedTrace(projection);
  assert.equal(rows.length, events.length);
  assert.deepEqual(rows.map((row) => row.kind), events.map((row) => row.kind));
  const sealed = foldCraftaxTrace(rows, { ...identity, sealed: true, status: "completed" });
  assert.equal(sealed.integrity.status, "sealed");
  assert.equal(sealed.steps.length, 2);
  assert.equal(sealed.frames.length, 3);
  assert.equal(sealed.frames[0].media.casDigest, CAS(events[0].sequence + 1));
});

test("reconciliation prefers the sealed trace but never hides what live already showed", () => {
  const live = foldCraftaxTrace(craftaxEvents(), identity);
  const sealedEvents = craftaxEvents().map((event) => event.kind === "frame"
    ? { ...event, payload: { ...event.payload, media: undefined } }
    : event);
  const complete = foldCraftaxTrace(sealedEvents, {
    ...identity,
    sealed: true,
    status: "completed"
  });
  assert.equal(reconcileCraftaxTrace(live, complete).source, "sealed");
  assert.equal(reconcileCraftaxTrace(live, complete).note, null);
	assert.equal(reconcileCraftaxTrace(live, complete).view.frames[0].media.casDigest, CAS(0));

  const truncated = foldCraftaxTrace(craftaxEvents().slice(0, 4), {
    ...identity,
    sealed: true
  });
  const reconciled = reconcileCraftaxTrace(live, truncated);
  assert.equal(reconciled.source, "live");
  assert.match(reconciled.note, /fewer than/);
  assert.equal(reconciled.view, live);

  assert.equal(reconcileCraftaxTrace(live, null).source, "live");
  assert.equal(reconcileCraftaxTrace(null, complete).source, "sealed");
});

test("the map renderer is given map rows, never the whole observation", () => {
  const view = foldCraftaxTrace(craftaxEvents(), identity);
  const rows = localMapRows(view.steps[0]);
  assert.deepEqual(rows, ["....", ".@..", "...."]);
  // The bug this replaces: the inventory and status lines were painted as tiles.
  assert.ok(!rows.some((row) => row.includes("inventory")));

  // Text-only producers get the `local_map:` block extracted and nothing else.
  const textOnly = {
    content: {
      readout: null,
      observation: "step 4\nlocal_map:\n##..\n.@#.\n\ninventory: stone 2\nhealth: 9"
    }
  };
  assert.deepEqual(localMapRows(textOnly), ["##..", ".@#."]);

  // No map at all is null, so the caller shows text instead of painting noise.
  assert.equal(
    localMapRows({ content: { readout: null, observation: "health: 9\nfood: 8" } }),
    null
  );
  assert.equal(localMapRows(null), null);
});

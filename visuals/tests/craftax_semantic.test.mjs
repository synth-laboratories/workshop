import assert from "node:assert/strict";
import test from "node:test";
import {
  groupTraceByStep,
  projectCraftaxSemanticTrace,
  projectCraftaxViewer,
  semanticCheckpointIndexes
} from "../templates/live.craftax.v1/projectCraftax.ts";

const LANE = "rollout_craftax_luna_med_seed7_2026_08_12";

function envelope(kind, sequence, payload = {}) {
  return {
    kind,
    sequence,
    occurred_at: new Date(Date.UTC(2026, 7, 12, 20, 0, sequence)).toISOString(),
    run_id: LANE,
    payload
  };
}

/** A realistic step: open policy span, stream many deltas, plan, close, act. */
function policyHeavyRollout({ deltaCount = 300 } = {}) {
  const events = [
    envelope("trace.opened", 1),
    envelope("env.episode.opened", 2),
    envelope("observation", 3, { readout: { env_steps: 0, observation_text: "You are in a forest." } }),
    envelope("span.policy.opened", 4, { call: { provider: "openrouter", model: "gpt-5.6-luna" } })
  ];
  let seq = 5;
  for (let index = 0; index < deltaCount; index += 1) {
    events.push(envelope("span.policy.data", seq++, { delta: true, channel: "reasoning", text: `t${index} ` }));
  }
  events.push(envelope("span.policy.data", seq++, {
    channel: "summary",
    model: "gpt-5.6-luna",
    tool_arguments: '{"actions":["up","up","left"]}',
    usage: { prompt_tokens: 1200, completion_tokens: 260, total_tokens: 1460, cost_usd: 0.000502 }
  }));
  events.push(envelope("span.policy.plan", seq++, { actions: ["up", "up", "left"] }));
  events.push(envelope("span.policy.closed", seq++, { length: 3 }));
  events.push(envelope("action", seq++, { action: "up" }));
  events.push(envelope("reward_signal", seq++, { value: 0.0 }));
  events.push(envelope("span.step.closed", seq++, { step: 0, action: "up" }));
  events.push(envelope("frame", seq++, { url: "/api/frames/f0.png" }));
  events.push(envelope("observation", seq++, { readout: { env_steps: 1 } }));
  events.push(envelope("achievement_unlocked", seq++, { achievement: "collect_wood" }));
  events.push(envelope("span.step.closed", seq++, { step: 1, action: "left" }));
  events.push(envelope("env.episode.closed", seq++, {}));
  events.push(envelope("capture.closed", seq++, { high_water: seq }));
  events.push(envelope("trace.reconciled", seq++, { digest: "a".repeat(64) }));
  return events;
}

test("hundreds of policy deltas fold into a single policy-call trace row", () => {
  const events = policyHeavyRollout({ deltaCount: 300 });
  const items = projectCraftaxSemanticTrace(events);
  const policyItems = items.filter((item) => item.category === "policy");
  assert.equal(policyItems.length, 1, "one policy call, not hundreds of delta rows");
  const call = policyItems[0];
  assert.equal(call.rawEvents.length, 304, "all transport envelopes stay reachable behind the fold");
  assert.match(String(call.interaction.thinking), /^t0 t1 /);
  assert.equal(call.interaction.responseType, "tool_call");
  assert.ok(String(call.interaction.tools).includes("submit" ) || String(call.interaction.tools).includes("actions"));
  assert.ok(String(call.interaction.input).includes("forest"));
});

test("one hundred thousand transport envelopes keep semantic DOM cardinality bounded", () => {
  const events = policyHeavyRollout({ deltaCount: 100_000 });
  const projection = projectCraftaxViewer(events);
  const semantic = projectCraftaxSemanticTrace(projection.ordered);
  const groups = groupTraceByStep(semantic);
  const checkpoints = semanticCheckpointIndexes(projection.ordered);

  assert.equal(events.length, 100_017);
  assert.ok(semantic.length < 20, `expected bounded semantic rows, got ${semantic.length}`);
  assert.ok(groups.length < 10, `expected bounded step groups, got ${groups.length}`);
  assert.ok(checkpoints.length < 20, `expected bounded replay ticks, got ${checkpoints.length}`);
  assert.equal(
    semantic.find((item) => item.category === "policy")?.rawEvents.length,
    100_004,
    "raw evidence remains inspectable behind the single semantic call"
  );
});

test("trace groups follow the environment hierarchy: lifecycle, steps, evidence", () => {
  const events = policyHeavyRollout();
  const groups = groupTraceByStep(projectCraftaxSemanticTrace(events));
  assert.equal(groups[0].key, "run");
  assert.ok(groups[0].items.every((item) => ["lifecycle", "evidence"].includes(item.category)));
  const stepGroups = groups.filter((group) => group.key.startsWith("step:") && group.step != null);
  assert.deepEqual(stepGroups.map((group) => group.step), [0, 1]);
  const step0 = stepGroups[0];
  assert.ok(step0.items.some((item) => item.category === "policy"), "policy call nests under its step");
  assert.ok(step0.items.some((item) => item.kind === "environment.step"));
  const trailing = groups.at(-1);
  assert.ok(trailing.items.some((item) => item.kind === "trace.reconciled"), "seal evidence stays visible at run level");
});

test("replay checkpoints skip transport deltas, frames, and observations", () => {
  const events = policyHeavyRollout({ deltaCount: 300 });
  const { ordered } = projectCraftaxViewer(events);
  const checkpoints = semanticCheckpointIndexes(ordered);
  assert.ok(checkpoints.length < 20, `expected semantic ticks, got ${checkpoints.length}`);
  assert.ok(checkpoints.length >= 8, "step, policy, reward, achievement, lifecycle checkpoints survive");
  for (const index of checkpoints.slice(0, -1)) {
    assert.notEqual(ordered[index].kind, "span.policy.data");
  }
  assert.equal(checkpoints.at(-1), ordered.length - 1, "the final durable event is always reachable");
});

test("a scoped lane filter isolates rollouts sharing one stream", () => {
  const mine = policyHeavyRollout({ deltaCount: 5 });
  const foreign = [
    envelope("trace.opened", 1),
    envelope("reward_signal", 2, { value: 9.0 })
  ].map((event) => ({ ...event, run_id: "rollout_unrelated_optimizer" }));
  const all = [...mine, ...foreign];
  const allowed = new Set([LANE]);
  const scoped = all.filter((event) => allowed.has(event.run_id));
  const projection = projectCraftaxViewer(scoped);
  assert.deepEqual(projection.lanes, [LANE]);
  assert.notEqual(projection.reward, 9.0);
});

test("tool-only responses are labeled, not presented as lost output", () => {
  const events = policyHeavyRollout({ deltaCount: 2 });
  const items = projectCraftaxSemanticTrace(events);
  const call = items.find((item) => item.category === "policy");
  assert.equal(call.interaction.responseType, "tool_call");
  assert.equal(call.interaction.output ?? undefined, undefined, "no text output is recorded as absent, not fabricated");
});

test("native Craftax snapshot and eval.run.terminal envelopes project without rewriting evidence", () => {
  const events = [
    envelope("snapshot", 1, {
      ascii: "@..\\n.T.",
      step_index: 0,
      total_reward: 0,
      achievements: []
    }),
    envelope("snapshot", 2, {
      ascii: ".@.\\n.T.",
      step_index: 12,
      total_reward: 1.5,
      achievements: ["collect_wood"]
    }),
    envelope("eval.run.terminal", 3, {
      stopped_on: "death",
      terminated: true,
      env_steps: 12,
      reward: 1.5
    })
  ];
  const projection = projectCraftaxViewer(events);
  const semantic = projectCraftaxSemanticTrace(projection.ordered);

  assert.equal(projection.terminal, true);
  assert.equal(projection.reward, 1.5);
  assert.equal(projection.ascii, ".@.\\n.T.");
  assert.deepEqual(projection.achievements, ["collect_wood"]);
  assert.equal(semantic.filter((item) => item.kind === "environment.step").length, 2);
  assert.equal(semantic.at(-1)?.kind, "eval.run.terminal");
  assert.equal(semantic.at(-1)?.label, "Run death");
  assert.equal(semantic.at(-1)?.rawEvents[0], events[2], "the original terminal envelope remains the evidence");
});

import assert from "node:assert/strict";
import test from "node:test";
import {
  craftaxEventSequence,
  craftaxTruthLabel,
  craftaxTruthState,
  groupTraceByStep,
  policyPartialDetail,
  projectCraftaxSemanticTrace,
  projectCraftaxViewer,
  scopeCraftaxEvents,
  replayMomentIndexes,
  craftaxReplayAvailability,
  environmentStepCount,
  mergeCraftaxOptimizerJournalEvents,
} from "../families/first_class_example_containers/live.craftax.v1/projectCraftax.ts";
import { summarizeCraftaxRun } from "../families/first_class_example_containers/live.craftax.v1/aggregateCraftax.ts";
import { craftaxAchievementIcon, craftaxStepPath, projectCraftaxAggregateTimeline } from "../families/first_class_example_containers/live.craftax.v1/aggregateTimeline.ts";

function event(lane, kind, sequence, payload = {}, second = sequence) {
  return {
    schema: "synth.trace-stream-event.v1",
    run_id: lane,
    lane,
    kind,
    sequence,
    ts: `2026-08-12T17:00:${String(second).padStart(2, "0")}.000Z`,
    payload,
  };
}

test("Craftax viewer isolates the selected lane in a time-ordered multiplex", () => {
  const events = [
    event("seed:1", "trace.opened", 1, {}, 1),
    event("seed:0", "trace.opened", 1, {}, 0),
    event("seed:1", "reward_signal", 2, { step: 1, value: 5 }, 3),
    event("seed:0", "reward_signal", 2, { step: 1, value: 0.5 }, 2),
  ];
  const first = projectCraftaxViewer(events);
  assert.deepEqual(first.lanes, ["seed:0", "seed:1"]);
  assert.equal(first.selectedLane, "seed:0");
  assert.ok(first.visibleEvents.every((row) => row.lane === "seed:0"));
  assert.equal(first.reward, 0.5);

  const second = projectCraftaxViewer(events, "seed:1");
  assert.ok(second.visibleEvents.every((row) => row.lane === "seed:1"));
  assert.equal(second.reward, 5);
});

test("run-level optimizer lifecycle is retained but never selected as a rollout", () => {
  const events = [
    event("eval", "eval.run.started", 1, { status: "running" }, 0),
    event("rollout-780005", "span.policy.opened", 1, {
      call: { provider: "openrouter", model: "z-ai/glm-5.3-flash" },
    }, 1),
    event("rollout-780005", "span.policy.data", 2, {
      assistant: "do",
      actions: ["do"],
    }, 2),
    event("rollout-780006", "trace.opened", 1, {}, 3),
    event("eval", "eval.run.terminal", 2, { status: "completed" }, 4),
  ];

  const projection = projectCraftaxViewer(events);
  assert.deepEqual(projection.lanes, ["rollout-780005", "rollout-780006"]);
  assert.equal(projection.selectedLane, "rollout-780005");
  assert.equal(projection.traceEvents.length, 2, "the default lane exposes its retained policy call");
  assert.ok(projection.ordered.some((row) => row.lane === "eval"), "run lifecycle remains in the durable journal");
});

test("terminal and enrichment optimizer lanes rejoin into 50 calls and 303 completed steps", () => {
  let sequenceNumber = 1;
  const optimizerEnvelope = (inner) => ({
    schemaVersion: "optimizer_event.v1",
    eventId: `optimizer:event:${sequenceNumber}`,
    type: "eval.trial.event",
    sequenceNumber: sequenceNumber++,
    occurredAt: inner.ts,
    optimizerRunId: "opt_eval_craftax_313e406208e5",
    delta: {
      trial_id: inner.lane,
      container_event: {
        kind: inner.kind,
        rollout_id: inner.lane,
        sequence: inner.sequence,
        occurred_at: inner.ts,
        payload: inner.payload,
      },
    },
  });
  const terminalEvents = [{
    schemaVersion: "optimizer_event.v1",
    eventId: "optimizer:started",
    type: "optimizer.run.started",
    sequenceNumber: 0,
    occurredAt: "2026-08-28T15:17:44.000Z",
    optimizerRunId: "opt_eval_craftax_313e406208e5",
    delta: { status: "running" },
  }];
  const enrichmentEvents = [65, 66, 85, 56, 31].flatMap((stepCount, laneIndex) => {
    const lane = `rollout-${laneIndex}`;
    const calls = Array.from({ length: 10 }, (_, call) => [
      optimizerEnvelope(event(lane, "span.policy.opened", call * 2 + 1, {
        call: { provider: "openrouter", model: "z-ai/glm-5.3-flash" },
      }, laneIndex + 1)),
      optimizerEnvelope(event(lane, "span.policy.closed", call * 2 + 2, {}, laneIndex + 1)),
    ]).flat();
    const steps = Array.from({ length: stepCount }, (_, step) =>
      optimizerEnvelope(event(lane, "span.step.closed", 100 + step, { step }, laneIndex + 1))
    );
    return [...calls, ...steps];
  });

  const merged = mergeCraftaxOptimizerJournalEvents(terminalEvents, enrichmentEvents);
  const projection = projectCraftaxViewer(merged);
  assert.equal(projection.lanes.length, 5);
  assert.equal(projection.ordered.filter((row) => row.kind === "span.policy.opened").length, 50);
  assert.equal(environmentStepCount(projection.ordered), 303);

  const duplicated = mergeCraftaxOptimizerJournalEvents(terminalEvents, [...enrichmentEvents, enrichmentEvents[0]]);
  assert.equal(duplicated.length, merged.length, "replayed optimizer envelopes are de-duplicated by durable identity");
});

test("run overview aggregates rollout distributions without inventing partial token totals", () => {
  const rows = [
    event("rollout-a", "span.policy.opened", 1, { call: { provider: "openrouter", model: "z-ai/glm-5.3-flash" } }),
    event("rollout-a", "span.policy.data", 2, { usage: { total_tokens: 120, cost_usd: 0.0012 } }),
    event("rollout-a", "span.policy.closed", 3),
    event("rollout-a", "span.step.closed", 4, { step: 0 }),
    event("rollout-a", "span.step.closed", 5, { step: 1 }),
    event("rollout-a", "reward_signal", 6, { value: 4 }),
    event("rollout-a", "achievement_unlocked", 7, { achievement: "collect_wood" }),
    event("rollout-b", "span.policy.opened", 1, { call: { provider: "openrouter", model: "z-ai/glm-5.3-flash" } }),
    event("rollout-b", "span.policy.data", 2, { usage: { total_tokens: 80, cost_usd: 0.0008 } }),
    event("rollout-b", "span.policy.closed", 3),
    event("rollout-b", "span.step.closed", 4, { step: 0 }),
    event("rollout-b", "reward_signal", 5, { value: 2 }),
    event("rollout-b", "achievement_unlocked", 6, { achievement: "collect_stone" }),
  ];
  const aggregate = summarizeCraftaxRun(rows);
  assert.equal(aggregate.rollouts.length, 2);
  assert.equal(aggregate.rewardMean, 3);
  assert.equal(aggregate.rewardMedian, 3);
  assert.deepEqual([aggregate.rewardMin, aggregate.rewardMax], [2, 4]);
  assert.deepEqual([aggregate.totalSteps, aggregate.minSteps, aggregate.maxSteps], [3, 1, 2]);
  assert.deepEqual([aggregate.totalCalls, aggregate.minCalls, aggregate.maxCalls], [2, 1, 1]);
  assert.equal(aggregate.totalTokens, 200);
  assert.equal(aggregate.totalCostUsd, 0.002);
  assert.equal(aggregate.reportedCosts, 2);
  assert.deepEqual(aggregate.achievementNames, ["collect_stone", "collect_wood"]);
  assert.deepEqual([aggregate.totalAchievements, aggregate.minAchievements, aggregate.maxAchievements], [2, 1, 1]);
  assert.equal(aggregate.achievementMedian, 1);
  assert.equal(aggregate.achievementRollouts, 2);

  const partial = summarizeCraftaxRun([
    ...rows,
    event("rollout-c", "span.policy.opened", 1, { call: { provider: "openrouter", model: "z-ai/glm-5.3-flash" } }),
    event("rollout-c", "span.policy.closed", 2),
  ]);
  assert.equal(partial.totalCalls, 3);
  assert.equal(partial.totalTokens, undefined, "one call without usage makes the run token total unavailable");
  assert.equal(partial.totalCostUsd, undefined, "one unpriced rollout makes the exact run total unavailable");
  assert.equal(partial.knownCostUsd, 0.002, "known rollout costs remain a labelled subtotal");
  assert.equal(partial.reportedCosts, 2);
});

test("aggregate Craftax timeline aligns rollout reward lines and achievement icons by environment step", () => {
  const rows = [
    event("rollout-a", "span.step.closed", 1, { step: 1 }),
    event("rollout-a", "achievement_unlocked", 2, { step: 1, achievement: "collect_wood" }),
    event("rollout-a", "reward_signal", 3, { step: 1, value: 1 }),
    event("rollout-a", "span.step.closed", 4, { step: 4 }),
    event("rollout-a", "achievement_unlocked", 5, { step: 4, achievement: "make_wood_pickaxe" }),
    event("rollout-a", "reward_signal", 6, { step: 4, value: 2 }),
    event("rollout-b", "snapshot", 1, { step: 2, total_reward: 2, achievements: { collect_stone: 1 } }),
    event("rollout-b", "snapshot", 2, { step: 3, total_reward: 2, achievements: { collect_stone: 1 } }),
  ];
  const timelines = projectCraftaxAggregateTimeline(rows, ["rollout-a", "rollout-b"], [
    { lane: "rollout-a", reward: 3, steps: 5 },
    { lane: "rollout-b", reward: 2, steps: 3 },
  ]);

  assert.deepEqual(timelines[0].points, [
    { step: 0, reward: 0 },
    { step: 1, reward: 1 },
    { step: 4, reward: 3 },
    { step: 5, reward: 3 },
  ]);
  assert.deepEqual(timelines[0].achievements.map(({ step, reward, name, icon }) => ({ step, reward, name, icon })), [
    { step: 1, reward: 1, name: "collect_wood", icon: "🪵" },
    { step: 4, reward: 3, name: "make_wood_pickaxe", icon: "⛏" },
  ]);
  assert.deepEqual(timelines[1].achievements.map(({ step, reward, name }) => ({ step, reward, name })), [
    { step: 2, reward: 2, name: "collect_stone" },
  ], "snapshot achievements are marked once, at first retained evidence");
  assert.equal(craftaxAchievementIcon("make_iron_sword"), "⚔");
  assert.match(craftaxStepPath(timelines[0].points, 5, 0, 3), /^M .* H .* V /);
});

test("terminal overview replaces provisional rewards with scored record truth", () => {
  const journal = [
    event("rollout-5", "reward_signal", 1, { value: 3 }),
    event("rollout-6", "reward_signal", 1, { value: 5 }),
    event("rollout-7", "reward_signal", 1, { value: 5 }),
    event("rollout-8", "reward_signal", 1, { value: 6 }),
    event("rollout-9", "reward_signal", 1, { value: 5 }),
    ...["rollout-5", "rollout-6", "rollout-7", "rollout-8", "rollout-9"].flatMap((lane) =>
      [event(lane, "span.policy.opened", 2, { call: { provider: "openrouter", model: "z-ai/glm-5.3-flash" } })]
    )
  ];
  const terminal = [
    { lane: "rollout-5", status: "failed", steps: 46 },
    { lane: "rollout-6", status: "failed", steps: 60 },
    { lane: "rollout-7", status: "failed", steps: 58 },
    { lane: "rollout-8", seed: 780008, status: "completed", reward: 6, steps: 40, costUsd: 0.0042, achievements: ["collect_wood"] },
    { lane: "rollout-9", status: "failed", steps: 80 }
  ];
  const aggregate = summarizeCraftaxRun(journal, terminal);
  assert.equal(aggregate.rewardMean, 6);
  assert.equal(aggregate.reportedRewards, 1);
  assert.deepEqual(aggregate.rollouts.map((rollout) => rollout.reward), [undefined, undefined, undefined, 6, undefined]);
  assert.equal(aggregate.totalSteps, 284);
  assert.equal(aggregate.reportedSteps, 5);
  assert.equal(aggregate.totalCalls, 5, "retained call starts remain a separately labelled journal count");
  assert.equal(aggregate.totalCostUsd, undefined);
  assert.equal(aggregate.knownCostUsd, 0.0042);
  assert.equal(aggregate.reportedCosts, 1);
  assert.equal(aggregate.rollouts[3].seed, 780008);
  assert.equal(aggregate.rollouts[3].costUsd, 0.0042);
  assert.equal(aggregate.reportedAchievements, 1);
});

test("terminal aggregates use authoritative scored reward and achievement distributions", () => {
  const terminal = [
    { lane: "rollout-5", status: "completed", reward: 1, achievements: ["wood"] },
    { lane: "rollout-6", status: "completed", reward: 2, achievements: ["wood", "stone"] },
    { lane: "rollout-7", status: "completed", reward: 4, achievements: [] },
    { lane: "rollout-8", status: "completed", reward: 6, achievements: ["wood", "stone", "table"] },
    { lane: "rollout-9", status: "completed", reward: 9, achievements: ["wood"] },
  ];
  const aggregate = summarizeCraftaxRun([], terminal);
  assert.deepEqual(
    [aggregate.rewardMean, aggregate.rewardMedian, aggregate.rewardMin, aggregate.rewardMax],
    [4.4, 4, 1, 9]
  );
  assert.deepEqual(
    [aggregate.totalAchievements, aggregate.achievementMedian, aggregate.minAchievements, aggregate.maxAchievements],
    [7, 1, 0, 3]
  );
});

test("native GameBench reward_signal reward alias remains visible during replay", () => {
  const projection = projectCraftaxViewer([
    event("seed:2001", "rollout.progress", 1, { status: "running", reward: 0 }),
    event("seed:2001", "reward_signal", 2, { reward: 1 }),
    event("seed:2001", "eval.run.terminal", 3, { status: "completed" }),
  ]);
  assert.equal(projection.reward, 1);
  assert.equal(projection.cumulativeReward, 1);
});

test("through-time cutoff hides future policy, reward, frame, and achievement evidence", () => {
  const events = [
    event("seed:0", "trace.opened", 1),
    event("seed:0", "observation", 2, { grid: "P....\n..T..", readout: { wood: 0 } }),
    event("seed:0", "frame", 3, { digest: "frame-0", format: "ascii" }),
    event("seed:0", "span.policy.opened", 4, { harness: "react", call: { provider: "openrouter", model: "meta/muse-spark-1.1" } }),
    event("seed:0", "span.policy.data", 5, { reasoning: "move to tree", actions: ["east"], usage: { total_tokens: 13 } }),
    event("seed:0", "span.policy.plan", 6, { actions: ["east", "do"], length: 2 }),
    event("seed:0", "span.policy.closed", 7, { length: 2 }),
    event("seed:0", "action", 8, { step: 1, action: "east" }),
    event("seed:0", "reward_signal", 9, { step: 1, value: 0.5 }),
    event("seed:0", "observation", 10, { grid: ".P...\n..T..", readout: { achievements: { collect_wood: true, collect_stone: false } } }),
    event("seed:0", "frame", 11, { digest: "frame-1", format: "png", url: "/artifacts/frame-1" }),
    event("seed:0", "status", 12, { status: "completed" }),
  ];

  const early = projectCraftaxViewer(events, "seed:0", 4);
  assert.equal(early.visibleEvents.at(-1).kind, "span.policy.data");
  assert.equal(early.reward, undefined);
  assert.deepEqual(early.achievements, []);
  assert.equal(early.frameUrl, null);
  assert.equal(early.frameEvents.length, 1);
  assert.equal(early.ascii, "P....\n..T..");
  assert.equal(early.policy.reasoning, "move to tree");
  assert.deepEqual(early.policy.actions, ["east"]);

  const complete = projectCraftaxViewer(events, "seed:0");
  assert.equal(complete.reward, 0.5);
  assert.deepEqual(complete.achievements, ["collect_wood"]);
  assert.equal(complete.frameUrl, "/artifacts/frame-1");
  assert.deepEqual(complete.frameEvents.map((frame) => frame.payload.url), ["/artifacts/frame-1"]);
  assert.equal(complete.ascii, null);
  assert.equal(complete.frameUnavailable, false);
  assert.deepEqual(complete.policy.actions, ["east", "do"]);
  assert.equal(complete.traceEvents.length, 4);
  assert.equal(complete.terminal, true);
  assert.equal(early.terminal, false);
});

test("image replay uses only ordered frame URLs emitted by Containers", () => {
  const view = projectCraftaxViewer([
    event("seed:0", "frame", 1, { digest: "digest-only", format: "png" }),
    event("seed:0", "frame", 2, { digest: "frame-0", format: "png", url: "http://container/frames/0.png", step: 0 }),
    event("seed:0", "observation", 3, { grid: "P..." }),
    event("seed:0", "frame", 4, { digest: "frame-1", format: "png", url: "http://container/frames/1.png", step: 1 }),
  ]);
  assert.deepEqual(view.frameEvents.map((frame) => frame.payload.url), [
    "http://container/frames/0.png",
    "http://container/frames/1.png",
  ]);
  assert.equal(view.frameUrl, "http://container/frames/1.png");
});

test("retained CAS media keeps a PNG replayable when its container URL is gone", () => {
  const casDigest = "a".repeat(64);
  const view = projectCraftaxViewer([
    event("seed:0", "frame", 1, {
      digest: "producer-label",
      format: "png",
      step: 0,
      media: {
        casDigest,
        mediaType: "image/png",
        width: 768,
        height: 768,
        producerDigest: "producer-label",
      },
    }),
  ]);
  assert.equal(view.frameUrl, null);
  assert.equal(view.frameUnavailable, false);
  assert.equal(view.frameEvents.length, 1);
  assert.deepEqual(view.frameMedia, {
    casDigest,
    mediaType: "image/png",
    width: 768,
    height: 768,
    producerDigest: "producer-label",
  });
});

test("real ReAct policy partials expose metadata, data, plan, usage, and fallback", () => {
  const events = [
    event("seed:0", "span.policy.opened", 1, {
      harness: "react",
      call: { provider: "openrouter", model: "meta/muse-spark-1.1", config: "muse_spark_medium" },
    }),
    event("seed:0", "span.policy.data", 2, {
      assistant: "",
      reasoning: "Need wood first",
      tool_arguments: '{"actions":["east","do"]}',
      actions: ["east", "do"],
      action_authority: "harness_fallback",
      fallback: true,
      parse_error: "policy returned no valid Craftax actions",
      usage: { prompt_tokens: 10, completion_tokens: 3, total_tokens: 13, cost_usd: null },
      prior_attempts: [{ usage: { prompt_tokens: 4, completion_tokens: 2, total_tokens: 6, cost_usd: 0.01 } }],
    }),
    event("seed:0", "span.policy.plan", 3, { actions: ["east", "do"], length: 2 }),
    event("seed:0", "span.policy.closed", 4, { length: 2 }),
  ];
  const view = projectCraftaxViewer(events);
  assert.equal(view.policy.provider, "openrouter");
  assert.equal(view.policy.model, "meta/muse-spark-1.1");
  assert.equal(view.policy.reasoning, "Need wood first");
  assert.equal(view.policy.toolArguments, '{"actions":["east","do"]}');
  assert.equal(view.policy.actionAuthority, "harness_fallback");
  assert.equal(view.policy.fallback, true);
  assert.match(view.policy.parseError, /no valid Craftax actions/);
  assert.deepEqual(view.policy.usage, {
    prompt_tokens: 14,
    completion_tokens: 5,
    total_tokens: 19,
    cost_usd: 0.01,
  });
  assert.equal(policyPartialDetail(events[0]), "openrouter · meta/muse-spark-1.1 · react");
  assert.equal(policyPartialDetail(events[2]), "east → do");
  assert.equal(policyPartialDetail(events[3]), "2 planned actions");
});

test("token deltas accumulate when the snapshot reasoning is empty and do not double-count usage", () => {
  const events = [
    event("seed:0", "span.policy.opened", 1, { harness: "react", call: { provider: "openrouter", model: "gpt-5.6-luna" } }),
    event("seed:0", "span.policy.data", 2, { delta: true, channel: "reasoning", text: "" }),
    event("seed:0", "span.policy.data", 3, { delta: true, channel: "tool", text: '{"actions":["do"]}' }),
    event("seed:0", "span.policy.data", 4, {
      assistant: "",
      reasoning: "",
      tool_arguments: '{"actions":["do"]}',
      actions: ["do"],
      action_authority: "policy",
      usage: { total_tokens: 13 },
    }),
    event("seed:0", "span.policy.plan", 5, { actions: ["do"], length: 1 }),
  ];
  const view = projectCraftaxViewer(events);
  assert.equal(view.policy.reasoning, undefined);
  assert.equal(view.policy.toolArguments, '{"actions":["do"]}');
  assert.deepEqual(view.policy.usage, { total_tokens: 13 });
  assert.equal(policyPartialDetail(events[2]), 'tool Δ {"actions":["do"]}');
  assert.equal(policyPartialDetail(events[1]), "reasoning Δ");
});

test("missing Craftax values remain missing and sequence_number aliases are honored", () => {
  const missing = projectCraftaxViewer([
    event("seed:0", "observation", 1, { readout: { energy: 3 } }),
    event("seed:0", "reward_signal", 2, { step: 1, value: null }),
    event("seed:0", "status", 3, { status: "completed" }),
  ]);
  assert.equal(missing.reward, undefined);
  assert.equal(missing.cumulativeReward, undefined);
  assert.deepEqual(missing.achievements, []);
  assert.deepEqual(missing.policy.usage, {});
  assert.equal(missing.frameUrl, null);
  assert.equal(missing.ascii, null);
  assert.equal(missing.frameUnavailable, false);

  const alias = { ...event("seed:0", "action", null, {}), sequence_number: "9" };
  assert.equal(craftaxEventSequence(alias, 0), 9);
});

test("V4 truth states distinguish missing, zero, tool-only, redacted, and failed", () => {
  assert.equal(craftaxTruthState(undefined), "pending");
  assert.equal(craftaxTruthState(undefined, { terminal: true }), "not_emitted");
  assert.equal(craftaxTruthState(undefined, { applicable: false }), "not_applicable");
  assert.equal(craftaxTruthState("[REDACTED]"), "redacted");
  assert.equal(craftaxTruthState(undefined, { failed: true }), "failed");
  assert.equal(craftaxTruthState(0, { terminal: true }), "present");
  assert.equal(craftaxTruthLabel("not_applicable"), "not applicable");
});

test("V1 folds hundreds of policy deltas into one semantic call with full interaction", () => {
  const deltas = Array.from({ length: 330 }, (_, index) => event("seed:0", "span.policy.data", index + 3, {
    call: 1,
    delta: true,
    channel: index < 280 ? "reasoning" : "tool",
    text: index < 280 ? "r" : "t",
  }, index + 3));
  const raw = [
    event("seed:0", "observation", 1, { readout: { observation_text: "real input" } }, 1),
    event("seed:0", "span.policy.opened", 2, { call: { provider: "openrouter", model: "gpt-5.6-luna" } }, 2),
    ...deltas,
    event("seed:0", "span.policy.data", 333, {
      call: 1,
      reasoning: "complete reasoning",
      tool_arguments: '{"actions":["do"]}',
      actions: ["do"],
    }, 59),
    event("seed:0", "span.policy.plan", 334, { actions: ["do"] }, 59),
    event("seed:0", "span.policy.closed", 335, { length: 1 }, 59),
  ];
  const semantic = projectCraftaxSemanticTrace(raw);
  assert.equal(semantic.length, 1);
  assert.equal(semantic[0].kind, "policy.call");
  assert.equal(semantic[0].rawEvents.length, 334);
  assert.equal(semantic[0].interaction.input, "real input");
  assert.equal(semantic[0].interaction.thinking, "complete reasoning");
  assert.equal(semantic[0].interaction.tools, '{"actions":["do"]}');
  assert.equal(semantic[0].interaction.responseType, "tool_call");
  assert.match(semantic[0].label, /call 1.*do/);
});

test("V2 semantic trace orders calls, environment steps, achievements, and closure", () => {
  const semantic = projectCraftaxSemanticTrace([
    event("seed:0", "trace.opened", 1),
    event("seed:0", "span.policy.opened", 2, { call: { model: "luna" } }),
    event("seed:0", "span.policy.data", 3, { call: 1, assistant: "move" }),
    event("seed:0", "span.policy.closed", 4, {}),
    event("seed:0", "span.step.closed", 5, { step: 1, action: "up" }),
    event("seed:0", "achievement_unlocked", 6, { step: 1, achievement: "collect_wood" }),
    event("seed:0", "capture.closed", 7, { high_water: 6 }),
  ]);
  assert.deepEqual(semantic.map((item) => item.kind), [
    "trace.opened", "policy.call", "environment.step", "achievement_unlocked", "capture.closed",
  ]);
  assert.equal(semantic[1].interaction.responseType, "text");
  assert.equal(semantic[2].step, 1);
  const groups = groupTraceByStep(semantic);
  assert.ok(groups.some((group) => group.label === "Step 1"));
});

test("A13 visual scope refuses unrelated rollouts sharing one producer root", () => {
  const rows = [
    event("campaign-a:0", "observation", 1, {}),
    event("campaign-a:1", "observation", 1, {}),
    event("unrelated:0", "reward_signal", 1, { value: 999 }),
  ];
  const scoped = scopeCraftaxEvents(rows, ["campaign-a:0", "campaign-a:1"]);
  assert.deepEqual(scoped.map((row) => row.lane), ["campaign-a:0", "campaign-a:1"]);
  assert.ok(!JSON.stringify(scoped).includes("999"));
});

test("V2 replay moments skip token deltas and observations", () => {
  const rows = [
    event("seed:0", "trace.opened", 1),
    event("seed:0", "observation", 2, {}),
    event("seed:0", "span.policy.opened", 3, {}),
    ...Array.from({ length: 1000 }, (_, index) => event("seed:0", "span.policy.data", index + 4, { delta: true, channel: "reasoning", text: "x" })),
    event("seed:0", "span.policy.closed", 1004, {}),
    event("seed:0", "span.step.closed", 1005, { step: 1 }),
  ];
  const moments = replayMomentIndexes(rows);
  assert.deepEqual(moments, [0, 2, 1003, 1004]);
  assert.ok(moments.length < rows.length / 100);
});

test("lifecycle-only rejected evidence reports markers without inventing environment steps", () => {
  const rows = Array.from({ length: 5 }, (_, index) =>
    event(`seed:${index}`, "status", index + 1, { status: "failed" })
  );
  const availability = craftaxReplayAvailability(rows, "rejected");
  assert.deepEqual(availability, {
    markers: 5,
    environmentSteps: 0,
    replayable: false,
    reason: "evidence rejected"
  });
});

test("missing PNG stays unavailable and does not fall back to ASCII", () => {
  const missingPng = projectCraftaxViewer([
    event("seed:0", "observation", 1, { grid: "P....\n..T.." }),
    event("seed:0", "frame", 2, { format: "png" }),
  ]);
  assert.equal(missingPng.frameUrl, null);
  assert.equal(missingPng.ascii, null);
  assert.equal(missingPng.frameUnavailable, true);

  const fixtureAscii = projectCraftaxViewer([
    event("seed:0", "frame", 1, { format: "ascii", text: "P....\n..T.." }),
  ]);
  assert.equal(fixtureAscii.ascii, "P....\n..T..");
  assert.equal(fixtureAscii.frameUnavailable, false);
});

test("host-envelope events without kind cannot crash the Craftax projection", () => {
  const projection = projectCraftaxViewer([
    {
      event_type: "optimizer.run.failed",
      sequence: 1,
      occurred_at: "2026-08-28T00:00:00Z",
      payload: { reason: "failed honestly" },
    },
  ]);

  assert.equal(projection.ordered.length, 1);
  assert.equal(projection.ordered[0].kind, "optimizer.run.failed");
  assert.doesNotThrow(() => projectCraftaxSemanticTrace(projection.ordered));
});

test("optimizer trial envelopes unwrap NanoHorizon policy spans into transcript calls", () => {
  const projection = projectCraftaxViewer([
    {
      type: "eval.trial.event",
      sequenceNumber: 11,
      occurredAt: "2026-08-28T05:39:51Z",
      optimizerRunId: "run:craftax",
      delta: {
        trial_id: "trial:craftax:780005",
        container_event: {
          kind: "span.policy.opened",
          sequence: 9,
          occurred_at: "2026-08-28T05:39:51Z",
          rollout_id: "roll:780005",
          payload: { call: { provider: "openrouter", model: "z-ai/glm-5.3-flash" } },
        },
      },
    },
    {
      type: "eval.trial.event",
      sequenceNumber: 12,
      occurredAt: "2026-08-28T05:39:52Z",
      optimizerRunId: "run:craftax",
      delta: {
        trial_id: "trial:craftax:780005",
        container_event: {
          kind: "span.policy.data",
          sequence: 10,
          occurred_at: "2026-08-28T05:39:52Z",
          rollout_id: "roll:780005",
          payload: {
            assistant: { content: null, reasoning_content: "Choose up, then do." },
            completion_tokens: 384,
            prompt_tokens: 1462,
            phase: "sample",
          },
        },
      },
    },
    {
      type: "eval.trial.event",
      sequenceNumber: 13,
      occurredAt: "2026-08-28T05:39:53Z",
      optimizerRunId: "run:craftax",
      delta: {
        trial_id: "trial:craftax:780005",
        container_event: {
          kind: "span.policy.data",
          sequence: 11,
          occurred_at: "2026-08-28T05:39:53Z",
          rollout_id: "roll:780005",
          payload: {
            assistant: { content: null, reasoning_content: "Try a shorter plan." },
            completion_tokens: 384,
            prompt_tokens: 1500,
            phase: "sample",
          },
        },
      },
    },
    {
      type: "eval.trial.event",
      sequenceNumber: 14,
      occurredAt: "2026-08-28T05:39:54Z",
      optimizerRunId: "run:craftax",
      delta: {
        trial_id: "trial:craftax:780005",
        container_event: {
          kind: "span.policy.closed",
          sequence: 12,
          occurred_at: "2026-08-28T05:39:54Z",
          rollout_id: "roll:780005",
          payload: {},
        },
      },
    },
  ]);

  assert.equal(projection.selectedLane, "roll:780005");
  assert.deepEqual(projection.traceEvents.map((row) => row.kind), [
    "span.policy.opened",
    "span.policy.data",
    "span.policy.data",
    "span.policy.closed",
  ]);
  assert.equal(projection.semanticTrace.length, 2);
  assert.equal(projection.semanticTrace[0].kind, "policy.call");
  assert.match(projection.semanticTrace[0].label, /z-ai\/glm-5.3-flash/);
  assert.equal(projection.semanticTrace[0].interaction?.thinking, "Choose up, then do.");
  assert.equal(projection.semanticTrace[0].interaction?.responseType, "pending");
  assert.equal(projection.semanticTrace[1].interaction?.thinking, "Try a shorter plan.");
  assert.deepEqual(projection.policy.usage, {
    prompt_tokens: 2962,
    completion_tokens: 768,
  });
});

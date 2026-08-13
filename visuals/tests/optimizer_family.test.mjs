import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { formatMissingNumber } from "../runtime/liveStream.ts";
import {
  extractContainerRolloutRefs,
  fixtureHasEnvFrames,
  formatChildEvalCost,
  formatChildEvalReward,
  projectAtCursor,
} from "../templates/optimizer.run.v1/components/projectEvents.ts";
import { normalizeOptimizerEvents } from "../templates/optimizer.run.v1/components/normalizeEvents.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function loadFixture(rel) {
  return JSON.parse(readFileSync(join(root, rel), "utf8"));
}

test("GEPA evaluations fixture is resource-refs, not NEV/frames", () => {
  const fixture = loadFixture(
    "templates/optimizer.gepa.evaluations.v1/examples/events.json",
  );
  const refs = extractContainerRolloutRefs(fixture.events);
  assert.ok(refs.length >= 2, "expected child container_rollout refs");
  for (const ref of refs) {
    assert.equal(ref.kind, "container_rollout");
    assert.match(ref.id, /^rollout_/);
    assert.equal(ref.role, "candidate_evaluation");
    assert.ok(ref.attributes?.stream_id);
    assert.match(String(ref.attributes?.reward_url), /^\/reward\?rollout_id=/);
  }
  const blob = JSON.stringify(fixture);
  assert.equal(blob.includes('"kind":"frame"'), false);
  assert.equal(/nev/i.test(blob), false);
  assert.equal(fixtureHasEnvFrames(fixture.events), false);
  assert.ok(!fixture.events.some((event) => event.schemaVersion === "synth.trace-stream-event.v1"));
});

test("missing child eval reward stays em dash, never 0", () => {
  const fixture = loadFixture(
    "templates/optimizer.gepa.evaluations.v1/examples/events.json",
  );
  const refs = extractContainerRolloutRefs(fixture.events);
  const missing = refs.find((ref) => ref.attributes?.reward == null);
  assert.ok(missing, "fixture should include a child ref with no reward");
  assert.equal(formatChildEvalReward(missing), "—");
  assert.equal(formatMissingNumber(missing.attributes?.reward), "—");
  assert.notEqual(formatChildEvalReward(missing), "0");
  assert.notEqual(formatChildEvalReward(missing), "$0.00");

  const projected = projectAtCursor(fixture.run, fixture.events);
  const pending = projected.gepa?.evaluations.find((row) => row.ref.id === missing.id);
  assert.equal(formatChildEvalReward(pending.ref), "—");
});

test("GEPA live fixture has no env frames and omits usage until present", () => {
  const fixture = loadFixture("templates/optimizer.gepa.live.v1/examples/events.json");
  assert.equal(fixtureHasEnvFrames(fixture.events), false);
  const start = projectAtCursor(fixture.run, fixture.events, 1);
  assert.equal(formatMissingNumber(start.usage.costUsd), "—");
  const later = projectAtCursor(fixture.run, fixture.events, 3);
  assert.equal(formatMissingNumber(later.usage.costUsd), "0.12");
});

test("canonical GEPA runtime events project terminal status and real usage", () => {
  const run = { id: "gepa_live", algorithmId: "gepa", status: "running" };
  const events = [
    {
      type: "runtime.job.completed",
      sequenceNumber: 1,
      occurredAt: "2026-08-12T20:00:00Z",
      optimizerRunId: run.id,
      algorithmId: "gepa",
      delta: {
        rollout_count: 4,
        cost_usd: 0.02,
        wall_seconds: 2.5,
        usage: { prompt_tokens: 2500, completion_tokens: 25 },
      },
    },
    {
      type: "optimizer.state.transitioned",
      sequenceNumber: 2,
      occurredAt: "2026-08-12T20:00:03Z",
      optimizerRunId: run.id,
      algorithmId: "gepa",
      delta: { to: "completed" },
    },
    {
      type: "gepa.run.finished",
      sequenceNumber: 3,
      occurredAt: "2026-08-12T20:00:04Z",
      optimizerRunId: run.id,
      algorithmId: "gepa",
      delta: {
        state: "completed",
        rollout_count: 8,
        cost_usd: 0.04,
        usage: { prompt_tokens: 5000, completion_tokens: 50 },
      },
    },
  ];
  const projected = projectAtCursor(run, events);
  assert.equal(projected.summary.status, "completed");
  assert.equal(projected.usage.rollouts, 8);
  assert.equal(projected.usage.promptTokens, 5000);
  assert.equal(projected.usage.completionTokens, 50);
  assert.equal(projected.usage.costUsd, 0.04);
  assert.equal(projected.usage.wallTimeMs, 2500);
});

test("canonical GEPA frontier members paint Pareto cells", () => {
  const projected = projectAtCursor(
    { id: "gepa_frontier", algorithmId: "gepa", status: "running" },
    [{
      type: "frontier.updated",
      sequenceNumber: 1,
      occurredAt: "2026-08-12T20:00:00Z",
      optimizerRunId: "gepa_frontier",
      algorithmId: "gepa",
      delta: {
        best_candidate_id: "cand_proposed",
        members: [
          { candidate_id: "cand_seed", train_reward: 0.5, status: "full_train_evaluated" },
          { candidate_id: "cand_proposed", train_reward: 0.75, status: "full_train_evaluated", parent_id: "cand_seed", is_best: true },
        ],
      },
    }],
  );
  assert.equal(projected.gepa.frontier.length, 2);
  assert.deepEqual(projected.gepa.frontier[1], {
    candidateId: "cand_proposed",
    quality: 0.75,
    heldoutQuality: undefined,
    costUsd: undefined,
    coveredExampleCount: undefined,
    evaluatedExampleCount: undefined,
    coverage: undefined,
    accent: true,
    status: "full_train_evaluated",
    parentId: "cand_seed",
  });
});

test("canonical GEPA child attachment becomes a first-class evaluation ref", () => {
  const ref = {
    kind: "container_rollout",
    id: "rollout_live_1",
    role: "candidate_evaluation",
    attributes: { stream_id: "stream_live_1", reward: null },
  };
  const event = {
    type: "optimizer.child_rollout.attached",
    sequenceNumber: 4,
    occurredAt: "2026-08-12T20:00:04Z",
    optimizerRunId: "gepa_live",
    algorithmId: "gepa",
    delta: { candidate_id: "cand_1", child_resource_ref: ref },
  };
  assert.deepEqual(extractContainerRolloutRefs([event]), [ref]);
  const projected = projectAtCursor(
    { id: "gepa_live", algorithmId: "gepa", status: "running" },
    [event],
  );
  assert.equal(projected.gepa.evaluations[0].candidateId, "cand_1");
  assert.equal(projected.gepa.evaluations[0].ref.id, "rollout_live_1");
});

test("GEPA child results and proposer calls remain inspectable projections", () => {
  const run = { id: "gepa_live", algorithmId: "gepa", status: "running" };
  const ref = {
    kind: "container_rollout",
    id: "rollout_live_2",
    role: "candidate_evaluation",
    attributes: { stream_id: "stream:rollout_live_2", reward_url: "/reward?rollout_id=rollout_live_2" },
  };
  const base = { occurredAt: "2026-08-12T20:00:00Z", optimizerRunId: run.id, algorithmId: "gepa" };
  const projected = projectAtCursor(run, [
    { ...base, type: "optimizer.child_rollout.attached", sequenceNumber: 1, delta: { candidate_id: "cand_2", stage: "heldout", example_id: "test:2", child_resource_ref: ref } },
    { ...base, type: "optimizer.evaluation_result.received", sequenceNumber: 2, delta: { rollout_id: ref.id, reward: 0.75, cost_usd: 0.01, usage: { total_tokens: 123 } } },
    { ...base, type: "runtime.job.completed", sequenceNumber: 3, delta: { lane: "proposer", generation: 0, runtime_effect_id: "effect_1", model: "gpt-5.6-luna", wall_seconds: 4.2, usage: { total_tokens: 456 } } },
    { ...base, type: "proposer.completed", sequenceNumber: 4, delta: { generation: 0, provider: "openai", backend: "codex_app_server", workspace: "/tmp/proposer/generation_000" } },
  ]);
  const evaluation = projected.gepa.evaluations[0];
  assert.equal(evaluation.stage, "heldout");
  assert.equal(evaluation.exampleId, "test:2");
  assert.equal(evaluation.reward, 0.75);
  assert.equal(evaluation.ref.attributes.reward, 0.75);
  assert.equal(evaluation.usage.total_tokens, 123);
  const trace = projected.gepa.proposerTraces[0];
  assert.equal(trace.generation, 0);
  assert.equal(trace.sequence, 4);
  assert.equal(trace.status, "completed");
  assert.equal(trace.runtimeEffectId, "effect_1");
  assert.equal(trace.model, "gpt-5.6-luna");
  assert.equal(trace.backend, "codex_app_server");
  assert.equal(trace.wallSeconds, 4.2);
  assert.deepEqual(trace.usage, { total_tokens: 456 });
  assert.equal(trace.provider, "openai");
  assert.equal(trace.workspace, "/tmp/proposer/generation_000");
  // The chronological trace narrative accumulates across proposer events.
  assert.deepEqual(trace.steps.map((step) => step.kind), ["status", "output"]);
  assert.equal(trace.endedAt, "2026-08-12T20:00:00Z");
});

test("GEPA exposes a running proposer trace as soon as proposal work starts", () => {
  const run = { id: "gepa_live", algorithmId: "gepa", status: "running" };
  const projected = projectAtCursor(run, [{
    type: "optimizer.state.transitioned",
    sequenceNumber: 1,
    occurredAt: "2026-08-12T20:00:00Z",
    optimizerRunId: run.id,
    algorithmId: "gepa",
    delta: {
      trigger: "proposer_started",
      to: "proposing",
      details: {
        generation: 2,
        model: "gpt-5.6-luna",
        backend: "codex_app_server",
        workspace: "/tmp/proposer/generation_002",
      },
    },
  }]);
  assert.equal(projected.gepa.proposerTraces.length, 1);
  const runningTrace = projected.gepa.proposerTraces[0];
  assert.equal(runningTrace.generation, 2);
  assert.equal(runningTrace.sequence, 1);
  assert.equal(runningTrace.status, "running");
  assert.equal(runningTrace.model, "gpt-5.6-luna");
  assert.equal(runningTrace.backend, "codex_app_server");
  assert.equal(runningTrace.workspace, "/tmp/proposer/generation_002");
  assert.equal(runningTrace.startedAt, "2026-08-12T20:00:00Z");
  assert.deepEqual(runningTrace.steps.map((step) => step.kind), ["context", "generation"]);
  assert.equal(projected.gepa.activity.label, "Creating candidates");
  assert.equal(projected.gepa.activity.proposalActive, true);
  assert.equal(projected.gepa.activity.evaluationActive, false);
});

test("GEPA projects candidate lifecycle before, during, and after evaluation", () => {
  const run = { id: "gepa_lifecycle", algorithmId: "gepa", status: "running" };
  const base = { occurredAt: "2026-08-12T20:00:00Z", optimizerRunId: run.id, algorithmId: "gepa" };
  const events = [
    { ...base, type: "candidate.registered", sequenceNumber: 1, delta: { candidate_id: "cand_live", parent_id: "cand_seed", source: "reflector", values: { system: "Inspect me before evaluation" } } },
    { ...base, type: "optimizer.state.transitioned", sequenceNumber: 2, delta: { trigger: "rollouts_started", to: "rollout_running", message: "Candidate minibatch rollouts started", details: { generation: 1 } } },
    { ...base, type: "optimizer.candidate_evaluation.allocated", sequenceNumber: 3, delta: { candidate_id: "cand_live", stage: "candidate_minibatch" } },
    { ...base, type: "candidate.minibatch_evaluated", sequenceNumber: 4, delta: { candidate_id: "cand_live", parent_id: "cand_seed", minibatch_reward: 0.72 } },
    { ...base, type: "candidate.accepted", sequenceNumber: 5, delta: { candidate_id: "cand_live", parent_id: "cand_seed", train_reward: 0.74 } },
  ];
  const before = projectAtCursor(run, events, 1);
  assert.equal(before.gepa.candidates[0].status, "registered");
  assert.equal(before.gepa.candidates[0].values.system, "Inspect me before evaluation");
  const during = projectAtCursor(run, events, 3);
  assert.equal(during.gepa.candidates[0].status, "evaluating");
  assert.equal(during.gepa.activity.label, "Evaluating proposed candidates");
  assert.deepEqual(during.gepa.activity.activeCandidateIds, ["cand_live"]);
  const after = projectAtCursor(run, events, 5);
  assert.equal(after.gepa.candidates[0].status, "accepted");
  assert.equal(after.gepa.candidates[0].score, 0.74);
  assert.deepEqual(after.gepa.activity.activeCandidateIds, []);
});

test("GEPA can represent proposer and evaluator lanes concurrently", () => {
  const run = { id: "gepa_parallel", algorithmId: "gepa", status: "running" };
  const base = { occurredAt: "2026-08-12T20:00:00Z", optimizerRunId: run.id, algorithmId: "gepa" };
  const projected = projectAtCursor(run, [
    { ...base, type: "optimizer.state.transitioned", sequenceNumber: 1, delta: { trigger: "rollouts_started", to: "rollout_running", message: "Candidate minibatch rollouts started" } },
    { ...base, type: "optimizer.candidate_evaluation.allocated", sequenceNumber: 2, delta: { candidate_id: "cand_eval", stage: "candidate_minibatch" } },
    { ...base, type: "proposer.started", sequenceNumber: 3, delta: { generation: 2, model: "gpt-5.6-luna" } },
  ]);
  assert.equal(projected.gepa.activity.proposalActive, true);
  assert.equal(projected.gepa.activity.evaluationActive, true);
  assert.equal(projected.gepa.activity.label, "Creating + evaluating candidates");
});

test("hosted GELO state keeps Craftax child streams inspectable", () => {
  const run = { id: "gelo_craftax", algorithmId: "go-ex", status: "running" };
  const projected = projectAtCursor(run, [{
    type: "goex.state.batch.updated",
    sequenceNumber: 9,
    occurredAt: "2026-08-12T20:00:00Z",
    optimizerRunId: run.id,
    algorithmId: "go-ex",
    snapshot: {
      slices: {
        board: { data: { phase: "core_proposal", tick: 2 } },
        themes: { data: { themes: [{ theme_id: "survival", title: "Survival" }] } },
        candidates: { data: { candidates: [{ candidate_id: "cand_1", reward_mean: 0.5 }] } },
        frontier: { data: { candidate_frontier: [{ candidate_id: "cand_1", fresh_reward_mean: 0.5 }] } },
        agents: { data: { coreProposer: { status: "running", round_index: 1 } } },
        "data-engine": { data: { child_streams: [{
          rollout_id: "rollout_craftax_1",
          candidate_id: "cand_1",
          seed: 101,
          dispatch_kind: "fresh_rollout",
          split: "train",
          state: "running",
          reward: null,
          stream: {
            id: "stream:rollout_craftax_1",
            transports: { poll: { url: "/rollouts/rollout_craftax_1/events" } },
            reward: { url: "/rollouts/rollout_craftax_1/reward" },
          },
        }], rollout_evidence: null } },
      },
    },
  }]);
  assert.equal(projected.goex.board.phase, "core_proposal");
  assert.equal(projected.goex.themes[0].theme_id, "survival");
  assert.equal(projected.goex.rollouts.length, 1);
  assert.equal(projected.goex.rollouts[0].ref.id, "rollout_craftax_1");
  assert.equal(projected.goex.rollouts[0].ref.attributes.stream_id, "stream:rollout_craftax_1");
  assert.equal(projected.goex.rollouts[0].status, "running");
  assert.equal(projected.goex.rollouts[0].reward, null);
  assert.equal(projected.goex.rollouts[0].ref.attributes.poll_url, "/rollouts/rollout_craftax_1/events");
  assert.equal(projected.goex.rollouts[0].ref.attributes.stream.transports.poll.url, "/rollouts/rollout_craftax_1/events");
});

test("canonical GELO lifecycle events keep candidates and child streams inspectable without state batches", () => {
  const run = { id: "gelo_local", algorithmId: "go-ex", status: "completed" };
  const base = { occurredAt: "2026-08-12T20:00:00Z", optimizerRunId: run.id, algorithmId: "go-ex" };
  const resource_ref = {
    resource_type: "rollout",
    rollout_id: "rollout_local_1",
    stream: {
      id: "stream:rollout_local_1",
      transports: { poll: { url: "/rollouts/rollout_local_1/events" } },
      reward: { url: "/rollouts/rollout_local_1/reward" },
    },
  };
  const projected = projectAtCursor(run, [
    { ...base, type: "goex.seed_candidate_registered", sequenceNumber: 1, delta: { candidate_id: "cand_local" } },
    { ...base, type: "goex.theme_state_changed", sequenceNumber: 2, delta: { theme_id: "survival", name: "Survival", to: "active" } },
    { ...base, type: "child.rollout.registered", sequenceNumber: 3, delta: { candidate_id: "cand_local", split: "train", evaluation_stage: "search_fresh", status: "subscribed", resource_ref } },
    { ...base, type: "child.rollout.completed", sequenceNumber: 4, delta: { candidate_id: "cand_local", split: "train", evaluation_stage: "search_fresh", status: "completed", reward: 0.5, resource_ref } },
    { ...base, type: "candidate.registered", sequenceNumber: 5, delta: { candidate_id: "cand_proposed", parent_id: "cand_local", source: "core_proposer", values: { system: "Inspect the live proposal" } } },
    { ...base, type: "proposer.delta", sequenceNumber: 6, delta: { generation: 0, channel: "content", text: "proposal " } },
    { ...base, type: "proposer.delta", sequenceNumber: 7, delta: { generation: 0, channel: "content", text: "reasoning" } },
    { ...base, type: "goex.core_proposer_finished", sequenceNumber: 8, delta: { cost_usd: 0.01 } },
    { ...base, type: "goex.acceptance_completed", sequenceNumber: 9, delta: { champion_candidate_id: "cand_proposed", baseline_candidate_id: "cand_local" } },
    { ...base, type: "goex.best_base_decided", sequenceNumber: 10, delta: { candidate_id: "cand_local", fresh_reward_mean: 0.5 } },
  ]);
  assert.equal(projected.goex.candidates.length, 2);
  assert.equal(projected.goex.candidates.find((candidate) => candidate.candidate_id === "cand_local").on_frontier, true);
  const proposal = projected.goex.candidates.find((candidate) => candidate.candidate_id === "cand_proposed");
  assert.equal(proposal.values.system, "Inspect the live proposal");
  assert.equal(proposal.status, "accepted");
  assert.equal(proposal.on_frontier, true);
  assert.equal(projected.goex.agents.coreProposer.streaming.content, "proposal reasoning");
  assert.equal(projected.goex.agents.coreProposer.status, "completed");
  assert.equal(projected.goex.themes[0].theme_id, "survival");
  assert.equal(projected.goex.rollouts.length, 1);
  assert.equal(projected.goex.rollouts[0].ref.id, "rollout_local_1");
  assert.equal(projected.goex.rollouts[0].ref.attributes.stream_id, "stream:rollout_local_1");
  assert.equal(projected.goex.rollouts[0].reward, 0.5);
});

test("optimizer normalization fails closed when sequence is missing", () => {
  assert.throws(
    () => normalizeOptimizerEvents([{ type: "candidate.created" }]),
    /missing a valid sequence number/,
  );
  assert.equal(normalizeOptimizerEvents([{ type: "ok", sequence_number: 4 }])[0].sequenceNumber, 4);
});

test("hosted SFT events paint curves, keep missing val loss, and do not treat ready as promoted", () => {
  const run = { id: "sft_hosted_1", algorithmId: "sft", status: "running", source: "hosted" };
  const events = normalizeOptimizerEvents([
    {
      schema_version: "optimizer_event.v1",
      type: "optimizer.visual.ready",
      sequence_number: 1,
      created_at: "2026-08-12T19:40:00Z",
      run_id: run.id,
      algorithm_id: "sft",
      delta: { ready: true, slot: "optimizer_run" },
    },
    {
      schema_version: "optimizer_event.v1",
      type: "sft.training.metrics",
      sequence_number: 2,
      created_at: "2026-08-12T19:40:01Z",
      run_id: run.id,
      algorithm_id: "sft",
      delta: {
        step: 10,
        epoch: 1,
        train_loss: 1.4,
        validation_loss: null,
        learning_rate: 0.0002,
      },
    },
    {
      schema_version: "optimizer_event.v1",
      type: "sft.checkpoint.created",
      sequence_number: 3,
      created_at: "2026-08-12T19:40:02Z",
      run_id: run.id,
      algorithm_id: "sft",
      item: { kind: "checkpoint", id: "ckpt_sft_hosted_1_10", status: "created", raw: { promoted: false } },
      delta: { checkpoint_id: "ckpt_sft_hosted_1_10", step: 10, promoted: false },
    },
    {
      schema_version: "optimizer_event.v1",
      type: "sft.checkpoint.ready",
      sequence_number: 4,
      created_at: "2026-08-12T19:40:03Z",
      run_id: run.id,
      algorithm_id: "sft",
      item: { kind: "checkpoint", id: "ckpt_sft_hosted_1_10", status: "ready", raw: { promoted: false } },
      delta: { checkpoint_id: "ckpt_sft_hosted_1_10", step: 10, promoted: false },
    },
    {
      schema_version: "optimizer_event.v1",
      type: "sft.checkpoint_evaluation.allocated",
      sequence_number: 5,
      created_at: "2026-08-12T19:40:04Z",
      run_id: run.id,
      algorithm_id: "sft",
      item: { kind: "evaluation", id: "eval_ckpt_sft_hosted_1_10_selection", status: "allocated" },
      delta: {
        evaluation_id: "eval_ckpt_sft_hosted_1_10_selection",
        checkpoint_id: "ckpt_sft_hosted_1_10",
        split_role: "selection",
      },
      artifact_refs: [
        {
          schema: "synth.resource-ref.v1",
          kind: "container_rollout",
          id: "rollout_ckpt_sft_hosted_1_10_seed0",
          role: "candidate_evaluation",
          attributes: { stream_id: "stream_rollout_ckpt_sft_hosted_1_10_seed0", reward: null },
        },
        {
          schema: "synth.resource-ref.v1",
          kind: "container_rollout",
          id: "rollout_ckpt_sft_hosted_1_10_seed1",
          role: "candidate_evaluation",
          attributes: { stream_id: "stream_rollout_ckpt_sft_hosted_1_10_seed1", reward: null },
        },
      ],
    },
    {
      schema_version: "optimizer_event.v1",
      type: "sft.checkpoint_rollout.completed",
      sequence_number: 6,
      created_at: "2026-08-12T19:40:05Z",
      run_id: run.id,
      algorithm_id: "sft",
      item: { kind: "rollout", id: "rollout_ckpt_sft_hosted_1_10_seed0", status: "completed" },
      delta: {
        evaluation_id: "eval_ckpt_sft_hosted_1_10_selection",
        rollout_id: "rollout_ckpt_sft_hosted_1_10_seed0",
        reward: null,
        score: null,
      },
    },
    {
      schema_version: "optimizer_event.v1",
      type: "sft.checkpoint_rollout.completed",
      sequence_number: 7,
      created_at: "2026-08-12T19:40:06Z",
      run_id: run.id,
      algorithm_id: "sft",
      item: { kind: "rollout", id: "rollout_ckpt_sft_hosted_1_10_seed1", status: "completed" },
      delta: {
        evaluation_id: "eval_ckpt_sft_hosted_1_10_selection",
        rollout_id: "rollout_ckpt_sft_hosted_1_10_seed1",
        reward: 1.0,
        score: 1.0,
      },
      usage_delta: { rollouts: 1, prompt_tokens: 84, completion_tokens: 3, cost_usd: 0.004 },
    },
  ]);
  const projected = projectAtCursor(run, events);
  const beforeMeter = projectAtCursor(run, events, 5);
  assert.equal(projected.sft.points.length, 1);
  assert.equal(projected.sft.points[0].step, 10);
  assert.equal(projected.sft.points[0].trainLoss, 1.4);
  assert.equal(projected.sft.points[0].validationLoss, undefined);
  assert.equal(formatMissingNumber(projected.sft.points[0].validationLoss), "—");
  assert.equal(formatMissingNumber(beforeMeter.usage.costUsd), "—");
  assert.equal(formatChildEvalReward(beforeMeter.sft.campaigns[0].children[0]), "—");
  assert.equal(formatChildEvalCost(beforeMeter.sft.campaigns[0].children[0]), "—");
  const checkpoint = projected.sft.checkpoints.find((row) => row.id === "ckpt_sft_hosted_1_10");
  assert.equal(checkpoint.ready, true);
  assert.equal(checkpoint.promoted, false);
  assert.equal(checkpoint.status, "ready");
  const campaign = projected.sft.campaigns[0];
  assert.equal(campaign.checkpointId, "ckpt_sft_hosted_1_10");
  assert.equal(campaign.children[0].kind, "container_rollout");
  assert.equal(formatChildEvalReward(campaign.children[0]), "—");
  assert.equal(formatChildEvalCost(campaign.children[0]), "—");
  assert.equal(formatChildEvalReward(campaign.children[1]), "1.00");
  assert.equal(formatChildEvalCost(campaign.children[1]), "$0.0040");
  assert.equal(projected.usage.costUsd, 0.004);
});

test("SFT allocated rollout IDs become inspectable children without embedded resource refs", () => {
  const run = { id: "sft_local", algorithmId: "sft", status: "running" };
  const events = normalizeOptimizerEvents([
    { type: "sft.checkpoint_evaluation.allocated", sequence_number: 1, run_id: run.id, algorithm_id: "sft", item: { id: "eval_1", status: "allocated" }, delta: { evaluation_id: "eval_1", checkpoint_id: "ckpt_1", split_role: "selection" } },
    { type: "sft.checkpoint_rollout.allocated", sequence_number: 2, run_id: run.id, algorithm_id: "sft", item: { id: "rollout_1", status: "allocated" }, delta: { evaluation_id: "eval_1", checkpoint_id: "ckpt_1", rollout_id: "rollout_1", stream_id: "stream:rollout_1", seed: 0, split_role: "selection" } },
    { type: "sft.checkpoint_rollout.completed", sequence_number: 3, run_id: run.id, algorithm_id: "sft", item: { id: "rollout_1", status: "completed" }, delta: { evaluation_id: "eval_1", rollout_id: "rollout_1", reward: 1 } },
  ]);
  const child = projectAtCursor(run, events).sft.campaigns[0].children[0];
  assert.equal(child.id, "rollout_1");
  assert.equal(child.attributes.stream_id, "stream:rollout_1");
  assert.equal(child.attributes.reward, 1);
});

test("live templates do not import fixture fallbacks", () => {
  for (const template of [
    "live.craftax.v1",
    "live.harbor_eval.v1",
    "live.digbench.v1",
    "live.eval_stream.v1",
    "live.intern_acceptance.v1",
    "live.container_rollouts.v1",
  ]) {
    const shell = readFileSync(join(root, "templates", template, "shell.tsx"), "utf8");
    assert.doesNotMatch(shell, /import\s+\w*[Ff]ixture|\?\?\s+liveFixture|return\s+liveFixture/);
  }
});

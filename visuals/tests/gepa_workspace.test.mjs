import assert from "node:assert/strict";
import test from "node:test";
import { projectAtCursor } from "../templates/optimizer.run.v1/components/projectEvents.ts";

// Condensed from the real banking77_gepa_sol_med_45856f25 run: same event
// types, field names, and decision values, with the 140 per-rollout events
// reduced to representatives.
const RUN = { id: "banking77_gepa_sol_med_45856f25", algorithmId: "gepa", status: "running" };
const base = { occurredAt: "2026-08-12T20:57:34Z", optimizerRunId: RUN.id, algorithmId: "gepa" };

function solEvents() {
  let seq = 0;
  const at = (minute, second = 0) =>
    new Date(Date.UTC(2026, 7, 12, 20, 57 + minute, second)).toISOString();
  const ref = (id) => ({
    kind: "container_rollout",
    id,
    role: "candidate_evaluation",
    schema: "synth.resource-ref.v1",
    attributes: { stream_id: `stream:${id}`, reward_url: `/reward?rollout_id=${id}` }
  });
  return [
    { ...base, sequenceNumber: ++seq, type: "optimizer.state.transitioned", occurredAt: at(0), delta: { from: "created", to: "initializing", trigger: "run_started", message: "GEPA run initializing", details: { policy_model: "gpt-4.1-nano", proposer_model: "gpt-5.6-sol" } } },
    { ...base, sequenceNumber: ++seq, type: "gepa.run.started", occurredAt: at(0, 1), delta: { state: "initializing", message: "GEPA run started" } },
    { ...base, sequenceNumber: ++seq, type: "candidate.registered", occurredAt: at(0, 6), delta: { candidate_id: "gepa_seed", source: "seed", status: "registered", message: "Seed candidate registered" } },
    { ...base, sequenceNumber: ++seq, type: "optimizer.state.transitioned", occurredAt: at(0, 7), delta: { from: "ready", to: "rollout_running", trigger: "rollouts_started", message: "Seed candidate rollouts started", details: { candidate_id: "gepa_seed", rollout_count: 50, stage: "seed_full_train" } } },
    { ...base, sequenceNumber: ++seq, type: "optimizer.candidate_evaluation.allocated", occurredAt: at(0, 8), delta: { candidate_id: "gepa_seed", stage: "seed_full_train", example_id: "train:0", child_resource_ref: ref("rollout_seed_0") } },
    { ...base, sequenceNumber: ++seq, type: "optimizer.child_rollout.attached", occurredAt: at(0, 8), delta: { candidate_id: "gepa_seed", stage: "seed_full_train", example_id: "train:0", child_resource_ref: ref("rollout_seed_0") } },
    { ...base, sequenceNumber: ++seq, type: "optimizer.evaluation_result.received", occurredAt: at(0, 9), delta: { candidate_id: "gepa_seed", rollout_id: "rollout_seed_0", stage: "seed_full_train", example_id: "train:0", reward: 1.0, cost_usd: 0.0, usage: { total_tokens: 630 } } },
    {
      ...base, sequenceNumber: ++seq, type: "optimizer.limit.estimate_updated", occurredAt: at(0, 15),
      delta: {
        limits: [
          { kind: "cost_usd", max_value: 2.45, spent: 0.0, remaining: 2.45, utilization: 0.0, hard: true },
          { kind: "total_rollouts", max_value: 240.0, spent: 50.0, remaining: 190.0, utilization: 50 / 240, hard: true }
        ]
      }
    },
    { ...base, sequenceNumber: ++seq, type: "candidate.evaluated", occurredAt: at(0, 18), delta: { candidate_id: "gepa_seed", train_reward: 0.76, message: "Seed candidate evaluated" } },
    { ...base, sequenceNumber: ++seq, type: "frontier.updated", occurredAt: at(0, 18), delta: { best_candidate_id: "gepa_seed", best_train_reward: 0.76, reason: "seed_full_train", frontier: [{ candidate_id: "gepa_seed", train_reward: 0.76, heldout_reward: null, parent_id: null, source: "seed" }] } },
    {
      ...base, sequenceNumber: ++seq, type: "optimizer.state.transitioned", occurredAt: at(0, 18),
      delta: {
        from: "ready", to: "proposing", trigger: "proposer_started", message: "Proposer started",
        details: { generation: 0, model: "gpt-5.6-sol", backend: "codex_app_server", parent_candidate_id: "gepa_seed", loss_count: 12, rollout_row_count: 100, workspace: "/runs/sol/proposer_workspaces/generation_000" }
      }
    },
    { ...base, sequenceNumber: ++seq, type: "runtime.job.completed", occurredAt: at(3, 21), delta: { lane: "proposer", generation: 0, model: "gpt-5.6-sol", runtime_effect_id: "effect_prop", wall_seconds: 185.2, cost_usd: 0.0, usage: { prompt_tokens: 72986, completion_tokens: 123, total_tokens: 73109, proposer_calls: 1 } } },
    { ...base, sequenceNumber: ++seq, type: "proposer.completed", occurredAt: at(3, 23), delta: { generation: 0, model: "gpt-5.6-sol", provider: "openai", backend: "codex_app_server", proposal_count: 1, message: "Proposer returned candidates", workspace: "/runs/sol/proposer_workspaces/generation_000" } },
    { ...base, sequenceNumber: ++seq, type: "optimizer.state.transitioned", occurredAt: at(3, 23), delta: { from: "proposing", to: "rollout_queueing", trigger: "proposer_finished", message: "Proposer returned candidates; rollout queue ready", details: { generation: 0, proposal_count: 1 } } },
    { ...base, sequenceNumber: ++seq, type: "candidate.registered", occurredAt: at(3, 23), delta: { candidate_id: "gepa_proposal", generation: 0, parent_id: "gepa_seed", proposal_index: 0, source: "reflector:parent_variation", status: "registered", message: "Proposed candidate registered" } },
    { ...base, sequenceNumber: ++seq, type: "optimizer.state.transitioned", occurredAt: at(3, 24), delta: { from: "rollout_queueing", to: "rollout_running", trigger: "rollouts_started", message: "Parent minibatch reference rollouts started", details: { candidate_id: "gepa_seed", row_count: 20, stage: "parent_minibatch_reference" } } },
    { ...base, sequenceNumber: ++seq, type: "parent_minibatch_reference.completed", occurredAt: at(3, 28), delta: { candidate_id: "gepa_seed", generation: 0, reward: 0.9, row_count: 20, message: "Parent minibatch reference completed" } },
    { ...base, sequenceNumber: ++seq, type: "optimizer.state.transitioned", occurredAt: at(3, 29), delta: { from: "rollout_queueing", to: "rollout_running", trigger: "rollouts_started", message: "Candidate minibatch rollouts started", details: { candidate_count: 1, generation: 0, rollout_count: 20, stage: "candidate_minibatch" } } },
    { ...base, sequenceNumber: ++seq, type: "optimizer.candidate_evaluation.allocated", occurredAt: at(3, 29), delta: { candidate_id: "gepa_proposal", stage: "candidate_minibatch", example_id: "train:0", child_resource_ref: ref("rollout_prop_0") } },
    { ...base, sequenceNumber: ++seq, type: "optimizer.child_rollout.attached", occurredAt: at(3, 29), delta: { candidate_id: "gepa_proposal", stage: "candidate_minibatch", example_id: "train:0", child_resource_ref: ref("rollout_prop_0") } },
    { ...base, sequenceNumber: ++seq, type: "optimizer.evaluation_result.received", occurredAt: at(3, 45), delta: { candidate_id: "gepa_proposal", rollout_id: "rollout_prop_0", stage: "candidate_minibatch", example_id: "train:0", reward: 0.0, cost_usd: 0.0, usage: { total_tokens: 640 } } },
    {
      ...base, sequenceNumber: ++seq, type: "candidate.minibatch_evaluated", occurredAt: at(4, 2),
      delta: { accepted_minibatch: false, candidate_id: "gepa_proposal", minibatch_delta: 0.0, minibatch_reward: 0.9, parent_id: "gepa_seed", parent_minibatch_reward: 0.9, message: "Candidate minibatch evaluated" }
    },
    {
      ...base, sequenceNumber: ++seq, type: "candidate.rejected", occurredAt: at(4, 2),
      delta: { accepted_full_train: false, accepted_minibatch: false, best_train_reward: 0.76, candidate_id: "gepa_proposal", candidate_minibatch_reward: 0.9, candidate_train_reward: null, comparison_result: "tie", parent_id: "gepa_seed", parent_minibatch_reward: 0.9, reason: "primary_not_improved", message: "Candidate rejected at minibatch" }
    },
    { ...base, sequenceNumber: ++seq, type: "optimizer.state.transitioned", occurredAt: at(4, 3), delta: { from: "rollout_queueing", to: "rollout_running", trigger: "rollouts_started", message: "Heldout rollouts started", details: { candidate_count: 1, rollout_count: 50, stage: "heldout" } } },
    { ...base, sequenceNumber: ++seq, type: "heldout.completed", occurredAt: at(4, 12), delta: { candidate_id: "gepa_seed", heldout_reward: 0.6, train_reward: 0.76, message: "Heldout evaluation completed" } },
    { ...base, sequenceNumber: ++seq, type: "optimizer.state.transitioned", occurredAt: at(4, 12), delta: { from: "evaluating", to: "completed", trigger: "run_completed", message: "GEPA run completed", details: { best_candidate_id: "gepa_seed", heldout_reward: 0.6, heldout_skipped: false } } },
    {
      ...base, sequenceNumber: ++seq, type: "gepa.run.finished", occurredAt: at(4, 13),
      delta: { best_candidate_id: "gepa_seed", cost_usd: 0.0, heldout_reward: 0.6, heldout_skipped: false, rollout_count: 140, state: "completed", message: "GEPA run finished", usage: { completion_tokens: 688, prompt_tokens: 179449, proposer_calls: 1, rollout_calls: 140, total_tokens: 180137 } }
    }
  ];
}

test("completed GEPA run reports terminal truth, not LIVE/waiting placeholders", () => {
  const projected = projectAtCursor(RUN, solEvents());
  const gepa = projected.gepa;
  assert.equal(projected.summary.status, "completed");
  assert.equal(gepa.activity.terminal, true);
  assert.equal(gepa.activity.label, "Search complete");
  assert.equal(gepa.activity.proposalActive, false);
  assert.equal(gepa.activity.evaluationActive, false);
  assert.equal(gepa.best.candidateId, "gepa_seed");
  assert.equal(gepa.best.trainReward, 0.76);
  assert.equal(gepa.best.heldoutReward, 0.6);
  assert.equal(gepa.heldout.reward, 0.6);
  assert.equal(gepa.models.proposer, "gpt-5.6-sol");
  assert.equal(gepa.models.policy, "gpt-4.1-nano");
  assert.equal(gepa.timing.startedAt, "2026-08-12T20:57:00.000Z");
  assert.equal(gepa.timing.endedAt, "2026-08-12T21:01:13.000Z");
});

test("budget limits come from limit estimates and reconcile with the terminal event", () => {
  const events = solEvents();
  const midRun = projectAtCursor(RUN, events, 8);
  const rollouts = midRun.gepa.limits.find((limit) => limit.kind === "total_rollouts");
  assert.equal(rollouts.max, 240);
  assert.equal(rollouts.spent, 50);
  const done = projectAtCursor(RUN, events);
  const finalRollouts = done.gepa.limits.find((limit) => limit.kind === "total_rollouts");
  assert.equal(finalRollouts.spent, 140);
  assert.equal(finalRollouts.remaining, 100);
  const proposerCalls = done.gepa.limits.find((limit) => limit.kind === "proposer_calls");
  assert.equal(proposerCalls.spent, 1);
  const cost = done.gepa.limits.find((limit) => limit.kind === "cost_usd");
  assert.equal(cost.spent, 0);
  assert.equal(cost.max, 2.45);
});

test("semantic stages progress through the search and settle at terminal states", () => {
  const events = solEvents();
  const during = projectAtCursor(RUN, events, 11);
  const stageMap = Object.fromEntries(during.gepa.stages.map((stage) => [stage.id, stage.status]));
  assert.equal(stageMap.seed, "completed");
  assert.equal(stageMap.proposal, "active");
  assert.equal(stageMap.minibatch, "pending");
  assert.equal(stageMap.complete, "pending");
  const done = projectAtCursor(RUN, events);
  const doneMap = Object.fromEntries(done.gepa.stages.map((stage) => [stage.id, stage.status]));
  assert.equal(doneMap.seed, "completed");
  assert.equal(doneMap.proposal, "completed");
  assert.equal(doneMap.minibatch, "completed");
  assert.equal(doneMap.full_train, "skipped");
  assert.equal(doneMap.heldout, "completed");
  assert.equal(doneMap.complete, "completed");
});

test("rejected candidate carries gate decision, scores, and rationale", () => {
  const projected = projectAtCursor(RUN, solEvents());
  const proposal = projected.gepa.candidates.find((candidate) => candidate.id === "gepa_proposal");
  assert.equal(proposal.status, "rejected_minibatch");
  assert.equal(proposal.decision.outcome, "rejected");
  assert.equal(proposal.decision.gate, "minibatch");
  assert.equal(proposal.decision.reason, "primary_not_improved");
  assert.equal(proposal.decision.comparison, "tie");
  assert.equal(proposal.decision.candidateScore, 0.9);
  assert.equal(proposal.decision.parentScore, 0.9);
  assert.equal(proposal.minibatchDelta, 0);
  assert.equal(proposal.generation, 0);
  assert.equal(String(proposal.parentId), "gepa_seed");
});

test("proposer trace is a chronological narrative linked to its candidate", () => {
  const projected = projectAtCursor(RUN, solEvents());
  const trace = projected.gepa.proposerTraces[0];
  assert.equal(trace.status, "completed");
  assert.equal(trace.model, "gpt-5.6-sol");
  assert.equal(trace.parentCandidateId, "gepa_seed");
  assert.equal(trace.lossCount, 12);
  assert.equal(trace.proposalCount, 1);
  assert.deepEqual(trace.candidateIds, ["gepa_proposal"]);
  assert.deepEqual(
    trace.steps.map((step) => step.kind),
    ["context", "generation", "status", "output", "candidate"]
  );
  const candidateStep = trace.steps.at(-1);
  assert.equal(candidateStep.candidateId, "gepa_proposal");
});

test("proposer.delta chunks stream into one open trace and transcript reconciles on reopen", () => {
  const base = { occurredAt: "2026-08-12T20:58:00Z", optimizerRunId: RUN.id, algorithmId: "gepa" };
  const events = [
    {
      ...base, sequenceNumber: 1, type: "optimizer.state.transitioned",
      delta: { trigger: "proposer_started", to: "proposing", details: { generation: 0, model: "gpt-5.6-sol" } }
    },
    ...Array.from({ length: 40 }, (_, index) => ({
      ...base, sequenceNumber: 2 + index, type: "proposer.delta",
      delta: { generation: 0, channel: "reasoning", text: `chunk${index} ` }
    })),
    { ...base, sequenceNumber: 42, type: "proposer.delta", delta: { generation: 0, channel: "content", text: "Final proposal text." } },
    { ...base, sequenceNumber: 43, type: "proposer.completed", delta: { generation: 0, model: "gpt-5.6-sol", proposal_count: 1 } },
    {
      ...base, sequenceNumber: 44, type: "proposer.transcript.loaded",
      delta: {
        generation: 0,
        critique: { text: "The parent prompt lacks disambiguation heuristics.", truncated: false },
        rationale: { text: "Structural rewrite adds the missing heuristics.", truncated: false },
        failure_patterns: [{ text: "Security context displaced the transaction intent", truncated: false }],
        winning_patterns: [{ text: "Explicit state words were reliable", truncated: false }],
        candidate_comparison: { text: "Parent is a terse seed prompt", truncated: false },
        proposals: [{
          proposal_type: "parent_variation",
          parent_candidate_ids: ["gepa_seed"],
          rationale: { text: "High-variance replacement", truncated: false },
          proposed_payload: { text: "Classify the customer banking query…", truncated: true, total_chars: 9000 }
        }]
      }
    }
  ];
  const streamingView = projectAtCursor(RUN, events, 42);
  assert.equal(streamingView.gepa.proposerTraces.length, 1, "chunks extend one trace, never add rows");
  const streamingTrace = streamingView.gepa.proposerTraces[0];
  assert.equal(streamingTrace.status, "running");
  assert.match(streamingTrace.streaming.reasoning, /^chunk0 chunk1 /);
  assert.ok(streamingTrace.streaming.reasoning.includes("chunk39"));
  assert.equal(streamingTrace.streaming.content, "Final proposal text.");
  const finalView = projectAtCursor(RUN, events);
  const finalTrace = finalView.gepa.proposerTraces[0];
  assert.equal(finalTrace.status, "completed");
  assert.equal(finalTrace.reflection.critique.text, "The parent prompt lacks disambiguation heuristics.");
  assert.equal(finalTrace.reflection.failurePatterns.length, 1);
  assert.equal(finalTrace.reflection.proposals[0].proposalType, "parent_variation");
  assert.equal(finalTrace.reflection.proposals[0].proposedPayload.truncated, true);
  assert.equal(finalTrace.reflection.proposals[0].proposedPayload.totalChars, 9000);
});

test("mid-run projection keeps live lanes and selections honest", () => {
  const events = solEvents();
  // Cursor inside candidate minibatch rollouts: proposal complete, evaluation live.
  const during = projectAtCursor(RUN, events, 20);
  assert.equal(during.gepa.activity.terminal, false);
  assert.equal(during.gepa.activity.evaluationActive, true);
  assert.equal(during.gepa.activity.proposalActive, false);
  assert.deepEqual(during.gepa.activity.activeCandidateIds, ["gepa_proposal"]);
  const proposal = during.gepa.candidates.find((candidate) => candidate.id === "gepa_proposal");
  assert.equal(proposal.status, "evaluating");
  // Registered-but-unscored candidates are still present for selection.
  const registered = projectAtCursor(RUN, events, 15);
  const fresh = registered.gepa.candidates.find((candidate) => candidate.id === "gepa_proposal");
  assert.equal(fresh.status, "registered");
  assert.equal(fresh.score, undefined);
});

test("failed rollout evidence stays null, updates coverage, and blocks heldout promotion", () => {
  const events = [
    {
      ...base, sequenceNumber: 1, type: "optimizer.candidate_evaluation.attempt.failed",
      delta: {
        candidate_id: "gepa_seed", stage: "heldout", example_id: "heldout:7",
        job_id: "job-7", attempt: 3, max_attempts: 3,
        reward: null, cost_usd: null,
        failure: { failure_type: "transport", reason_code: "stream_timeout", message: "stream timed out", retryable: true }
      }
    },
    {
      ...base, sequenceNumber: 2, type: "optimizer.evaluation.coverage.updated",
      delta: { candidate_id: "gepa_seed", stage: "heldout", required: 10, scored: 9, failed: 1, pending: 0, complete: false }
    },
    {
      ...base, sequenceNumber: 3, type: "heldout.blocked",
      delta: { candidate_id: "gepa_seed", required: 10, scored: 9, failed: 1, missing: 0, promotion_eligible: false, reason: "incomplete_heldout_coverage" }
    }
  ];
  const projected = projectAtCursor(RUN, events);
  assert.deepEqual(projected.gepa.coverage[0], {
    candidateId: "gepa_seed", stage: "heldout", required: 10, scored: 9,
    failed: 1, pending: 0, complete: false, promotionEligible: false, sequence: 2
  });
  assert.equal(projected.gepa.failedAttempts[0].failureClass, "stream_timeout");
  assert.equal(projected.gepa.failedAttempts[0].attempt, 3);
  assert.equal(projected.gepa.heldout.blocked, true);
  assert.equal(projected.gepa.heldout.reward, undefined);
  assert.equal(projected.gepa.stages.find((stage) => stage.id === "heldout").status, "failed");
});

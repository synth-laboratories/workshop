import assert from "node:assert/strict";
import test from "node:test";
import { projectAtCursor } from "../families/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import { candidatePalette, elapsedLabel, generationPalette, incumbentCandidateIds, orderedScoredCandidates } from "../families/optimizers/_shared/optimizer.run.v1/overlays/gepa/model.ts";

// Condensed from the real banking77_gepa_sol_med_45856f25 run: same event
// types, field names, and decision values, with the 140 per-rollout events
// reduced to representatives.
const RUN = { id: "banking77_gepa_sol_med_45856f25", algorithmId: "gepa", status: "running" };
const base = { occurredAt: "2026-08-12T20:57:34Z", optimizerRunId: RUN.id, algorithmId: "gepa" };

test("generation colors are stable and keep the seed neutral", () => {
  assert.equal(candidatePalette({ source: "seed" }).color, "#667085");
  assert.equal(candidatePalette({ generation: 0 }).color, "#2563eb");
  assert.equal(candidatePalette({ generation: 1 }).color, "#7c3aed");
  assert.notEqual(candidatePalette({ generation: 0 }).color, candidatePalette({ generation: 1 }).color);
  assert.deepEqual(generationPalette(6), generationPalette(0), "the bounded palette repeats deterministically");
});

test("durable setup events retain task, dataset, container, and selected taskset context", () => {
  const projected = projectAtCursor(RUN, [
    {
      ...base, sequenceNumber: 1, type: "optimizer.state.transitioned",
      delta: { from: "created", to: "initializing", trigger: "run_started", details: {
        train_ids: Array.from({ length: 50 }, (_, index) => `train:${index}`),
        heldout_ids: Array.from({ length: 50 }, (_, index) => `test:${index}`),
        policy_model: "openai/gpt-5.6-luna", proposer_model: "openai/gpt-5.6-luna"
      } }
    },
    { ...base, sequenceNumber: 2, type: "gepa.run.started", delta: { container_url: "http://127.0.0.1:8127" } },
    {
      ...base, sequenceNumber: 3, type: "container.contract.verified", delta: {
        container_spec_id: "banking77-gepa-b-v6", workshop_instance: "B",
        credential_mode: "workshop_ephemeral_proxy", runtime_family: "banking77",
        reward_authority: "container_evaluator", evaluator_id: "banking77-evaluator-v1",
        retention: "run", scale_leases: 4,
        dataset: {
          source: "PolyAI/banking77", config: "test", revision: "evals:abc", row_count: 3080,
          label_count: 77, dataset_digest: "sha256:dataset",
          splits: { train: { count: 2114 }, selection: { count: 623 }, heldout: { count: 343 } }
        },
        policy_refs: [{ harness: "banking77_classifier", config: "chatgpt_proxy" }]
      }
    },
    {
      ...base, sequenceNumber: 4, type: "container.task_info.loaded", delta: {
        task: { id: "banking77-intents-v1", name: "Banking77 intent classification", description: "Classify one message.", task_family: "banking77", version: "v1" },
        dataset: {
          source: "PolyAI/banking77", config: "test", revision: "evals:abc", row_count: 3080,
          label_count: 77, dataset_digest: "sha256:dataset",
          splits: { train: { count: 2114 }, selection: { count: 623 }, heldout: { count: 343 } }
        }
      }
    },
    { ...base, sequenceNumber: 5, type: "container.program.loaded", delta: { program_id: "banking77-classifier-v1", mutable_fields: ["classification_system_prompt"] } },
    { ...base, sequenceNumber: 6, type: "taskset.tasks.loaded", delta: { minibatch_rows: 20, reflection_rows: 50, pareto_rows: 50, heldout_rows: 50, task_pools: { pareto: Array.from({ length: 50 }, (_, index) => `train:${index}`) } } }
  ]);

  assert.deepEqual(projected.gepa.contract.task, {
    id: "banking77-intents-v1", name: "Banking77 intent classification", objective: undefined,
    description: "Classify one message.", family: "banking77", version: "v1", outputKind: undefined
  });
  assert.deepEqual(projected.gepa.contract.dataset, {
    source: "PolyAI/banking77", config: "test", revision: "evals:abc", digest: "sha256:dataset",
    rowCount: 3080, labelCount: 77, splits: { train: 2114, selection: 623, heldout: 343 }
  });
  assert.deepEqual(projected.gepa.contract.splits, { train: 50, minibatch: 20, reflection: 50, pareto: 50, heldout: 50 });
  assert.deepEqual(projected.gepa.contract.container, {
    url: "http://127.0.0.1:8127", verified: true, specId: "banking77-gepa-b-v6", workshopInstance: "B",
    credentialMode: "workshop_ephemeral_proxy", evaluatorId: "banking77-evaluator-v1",
    runtimeFamily: "banking77", targetId: undefined, rewardAuthority: "container_evaluator",
    policyHarness: "banking77_classifier", policyConfig: "chatgpt_proxy", scaleLeases: 4, retention: "run"
  });
});

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
      delta: { trigger: "proposer_started", to: "proposing", details: { generation: 0, model: "gpt-5.6-sol", proposal_count: 3 } }
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
  assert.equal(streamingView.gepa.activity.requestedProposalCount, 3);
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

test("sealed Trace V5 projection preserves proposer input, visible thinking, tools, and output", () => {
  const items = [
    { id: "input-1", sequence: 1, family: "input", kind: "message.input", title: "GEPA proposer request", body: "Improve the parent prompt." },
    { id: "thinking-1", sequence: 2, family: "thinking", kind: "reasoning.summary", title: "Reasoning summary", body: "I will inspect the failing examples." },
    { id: "tool-1", sequence: 3, family: "tool", kind: "tool.shell", title: "Run shell command", body: "python analyze.py", detail: "12 failure clusters", status: "completed" },
    { id: "artifact-1", sequence: 4, family: "artifact", kind: "tool.file_change", title: "proposal/manifest.json", detail: "create proposal/manifest.json" },
    { id: "output-1", sequence: 5, family: "output", kind: "message.output", title: "Proposer response", body: "Created three candidates." }
  ];
  const projected = projectAtCursor(RUN, [
    { ...base, sequenceNumber: 1, type: "optimizer.state.transitioned", delta: { trigger: "proposer_started", to: "proposing", details: { generation: 0, model: "gpt-5.6-sol" } } },
    { ...base, sequenceNumber: 2, type: "proposer.trace_v5.loaded", delta: { generation: 0, items } }
  ]);
  assert.deepEqual(projected.gepa.proposerTraces[0].traceV5Items, items);
  assert.deepEqual(projected.gepa.proposerTraces[0].traceV5Items.map((item) => item.family), ["input", "thinking", "tool", "artifact", "output"]);
  assert.equal(projected.gepa.proposerTraces[0].traceV5Items[2].detail, "12 failure clusters");
});

test("live elapsed time advances beyond the last quiet proposer event", () => {
  const originalNow = Date.now;
  Date.now = () => Date.parse("2026-08-12T20:59:00Z");
  try {
    assert.equal(elapsedLabel({
      startedAt: "2026-08-12T20:57:00Z",
      lastEventAt: "2026-08-12T20:57:30Z"
    }, false), "2m 0s");
  } finally {
    Date.now = originalNow;
  }
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

test("a partial result creates a rollout immediately and checkpoint replay is idempotent", () => {
  const child = {
    kind: "container_rollout", id: "roll_live_1", role: "candidate_evaluation",
    schema: "synth.resource-ref.v1", attributes: { stream_id: "stream:roll_live_1" }
  };
  const result = {
    ...base, type: "optimizer.evaluation_result.received",
    delta: {
      evaluation_id: "seed:seed_full_train:train:2", candidate_id: "gepa_seed",
      stage: "seed_full_train", example_id: "train:2", rollout_id: "roll_live_1",
      child_resource_ref: child, reward: 0.75, cost_usd: null, partial: true,
      active_workers: 3, semaphore_size: 3, queued_rollouts: 8
    }
  };
  const projected = projectAtCursor(RUN, [
    { ...base, sequenceNumber: 1, type: "optimizer.candidate_evaluation.allocated", delta: { candidate_id: "gepa_seed", stage: "seed_full_train", example_id: "train:2", child_resource_ref: child } },
    { ...base, sequenceNumber: 2, type: "optimizer.child_rollout.attached", delta: { candidate_id: "gepa_seed", stage: "seed_full_train", example_id: "train:2", child_resource_ref: child } },
    { ...result, sequenceNumber: 3 },
    { ...result, sequenceNumber: 4, delta: { ...result.delta, partial: false } }
  ]);
  assert.equal(projected.gepa.evaluations.length, 1);
  assert.equal(projected.gepa.evaluations[0].reward, 0.75);
  assert.equal(projected.gepa.evaluations[0].costUsd, undefined);
  assert.equal(projected.gepa.rolloutsCompleted, 1);
  assert.deepEqual(projected.gepa.runtime, {
    activeWorkers: 3, semaphoreSize: 3, queuedRollouts: 8, costTelemetryComplete: false,
    job: { state: "running", occurredAt: "2026-08-12T20:57:34Z" }
  });
});

test("observed rollout throughput uses completion timestamps, not configured capacity", () => {
  const result = (sequenceNumber, second, id) => ({
    ...base, sequenceNumber, occurredAt: `2026-08-12T20:57:${String(second).padStart(2, "0")}Z`,
    type: "optimizer.evaluation_result.received",
    delta: {
      candidate_id: "gepa_seed", stage: "seed_full_train", example_id: `train:${id}`,
      rollout_id: `roll_${id}`, reward: 1, partial: true,
      active_workers: 3, semaphore_size: 3, queued_rollouts: 6 - id,
      child_resource_ref: {
        kind: "container_rollout", id: `roll_${id}`, role: "candidate_evaluation",
        schema: "synth.resource-ref.v1", attributes: {}
      }
    }
  });
  const projected = projectAtCursor(RUN, [result(1, 0, 0), result(2, 10, 1), result(3, 20, 2)]);
  assert.equal(projected.gepa.runtime.rolloutsPerMinute, 6);
  assert.equal(projected.gepa.runtime.activeWorkers, 3);
  assert.equal(projected.gepa.runtime.semaphoreSize, 3);
  assert.equal(projected.gepa.runtime.queuedRollouts, 4);
});

test("live cost sums only when every completed rollout reports cost", () => {
  const ref = (id) => ({ kind: "container_rollout", id, role: "candidate_evaluation", schema: "synth.resource-ref.v1", attributes: {} });
  const event = (sequenceNumber, cost_usd) => ({
    ...base, sequenceNumber, type: "optimizer.evaluation_result.received",
    delta: { candidate_id: "gepa_seed", stage: "seed_full_train", example_id: `train:${sequenceNumber}`, rollout_id: `r${sequenceNumber}`, child_resource_ref: ref(`r${sequenceNumber}`), reward: 1, cost_usd }
  });
  assert.equal(projectAtCursor(RUN, [event(1, 0.01), event(2, 0.02)]).gepa.runtime.reportedCostUsd, 0.03);
  const unknown = projectAtCursor(RUN, [event(1, 0.01), event(2, null)]).gepa.runtime;
  assert.equal(unknown.costTelemetryComplete, false);
  assert.equal(unknown.reportedCostUsd, undefined);
});

test("usage projection preserves null after a later known receipt", () => {
  const usageEvent = (sequenceNumber, cost_usd) => ({
    ...base,
    sequenceNumber,
    type: "runtime.job.completed",
    usageDelta: { cost_usd, prompt_tokens: 10 }
  });
  const projected = projectAtCursor(RUN, [
    usageEvent(1, 0.01),
    usageEvent(2, null),
    usageEvent(3, 0.02)
  ]);
  assert.equal(projected.usage.costUsd, null);
  assert.equal(projected.usage.promptTokens, 30);
});

test("terminal replacement and token-only receipts cannot restore unknown cost", () => {
  const projected = projectAtCursor(RUN, [
    { ...base, sequenceNumber: 1, type: "runtime.job.completed", usageDelta: { cost_usd: null, prompt_tokens: 10 } },
    { ...base, sequenceNumber: 2, type: "optimizer.run.completed", snapshot: { usage: { cost_usd: 0.02, prompt_tokens: 10 } } }
  ]);
  assert.equal(projected.usage.costUsd, null);

  const omitted = projectAtCursor(RUN, [
    { ...base, sequenceNumber: 1, type: "runtime.job.completed", usageDelta: { prompt_tokens: 10, completion_tokens: 2 } }
  ]);
  assert.equal(omitted.usage.costUsd, null);
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

test("a circuit breaker overrides stale running metadata and explains termination", () => {
  const projected = projectAtCursor(
    { ...RUN, status: "running" },
    [
      { ...base, sequenceNumber: 1, type: "optimizer.state.transitioned", delta: { to: "rollout_running", trigger: "rollouts_started", details: { stage: "parent_minibatch_reference" } } },
      {
        ...base,
        sequenceNumber: 2,
        occurredAt: "2026-08-13T18:08:41Z",
        type: "rollout.circuit_breaker.tripped",
        delta: {
          message: "Rollout circuit breaker tripped",
          reason: "rolling_failure_rate_exceeded",
          rolling_failure_rate: 0.15625,
          tolerance: 0.15,
          sample_count: 32
        }
      }
    ]
  );
  assert.equal(projected.summary.status, "terminated");
  assert.equal(projected.gepa.activity.terminal, true);
  assert.equal(projected.gepa.activity.label, "Run terminated");
  assert.match(projected.gepa.activity.detail, /15\.63% failure rate exceeded 15\.00% tolerance/);
  assert.deepEqual(projected.gepa.runtime.job, {
    state: "terminated",
    eventType: "rollout.circuit_breaker.tripped",
    reason: "rolling_failure_rate_exceeded",
    message: "Rollout circuit breaker tripped",
    occurredAt: "2026-08-13T18:08:41Z",
    rollingFailureRate: 0.15625,
    tolerance: 0.15
  });
  assert.equal(projected.gepa.heldout, undefined);
});

test("terminal projection converts unfinished candidates to honest not-evaluated state", () => {
  const projected = projectAtCursor(RUN, [
    { ...base, sequenceNumber: 1, type: "candidate.registered", delta: { candidate_id: "queued", generation: 1 } },
    { ...base, sequenceNumber: 2, type: "optimizer.candidate_evaluation.allocated", delta: { candidate_id: "active", stage: "candidate_full_train" } },
    { ...base, sequenceNumber: 3, type: "rollout.circuit_breaker.tripped", delta: { reason: "rolling_failure_rate_exceeded", message: "stopped" } }
  ]);
  assert.deepEqual(projected.gepa.candidates.map((candidate) => candidate.status), ["aborted", "aborted"]);
  assert.ok(projected.gepa.candidates.every((candidate) => candidate.abortedReason === "rolling_failure_rate_exceeded"));
});

test("authoritative full-train rejection compares against the decision-time incumbent", () => {
  const projected = projectAtCursor(RUN, [
    { ...base, sequenceNumber: 1, type: "candidate.registered", delta: { candidate_id: "seed", source: "seed" } },
    { ...base, sequenceNumber: 2, type: "candidate.evaluated", delta: { candidate_id: "seed", train_reward: .41 } },
    { ...base, sequenceNumber: 3, type: "candidate.registered", delta: { candidate_id: "winner", parent_id: "seed", generation: 0 } },
    { ...base, sequenceNumber: 4, type: "candidate.accepted", delta: { candidate_id: "winner", candidate_train_reward: .58, score: { evaluation_stage: "candidate_full_train", challenger_selection_score: .58, incumbent_selection_score: .41, selection_delta: .17, selection_objective: "physician_score", comparison: { result: "challenger_dominates", incumbent_candidate_id: "seed", rationale: "strict improvement" } } } },
    { ...base, sequenceNumber: 5, type: "candidate.registered", delta: { candidate_id: "sibling", parent_id: "seed", generation: 0 } },
    { ...base, sequenceNumber: 6, type: "candidate.rejected", delta: { candidate_id: "sibling", candidate_train_reward: .49, parent_minibatch_reward: .30, score: { evaluation_stage: "candidate_full_train", challenger_selection_score: .49, incumbent_selection_score: .58, selection_delta: -.09, selection_objective: "physician_score", comparison: { result: "incumbent_dominates", incumbent_candidate_id: "winner", rationale: "winner remains stronger" } } } }
  ]);
  const sibling = projected.gepa.candidates.find((candidate) => candidate.id === "sibling");
  assert.equal(sibling.status, "rejected_full_train");
  assert.equal(sibling.decision.incumbentId, "winner");
  assert.equal(sibling.decision.parentScore, .58);
  assert.equal(sibling.decision.selectionDelta, -.09);
  assert.equal(projected.gepa.incumbentId, "winner");
  assert.deepEqual(orderedScoredCandidates(projected.gepa).map((point) => point.id), ["seed", "winner", "sibling"]);
  assert.deepEqual(incumbentCandidateIds(projected.gepa), ["seed", "winner"]);
});

test("contract, frontier coverage history, and limit forecasts preserve durable semantics", () => {
  const projected = projectAtCursor(RUN, [
    { ...base, sequenceNumber: 1, type: "container.task_info.loaded", delta: { task_id: "healthbench", task_name: "HealthBench", objective: "safe care", output_kind: "text" } },
    { ...base, sequenceNumber: 2, type: "container.program.loaded", delta: { program_id: "health_assistant", mutable_fields: ["system_prompt"] } },
    { ...base, sequenceNumber: 3, type: "objective_set.declared", delta: { objective_set_id: "hb", frontier_type: "per_example", selection_objective: "physician_score", objectives: [{ name: "physician_score", direction: "maximize", aggregation: "mean", split_policy: "train" }] } },
    { ...base, sequenceNumber: 4, type: "taskset.tasks.loaded", delta: { minibatch_rows: 20, reflection_rows: 20, pareto_rows: 60, heldout_rows: 50 } },
    { ...base, sequenceNumber: 5, type: "container.contract.verified", delta: { runtime_family: "normalized", target_id: "healthbench", reward_authority: "container", policy_refs: [{ harness: "chat_completions", config: "groq-8b" }], scale_leases: 30, retention: "durable" } },
    { ...base, sequenceNumber: 6, type: "frontier.updated", delta: { best_candidate_id: "seed", best_train_reward: .41, best_candidate_example_count: 53, covered_train_example_count: 59, train_example_count: 60, coverage_semantics: "solved_reward_positive", frontier_size: 2, members: [], added_candidate_ids: ["seed"], removed_candidate_ids: [] } },
    { ...base, sequenceNumber: 7, type: "optimizer.limit.estimate_updated", delta: { limits: [{ kind: "total_rollouts", max: 710, spent: 332, reserved: 30, remaining: 348, utilization: .467, hard: true, source: "recipe", forecast: { confidence: "medium", model: "linear", seconds_to_limit: 1120, sample_count: 8 } }], nearest: { kind: "total_rollouts", max: 710, spent: 332, remaining: 348 } } }
  ]);
  assert.equal(projected.gepa.contract.objectiveSet.frontierType, "per_example");
  assert.equal(projected.gepa.contract.container.scaleLeases, 30);
  assert.equal(projected.gepa.frontierHistory[0].optimisticSolved, 59);
  assert.equal(projected.gepa.frontierHistory[0].bestCandidateSolved, 53);
  assert.equal(projected.gepa.limits[0].reserved, 30);
  assert.equal(projected.gepa.limits[0].forecast.secondsToLimit, 1120);
  assert.equal(projected.gepa.nearestLimit.kind, "total_rollouts");
});

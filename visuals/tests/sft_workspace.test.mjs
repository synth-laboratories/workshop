import assert from "node:assert/strict";
import test from "node:test";
import { projectAtCursor } from "../families/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import { projectRunViewV2 } from "../families/optimizers/_shared/optimizer.run.v1/components/projectRunViewV2.ts";
import {
  sftAggregateBaseline,
  sftComparison,
  sftCurationFunnel,
  sftDistinctEvaluations,
  sftDistribution,
  sftHeldoutSummary,
  sftEffectiveStatus,
  sftStages
} from "../families/optimizers/_shared/optimizer.run.v1/overlays/sft/model.ts";

const RUN = { id: "sft_hosted_workspace", algorithmId: "sft", status: "running", source: "hosted" };
const base = { occurredAt: "2026-08-12T19:40:00Z", optimizerRunId: RUN.id, algorithmId: "sft" };

function hostedSftEvents() {
  let seq = 0;
  const next = () => ++seq;
  return [
    { ...base, sequenceNumber: next(), type: "sft.dataset.validated", snapshot: { splits: { train: { count: 30_000, digest: "sha256:abc" }, val: { count: 2_000, digest: "sha256:def" } } } },
    { ...base, sequenceNumber: next(), type: "sft.training.metrics", delta: { step: 10, epoch: 1, train_loss: 1.4, validation_loss: null, learning_rate: 0.0002 } },
    { ...base, sequenceNumber: next(), type: "sft.training.metrics", delta: { step: 20, epoch: 1, train_loss: 1.1, validation_loss: 1.2, learning_rate: 0.00018 } },
    { ...base, sequenceNumber: next(), type: "sft.checkpoint.created", item: { kind: "checkpoint", id: "ckpt_10k", status: "created", raw: { step: 10_000 } } },
    { ...base, sequenceNumber: next(), type: "sft.checkpoint.ready", item: { kind: "checkpoint", id: "ckpt_10k", status: "ready", raw: { step: 10_000 } } },
    {
      ...base, sequenceNumber: next(), type: "sft.checkpoint_evaluation.allocated",
      item: { kind: "evaluation", id: "eval_ckpt_10k_selection", status: "allocated" },
      delta: { evaluation_id: "eval_ckpt_10k_selection", checkpoint_id: "ckpt_10k", split_role: "selection" },
      artifactRefs: [
        { schema: "synth.resource-ref.v1", kind: "container_rollout", id: "rollout_ckpt10k_seed0", role: "candidate_evaluation", attributes: { stream_id: "stream:rollout_ckpt10k_seed0", reward: null } },
        { schema: "synth.resource-ref.v1", kind: "container_rollout", id: "rollout_ckpt10k_seed1", role: "candidate_evaluation", attributes: { stream_id: "stream:rollout_ckpt10k_seed1", reward: null } }
      ]
    },
    {
      ...base, sequenceNumber: next(), type: "sft.checkpoint_rollout.completed",
      item: { kind: "rollout", id: "rollout_ckpt10k_seed1", status: "completed" },
      delta: { evaluation_id: "eval_ckpt_10k_selection", rollout_id: "rollout_ckpt10k_seed1", reward: 1.0 },
      usageDelta: { rollouts: 1, cost_usd: 0.004 }
    }
  ];
}

test("SFT V2 projection marks durable checkpoints ready and selected", () => {
  const projected = projectRunViewV2(
    { ...RUN, status: "completed" },
    {
      algorithm: "sft",
      header: {
        runId: RUN.id,
        algorithm: "sft",
        lifecycle: "terminal",
        condition: "healthy",
        placement: "hosted",
        specId: "spec-1",
        specDigest: "sha256:spec",
        executionBindings: [],
        inputRefs: [],
        outputRefs: [],
        visualRefs: [],
        usage: { steps: 30 },
        evidence: { completeness: "complete", refs: [] },
        terminal: { kind: "completed", finalSequence: 44, sealedAt: "2026-09-02T17:58:06Z" },
        projectionSchemaVersion: "sft_projection.v1",
        asOfSequence: 44,
        projectionRevision: 44
      },
      projection: {
        checkpoints: ["ckpt_10", "ckpt_20"],
        selectedCheckpointId: "ckpt_10"
      }
    }
  );
  assert.equal(projected.sft.checkpoints.filter((row) => row.ready).length, 2);
  assert.equal(projected.sft.checkpoints.find((row) => row.id === "ckpt_10").selected, true);
  assert.equal(projected.sft.lineage.selectedCheckpointId, "ckpt_10");
});

test("SFT V2 projection keeps bounded evaluation summaries in first paint", () => {
  const projected = projectRunViewV2(
    { ...RUN, status: "completed" },
    {
      algorithm: "sft",
      header: {
        runId: RUN.id,
        algorithm: "sft",
        lifecycle: "terminal",
        condition: "healthy",
        placement: "hosted",
        specId: "spec-1",
        specDigest: "sha256:spec",
        executionBindings: [], inputRefs: [], outputRefs: [], visualRefs: [],
        usage: { steps: 30 },
        evidence: { completeness: "complete", refs: [] },
        terminal: { kind: "completed", finalSequence: 44, sealedAt: "2026-09-02T17:58:06Z" },
        projectionSchemaVersion: "sft_projection.v1",
        asOfSequence: 44,
        projectionRevision: 44
      },
      projection: {
        checkpoints: ["ckpt_10"],
        selectedCheckpointId: "ckpt_10",
        evaluations: [
          { id: "ckpt_10", phase: "checkpoint", checkpointId: "ckpt_10", step: 10, metric: "calibration_accuracy", score: 0, sampleCount: 1 },
          { id: "heldout:40", phase: "heldout", metric: "accuracy", score: 0, sampleCount: 1 }
        ]
      }
    }
  );
  assert.equal(projected.sft.evaluations.length, 2);
  assert.equal(projected.sft.evaluations[0].role, "checkpoint");
  assert.equal(projected.sft.evaluations[0].checkpoint_id, "ckpt_10");
  assert.equal(projected.sft.evaluations[1].role, "heldout");
  const stages = sftStages(projected.sft, "completed", undefined);
  assert.equal(stages.find((stage) => stage.id === "evaluation").status, "completed");
});

test("classification heldout summaries surface paired uplift without rollout arms", () => {
  const projected = projectRunViewV2(
    { ...RUN, status: "completed" },
    {
      algorithm: "sft",
      header: {
        runId: RUN.id, algorithm: "sft", lifecycle: "terminal", condition: "healthy",
        placement: "hosted", specId: "spec-1", specDigest: "sha256:spec",
        executionBindings: [], inputRefs: [], outputRefs: [], visualRefs: [],
        usage: { steps: 100 }, evidence: { completeness: "complete", refs: [] },
        terminal: { kind: "completed", finalSequence: 3000, sealedAt: "2026-09-02T20:00:00Z" },
        projectionSchemaVersion: "sft_projection.v1", asOfSequence: 3000, projectionRevision: 3000
      },
      projection: {
        evaluations: [{
          id: "heldout:ckpt_25", phase: "heldout", checkpointId: "ckpt_25",
          score: 0.52, delta: 0.16, ciLow: 0.11, ciHigh: 0.21,
          pairedN: 400, verdict: "improvement_demonstrated", claimReady: true
        }]
      }
    }
  );
  const summary = sftHeldoutSummary(projected.sft);
  assert.equal(summary.paired, 400);
  assert.equal(summary.baseScore, 0.36);
  assert.equal(summary.trainedScore, 0.52);
  assert.deepEqual(summary.upliftCi, [0.11, 0.21]);
  assert.equal(summary.claimReady, true);
  assert.equal(sftStages(projected.sft, "completed").find((stage) => stage.id === "heldout").status, "completed");
});

test("SFT stages: ready checkpoint is never presented as promoted", () => {
  const projected = projectAtCursor(RUN, hostedSftEvents());
  const stages = sftStages(projected.sft, "running", undefined);
  const byId = Object.fromEntries(stages.map((stage) => [stage.id, stage]));
  assert.equal(byId.dataset.status, "completed");
  assert.equal(byId.training.status, "active");
  assert.equal(byId.checkpoints.status, "active");
  assert.equal(byId.checkpoints.detail, "1/1 ready");
  assert.equal(byId.evaluation.status, "active");
  assert.equal(byId.promotion.status, "pending", "ready must not imply promoted");
  const checkpoint = projected.sft.checkpoints.find((row) => row.id === "ckpt_10k");
  assert.equal(checkpoint.ready, true);
  assert.equal(checkpoint.promoted, false);
});

test("SFT stages: explicit promotion event completes the promotion stage", () => {
  const events = [
    ...hostedSftEvents(),
    { ...base, sequenceNumber: 100, type: "sft.checkpoint.promoted", item: { kind: "checkpoint", id: "ckpt_10k", status: "promoted", raw: {} }, delta: { checkpoint_id: "ckpt_10k", uplift_claimed: true, improvement_verdict: "improvement_demonstrated" } },
    { ...base, sequenceNumber: 101, type: "optimizer.state.transitioned", delta: { to: "completed" } }
  ];
  const projected = projectAtCursor(RUN, events);
  const stages = sftStages(
    projected.sft,
    "completed",
    typeof projected.summary.summary?.promotedCheckpointId === "string"
      ? projected.summary.summary.promotedCheckpointId
      : undefined
  );
  const byId = Object.fromEntries(stages.map((stage) => [stage.id, stage]));
  assert.equal(byId.promotion.status, "completed");
});

test("SFT stages: zero-evidence promote event selects but does not claim uplift", () => {
  const events = [
    ...hostedSftEvents(),
    {
      ...base,
      sequenceNumber: 100,
      type: "sft.checkpoint.promoted",
      item: { kind: "checkpoint", id: "ckpt_10k", status: "selected", raw: {} },
      delta: {
        checkpoint_id: "ckpt_10k",
        uplift_claimed: false,
        improvement_verdict: "no_measured_improvement",
        rule: "retain_latest_checkpoint"
      }
    }
  ];
  const projected = projectAtCursor(RUN, events);
  const checkpoint = projected.sft.checkpoints.find((row) => row.id === "ckpt_10k");
  assert.equal(checkpoint.selected, true);
  assert.equal(checkpoint.promoted, false);
  assert.equal(projected.summary.summary?.promotedCheckpointId, undefined);
  const stages = sftStages(projected.sft, "completed", undefined);
  const byId = Object.fromEntries(stages.map((stage) => [stage.id, stage]));
  assert.equal(byId.promotion.status, "completed");
  assert.match(byId.promotion.detail, /no measured improvement/);
});

test("queued SFT run stays honestly queued with no fabricated progress", () => {
  const projected = projectAtCursor(
    { ...RUN, status: "queued" },
    [{ ...base, sequenceNumber: 1, type: "optimizer.state.transitioned", delta: { to: "queued", message: "Waiting for a single accelerator" } }]
  );
  assert.equal(projected.summary.status, "queued");
  const stages = sftStages(projected.sft, "queued", undefined);
  assert.ok(stages.every((stage) => stage.status === "pending"), "no stage may claim progress while queued");
});

test("streamed aggregate baseline overrides stale queued presentation without inventing rows", () => {
  const projected = projectAtCursor(
    { ...RUN, status: "queued" },
    [{
      ...base,
      sequenceNumber: 1,
      type: "training.evaluation.completed",
      delta: {
        role: "selection",
        candidate: "base",
        checkpoint_id: "inference-0-reference",
        step: 0,
        metric: "accuracy",
        score: 0.795,
        n: 400
      }
    }]
  );
  assert.deepEqual(sftAggregateBaseline(projected.sft), {
    checkpointId: "inference-0-reference",
    metric: "accuracy",
    score: 0.795,
    n: 400
  });
  assert.equal(sftEffectiveStatus(projected.sft, "queued"), "running");
  const baseline = sftStages(projected.sft, "running").find((stage) => stage.id === "baseline");
  assert.equal(baseline.status, "completed");
  assert.match(baseline.detail, /400 selection examples/);
});

test("duplicate evaluation aliases collapse to one visible summary", () => {
  const projected = projectAtCursor(RUN, [
    { ...base, sequenceNumber: 1, type: "training.evaluation.completed", delta: { role: "selection", checkpoint_id: "inference-0", step: 0, metric: "accuracy", score: 0.795, n: 400 } },
    { ...base, sequenceNumber: 2, type: "sft.checkpoint_evaluation.completed", delta: { role: "selection", checkpoint_id: "inference-0", step: 0, metric: "accuracy", score: 0.795, n: 400 } }
  ]);
  assert.equal(projected.sft.evaluations.length, 2);
  assert.equal(sftDistinctEvaluations(projected.sft).length, 1);
});

/* ── Paired heldout comparison ─────────────────────────────────────────── */

function heldoutEvent(sequenceNumber, payload) {
  return { ...base, sequenceNumber, type: "sft.heldout_evaluation.completed", snapshot: payload };
}

const PAIRED = {
  split_digest: "sha256:heldoutcafebabe",
  base: {
    label: "Base student",
    details: [
      { seed: 1, reward: 1.0, steps: 40, achievements: ["collect_wood"] },
      { seed: 2, reward: 0.0, steps: 80, achievements: [] },
      { seed: 3, reward: 2.0, steps: 55, achievements: ["collect_wood", "place_table"] },
      { seed: 4, reward: null, steps: 12 }
    ]
  },
  trained: {
    label: "Promoted ckpt_10k",
    details: [
      { seed: 1, reward: 3.0, steps: 52, achievements: ["collect_wood", "place_table"] },
      { seed: 2, reward: 1.0, steps: 61, achievements: ["collect_wood"] },
      { seed: 3, reward: 2.0, steps: 58, achievements: ["collect_wood", "place_table", "make_pickaxe"] },
      { seed: 4, reward: 1.0, steps: 44 }
    ]
  }
};

test("paired comparison: statistics use only seeds both arms scored", () => {
  const projected = projectAtCursor(RUN, [...hostedSftEvents(), heldoutEvent(200, PAIRED)]);
  const comparison = sftComparison(projected.sft);
  assert.equal(comparison.paired, 3, "seed 4 has no base reward and must not be paired");
  assert.equal(comparison.unpaired, 1);
  // base over paired seeds = (1 + 0 + 2)/3 = 1.0 ; trained = (3 + 1 + 2)/3 = 2.0
  assert.equal(comparison.baseMean, 1);
  assert.equal(comparison.trainedMean, 2);
  assert.equal(comparison.absoluteUplift, 1);
  assert.equal(comparison.wins, 2);
  assert.equal(comparison.losses, 0);
  assert.equal(comparison.ties, 1);
  assert.equal(comparison.splitDigest, "sha256:heldoutcafebabe");
});

test("paired comparison: a missing reward is never imputed as zero", () => {
  const projected = projectAtCursor(RUN, [...hostedSftEvents(), heldoutEvent(200, PAIRED)]);
  const comparison = sftComparison(projected.sft);
  const seed4 = comparison.rows.find((row) => row.seed === "4");
  assert.equal(seed4.outcome, "unpaired");
  assert.equal(seed4.baseReward, null, "absent reward stays null, not 0");
  assert.equal(seed4.delta, null);
  // If seed 4 were zeroed in, the base mean would drop to 0.75.
  assert.equal(comparison.baseMean, 1);
});

test("paired comparison: achievement coverage delta is computed both ways", () => {
  const projected = projectAtCursor(RUN, [...hostedSftEvents(), heldoutEvent(200, PAIRED)]);
  const comparison = sftComparison(projected.sft);
  assert.deepEqual(comparison.achievementsGained, ["make_pickaxe"]);
  assert.deepEqual(comparison.achievementsLost, []);
});

test("paired comparison: CI needs at least two paired seeds", () => {
  const single = {
    base: { details: [{ seed: 7, reward: 1 }] },
    trained: { details: [{ seed: 7, reward: 2 }] }
  };
  const projected = projectAtCursor(RUN, [...hostedSftEvents(), heldoutEvent(200, single)]);
  const comparison = sftComparison(projected.sft);
  assert.equal(comparison.paired, 1);
  assert.equal(comparison.upliftCi, null);
});

test("no heldout evaluation means no comparison and no uplift claim", () => {
  const projected = projectAtCursor(RUN, hostedSftEvents());
  assert.equal(projected.sft.comparison, undefined);
  assert.equal(sftComparison(projected.sft), null);
  const stages = sftStages(projected.sft, "running", undefined);
  const heldout = stages.find((stage) => stage.id === "heldout");
  assert.equal(heldout.status, "pending", "training progress must not advance the heldout stage");
});

test("legacy sft.heldout_eval.completed alias still projects", () => {
  const projected = projectAtCursor(RUN, [
    ...hostedSftEvents(),
    { ...base, sequenceNumber: 200, type: "sft.heldout_eval.completed", snapshot: PAIRED }
  ]);
  assert.equal(sftComparison(projected.sft).paired, 3);
});

/* ── Baseline and curation ─────────────────────────────────────────────── */

test("baseline distribution reports missing seeds instead of scoring them zero", () => {
  const projected = projectAtCursor(RUN, [
    ...hostedSftEvents(),
    {
      ...base, sequenceNumber: 150, type: "sft.baseline_evaluation.completed",
      snapshot: {
        split_digest: "sha256:baselinesplit",
        seeds: [
          { seed: 1, reward: 2, steps: 30 },
          { seed: 2, reward: 4, steps: 40 },
          { seed: 3, reward: null, status: "timeout" }
        ]
      }
    }
  ]);
  const distribution = sftDistribution(projected.sft.baseline.seeds.map((seed) => seed.reward));
  assert.equal(distribution.n, 3);
  assert.equal(distribution.scored, 2);
  assert.equal(distribution.missing, 1);
  assert.equal(distribution.mean, 3, "mean over scored seeds only");
  assert.equal(projected.sft.baseline.splitDigest, "sha256:baselinesplit");
});

test("curation funnel counts acceptance and rejection reasons", () => {
  const projected = projectAtCursor(RUN, [
    ...hostedSftEvents(),
    { ...base, sequenceNumber: 160, type: "sft.teacher_rollout.completed", delta: { rollout_id: "t1" } },
    { ...base, sequenceNumber: 161, type: "sft.teacher_rollout.completed", delta: { rollout_id: "t2" } },
    { ...base, sequenceNumber: 162, type: "sft.curation.candidate_evaluated", delta: { id: "t1", seed: 11, reward: 3, score: 0.9, decision: "accepted", achievements: ["collect_wood"] } },
    { ...base, sequenceNumber: 163, type: "sft.curation.candidate_evaluated", delta: { id: "t2", seed: 12, reward: 0, score: 0.1, decision: "rejected", reason: "invalid action" } }
  ]);
  const funnel = sftCurationFunnel(projected.sft);
  assert.equal(projected.sft.curation.collected, 2);
  assert.equal(projected.sft.curation.considered, 2);
  assert.equal(projected.sft.curation.accepted, 1);
  assert.equal(funnel.acceptanceRate, 0.5);
  assert.deepEqual(funnel.topRejections, [{ reason: "invalid action", count: 1 }]);
  assert.deepEqual(funnel.achievementsCovered, ["collect_wood"]);
  assert.equal(funnel.accepted.length, 1);
  assert.equal(funnel.rejected.length, 1);
});

test("curation counts stay null when nothing was reported", () => {
  const projected = projectAtCursor(RUN, hostedSftEvents());
  assert.equal(projected.sft.curation.collected, null);
  assert.equal(projected.sft.curation.considered, null, "null means unreported, not zero candidates");
  assert.equal(projected.sft.curation.accepted, null);
});

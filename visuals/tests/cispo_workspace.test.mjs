import assert from "node:assert/strict";
import test from "node:test";
import { algorithmLabel } from "../families/optimizers/_shared/optimizer.run.v1/components/algorithmLabel.ts";
import { projectAtCursor } from "../families/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";

const RUN = {
  id: "cispo_mlx_workspace",
  algorithmId: "cispo",
  status: "running",
  source: "local",
  objective: "CISPO clipped-importance policy optimization",
  summary: { trainingArtifactId: "sft-adapter-7" }
};
const base = { occurredAt: "2026-08-20T20:00:00Z", optimizerRunId: RUN.id, algorithmId: "cispo" };

function cispoEvents() {
  let seq = 0;
  const next = () => ++seq;
  return [
    {
      ...base,
      sequenceNumber: next(),
      type: "cispo.clip.identity",
      delta: { clip: { clip_low: 0.2, clip_high: 4.0 } }
    },
    {
      ...base,
      sequenceNumber: next(),
      type: "training.metrics",
      delta: {
        step: 1,
        group_size: 16,
        reward_variance: 0.12,
        advantage_mean: 0.08,
        advantage_std: 0.31,
        optimizer_step: 1,
        train_loss: 1.4
      }
    },
    {
      ...base,
      sequenceNumber: next(),
      type: "training.checkpoint.created",
      item: { kind: "checkpoint", id: "cispo-ckpt-1", status: "created", raw: { step: 1 } }
    },
    {
      ...base,
      sequenceNumber: next(),
      type: "training.checkpoint.ready",
      item: { kind: "checkpoint", id: "cispo-ckpt-1", status: "ready", raw: { step: 1 } }
    },
    {
      ...base,
      sequenceNumber: next(),
      type: "sft.model.materialized",
      item: {
        kind: "model",
        id: "cispo-ckpt-1",
        raw: { baseModel: "Qwen/Qwen3.5-2B", checkpointId: "cispo-ckpt-1", parentArtifactId: "sft-adapter-7" }
      }
    }
  ];
}

test("algorithm labels keep CISPO distinct from SFT", () => {
  assert.equal(algorithmLabel("cispo"), "CISPO");
  assert.equal(algorithmLabel("sft"), "SFT");
  assert.notEqual(algorithmLabel("cispo"), algorithmLabel("sft"));
});

test("CISPO runs project onto the shared training workspace with CISPO identity", () => {
  const projected = projectAtCursor(RUN, cispoEvents());
  assert.ok(projected.sft, "CISPO must reuse the shared training projection");
  assert.ok(projected.cispo, "CISPO identity slice must be present");
  assert.equal(projected.summary.algorithmId, "cispo");
  assert.equal(projected.cispo.objective, "CISPO clipped-importance policy optimization");
  assert.equal(projected.cispo.clipLow, 0.2);
  assert.equal(projected.cispo.clipHigh, 4.0);
  assert.equal(projected.cispo.groupSize, 16);
  assert.equal(projected.cispo.rewardVariance, 0.12);
  assert.equal(projected.cispo.advantageMean, 0.08);
  assert.equal(projected.cispo.advantageStd, 0.31);
  assert.equal(projected.cispo.optimizerSteps, 1);
  assert.equal(projected.cispo.metricSteps, 1);
  assert.equal(projected.cispo.aggregatesReported, true);
  assert.equal(projected.cispo.warmStartArtifactId, "sft-adapter-7");
  assert.deepEqual(projected.cispo.checkpointIds, ["cispo-ckpt-1"]);
  assert.equal(projected.cispo.noLearningSignal, false);
  assert.equal(projected.sft.checkpoints[0].id, "cispo-ckpt-1");
  assert.equal(projected.sft.checkpoints[0].ready, true);
});

test("SFT runs do not receive a CISPO identity slice", () => {
  const projected = projectAtCursor(
    { id: "sft_hosted_workspace", algorithmId: "sft", status: "running" },
    [{
      occurredAt: "2026-08-20T20:00:00Z",
      optimizerRunId: "sft_hosted_workspace",
      algorithmId: "sft",
      sequenceNumber: 1,
      type: "sft.training.metrics",
      delta: { step: 1, train_loss: 1.1 }
    }]
  );
  assert.ok(projected.sft);
  assert.equal(projected.cispo, undefined);
});

test("uniform CISPO groups surface a truthful no-learning-signal stop", () => {
  const projected = projectAtCursor(RUN, [
    {
      ...base,
      sequenceNumber: 1,
      type: "cispo.no_learning_signal",
      delta: { reason: "uniform_group" }
    }
  ]);
  assert.equal(projected.cispo.noLearningSignal, true);
});

/*
 * The delta the Rust mapping arms actually produce for the payload the MLX
 * wheel emits today. Both arms build it from
 * `training_adapter::TRAINING_METRIC_FIELDS`, and two Rust tests pin that the
 * CISPO aggregates are absent from it rather than zeroed:
 * `training_adapter::a_field_the_runtime_never_reported_stays_absent_not_zero`
 * and `sidecar_training::real_mlx_metric_payload_leaves_the_cispo_aggregates_absent`.
 * Keep this literal in step with them: it is the seam between the two halves.
 */
function mlxStepDelta(step) {
  return { step, epoch: 1, train_loss: 1.4 - step * 0.1, learning_rate: 0.00005, throughput: 64.0 };
}

function mlxCispoEvents() {
  let seq = 0;
  const next = () => ++seq;
  return [
    { ...base, sequenceNumber: next(), type: "cispo.clip.identity", delta: { clip: { eps_low: 1.0, eps_high: 4.0 } } },
    { ...base, sequenceNumber: next(), type: "training.metrics", delta: mlxStepDelta(1) },
    { ...base, sequenceNumber: next(), type: "training.metrics", delta: mlxStepDelta(2) },
    { ...base, sequenceNumber: next(), type: "training.metrics", delta: mlxStepDelta(3) }
  ];
}

test("a field no runtime reports stays null the whole way to the renderer", () => {
  const projected = projectAtCursor(RUN, mlxCispoEvents());
  assert.equal(projected.cispo.groupSize, null);
  assert.equal(projected.cispo.rewardVariance, null);
  assert.equal(projected.cispo.advantageMean, null);
  assert.equal(projected.cispo.advantageStd, null);
  assert.equal(projected.cispo.aggregatesReported, false, "nothing measured them, so nothing may claim them");
  // Clip identity is forwarded wholesale and does reach the panel today.
  assert.equal(projected.cispo.clipLow, 1.0);
  assert.equal(projected.cispo.clipHigh, 4.0);
});

test("an unreported optimizer step is null, never a hardcoded one", () => {
  const projected = projectAtCursor(RUN, mlxCispoEvents());
  assert.equal(projected.cispo.optimizerSteps, null, "no optimizer_step was reported");
  assert.notEqual(projected.cispo.optimizerSteps, 1, "the old fallback pinned every live run at one step");
  // What was observed is a count of step-bearing metric events, and it is a
  // weaker claim than the runtime's own optimizer-step counter.
  assert.equal(projected.cispo.metricSteps, 3);
});

test("an explicit null in the delta is absence, not a measured zero", () => {
  const projected = projectAtCursor(RUN, [
    {
      ...base,
      sequenceNumber: 1,
      type: "training.metrics",
      delta: {
        step: 1,
        train_loss: 1.4,
        group_size: null,
        reward_variance: null,
        advantage_mean: null,
        advantage_std: null,
        optimizer_step: null
      }
    }
  ]);
  assert.equal(projected.cispo.groupSize, null);
  assert.equal(projected.cispo.rewardVariance, null);
  assert.equal(projected.cispo.advantageMean, null);
  assert.equal(projected.cispo.advantageStd, null);
  assert.equal(projected.cispo.optimizerSteps, null);
  assert.equal(projected.cispo.aggregatesReported, false);
});

test("a partially reporting runtime keeps the aggregates and dashes only what it omitted", () => {
  const projected = projectAtCursor(RUN, [
    {
      ...base,
      sequenceNumber: 1,
      type: "training.metrics",
      delta: { step: 1, train_loss: 1.4, group_size: 8, optimizer_step: 4 }
    },
    {
      ...base,
      sequenceNumber: 2,
      type: "training.metrics",
      delta: { step: 2, train_loss: 1.2, group_size: 8, optimizer_step: 9 }
    }
  ]);
  assert.equal(projected.cispo.aggregatesReported, true);
  assert.equal(projected.cispo.groupSize, 8);
  assert.equal(projected.cispo.rewardVariance, null);
  assert.equal(projected.cispo.advantageMean, null);
  assert.equal(projected.cispo.optimizerSteps, 9, "high-water mark of the reported counter");
});

test("a reported zero is a value, not absence", () => {
  const projected = projectAtCursor(RUN, [
    {
      ...base,
      sequenceNumber: 1,
      type: "training.metrics",
      delta: { step: 1, reward_variance: 0, advantage_mean: 0, advantage_std: 0, group_size: 2 }
    }
  ]);
  assert.equal(projected.cispo.rewardVariance, 0);
  assert.equal(projected.cispo.advantageMean, 0);
  assert.equal(projected.cispo.aggregatesReported, true, "a measured zero variance is the no-signal case, reported");
});

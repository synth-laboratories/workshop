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

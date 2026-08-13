import assert from "node:assert/strict";
import test from "node:test";
import { projectAtCursor } from "../templates/optimizer.run.v1/components/projectEvents.ts";
import { sftStages } from "../templates/optimizer.run.v1/overlays/sft/model.ts";

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
    { ...base, sequenceNumber: 100, type: "sft.checkpoint.promoted", item: { kind: "checkpoint", id: "ckpt_10k", status: "promoted", raw: {} } },
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

test("queued SFT run stays honestly queued with no fabricated progress", () => {
  const projected = projectAtCursor(
    { ...RUN, status: "queued" },
    [{ ...base, sequenceNumber: 1, type: "optimizer.state.transitioned", delta: { to: "queued", message: "Waiting for a single accelerator" } }]
  );
  assert.equal(projected.summary.status, "queued");
  const stages = sftStages(projected.sft, "queued", undefined);
  assert.ok(stages.every((stage) => stage.status === "pending"), "no stage may claim progress while queued");
});

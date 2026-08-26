import assert from "node:assert/strict";
import test from "node:test";
import { projectManagedOptimizerPayload } from "../../apps/synth_desktop/src/renderer/src/runtime/runProgress/managedPayload.ts";

const png = (marker) => `data:image/png;base64,${marker}`;
const started = (sequenceNumber, trialId, seed) => ({
  type: "eval.trial.started", sequenceNumber,
  delta: { trial_id: trialId, seed }, raw: {}, item: null,
});
const frame = (sequenceNumber, trialId, marker) => ({
  type: "eval.trial.event", sequenceNumber,
  delta: { trial_id: trialId, containerEvent: {
    event: "rollout.step", frame: { data_url: png(marker), width: 768, height: 768 },
  } },
  raw: { trial_id: trialId, container_event: {
    event: "rollout.step", frame: { data_url: png(marker), sha256: marker },
  } },
});

test("managed optimizer payload retains only the latest native frame per seed", () => {
  const projected = projectManagedOptimizerPayload({
    run: { id: "opt_eval_test" },
    events: [started(1, "trial-a", 91001), frame(2, "trial-a", "old"), frame(3, "trial-a", "latest")],
  });
  assert.equal(projected.mediaBySeed["91001"].frame_data_url, png("latest"));
  assert.equal(projected.mediaBySeed["91001"].sequence_number, 3);
  assert.equal(projected.mediaBySeed["91001"].sha256, "latest");
  assert.equal(JSON.stringify(projected.events).includes("data:image/png"), false);
  assert.equal(projected.events[2].delta.containerEvent.frame.width, 768);
});

test("managed optimizer payload leaves non-optimizer values unchanged", () => {
  const value = { frames: [{ payload: true }] };
  assert.equal(projectManagedOptimizerPayload(value), value);
});

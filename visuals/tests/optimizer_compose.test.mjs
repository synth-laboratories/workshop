import assert from "node:assert/strict";
import test from "node:test";
import { eventMatchesIncludeKinds } from "../runtime/liveStream.ts";
import {
  looksLikeEvalTrace,
  optimizerEventsToLiveEval
} from "../runtime/optimizerCompose.ts";

const gepaAccepted = {
  schema_version: "optimizer_event.v1",
  type: "candidate.accepted",
  sequence_number: 2,
  created_at: "2026-08-26T16:00:02.000Z",
  run_id: "opt_gepa_compose",
  algorithm_id: "gepa",
  delta: { marker: "CUA-OPT-GEPA", candidate_id: "cand_live", train_reward: 0.74 }
};

const sftMetrics = {
  schemaVersion: "optimizer_event.v1",
  type: "sft.training.metrics",
  sequenceNumber: 4,
  occurredAt: "2026-08-26T16:00:04.000Z",
  optimizerRunId: "opt_sft_compose",
  algorithmId: "sft",
  delta: { marker: "CUA-OPT-SFT", step: 20, train_loss: 1.1 }
};

const cispoClip = {
  schema_version: "optimizer_event.v1",
  type: "cispo.clip.identity",
  sequence_number: 3,
  created_at: "2026-08-26T16:00:03.000Z",
  run_id: "opt_cispo_compose",
  algorithm_id: "cispo",
  delta: { marker: "CUA-OPT-CISPO", clip: 0.2 }
};

const evalFinished = {
  ts: "2026-08-26T16:00:02.000Z",
  run_id: "harbor_run",
  kind: "rollout.finished",
  sequence: 2,
  payload: { marker: "PROTOTYPE-REWARD-3.1", reward: 3.1 }
};

test("optimizerEventsToLiveEval maps GEPA, SFT, and CISPO type onto kind", () => {
  const mapped = optimizerEventsToLiveEval([
    {
      schema_version: "optimizer_event.v1",
      type: "optimizer.visual.ready",
      sequence_number: 1,
      created_at: "2026-08-26T16:00:01.000Z",
      run_id: "opt_gepa_compose",
      algorithm_id: "gepa",
      delta: { ready: true }
    },
    gepaAccepted,
    cispoClip,
    sftMetrics
  ]);
  assert.equal(mapped.ok, true);
  assert.deepEqual(
    mapped.events.map((event) => event.kind),
    ["candidate.accepted", "cispo.clip.identity", "sft.training.metrics"]
  );
  assert.equal(mapped.events[0].payload.marker, "CUA-OPT-GEPA");
  assert.equal(mapped.events[1].payload.marker, "CUA-OPT-CISPO");
  assert.equal(mapped.events[2].payload.marker, "CUA-OPT-SFT");
});

test("includeKinds matches optimizer envelope type", () => {
  const mapped = optimizerEventsToLiveEval([gepaAccepted, cispoClip, sftMetrics]);
  assert.equal(mapped.ok, true);
  const visible = mapped.events.filter((event) =>
    eventMatchesIncludeKinds(event, ["cispo.clip.identity", "sft.training.metrics"])
  );
  assert.deepEqual(visible.map((event) => event.kind), [
    "cispo.clip.identity",
    "sft.training.metrics"
  ]);
  assert.equal(
    eventMatchesIncludeKinds({ type: "candidate.accepted" }, ["candidate.accepted"]),
    true
  );
});

test("eval traces on optimizer_run fail closed instead of flattening", () => {
  assert.equal(looksLikeEvalTrace(evalFinished), true);
  const flattened = optimizerEventsToLiveEval([evalFinished]);
  assert.equal(flattened.ok, false);
  assert.match(flattened.error, /does not flatten eval traces/);

  const mixed = optimizerEventsToLiveEval([gepaAccepted, evalFinished]);
  assert.equal(mixed.ok, false);
  assert.match(mixed.error, /does not flatten eval traces/);
});

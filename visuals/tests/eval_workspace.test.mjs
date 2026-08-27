/**
 * Eval workspace projection, driven by the committed example — which is itself
 * generated from the events a real Craftax luna run mirrored through Workshop.
 * If Rust changes what it emits, the example regenerates and these assertions
 * move with it, so the visual can never quietly render a stale contract.
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { projectAtCursor } from "../families/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import {
  evalComparison,
  evalStages,
  metricMean,
  trialCounts
} from "../families/optimizers/_shared/optimizer.run.v1/overlays/eval/model.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const example = JSON.parse(
  readFileSync(
    join(root, "families/optimizers/eval/optimizer.eval.live.v1/examples/events.json"),
    "utf8"
  )
);

function projected(atSeq) {
  return projectAtCursor(example.run, example.events, atSeq);
}

test("eval projection carries the whole matrix, not just a headline", () => {
  const state = projected().eval;
  assert.ok(state, "eval state should project for an eval run");
  assert.equal(state.trials.length, 4);
  assert.equal(state.candidates.length, 2);
  assert.equal(state.scorecards.length, 2);
  assert.equal(state.plannedTrials, 4);
  assert.ok(state.manifestDigest?.startsWith("sha256:"));
  assert.deepEqual(state.seedLedger.screening, [101, 102]);
  assert.deepEqual(state.seedLedger.confirmation, []);
});

test("a candidate is a row: the comparison is never collapsed", () => {
  const state = projected().eval;
  const rows = evalComparison(state);
  assert.equal(rows.length, 2);
  const byLabel = Object.fromEntries(rows.map((row) => [row.label, row]));
  assert.equal(byLabel["luna-low"].isBaseline, true);
  assert.equal(byLabel["luna-med"].isBaseline, false);
  // Real measured means from the run, carried through unchanged.
  assert.equal(byLabel["luna-low"].primary, 0.55);
  assert.ok(Math.abs(byLabel["luna-med"].primary - 0.1) < 1e-9);
  // The baseline has no lift against itself.
  assert.equal(byLabel["luna-low"].lift, null);
});

test("cost reaches the scorecard so a comparison can be read per dollar", () => {
  const rows = evalComparison(projected().eval);
  const byLabel = Object.fromEntries(rows.map((row) => [row.label, row]));
  assert.ok(byLabel["luna-low"].costUsd > 0);
  assert.ok(byLabel["luna-med"].costUsd > byLabel["luna-low"].costUsd);
});

test("report-only recipes skip confirm and never claim a promotion", () => {
  const state = projected().eval;
  const stages = Object.fromEntries(evalStages(state, "completed").map((s) => [s.id, s]));
  assert.equal(stages.plan.status, "completed");
  assert.equal(stages.screen.status, "completed");
  assert.equal(stages.confirm.status, "skipped");
  assert.equal(stages.confirm.detail, "report-only recipe");
  assert.equal(stages.select.status, "completed");
  assert.equal(state.selection.status, "inconclusive");
  assert.equal(state.selection.winnerId, null);
});

test("orchestration completing is not a promotion", () => {
  const view = projected();
  assert.equal(view.summary.status, "completed");
  assert.notEqual(view.eval.selection.status, "promoted");
});

test("a metric with no valid trial is missing, never zero", () => {
  const state = projected().eval;
  // Nothing in this run reports `latency`; asking for it yields null, not 0.
  assert.equal(metricMean(state.scorecards[0], "latency"), null);
  const counts = trialCounts(state);
  assert.equal(counts.valid, 4);
  assert.equal(counts.failed, 0);
  assert.equal(counts.terminal, 4);
});

test("scrubbing back mid-run shows work in flight and no verdict", () => {
  // Sequence 8 lands mid-dispatch: two trials leased a semaphore token, two
  // are still waiting for one.
  const state = projected(8).eval;
  assert.equal(state.selection, null);
  assert.equal(state.scorecards.length, 0, "no candidate is scored mid-stage");
  const counts = trialCounts(state);
  assert.equal(counts.running, 2);
  assert.equal(counts.queued, 2);
  assert.equal(counts.terminal, 0);
  const stages = Object.fromEntries(evalStages(state, "running").map((s) => [s.id, s]));
  assert.equal(stages.screen.status, "active");
  assert.equal(stages.select.status, "pending");
});

test("every trial carries its evidence directory", () => {
  const state = projected().eval;
  for (const trial of state.trials) {
    assert.ok(trial.evidenceDir, `trial ${trial.id} should link its evidence`);
    assert.deepEqual(trial.missingArtifacts, []);
  }
});

test("a budget-exhausted candidate says how much of the episode it actually played", () => {
  // A policy that spends its budget keeps emitting actions — a fallback finishes
  // the episode — so the row must distinguish a score the model earned from one
  // it coasted to. Absent coverage stays absent; it is never read as 0%.
  const capped = JSON.parse(JSON.stringify(example));
  const scored = capped.events.filter((event) => event.item?.kind === "candidate");
  assert.ok(scored.length >= 2, "the example should score both candidates");
  scored[0].item.policyStepFraction = 0.04;
  scored[0].item.budgetExhaustedTrials = 1;

  const rows = evalComparison(projectAtCursor(capped.run, capped.events).eval);
  const cappedRow = rows.find((row) => row.candidateId === scored[0].item.id);
  const untouched = rows.find((row) => row.candidateId !== scored[0].item.id);

  assert.equal(cappedRow.policyStepFraction, 0.04);
  assert.equal(cappedRow.budgetExhaustedTrials, 1);
  assert.equal(untouched.policyStepFraction, null, "no coverage reported is null, not zero");
  assert.equal(untouched.budgetExhaustedTrials, 0);
});

test("real Craftax step events retain engine PNGs and rollout telemetry", () => {
  const live = JSON.parse(JSON.stringify(example));
  const sequenceNumber = Math.max(...live.events.map((event) => event.sequenceNumber)) + 1;
  live.events.push({
    schemaVersion: "optimizer_event.v1",
    optimizerRunId: live.run.id,
    algorithmId: "eval",
    eventId: `${live.run.id}:eval:${sequenceNumber}`,
    sequenceNumber,
    occurredAt: "2026-08-26T19:00:00Z",
    type: "eval.trial.event",
    level: "debug",
    delta: {
      trial_id: "trial_live_deepseek_101",
      containerEvent: {
        event: "rollout.step",
        kind: "environment.step",
        seed: 101,
        ply: 40,
        actions: ["left", "do", "make_wood_pickaxe"],
        policy_reason: "Collect wood and convert progress into a tool.",
        reward_total: 1.25,
        reward_delta: 1,
        achievements: ["collect_wood"],
        resources: { wood: 4, stone: 1 },
        player_pos: [17, 23],
        frame: {
          content_type: "image/png",
          width: 768,
          height: 768,
          sha256: "abc123",
          data_url: "data:image/png;base64,iVBORw0KGgo="
        }
      }
    },
    snapshot: null,
    item: null,
    error: null,
    usageDelta: null,
    artifactRefs: []
  });

  const rollout = projectAtCursor(live.run, live.events).eval.rollouts[0];
  assert.equal(rollout.seed, 101);
  assert.equal(rollout.ply, 40);
  assert.equal(rollout.rewardTotal, 1.25);
  assert.deepEqual(rollout.achievements, ["collect_wood"]);
  assert.deepEqual(rollout.resources, { wood: 4, stone: 1 });
  assert.ok(rollout.frame.dataUrl.startsWith("data:image/png;base64,"));
  assert.deepEqual(rollout.actions, ["left", "do", "make_wood_pickaxe"]);
});

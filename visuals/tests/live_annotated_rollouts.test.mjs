import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  activeFindings,
  countByKind,
  eventDetail,
  isAnnotationEvent,
  labelTally,
  projectLanes,
  unwrapRelayed,
} from "../families/first_class_example_containers/live.annotated_rollouts.v1/project.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function rollout(rolloutId, sequence, kind, payload) {
  return { run_id: "run_1", rollout_id: rolloutId, lane: rolloutId, sequence, kind, ts: `2026-09-01T00:00:${String(sequence).padStart(2, "0")}Z`, payload };
}
function annotation(rolloutId, sequence, kind, payload) {
  return { ...rollout(rolloutId, sequence, kind, payload), stream_id: `stream:${rolloutId}:annotations` };
}

function laneEvents(id) {
  return [
    rollout(id, 1, "trace.opened", { rollout_id: id }),
    rollout(id, 2, "env.episode.opened", { seed: 0, max_steps: 20 }),
    rollout(id, 3, "observation", { step: 0, readout: { achievements: [], inventory: { health: 9, food: 9, drink: 9, energy: 9, wood: 0 } } }),
    annotation(id, 1, "annotation.protocol.bound", { rollout_id: id, protocol_revision_id: "anprev_abc", protocol_id: "craftax.live.v1", model: null }),
    rollout(id, 4, "span.policy.plan", { actions: ["up", "do"], length: 2 }),
    rollout(id, 5, "action", { step: 1, action: "up" }),
    rollout(id, 6, "reward_signal", { step: 1, value: 0 }),
    rollout(id, 7, "observation", { step: 1, readout: { achievements: [], inventory: { health: 9, food: 9, drink: 9, energy: 9, wood: 0 } } }),
    annotation(id, 2, "annotation.finding", { finding_id: "fm:blocked:1", kind: "failure_mode", label: "feedback_incorporation.repeated_blocked_action", status: "provisional", step: 1, confidence: 0.5, evidence: { stream_id: `stream:${id}`, sequences: [5] }, source_sequence: 5, detail: { repeats: 3 } }),
    annotation(id, 3, "annotation.metric", { name: "cumulative_reward", value: 0, step: 1, source_sequence: 6 }),
    rollout(id, 8, "action", { step: 2, action: "do" }),
    rollout(id, 9, "reward_signal", { step: 2, value: 1 }),
    rollout(id, 10, "observation", { step: 2, readout: { achievements: ["collect_wood"], inventory: { health: 8, food: 9, drink: 9, energy: 9, wood: 1 } } }),
    annotation(id, 4, "annotation.finding", { finding_id: "fm:blocked:2", kind: "failure_mode", label: "feedback_incorporation.repeated_blocked_action", status: "provisional", step: 2, confidence: 0.7, supersedes: "fm:blocked:1", evidence: { sequences: [5, 8] }, source_sequence: 8, detail: { repeats: 4 } }),
    annotation(id, 5, "annotation.finding", { finding_id: "ach:collect_wood", kind: "achievement", label: "collect_wood", status: "provisional", step: 2, confidence: 1, evidence: { sequences: [10] }, source_sequence: 10, detail: { basis: "readout" } }),
    annotation(id, 6, "annotation.finding", { finding_id: "ms:resources.collect_first_wood", kind: "milestone", label: "resources.collect_first_wood", status: "provisional", step: 2, confidence: 1, evidence: { sequences: [10] }, source_sequence: 10, detail: { basis: "engine", group: "basic_resources" } }),
    annotation(id, 7, "annotation.finding.retracted", { finding_id: "fm:blocked:2", reason: "progress resumed", source_sequence: 10 }),
    annotation(id, 8, "annotation.metric", { name: "cumulative_reward", value: 1, step: 2, source_sequence: 9 }),
    annotation(id, 9, "annotation.model.requested", { request_id: "judge:1", model: "judge", source_sequence: 10 }),
    annotation(id, 10, "annotation.model.completed", { request_id: "judge:1", model: "judge", usage: { total_tokens: 15 }, source_sequence: 10 }),
    annotation(id, 11, "annotation.finding", { finding_id: "intent:1", kind: "intent", label: "crafting.place_table", status: "provisional", step: 2, confidence: 0.5, evidence: { sequences: [10] }, source_sequence: 10, detail: { basis: "model", call: 1 } }),
    rollout(id, 11, "env.episode.closed", { status: "completed", steps: 2 }),
    rollout(id, 12, "status", { status: "completed", steps: 2 }),
    annotation(id, 12, "annotation.closed", { rollout_id: id, outcome: "completed", findings: 4, retractions: 1 }),
    annotation(id, 13, "capture.high_water", { high_water: 12 }),
    annotation(id, 14, "capture.closed", { high_water: 12 }),
  ];
}

test("template declares one multi-stream input and no terminal requirement", () => {
  const meta = JSON.parse(readFileSync(join(root, "families/first_class_example_containers/live.annotated_rollouts.v1/template.json"), "utf8"));
  assert.equal(meta.id, "live.annotated_rollouts.v1");
  assert.deepEqual(meta.inputs.map((input) => input.name), ["stream"]);
  assert.equal(meta.inputs[0].multiple, true);
  assert.equal(meta.inputs[0].required, true);
  assert.equal(meta.observationContract.readiness.requireTerminal, false);
});

test("rollout and annotation streams fold into one lane with a summary layer over the underlying events", () => {
  const lanes = projectLanes([...laneEvents("roll_a"), ...laneEvents("roll_b").slice(0, 9)]);
  assert.deepEqual(lanes.map((lane) => lane.name), ["roll_a", "roll_b"]);
  const [a, b] = lanes;

  // Underlying rollout facts survive untouched.
  assert.equal(a.status, "finished");
  assert.equal(a.done, 2);
  assert.equal(a.total, 20);
  assert.equal(a.reward, 1);
  assert.deepEqual(a.achievements, ["collect_wood"]);
  assert.equal(a.health, 8);
  assert.equal(a.calls, 1);
  assert.equal(a.rolloutEvents, 12);

  // Annotation layer.
  assert.equal(a.protocol.revisionId, "anprev_abc");
  assert.equal(a.annotationEvents, 14);
  assert.equal(a.annotationClosed, true);
  assert.equal(a.annotationOutcome, "completed");
  const byId = Object.fromEntries(a.findings.map((row) => [row.findingId, row]));
  assert.equal(byId["fm:blocked:1"].status, "superseded");
  assert.equal(byId["fm:blocked:1"].supersededBy, "fm:blocked:2");
  assert.equal(byId["fm:blocked:2"].status, "retracted");
  assert.equal(byId["fm:blocked:2"].retractedReason, "progress resumed");
  assert.equal(byId["ach:collect_wood"].status, "provisional");
  assert.equal(byId["ach:collect_wood"].basis, "readout");
  assert.deepEqual(byId["ach:collect_wood"].sequences, [10]);
  assert.equal(byId["intent:1"].basis, "model");
  assert.deepEqual(activeFindings(a).map((row) => row.findingId), ["ach:collect_wood", "ms:resources.collect_first_wood", "intent:1"]);
  assert.deepEqual(countByKind(activeFindings(a)), { achievement: 1, milestone: 1, intent: 1 });
  assert.deepEqual(a.metrics, { cumulative_reward: 1 });
  assert.deepEqual(a.metricSeries.cumulative_reward.map((row) => row.value), [0, 1]);
  assert.deepEqual(a.model, { requested: 1, completed: 1, failed: 0 });
  assert.deepEqual(a.markers.map((marker) => [marker.findingId, marker.status]), [
    ["fm:blocked:1", "superseded"],
    ["fm:blocked:2", "retracted"],
    ["ach:collect_wood", "provisional"],
    ["ms:resources.collect_first_wood", "provisional"],
    ["intent:1", "provisional"],
  ]);

  // A lane still running shows the open provisional streak.
  assert.equal(b.status, "running");
  assert.equal(b.annotationClosed, false);
  assert.deepEqual(activeFindings(b).map((row) => row.findingId), ["fm:blocked:1"]);

  const tally = labelTally(lanes, "failure_mode");
  assert.deepEqual(tally, [{ label: "feedback_incorporation.repeated_blocked_action", lanes: 1, count: 1 }]);
  assert.deepEqual(labelTally(lanes, "milestone"), [{ label: "resources.collect_first_wood", lanes: 1, count: 1 }]);
});

test("relayed optimizer envelopes unwrap to the same reducer inputs", () => {
  const relayed = {
    run_id: "opt_eval_1",
    kind: "eval.trial.event",
    type: "eval.trial.event",
    sequence: 40,
    payload: {
      delta: {
        trial_id: "trial:craftax:0",
        message: "annotation.finding",
        stream: "annotation",
        container_event: { rollout_id: "roll_z", stream_id: "stream:roll_z:annotations", sequence: 2, kind: "annotation.finding", occurred_at: "2026-09-01T00:00:02Z", payload: { finding_id: "ach:collect_wood", kind: "achievement", label: "collect_wood", status: "provisional", step: 2, evidence: { sequences: [10] } } },
      },
    },
  };
  const event = unwrapRelayed(relayed);
  assert.equal(event.kind, "annotation.finding");
  assert.equal(event.sequence, 2);
  assert.equal(event.lane, "roll_z");
  assert.equal(isAnnotationEvent(event), true);
  const container = {
    run_id: "opt_eval_1",
    kind: "eval.trial.event",
    type: "eval.trial.event",
    sequence: 41,
    payload: { delta: { message: "observation", container_event: { rollout_id: "roll_z", sequence: 3, kind: "observation", payload: { step: 2, readout: { achievements: ["collect_wood"], inventory: { health: 7 } } } } } },
  };
  const lanes = projectLanes([container, relayed]);
  assert.equal(lanes.length, 1);
  assert.equal(lanes[0].name, "roll_z");
  assert.deepEqual(lanes[0].achievements, ["collect_wood"]);
  assert.equal(lanes[0].health, 7);
  assert.equal(activeFindings(lanes[0]).length, 1);
  assert.equal(eventDetail(unwrapRelayed(container)), "observation · step 2");
  assert.equal(eventDetail(event), "achievement · collect_wood");
});

test("a rollout without a bound protocol projects an empty annotation layer, never a fabricated one", () => {
  const lanes = projectLanes(laneEvents("roll_plain").filter((event) => !event.stream_id));
  assert.equal(lanes.length, 1);
  assert.equal(lanes[0].findings.length, 0);
  assert.equal(lanes[0].protocol, undefined);
  assert.equal(lanes[0].annotationClosed, false);
  assert.equal(lanes[0].status, "finished");
});

test("the bundled fixture, produced by the real craftax.live.v1 protocol, projects two annotated lanes", () => {
  const fixture = JSON.parse(readFileSync(join(root, "fixtures/live_annotated_rollouts_craftax.json"), "utf8"));
  const lanes = projectLanes(fixture.events);
  assert.deepEqual(lanes.map((lane) => lane.name).sort(), ["roll_ab9de205861d", "roll_craftax_seed7"]);
  const real = lanes.find((lane) => lane.name === "roll_ab9de205861d");
  const synthetic = lanes.find((lane) => lane.name === "roll_craftax_seed7");

  // Twelve straight moves on open grass: metrics flow, nothing to accuse.
  assert.equal(real.status, "finished");
  assert.equal(real.annotationClosed, true);
  assert.equal(real.findings.length, 0);
  assert.ok(real.metrics.cumulative_reward != null);
  assert.equal(real.metrics.policy_calls, 1);
  assert.equal(real.protocol.protocolId, "craftax.live.v1");

  // The synthetic lane exercises every deterministic signal.
  assert.equal(synthetic.status, "finished");
  assert.deepEqual(synthetic.achievements, ["collect_wood", "place_table"]);
  const ids = synthetic.findings.map((row) => row.findingId);
  assert.deepEqual(ids, [
    "fm:blocked:1",
    "ach:collect_wood",
    "ms:resources.collect_first_wood",
    "ms:resources.accumulate_two_wood",
    "ach:place_table",
    "ms:crafting.place_table",
    "fm:health:2",
    "fm:noop:3",
  ]);
  const byId = Object.fromEntries(synthetic.findings.map((row) => [row.findingId, row]));
  assert.equal(byId["fm:blocked:1"].label, "feedback_incorporation.repeated_blocked_action");
  assert.equal(byId["fm:blocked:1"].detail.reason, "blocked_by_water");
  assert.equal(byId["ach:place_table"].basis, "engine_event");
  assert.equal(byId["ms:crafting.place_table"].detail.basis, "engine");
  assert.deepEqual(byId["ms:crafting.place_table"].detail.prerequisites_verified, ["resources.accumulate_two_wood"]);
  assert.equal(byId["fm:noop:3"].label, "safety_survival.ignored_threat");
  assert.deepEqual(byId["fm:noop:3"].detail.hostiles, ["zombie"]);
  assert.equal(byId["fm:health:2"].label, "safety_survival.low_health");
  assert.ok(synthetic.findings.every((row) => row.status === "provisional"));
  assert.ok(synthetic.markers.every((marker) => marker.step != null));
  assert.equal(synthetic.metrics.achievements_total, 2);
  assert.equal(synthetic.metrics.milestones_total, 3);
  assert.deepEqual(countByKind(activeFindings(synthetic)), { failure_mode: 3, achievement: 2, milestone: 3 });
  assert.deepEqual(labelTally(lanes, "failure_mode").map((row) => row.label), [
    "feedback_incorporation.repeated_blocked_action",
    "safety_survival.ignored_threat",
    "safety_survival.low_health",
  ]);
});


test("consumer controls and protocol rebinds are shown as history, and the lane follows the new revision", () => {
  const events = [
    rollout("roll_c", 1, "trace.opened", { rollout_id: "roll_c" }),
    annotation("roll_c", 1, "annotation.protocol.bound", { rollout_id: "roll_c", protocol_revision_id: "anprev_a", protocol_id: "craftax.live.v1", model: null }),
    annotation("roll_c", 2, "annotation.control.received", { op: "message", control_id: "human-1", applied: true, handled: true, source_sequence: 5 }),
    annotation("roll_c", 3, "annotation.finding", { finding_id: "note:1", kind: "note", label: "operator note", status: "provisional", evidence: { sequences: [] }, source_sequence: 5, detail: { basis: "consumer" }, protocol_revision_id: "anprev_a" }),
    annotation("roll_c", 4, "annotation.control.refused", { control_id: "ctl:2", reason: "annotation_protocol_unknown", source_sequence: 6 }),
    annotation("roll_c", 5, "annotation.control.received", { op: "protocol.update", control_id: "swap", applied: true, protocol_revision_id: "anprev_b", source_sequence: 7 }),
    annotation("roll_c", 6, "annotation.protocol.rebound", { previous_protocol_revision_id: "anprev_a", protocol_revision_id: "anprev_b", protocol_id: "craftax.live.v1", state_carried: true, model: "judge", source_sequence: 7 }),
    annotation("roll_c", 7, "annotation.control.received", { op: "stop", control_id: "stop", applied: true, source_sequence: 9 }),
    annotation("roll_c", 8, "annotation.closed", { rollout_id: "roll_c", outcome: "stopped_by_consumer" }),
  ];
  const [lane] = projectLanes(events);
  assert.equal(lane.rebinds, 1);
  assert.deepEqual(lane.protocol, { revisionId: "anprev_b", protocolId: "craftax.live.v1", model: "judge" });
  assert.deepEqual(lane.controls.map((row) => [row.op, row.accepted, row.reason]), [
    ["message", true, undefined],
    [undefined, false, "annotation_protocol_unknown"],
    ["protocol.update", true, undefined],
    ["stop", true, undefined],
  ]);
  assert.equal(lane.annotationOutcome, "stopped_by_consumer");
  assert.equal(activeFindings(lane)[0].basis, "consumer");
  assert.equal(eventDetail(events[6]), "protocol rebound → anprev_b (state carried)");
  assert.equal(eventDetail(events[4]), "control refused · annotation_protocol_unknown");
});

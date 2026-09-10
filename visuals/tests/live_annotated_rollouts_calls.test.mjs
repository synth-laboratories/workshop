import assert from "node:assert/strict";
import test from "node:test";
import { llmCalls, logicalTimeline, projectLanes } from "../families/first_class_example_containers/live.annotated_rollouts.v1/project.ts";

function rollout(sequence, kind, payload) {
  return { run_id: "run_calls", rollout_id: "roll_calls", lane: "roll_calls", sequence, kind, ts: `2026-09-01T00:00:${String(sequence).padStart(2, "0")}Z`, payload };
}

function annotation(sequence, kind, payload) {
  return { ...rollout(sequence, kind, payload), stream_id: "stream:roll_calls:annotations" };
}

test("annotations attach to policy calls by cited rollout sequence", () => {
  const events = logicalTimeline([
    rollout(1, "span.policy.opened", { call_id: "policy-1", model: "policy-model" }),
    rollout(2, "action", { response: "first" }),
    rollout(3, "span.policy.closed", { call_id: "policy-1", status: "completed" }),
    annotation(1, "annotation.finding", { finding_id: "finding-1", kind: "note", label: "grounded", source_sequence: 2, evidence: { sequences: [2] }, detail: {} }),
    rollout(4, "span.policy.opened", { call_id: "policy-2", model: "policy-model" }),
    rollout(5, "action", { response: "second" }),
    rollout(6, "span.policy.closed", { call_id: "policy-2", status: "completed" }),
    annotation(2, "annotation.finding", { finding_id: "unlinked", kind: "note", label: "not guessed", source_sequence: 99, evidence: { sequences: [99] }, detail: {} }),
  ]).map((row) => row.event);
  const [lane] = projectLanes(events);
  const calls = llmCalls(lane);
  assert.equal(calls.length, 2);
  assert.deepEqual(calls[0].findings.map((finding) => finding.findingId), ["finding-1"]);
  assert.deepEqual(calls[1].findings, []);
});

test("annotator calls attach findings by request id or observed source sequence", () => {
  const events = logicalTimeline([
    rollout(1, "observation", { step: 1 }),
    annotation(1, "annotation.model.requested", { request_id: "judge-1", model: "judge", source_sequence: 1 }),
    annotation(2, "annotation.model.completed", { request_id: "judge-1", source_sequence: 1 }),
    annotation(3, "annotation.finding", { finding_id: "explicit", kind: "note", label: "explicit", source_sequence: 8, evidence: { sequences: [] }, detail: { request_id: "judge-1", basis: "model" } }),
    annotation(4, "annotation.finding", { finding_id: "same-source", kind: "note", label: "same source", source_sequence: 1, evidence: { sequences: [] }, detail: { basis: "model" } }),
  ]).map((row) => row.event);
  const [lane] = projectLanes(events);
  const [call] = llmCalls(lane);
  assert.equal(call.role, "annotator");
  assert.equal(call.status, "completed");
  assert.deepEqual(call.findings.map((finding) => finding.findingId), ["explicit", "same-source"]);
});

test("verifier spans attach rubric findings by cited grade sequence", () => {
  const events = logicalTimeline([
    rollout(1, "span.evaluator.opened", { call_id: "grader-1", model: "grader-model", provider: "openrouter" }),
    rollout(2, "rubric.grade", { rubric_id: "r1", criteria_met: false }),
    rollout(3, "rubric.grade", { rubric_id: "r2", criteria_met: true }),
    rollout(4, "span.evaluator.closed", { call_id: "grader-1", status: "completed" }),
    annotation(1, "annotation.finding", { finding_id: "rubric-1", kind: "failure_mode", label: "rubric_item.unmet", source_sequence: 2, evidence: { sequences: [2] }, detail: { basis: "grader" } }),
    annotation(2, "annotation.finding", { finding_id: "rubric-2", kind: "achievement", label: "rubric_item.met", source_sequence: 3, evidence: { sequences: [3] }, detail: { basis: "grader" } }),
  ]).map((row) => row.event);
  const [lane] = projectLanes(events);
  const [call] = llmCalls(lane);
  assert.equal(call.role, "verifier");
  assert.equal(call.status, "completed");
  assert.equal(call.model, "grader-model");
  assert.deepEqual(call.findings.map((finding) => finding.findingId), ["rubric-1", "rubric-2"]);
});

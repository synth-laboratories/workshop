import assert from "node:assert/strict";
import test from "node:test";
import { laneTraceV5Items } from "../families/first_class_example_containers/live.annotated_rollouts.v1/traceV5.ts";

function row(kind, sequence, payload) {
  return { kind, stream: "rollout", sequence, occurredAt: `2026-09-01T00:00:0${sequence}Z`, detail: kind, payload, verifier: false };
}

test("selected rollout projects openable V5 inputs, CoT summary, reasoning, tools, and output", () => {
  const lane = { name: "rollout-1", trace: [
    row("observation", 1, { readout: { observation_text: "You are beside a tree.", step: 0 } }),
    row("span.policy.opened", 2, { call_number: 1, call: { model: "gpt-test" } }),
    row("span.policy.data", 3, {
      messages: [{ role: "system", content: "Play safely." }, { role: "user", content: "Chop the tree." }],
      reasoning_summary: "Find and click the tree.",
      reasoning: "The tree is visible west of the player.",
      assistant: { content: "Clicking the tree.", tool_calls: [{ function: { name: "click", arguments: { x: 22, y: 18 } } }] },
      tool_results: [{ ok: true }],
    }),
    row("span.policy.closed", 4, { status: "completed" }),
  ] };
  const projection = laneTraceV5Items(lane);
  assert.equal(projection.callCount, 1);
  assert.equal(projection.items.find((item) => item.kind === "input_messages")?.openLabel, "Open input");
  assert.match(projection.items.find((item) => item.kind === "input_messages")?.detail ?? "", /Play safely/);
  assert.match(projection.items.find((item) => item.kind === "cot_summary")?.body ?? "", /Find and click/);
  assert.match(projection.items.find((item) => item.kind === "reasoning")?.detail ?? "", /visible west/);
  assert.equal(projection.items.find((item) => item.kind === "click")?.openLabel, "Open arguments");
  assert.match(projection.items.find((item) => item.kind === "assistant_output")?.body ?? "", /Clicking the tree/);
});

test("missing CoT is shown as not emitted instead of fabricated", () => {
  const lane = { name: "rollout-2", trace: [
    row("span.policy.opened", 1, { call_number: 1 }),
    row("span.policy.data", 2, { assistant: { content: "done" } }),
    row("span.policy.closed", 3, { status: "completed" }),
  ] };
  const projection = laneTraceV5Items(lane);
  assert.equal(projection.items.find((item) => item.kind === "cot_summary")?.status, "not emitted");
  assert.equal(projection.items.find((item) => item.kind === "reasoning")?.status, "not emitted");
});

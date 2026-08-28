import assert from "node:assert/strict";
import test from "node:test";
import { callForSequence, projectAgentTurns, reconcileCallSelection } from "../runtime/agentTranscript.ts";

const event = (kind, sequence, payload = {}) => ({ kind, sequence, run_id: "lane", payload });

test("projects multiple calls with ranges, step links, authority, and honest evidence states", () => {
  const projection = projectAgentTurns([
    event("observation", 1, { readout: { env_steps: 2, observation_text: "forest" } }),
    event("span.policy.opened", 2, { call: { provider: "openai", model: "codex", authority: "agent" } }),
    event("span.policy.data", 3, { channel: "summary", output: "move north", usage: { total_tokens: 8, cost_usd: 0.01 } }),
    event("span.policy.closed", 4), event("span.step.closed", 5, { step: 2 }),
    event("observation", 6, { readout: { env_steps: 3, observation_text: "water" } }),
    event("span.policy.opened", 7, { call: { provider: "openai", model: "codex" } }),
    event("span.policy.data", 8, { channel: "summary", reasoning: "[REDACTED]", tool_calls: [{ name: "act" }], tool_results: [{ ok: true }] }),
    event("span.policy.closed", 9), event("span.step.closed", 10, { step: 3 }), event("eval.run.terminal", 11)
  ]);
  assert.equal(projection.calls.length, 2);
  assert.deepEqual([projection.calls[0].sourceSequenceStart, projection.calls[0].sourceSequenceEnd], [2, 4]);
  assert.equal(projection.calls[0].reasoning.state, "not_emitted");
  assert.equal(projection.calls[1].reasoning.state, "redacted");
  assert.equal(projection.calls[0].toolResults.state, "not_applicable");
  assert.equal(projection.calls[1].toolResults.state, "visible");
  assert.equal(projection.calls[0].outcome, "completed");
  assert.deepEqual(projection.calls[0].closure, {
    outcome: "completed",
    reason: "producer_completed",
    source: "span.policy.closed",
    sourceSequence: 4
  });
  assert.equal(projection.callIdByEnvironmentStep.get(3), projection.calls[1].id);
  assert.equal(callForSequence(projection.calls, 11)?.id, projection.calls[1].id);
});

test("a parent terminal deterministically aborts an unresolved policy call", () => {
  const projection = projectAgentTurns([
    event("span.policy.opened", 1, { call: { provider: "openai", model: "codex" } }),
    event("span.policy.data", 2, { delta: true, channel: "content", text: "partial" }),
    event("eval.run.terminal", 3, { kind: "failed" })
  ]);
  assert.equal(projection.calls.length, 1);
  assert.equal(projection.calls[0].outcome, "aborted");
  assert.deepEqual(projection.calls[0].closure, {
    outcome: "aborted",
    reason: "parent_terminal_before_policy_close",
    source: "eval.run.terminal",
    sourceSequence: 3
  });
  assert.notEqual(projection.calls[0].output.state, "pending");
});

test("producer terminal outcomes stay inside the closed call enum", () => {
  const projection = projectAgentTurns([
    event("span.policy.opened", 1),
    event("span.policy.closed", 2, { outcome: "timed_out" })
  ]);
  assert.equal(projection.calls[0].outcome, "timed_out");
  assert.equal(projection.calls[0].closure.reason, "producer_timed_out");
});

test("focus chooses a policy call and selection survives incremental completion and reload", () => {
  const partial = [event("observation", 1, { step: 0 }), event("span.policy.opened", 2), event("span.policy.data", 3, { delta: true, channel: "content", text: "go" })];
  const first = projectAgentTurns(partial); const selected = reconcileCallSelection(first.calls, null, true);
  assert.equal(first.calls[0].output.state, "visible");
  const complete = projectAgentTurns([...partial, event("span.policy.closed", 4)]);
  assert.equal(reconcileCallSelection(complete.calls, selected, true), selected);
  assert.equal(reconcileCallSelection(complete.calls, "stale-revision-id", true), complete.calls[0].id);
});

test("non-Craftax Trace V5 evidence projects without environment-specific behavior", () => {
  const projection = projectAgentTurns([event("observation", 1, { readout: { observation_text: "SQL schema" } }), event("span.policy.opened", 2, { call: { provider: "anthropic", model: "claude" } }), event("span.policy.data", 3, { channel: "summary", assistant: "SELECT 1" }), event("span.policy.closed", 4)]);
  assert.equal(projection.calls[0].input.value, "SQL schema"); assert.equal(projection.calls[0].output.value, "SELECT 1");
});

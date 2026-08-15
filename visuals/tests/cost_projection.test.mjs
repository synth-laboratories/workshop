import assert from "node:assert/strict";
import test from "node:test";

import { projectAtCursor } from "../families/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";

test("usage projection preserves unknown cost after a later known receipt", () => {
  const run = { id: "cost-fold", algorithmId: "gepa", status: "running" };
  const usageEvent = (sequenceNumber, cost_usd) => ({
    sequenceNumber,
    optimizerRunId: run.id,
    algorithmId: "gepa",
    occurredAt: "2026-08-13T12:00:00Z",
    type: "runtime.job.completed",
    usageDelta: { cost_usd, prompt_tokens: 10 }
  });
  const projected = projectAtCursor(run, [
    usageEvent(1, 0.01),
    usageEvent(2, null),
    usageEvent(3, 0.02)
  ]);

  assert.equal(projected.usage.costUsd, null);
  assert.equal(projected.usage.promptTokens, 30);
});

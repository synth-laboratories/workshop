/**
 * A failed GEPA search must be able to say why.
 *
 * The "Why this search failed" panel tested `run.error`, which only a host that
 * already knows the failure ever populates. A search refused before it proposed
 * anything arrived as `status: "failed"` with no error, so the panel rendered
 * nothing and the reader was left with a failed run over empty panels -- the
 * exact state the panel exists to explain. The reason is in the stream; the
 * projection now recovers it from there.
 */

import assert from "node:assert/strict";
import test from "node:test";
import { projectAtCursor } from "../families/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";

const RUN = {
  id: "gepa_failure_detail",
  algorithmId: "gepa",
  status: "running",
  source: "local",
  objective: "GEPA prompt search"
};
const base = { occurredAt: "2026-09-03T20:00:00Z", optimizerRunId: RUN.id, algorithmId: "gepa" };

test("a failure reason carried on the event reaches the panel without run.error", () => {
  const projected = projectAtCursor(RUN, [
    {
      ...base,
      sequenceNumber: 1,
      type: "optimizer.run.failed",
      delta: { status: "failed" },
      error: { message: "container GET /program failed with HTTP 404" }
    }
  ]);
  assert.equal(projected.summary.status, "failed");
  assert.equal(projected.gepa?.failureDetail, "container GET /program failed with HTTP 404");
});

test("a bare string failure reason is accepted, not dropped", () => {
  const projected = projectAtCursor(RUN, [
    {
      ...base,
      sequenceNumber: 1,
      type: "run.failed",
      delta: { status: "failed", message: "budget exhausted before the first proposal" }
    }
  ]);
  assert.equal(projected.gepa?.failureDetail, "budget exhausted before the first proposal");
});

test("the earliest failure is kept, because later ones are its fallout", () => {
  const projected = projectAtCursor(RUN, [
    {
      ...base,
      sequenceNumber: 1,
      type: "optimizer.run.failed",
      delta: { status: "failed" },
      error: { message: "container GET /program failed with HTTP 404" }
    },
    {
      ...base,
      sequenceNumber: 2,
      type: "optimizer.run.failed",
      delta: { status: "failed" },
      error: { message: "no candidate was ever proposed" }
    }
  ]);
  assert.equal(projected.gepa?.failureDetail, "container GET /program failed with HTTP 404");
});

test("a run that did not fail claims no failure", () => {
  const projected = projectAtCursor(RUN, [
    { ...base, sequenceNumber: 1, type: "optimizer.run.started", delta: { status: "running" } }
  ]);
  assert.equal(projected.gepa?.failureDetail, undefined);
});

test("a failure that says nothing invents no reason", () => {
  const projected = projectAtCursor(RUN, [
    { ...base, sequenceNumber: 1, type: "optimizer.run.failed", delta: { status: "failed" } }
  ]);
  assert.equal(projected.summary.status, "failed");
  assert.equal(projected.gepa?.failureDetail, undefined);
});

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
import { readFileSync } from "node:fs";
import { optimizerFailureDetail, projectAtCursor } from "../families/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";

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

test("the recovered reason is on the projection summary, for every workspace", () => {
  // SFT and CISPO have their own "Why this run failed" panel, and it was just
  // as inert for the same reason: it also tested `run.error`. A rejected
  // Banking77 CISPO run showed "Training failed" above a four-item checklist
  // headed "What is still needed", telling the reader to go collect evidence
  // when the next step was to fix the rejection.
  const projected = projectAtCursor(RUN, [
    {
      ...base,
      sequenceNumber: 1,
      type: "optimizer.run.failed",
      delta: { status: "failed" },
      error: { message: "hosted job rejected: unknown base model" }
    }
  ]);
  assert.equal(projected.summary.failureDetail, "hosted job rejected: unknown base model");
});

test("a run that succeeded carries no failure detail on its summary", () => {
  const projected = projectAtCursor(RUN, [
    { ...base, sequenceNumber: 1, type: "optimizer.run.completed", delta: { status: "completed" } }
  ]);
  assert.equal(projected.summary.failureDetail, undefined);
});

test("the shell's run normalizer carries the failure reason through", () => {
  // `normalizeRun` rebuilds the run field by field, and omitting `error` here
  // darkened every "Why this run failed" panel in the family at once: GEPA's
  // and SFT/CISPO's both test `run.error`, the host delivers it -- it is in the
  // run's stored payload_json -- and it was dropped on the way in. A failed
  // Banking77 CISPO run showed a four-item checklist headed "What is still
  // needed" while its own record held "training job failed: ... Connection
  // refused".
  const shell = readFileSync(
    new URL("../families/optimizers/_shared/optimizer.run.v1/components/FamilyShell.tsx", import.meta.url),
    "utf8"
  );
  const normalizer = shell.slice(shell.indexOf("function normalizeRun"), shell.indexOf("export function OptimizerFamilyShell"));
  assert.match(normalizer, /error: raw\.error \?\? undefined/);
});

test("a traceback reports the exception it ends with, not the noise it starts with", () => {
  // A real GEPA failure. Read from the top, the first line that is not obvious
  // noise is a warning about logging -- printed under "Why this search failed"
  // while the actual exception sat four screens below it.
  const stderrTail = [
    "[telemetry-warning] [synth-optimizers] warning: VictoriaLogs write URL not configured for synth-optimizers; VL event skipped",
    "Traceback (most recent call last):",
    "  File \"<frozen runpy>\", line 203, in _run_module_as_main",
    "  File \".../synth_optimizers/gepa.py\", line 1631, in _http_json",
    "    raise ValueError(f\"container GET {path} failed with HTTP {response.status}\")",
    "    ^^^^^^^^^^^^^^^^^^^",
    "ValueError: container GET /program failed with HTTP 404"
  ].join("\n");
  assert.equal(optimizerFailureDetail({ stderrTail }), "ValueError: container GET /program failed with HTTP 404");
});

test("a telemetry warning is never offered as a cause", () => {
  // Telemetry could not be written. That is never why a run failed.
  assert.equal(
    optimizerFailureDetail({ stderrTail: "[telemetry-warning] VictoriaLogs write URL not configured\nreal cause here" }),
    "real cause here"
  );
});

test("messages that are not tracebacks keep their existing reading", () => {
  assert.equal(optimizerFailureDetail({ message: "hosted job rejected: unknown base model" }), "hosted job rejected: unknown base model");
  assert.equal(optimizerFailureDetail({ message: "container error: boom" }), "boom");
  assert.equal(optimizerFailureDetail(undefined), undefined);
});

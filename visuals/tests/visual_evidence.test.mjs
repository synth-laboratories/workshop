import assert from "node:assert/strict";
import test from "node:test";

import {
  decideVisualEvidence,
  visualEvidenceBlocksCompletion
} from "../runtime/visualEvidence.ts";

test("partial and failed visual evidence never blocks task completion", () => {
  const partial = decideVisualEvidence({
    readyReceipt: false,
    reviewed: false,
    hasVisual: true,
    renderFailed: false,
    decidedAt: "2026-08-17T16:00:00Z"
  });
  const failed = decideVisualEvidence({
    readyReceipt: false,
    reviewed: false,
    hasVisual: false,
    renderFailed: true,
    decidedAt: "2026-08-17T16:00:00Z"
  });
  assert.equal(partial.state, "partial");
  assert.equal(failed.state, "failed");
  assert.equal(visualEvidenceBlocksCompletion(partial), false);
  assert.equal(visualEvidenceBlocksCompletion(failed), false);
});

test("a readiness receipt is ready, reviews without a receipt are reviewed", () => {
  assert.equal(
    decideVisualEvidence({
      readyReceipt: true,
      reviewed: true,
      hasVisual: true,
      renderFailed: false,
      decidedAt: "2026-08-17T16:00:00Z"
    }).state,
    "ready"
  );
  assert.equal(
    decideVisualEvidence({
      readyReceipt: false,
      reviewed: true,
      hasVisual: true,
      renderFailed: false,
      decidedAt: "2026-08-17T16:00:00Z"
    }).state,
    "reviewed"
  );
});

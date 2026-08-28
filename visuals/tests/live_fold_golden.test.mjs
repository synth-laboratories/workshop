/**
 * The TypeScript half of the golden-fixture equivalence suite.
 *
 * `src-tauri/src/stream_fold.rs` is the authoritative fold; `runtime/liveStream.ts`
 * is the mirror hosts without Rust still need in order to draw a pane. Both
 * assert against `fixtures/live_fold_golden.json`, so the two cannot drift
 * without one of them failing — which is what the spool and the ingest did to
 * each other before this file existed.
 *
 * A failure here means one of two things. Either the mirror changed, and the
 * fix is in the mirror; or the fold changed deliberately, in which case
 * regenerate with `node visuals/tests/live_fold_golden_gen.mjs` and review the
 * diff. The diff is the point: a silent projection change corrupts sealed
 * artifacts, which is the premise this whole system rests on.
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { buildGolden, GOLDEN_PATH, GOLDEN_SCHEMA, repoRoot } from "./live_fold_golden_gen.mjs";

const golden = JSON.parse(readFileSync(GOLDEN_PATH, "utf8"));

test("the golden declares the schema its readers know", () => {
  assert.equal(golden.schema, GOLDEN_SCHEMA);
  assert.ok(golden.cases.length >= 8, "the golden lost its fixtures");
});

test("every fixture the golden names is still checked in and still parses", () => {
  for (const entry of golden.cases) {
    if (!entry.source.file) continue;
    const parsed = JSON.parse(readFileSync(join(repoRoot, entry.source.file), "utf8"));
    assert.ok(parsed, `${entry.source.file} is unreadable`);
  }
});

test("the TypeScript mirror reproduces the golden exactly", () => {
  assert.deepEqual(buildGolden(), golden);
});

test("the real multiplexed Craftax capture is in the golden and is not trivial", () => {
  // The 1.2 MB capture is the one fixture with non-numeric string sequences
  // and ten lanes on one stream. It is the case that killed a scalar cutoff
  // and a per-scope numeric cutoff vector, so a golden without it proves much
  // less than it looks like it proves.
  const craftax = golden.cases.find((entry) =>
    entry.source.file?.endsWith("live.craftax.v1/examples/cua-luna-low-10.json"),
  );
  assert.ok(craftax, "the multiplexed Craftax capture left the golden");
  assert.equal(craftax.deliveredCount, 284);
  assert.equal(craftax.evidenceCount, 284, "the capture carries no control envelopes");
  assert.deepEqual(craftax.gaps, [], "opaque string sequences are not a sequence space");
  assert.deepEqual(craftax.conflicts, []);
  assert.equal(
    Object.keys(craftax.lastSequenceByScope).length,
    0,
    "no lane in this capture has a numeric sequence to reach a high-water mark with",
  );
});

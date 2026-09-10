/**
 * Evidence on intent, shared by both optimizer shells.
 *
 * Live costs nothing beyond the projection. Leaving live reads one bounded
 * window of raw events for the scrubber and asks the backend for the state at
 * the cursor. Fixtures without either client keep the injected journal.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { HISTORY_WINDOW, planEvidenceWindow } from "../families/optimizers/_shared/optimizer.run.v1/components/useHistoricalCursor.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const shared = "families/optimizers/_shared/optimizer.run.v1";
const familyShell = readFileSync(join(root, shared, "components/FamilyShell.tsx"), "utf8");
const genericShell = readFileSync(join(root, shared, "shell.tsx"), "utf8");
const hook = readFileSync(join(root, shared, "components/useHistoricalCursor.ts"), "utf8");

test("nothing is fetched while live, and nothing without a client", () => {
  const base = { followLive: false, hasClient: true, tail: 7_177, injectedCount: 0, injectedTail: 0, loadedFrom: null, loading: false };
  assert.equal(planEvidenceWindow({ ...base, followLive: true }), null);
  assert.equal(planEvidenceWindow({ ...base, hasClient: false }), null);
  assert.equal(planEvidenceWindow({ ...base, tail: 0 }), null);
  assert.equal(planEvidenceWindow({ ...base, loading: true }), null);
});

test("leaving live reads one bounded window from the tail, never the whole journal", () => {
  const plan = planEvidenceWindow({ followLive: false, hasClient: true, tail: 7_177, injectedCount: 0, injectedTail: 0, loadedFrom: null, loading: false });
  assert.deepEqual(plan, { from: 7_177 - HISTORY_WINDOW + 1, to: 7_177 });
  assert.ok(plan.to - plan.from + 1 <= HISTORY_WINDOW);
  assert.equal(planEvidenceWindow({ followLive: false, hasClient: true, tail: 7_177, injectedCount: 0, injectedTail: 0, loadedFrom: 6_678, loading: false }), null, "a loaded window is not re-read");
  assert.deepEqual(planEvidenceWindow({ followLive: false, hasClient: true, tail: 120, injectedCount: 0, injectedTail: 0, loadedFrom: null, loading: false }), { from: 1, to: 120 });
});

test("an injected journal that already reaches the tail is used as-is", () => {
  assert.equal(
    planEvidenceWindow({ followLive: false, hasClient: true, tail: 300, injectedCount: 300, injectedTail: 300, loadedFrom: null, loading: false }),
    null
  );
  assert.notEqual(
    planEvidenceWindow({ followLive: false, hasClient: true, tail: 300, injectedCount: 5, injectedTail: 5, loadedFrom: null, loading: false }),
    null,
    "a short injected prefix is not mistaken for the whole history"
  );
});

test("both shells share the one hook and neither reduces the journal for history when the backend can", () => {
  for (const [name, source] of [["FamilyShell", familyShell], ["shell", genericShell]]) {
    assert.match(source, /useHistoricalCursor\(/, `${name} uses the shared hook`);
    assert.equal(source.includes("projectAtCursor("), false, `${name} no longer owns a local reducer call`);
    assert.equal(source.includes("evidence.load({ from: 1, to: tail })"), false, `${name} never loads the whole journal`);
    assert.match(source, /history=\{props\.history\}|history: props\.history/, `${name} passes the backend history client through`);
  }
  assert.match(hook, /history\s*\.projectionAt\(cursorSequence\)/);
  assert.match(hook, /projectAtCursor\(run, timelineEvents, cursorSequence\)/, "fixtures without a backend keep the local reducer");
  assert.match(hook, /export const HISTORY_WINDOW = 500/);
});

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  readReportedFacts,
  summarizeAchievementReportedFacts,
  summarizeNumericReportedFact
} from "../runtime/reportedFacts.ts";

const fact = (value, source = "container_receipt", unavailableReason = null) => ({
  value,
  source,
  unavailableReason
});

const reportedFacts = (overrides = {}) => ({
  calls: fact(2),
  steps: fact(7),
  tokens: fact(90),
  costUsd: fact(0.04),
  achievements: fact(["collect_wood"]),
  frames: fact(8),
  ...overrides
});

test("reads the exact six-fact contract and preserves source/reason independently", () => {
  const read = readReportedFacts({
    reportedFacts: reportedFacts({
      calls: fact(0, "sealed_trace", null),
      frames: fact(null, "container_receipt", "producer_did_not_emit")
    })
  });
  assert.equal(read.status, "present");
  assert.deepEqual(read.facts.calls, { value: 0, source: "sealed_trace", unavailableReason: null });
  assert.deepEqual(read.facts.frames, {
    value: null,
    source: "container_receipt",
    unavailableReason: "producer_did_not_emit"
  });
});

test("authoritative zero is zero while unavailable stays null", () => {
  const zero = { reportedFacts: reportedFacts({ calls: fact(0) }) };
  const unavailable = {
    reportedFacts: reportedFacts({ calls: fact(null, "container_receipt", "not_reported") })
  };
  assert.equal(summarizeNumericReportedFact([zero], "calls", [99]).value, 0);
  const missing = summarizeNumericReportedFact([unavailable], "calls", [99]);
  assert.equal(missing.value, null);
  assert.deepEqual(missing.sources, ["container_receipt"]);
  assert.deepEqual(missing.unavailableReasons, ["not_reported"]);
});

test("reported facts never mix with raw usage or inferred step counts", () => {
  const complete = { reportedFacts: reportedFacts({ steps: fact(4) }) };
  const absent = { raw: { usage: { calls: 500 }, frames: 500 } };
  const summary = summarizeNumericReportedFact([complete, absent], "steps", [400, 500]);
  assert.equal(summary.authoritative, true);
  assert.equal(summary.value, null);
  assert.equal(summary.present, 1);
  assert.match(summary.contractErrors[0], /reported_facts_absent/);
});

test("authoritative empty achievements are distinct from unavailable achievements", () => {
  const empty = summarizeAchievementReportedFacts([
    { reportedFacts: reportedFacts({ achievements: fact([]) }) }
  ], [["raw_must_not_win"]]);
  assert.deepEqual(empty.value, []);
  assert.deepEqual(empty.byRecord, [[]]);
  assert.equal(empty.present, 1);

  const unavailable = summarizeAchievementReportedFacts([
    { reportedFacts: reportedFacts({ achievements: fact(null, "sealed_trace", "not_emitted") }) }
  ], [["raw_must_not_win"]]);
  assert.equal(unavailable.value, null);
  assert.deepEqual(unavailable.byRecord, [null]);
  assert.deepEqual(unavailable.unavailableReasons, ["not_emitted"]);
});

test("partial or embellished contracts fail closed", () => {
  const partial = reportedFacts();
  delete partial.frames;
  assert.equal(readReportedFacts({ reportedFacts: partial }).status, "invalid");

  const embellished = reportedFacts({ calls: { ...fact(1), detail: "not in contract" } });
  const read = readReportedFacts({ reportedFacts: embellished });
  assert.equal(read.status, "invalid");
  assert.match(read.reason, /exactly value, source, and unavailableReason/);
});

test("trace workbench routes every terminal fact through the authoritative reader", () => {
  const source = readFileSync(new URL(
    "../families/first_class_example_containers/_shared/traceWorkbench.tsx",
    import.meta.url
  ), "utf8");
  for (const name of ["calls", "steps", "tokens", "costUsd", "frames"]) {
    assert.match(source, new RegExp(`summarizeNumericReportedFact\\(factRecords, "${name}"`));
  }
  assert.match(source, /summarizeAchievementReportedFacts/);
  assert.match(source, /No achievements achieved\./);
  assert.match(source, /source:/);
  assert.match(source, /unavailable reason:/);
  assert.doesNotMatch(source, /flatMap\(\(step\) => step\.action\.applied/);
});

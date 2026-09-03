/**
 * Presentation contracts for the shared trace workstation.
 *
 * Each assertion corresponds to a defect found by eye in the 2026-09-03 sweep
 * of the Banking77, Craftax, and HealthBench trace workstations. None of them
 * were reported by the machine capture audit: every one is a claim the surface
 * makes in words or numbers rather than a geometry or legibility fault, which
 * is the class the audit cannot see and therefore the class worth locking.
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const visualsRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => readFileSync(join(visualsRoot, relative), "utf8");

const WORKBENCH = "families/first_class_example_containers/_shared/traceWorkbench.tsx";

test("the evidence stat names its units instead of reading as arithmetic", () => {
  const source = read(WORKBENCH);
  // `2 + 2` under the caption `grader + traces` is summed to 4 by the eye
  // before it is read as two counts.
  assert.doesNotMatch(source, /\$\{aggregate\.evaluatorEvidence\} \+ \$\{aggregate\.traceCount\}/);
  assert.match(source, /counted\(aggregate\.evaluatorEvidence, "grade"\)/);
  assert.match(source, /counted\(aggregate\.traceCount, "trace"\)/);
});

test("a counted noun is singular at one", () => {
  const source = read(WORKBENCH);
  // Naming the unit inline is only an improvement if it also reads as English
  // at one: the first cut of the fix shipped `1 grader + 1 traces`.
  assert.match(source, /const counted = \(value: number, noun: string\)[^\n]*value === 1 \? "" : "s"/);
});

test("a zero call count beside real money is reported as unreconciled, not as zero", () => {
  const source = read(WORKBENCH);
  // HealthBench billed $0.030046 against `calls: 0` and the header printed
  // `0 billed calls · $0.030046` -- asserting as measured fact something the
  // cost beside it disproves.
  assert.match(source, /providerCallsReconciled = providerCalls !== null\s*\n\s*&& !\(providerCalls === 0 && \(providerCost \?\? 0\) > 0\)/);
  assert.match(source, /providerCallsReconciled \? `\$\{exactCount\(providerCalls \?\? 0\)\} billed calls` : "calls not reconciled"/);
  // The usage card must fall back with the same predicate, or the summary and
  // the card it opens onto disagree.
  assert.match(source, /\{!providerCallsReconciled\s*\n\s*\? usageCard\("Model calls"/);
});

test("the trajectory rail reserves no height its calls do not need", () => {
  const source = read(WORKBENCH);
  // A 40% floor under a one-call rollout held hundreds of empty pixels before
  // "Observed". Fixing only the stream-only layout left the frame-bearing one
  // worse, not better: the frame makes the column taller, so the same floor
  // reserves more empty rail.
  assert.doesNotMatch(source, /minmax\(140px, 40%\)/);
  assert.match(source, /gridTemplateRows: "var\(--tw-call-rows, fit-content\(40%\) minmax\(0, 1fr\)\)"/);
});

test("the environment-absence banner reads as a sentence", () => {
  const source = read(WORKBENCH);
  // "or an achievement either" trails a clause with no preceding negative to
  // hang "either" on.
  assert.doesNotMatch(source, /achievement either/);
  assert.match(source, /recorded an environment action or an achievement, so the per-call environment section is not shown/);
});

test("a run with no environment is shown no environment scoreboard", () => {
  const source = read(WORKBENCH);
  // A Banking77 classification run carried an "Environment steps" card and an
  // achievements table reading "No achievements achieved." -- a game's
  // scoreboard reporting nothing, over a task with no environment.
  assert.match(source, /\{noEnvironmentReported \? null : usageCard\("Environment steps"/);
  assert.match(source, /\{noEnvironmentReported \? null : <div>\s*\n\s*<div[^\n]*>Achievements · unique seeds/);
});

test("an environment is only ruled out on authoritative facts, never on silence", () => {
  const source = read(WORKBENCH);
  // "We know there were no steps" and "we were told nothing about steps" are
  // different claims; only the first licenses dropping the card. Every clause
  // of the predicate must therefore require an authoritative fact.
  const predicate = source.slice(
    source.indexOf("const noEnvironmentReported"),
    source.indexOf("const formatDuration")
  );
  assert.match(predicate, /stepUsage\.authoritative/);
  assert.match(predicate, /frameUsage\.authoritative/);
  assert.match(predicate, /achievementFacts\.authoritative/);
  // A run with nothing terminal has reported nothing yet either way.
  assert.match(predicate, /terminalCount > 0/);
});

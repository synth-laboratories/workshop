/**
 * Presentation contracts for the experiment overview.
 *
 * This template had never been captured before the 2026-09-03 sweep -- the
 * whole family was outside QA -- and the first look found both of these.
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const visualsRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => readFileSync(join(visualsRoot, relative), "utf8");

const SHELL = "families/experiments/experiment.overview.v1/shell.tsx";

test("a measured quantity keeps its decimals, so it cannot be read as a count", () => {
  const source = read(SHELL);
  // `String(value)` printed a mean reward of 1.0 as `1`, so one screen showed
  // the same number as `1` in the metric cards, `1` in the variants row and
  // `1.000` in the assessment prose, while sibling surfaces write `1.00`.
  assert.doesNotMatch(source, /function display\(/);
  assert.match(source, /function measured\(value: unknown\): string \{[\s\S]*?value\.toFixed\(2\)/);
  for (const site of [
    /measured\(metric\.value\)/,
    /measured\(arm\.score\)/,
    /rollout\.reward == null \? MISSING : measured\(rollout\.reward\)/,
    /reward \$\{measured\(trace\.reward\)\}/
  ]) assert.match(source, site);
});

test("counts are not routed through the measured formatter", () => {
  const source = read(SHELL);
  // OverviewStrip renders Runs and Progress; `Runs 1.00` would be worse than
  // the defect being fixed.
  const strip = source.slice(source.indexOf("function OverviewStrip"), source.indexOf("function Disclosure"));
  assert.match(strip, /\["Runs", arms\.length \|\| MISSING\]/);
  assert.doesNotMatch(strip, /measured\(/);
});

test("a finished run is not given an ETA", () => {
  const source = read(SHELL);
  // `ETA —` reports a not-applicable field as a measurement that went missing.
  assert.match(source, /\.\.\.\(terminal \? \[\] : \[\["ETA", progress\?\.eta\] as const\]\)/);
  assert.match(source, /const terminal = \[[\s\S]*?"completed"[\s\S]*?\]\s*\n?\s*\.includes\(/);
  // Elapsed, usage and cost still mean something about a run that has ended.
  assert.match(source, /\["Elapsed", progress\?\.elapsed\][\s\S]{0,200}\["Cost", progress\?\.cost\]/);
});

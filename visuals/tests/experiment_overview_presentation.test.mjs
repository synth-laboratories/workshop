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
  assert.match(source, /function measured\(value: unknown\): string \{[\s\S]*?Math\.abs\(value\)\.toFixed\(2\)/);
  // Signed, so direction survives: `-0.14` beside a bare `0.06` reads as a
  // signed number next to a magnitude.
  assert.match(source, /value >= 0 \? "\+" : "−"/);
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
  // One list of terminal statuses, shared with the rollout table.
  assert.match(source, /const TERMINAL_EXPERIMENT_STATUSES = \[[^\]]*"completed"[^\]]*\]/);
  assert.match(source, /const terminal = TERMINAL_EXPERIMENT_STATUSES\.includes\(/);
  // Elapsed, usage and cost still mean something about a run that has ended.
  assert.match(source, /\["Elapsed", progress\?\.elapsed\][\s\S]{0,200}\["Cost", progress\?\.cost\]/);
});

test("a terminal run drops the columns nothing filled, and keeps the ones it did", () => {
  const source = read(SHELL);
  // A Banking77 classification run carried Steps, Calls, Tokens and
  // Achievements columns whose every cell was "—": a game's scoreboard over a
  // task with no environment. HealthBench, on the same template, has real
  // token and cost values and must keep those columns.
  assert.match(source, /const droppable = \{ Steps: "steps", Calls: "modelCalls", Tokens: "tokens", Cost: "costUsd", Achievements: "achievements" \}/);
  assert.match(source, /rollouts\.every\(\(row\) => row\[field as keyof Rollout\] == null\)/);
  // Only once the run has ended: a live table that dropped and re-added a
  // column as the first value arrived would shift every cell sideways.
  assert.match(source, /terminal\s*\n?\s*\? Object\.entries\(droppable\)/);
  assert.match(source, /: \[\]\s*\n?\s*\);/);
  // Rollout, State, Reward and Trace are never dropped -- an empty Reward
  // column is a fact about the run, not clutter.
  const columns = source.slice(source.indexOf('const columns = ["Rollout"'), source.indexOf("return <section", source.indexOf('const columns = ["Rollout"')));
  for (const kept of ["Rollout", "State", "Reward", "Trace"]) assert.ok(!columns.includes(`droppable.${kept}`));
});

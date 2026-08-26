import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  assertLiveEvalSlot,
  formatMissingNumber,
  ingestLiveEnvelopes,
  isGuessedStreamUrl
} from "../runtime/liveStream.ts";
import { projectLiveEval } from "../runtime/liveEvalReducer.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function templatePath(id, directory = join(root, "families")) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const path = join(directory, entry.name);
    if (entry.name === id && existsSync(join(path, "template.json"))) return path;
    if (!existsSync(join(path, "template.json"))) {
      const nested = templatePath(id, path);
      if (nested) return nested;
    }
  }
  return undefined;
}

function loadEvents(rel) {
  const parsed = JSON.parse(readFileSync(join(root, rel), "utf8"));
  return parsed.events ?? parsed;
}

test("v0.2 live templates bind slot stream only", () => {
  for (const id of ["live.craftax.v1", "live.harbor_eval.v1"]) {
    const meta = JSON.parse(readFileSync(join(templatePath(id), "template.json"), "utf8"));
    assert.deepEqual((meta.inputs ?? meta.slots).map((slot) => slot.name), ["stream"]);
    assert.equal(assertLiveEvalSlot("stream"), null);
    assert.match(assertLiveEvalSlot("live") ?? "", /Forbidden/);
    assert.match(assertLiveEvalSlot("jobs") ?? "", /Forbidden/);
  }
});

test("v0.2 Craftax fixture: control records are not evidence and missing usage stays missing", () => {
  const events = loadEvents("families/first_class_example_containers/live.craftax.v1/examples/events.json");
  const ingested = ingestLiveEnvelopes(events);
  assert.equal(ingested.ready, true);
  assert.ok(!ingested.events.some((event) => event.kind === "stream.subscribed"));
  const usage = events.find((event) => event.kind === "status")?.payload?.usage;
  assert.equal(formatMissingNumber(usage?.total_tokens), "—");
  assert.equal(formatMissingNumber(0), "0.00");
});

test("v0.2 Harbor missing reward.txt stays missing, never 0", () => {
  const events = [
    { kind: "stream.subscribed", sequence: null, payload: { ready: true } },
    { kind: "trial.planned", sequence: 1, payload: { instruction: "no score" } },
    { kind: "verifier", sequence: 2, payload: { script: "tests/test.sh" } },
    { kind: "status", sequence: 3, payload: { status: "completed" } }
  ];
  const projection = projectLiveEval(events);
  assert.equal(projection.has_reward_txt, false);
  assert.equal(projection.reward, null);
  assert.equal(formatMissingNumber(projection.reward), "—");
});

test("v0.2 two run_ids stay isolated in live projections", () => {
  const mixed = [
    { kind: "observation", sequence: 1, run_id: "roll_a", payload: { text: "ALPHA-ONLY" } },
    { kind: "reward_signal", sequence: 2, run_id: "roll_a", payload: { value: 4 } },
    { kind: "observation", sequence: 1, run_id: "roll_b", payload: { text: "BRAVO-ONLY" } },
    { kind: "reward_signal", sequence: 2, run_id: "roll_b", payload: { value: 1 } }
  ];
  const a = projectLiveEval(mixed.filter((event) => event.run_id === "roll_a"));
  const b = projectLiveEval(mixed.filter((event) => event.run_id === "roll_b"));
  assert.equal(a.reward, 4);
  assert.equal(b.reward, 1);
  assert.ok(JSON.stringify(a.events).includes("ALPHA-ONLY"));
  assert.ok(!JSON.stringify(a.events).includes("BRAVO-ONLY"));
  assert.ok(JSON.stringify(b.events).includes("BRAVO-ONLY"));
  assert.ok(!JSON.stringify(b.events).includes("ALPHA-ONLY"));
});

test("v0.2 live shells read the bindings envelope, not bindings.find", () => {
  for (const rel of [
    "families/first_class_example_containers/live.harbor_eval.v1/shell.tsx",
    "families/first_class_example_containers/live.eval_stream.v1/shell.tsx",
    "families/compatibility/live.intern_acceptance.v1/shell.tsx",
    "families/first_class_example_containers/live.craftax.v1/shell.tsx",
    "families/analysis/compose.visual.v1/shell.tsx",
    "families/analysis/sourced.visual.v1/shell.tsx"
  ]) {
    const source = readFileSync(join(root, rel), "utf8");
    assert.equal(
      source.includes("bindings?.find("),
      false,
      `${rel} must not call find on the bindings envelope`
    );
  }
});

test("v0.2 guessed /events URLs remain refused", () => {
  assert.equal(isGuessedStreamUrl("http://127.0.0.1:8298/events"), true);
  assert.equal(isGuessedStreamUrl("http://127.0.0.1:8298/rollouts/r1/events?after=12"), false);
});

test("live.eval_stream.v1 is a shortcut pane on advertised compose parts", () => {
  const path = templatePath("live.eval_stream.v1");
  const meta = JSON.parse(readFileSync(join(path, "template.json"), "utf8"));
  const example = JSON.parse(readFileSync(join(path, "examples/fixture_binding.json"), "utf8"));
  assert.equal(meta.slots, undefined);
  assert.deepEqual(meta.inputs.map((input) => input.name), ["stream"]);
  assert.equal(meta.inputs[0].required, true);
  assert.ok(!meta.inputs.some((input) => input.name === "optimizer_run"));
  assert.deepEqual(
    meta.components.map((row) => row.id).sort(),
    ["detail_modal.v1", "event_stream.v1", "metrics.v1", "scrubber.v1"]
  );
  assert.ok(Array.isArray(example.inputs));
  assert.equal(example.slots, undefined);
  assert.equal(example.inputs[0].input, "stream");
  assert.equal(example.inputs[0].slot, undefined);
  const shell = readFileSync(join(path, "shell.tsx"), "utf8");
  assert.match(shell, /components\/metrics\.v1\/Metrics\.tsx/);
  assert.match(shell, /components\/scrubber\.v1\/Scrubber\.tsx/);
  assert.match(shell, /components\/event_stream\.v1\/EventStream\.tsx/);
  assert.match(shell, /components\/detail_modal\.v1\/DetailModal\.tsx/);
  assert.equal(shell.includes("MetricStrip"), false);
  assert.equal(shell.includes("optimizerEventsToLiveEval"), false);
  assert.equal(shell.includes("parseComposeSpec"), false);
});

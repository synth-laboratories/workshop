import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  assertLiveEvalSlot,
  formatMissingNumber,
  ingestLiveEnvelopes,
  isGuessedStreamUrl
} from "../runtime/liveStream.ts";
import { projectDigbenchLane, projectLiveEval } from "../runtime/liveEvalReducer.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function loadEvents(rel) {
  const parsed = JSON.parse(readFileSync(join(root, rel), "utf8"));
  return parsed.events ?? parsed;
}

test("v0.2 live templates bind slot stream only", () => {
  for (const id of ["live.craftax.v1", "live.harbor_eval.v1", "live.digbench.v1"]) {
    const meta = JSON.parse(readFileSync(join(root, `templates/${id}/template.json`), "utf8"));
    assert.deepEqual(meta.slots.map((slot) => slot.name), ["stream"]);
    assert.equal(assertLiveEvalSlot("stream"), null);
    assert.match(assertLiveEvalSlot("live") ?? "", /Forbidden/);
    assert.match(assertLiveEvalSlot("jobs") ?? "", /Forbidden/);
  }
});

test("v0.2 Craftax fixture: control records are not evidence and missing usage stays missing", () => {
  const events = loadEvents("templates/live.craftax.v1/examples/events.json");
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

test("v0.2 dig.bench fixture is text-only and incomplete reward is null", () => {
  const events = loadEvents("templates/live.digbench.v1/examples/events.json");
  assert.ok(!events.some((event) => event.kind === "frame"));
  const laneEvents = events.filter((event) => event.run_id === "digbench_p1");
  const projection = projectLiveEval(laneEvents);
  const lane = projectDigbenchLane(laneEvents);
  assert.equal(projection.has_live_frames, false);
  assert.equal(projection.reward, null);
  assert.equal(formatMissingNumber(projection.reward), "—");
  assert.equal(lane.evidence_class, "stub");
  assert.ok(laneEvents.some((event) => event.kind === "observation"));
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
    "templates/live.harbor_eval.v1/shell.tsx",
    "templates/live.digbench.v1/shell.tsx",
    "templates/live.eval_stream.v1/shell.tsx",
    "templates/live.intern_acceptance.v1/shell.tsx",
    "templates/live.craftax.v1/shell.tsx"
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

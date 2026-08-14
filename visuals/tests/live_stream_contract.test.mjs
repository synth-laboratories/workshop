import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  LIVE_EVAL_SLOT,
  assertDeclaredStreamSource,
  assertLiveEvalSlot,
  formatMissingNumber,
  formatMissingUsd,
  ingestLiveEnvelopes,
  ingestLiveEnvelopeBatch,
  isGuessedStreamUrl,
} from "../runtime/liveStream.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

test("live.harbor_eval.v1 binds slot stream, not jobs", () => {
  const meta = JSON.parse(
    readFileSync(join(root, "templates/live.harbor_eval.v1/template.json"), "utf8"),
  );
  assert.deepEqual(
    meta.slots.map((slot) => slot.name),
    [LIVE_EVAL_SLOT],
  );
});

test("forbidden live-eval slots live and jobs fail closed", () => {
  assert.match(assertLiveEvalSlot("live", "live.harbor_eval.v1") ?? "", /Forbidden/);
  assert.match(assertLiveEvalSlot("jobs", "live.container_rollouts.v1") ?? "", /Forbidden/);
  assert.equal(assertLiveEvalSlot("stream", "live.harbor_eval.v1"), null);
  assert.equal(assertLiveEvalSlot("acceptance", "live.intern_acceptance.v1"), null);
});

test("batch ingest preserves lane-local identity, gaps, controls, and duplicate truth", () => {
  const initial = ingestLiveEnvelopeBatch(undefinedState(), [
    { kind: "stream.subscribed", control: true },
    { lane: "a", sequence: 1, kind: "observation", payload: { text: "a1" } },
    { lane: "b", sequence: 1, kind: "observation", payload: { text: "b1" } },
    { lane: "a", sequence: 3, kind: "observation", payload: { text: "a3" } },
  ]);
  assert.equal(initial.ready, true);
  assert.equal(initial.events.length, 3);
  assert.deepEqual(initial.gaps, [{ scope: "a", after: 1, before: 3 }]);

  const healed = ingestLiveEnvelopeBatch(initial, [
    { lane: "a", sequence: 2, kind: "observation", payload: { text: "a2" } },
    { lane: "b", sequence: 1, kind: "observation", payload: { text: "b1" } },
  ]);
  assert.equal(healed.events.length, 4, "exact duplicate is dropped");
  assert.deepEqual(healed.gaps, []);
  assert.equal(healed.lastSequenceByScope.get("a"), 3);
});

function undefinedState() {
  return ingestLiveEnvelopes([]);
}

test("guessed Craftax/Harbor URLs are refused without a declared descriptor", () => {
  assert.equal(isGuessedStreamUrl("http://127.0.0.1:8098/events"), true);
  assert.equal(isGuessedStreamUrl("http://127.0.0.1:8098/rollouts/r1/stream"), true);
  assert.match(
    assertDeclaredStreamSource("http://127.0.0.1:8098/events") ?? "",
    /guessed/,
  );
  assert.equal(
    assertDeclaredStreamSource("http://127.0.0.1:8098/rollouts/r1/stream"),
    null,
  );
  assert.equal(
    assertDeclaredStreamSource("http://127.0.0.1:8098/rollouts/r1/stream", {
      id: "stream_r1",
      transports: { sse: { url: "http://127.0.0.1:8098/rollouts/r1/stream" } },
    }),
    null,
  );
});

test("missing reward and cost stay missing, never 0 or $0.00", () => {
  assert.equal(formatMissingNumber(undefined), "—");
  assert.equal(formatMissingNumber(null), "—");
  assert.equal(formatMissingNumber(0), "0.00");
  assert.equal(formatMissingUsd(undefined), "—");
  assert.equal(formatMissingUsd(0.00134), "$0.0013");
});

test("bind refuses guessed live_sse URLs and jobs slots", async () => {
  const { bindTemplateSlots, propsFromBindings } = await import("../runtime/bind.ts");
  const template = {
    id: "live.harbor_eval.v1",
    slots: [{ name: "stream", accepts: ["live_sse"], required: true }],
  };
  const guessed = await bindTemplateSlots(template, [
    { slot: "stream", kind: "live_sse", source: "http://127.0.0.1:8098/events" },
  ]);
  assert.ok(guessed.errors.some((error) => /guessed/.test(error)));
  const jobs = propsFromBindings({
    schemaVersion: "synth.visual-bindings.v1",
    slots: [{ slot: "jobs", kind: "live_sse", source: "http://127.0.0.1:8098/declared" }],
  });
  assert.ok(jobs.errors.some((error) => /Forbidden/.test(error)));
});

test("normalized live bindings retain the declared poll endpoint for recovery", async () => {
  const { bindTemplateSlots, propsFromBindings } = await import("../runtime/bind.ts");
  const binding = {
    slot: "stream",
    kind: "live_sse",
    source: "http://127.0.0.1:8098/rollouts/r1/stream",
    poll_url: "http://127.0.0.1:8098/rollouts/r1/events",
  };
  const props = propsFromBindings({
    schemaVersion: "synth.visual-bindings.v1",
    slots: [binding],
  });
  assert.equal(props.errors.length, 0);
  assert.equal(props.props.stream.poll_url, binding.poll_url);

  const result = await bindTemplateSlots({
    id: "live.craftax.v1",
    slots: [{ name: "stream", accepts: ["live_sse"], required: true }],
  }, [binding]);
  assert.equal(result.errors.length, 0);
  assert.equal(result.slots.stream.data.poll_url, binding.poll_url);
});

test("live stream recovery also runs after EventSource reaches CLOSED", () => {
  const hook = readFileSync(join(root, "chrome/useLiveEvalStream.ts"), "utf8");
  assert.match(hook, /es\.onerror = \(\) => \{\s*if \(!abort\.signal\.aborted\)/);
  assert.doesNotMatch(hook, /readyState !== EventSource\.CLOSED/);
});

test("ingest de-dupes, ignores heartbeats, and treats stream.subscribed as ready", () => {
  const state = ingestLiveEnvelopes([
    { kind: "stream.subscribed", event_id: "sub", run_id: "run", payload: { ready: true } },
    { kind: "heartbeat", event_id: "hb", run_id: "run", payload: {} },
    { kind: "snapshot", event_id: "e1", sequence: 1, run_id: "run", payload: { reward: 1.5 } },
    { kind: "snapshot", event_id: "e1", sequence: 1, run_id: "run", payload: { reward: 1.5 } },
    { kind: "snapshot", event_id: "e2", sequence: 2, run_id: "run", payload: {} },
  ]);
  assert.equal(state.ready, true);
  assert.equal(state.events.length, 2);
  assert.equal(state.events[0].event_id, "e1");
  assert.equal(state.events[1].event_id, "e2");
});

test("finite fixture replay is ready without a live subscription control", () => {
  const hook = readFileSync(join(root, "chrome/useLiveEvalStream.ts"), "utf8");
  assert.match(
    hook,
    /if \(fixtureEvents\?\.length\) \{\s*\/\/[^]*?setReady\(true\);\s*setLive\(true\);/,
    "local fixtures must finish as ready instead of falling back to connecting"
  );
  assert.match(
    hook,
    /setReady\(fixtureReady \|\| ingest\.current\.ready\)/,
    "publishing fixture envelopes must not erase the local-ready state"
  );
});

test("live Craftax resolves persisted fixture references from packaged template assets", () => {
  const shell = readFileSync(join(root, "templates/live.craftax.v1/shell.tsx"), "utf8");
  assert.match(shell, /import\.meta\.glob\("\.\/examples\/\*\.json"/);
  assert.match(shell, /props\.data \?\? props\.stream \?\? bundledFixtureStream\(bindingList\)/);
  const fixture = JSON.parse(
    readFileSync(join(root, "templates/live.craftax.v1/examples/cua-luna-low-10.json"), "utf8"),
  );
  assert.equal(fixture.events.length, 284);
  assert.equal(fixture.events.filter((event) => event.kind === "snapshot").length, 274);
  assert.equal(fixture.events.filter((event) => event.kind === "eval.run.terminal").length, 10);
});

test("multiplexed rollout-local event ids never collapse across lanes", () => {
  const state = ingestLiveEnvelopes([
    { kind: "observation", event_id: "1", sequence: 1, rollout_id: "seed-0", lane: "seed-0", payload: { step: 0 } },
    { kind: "observation", event_id: "1", sequence: 1, rollout_id: "seed-1", lane: "seed-1", payload: { step: 0 } },
    { kind: "reward_signal", event_id: "2", sequence: 2, rollout_id: "seed-0", lane: "seed-0", payload: { value: 1 } },
    { kind: "reward_signal", event_id: "2", sequence: 2, rollout_id: "seed-1", lane: "seed-1", payload: { value: 0 } },
    // A reconnect replay from seed-0 is still dropped exactly once.
    { kind: "observation", event_id: "1", sequence: 1, rollout_id: "seed-0", lane: "seed-0", payload: { step: 0 } },
  ]);
  assert.equal(state.events.length, 4);
  assert.deepEqual(state.events.map((event) => event.rollout_id), ["seed-0", "seed-1", "seed-0", "seed-1"]);
});

test("A15 exact reconnect duplicates collapse but conflicting duplicates fail closed", () => {
  const exact = ingestLiveEnvelopes([
    { kind: "observation", event_id: "7", sequence: 7, rollout_id: "r1", digest: "same", payload: { step: 1 } },
    { kind: "observation", event_id: "7", sequence: 7, rollout_id: "r1", digest: "same", payload: { step: 1 } },
  ]);
  assert.equal(exact.events.length, 1);
  assert.deepEqual(exact.conflicts, []);

  const conflict = ingestLiveEnvelopes([
    { kind: "observation", event_id: "7", sequence: 7, rollout_id: "r1", digest: "first", payload: { step: 1 } },
    { kind: "observation", event_id: "7", sequence: 7, rollout_id: "r1", digest: "changed", payload: { step: 2 } },
  ]);
  assert.equal(conflict.events.length, 1);
  assert.match(conflict.conflicts[0], /Conflicting duplicate envelope r1:7/);
});

test("A11/A15 sequence gaps remain explicit per rollout and controls do not create gaps", () => {
  const state = ingestLiveEnvelopes([
    { kind: "stream.subscribed", event_id: "sub", rollout_id: "r1", control: true, payload: { ready: true } },
    { kind: "observation", event_id: "1", sequence: 1, rollout_id: "r1", payload: {} },
    { kind: "heartbeat", event_id: "hb", rollout_id: "r1", control: true, payload: {} },
    { kind: "observation", event_id: "4", sequence: 4, rollout_id: "r1", payload: {} },
    { kind: "observation", event_id: "2", sequence: 2, rollout_id: "r2", payload: {} },
  ]);
  assert.deepEqual(state.gaps, [{ scope: "r1", after: 1, before: 4 }]);
  assert.equal(state.lastSequenceByScope.get("r1"), 4);
  assert.equal(state.lastSequenceByScope.get("r2"), 2);
});

test("A11 out-of-order backfill closes a temporary gap without duplicating evidence", () => {
  const state = ingestLiveEnvelopes([
    { kind: "observation", event_id: "1", sequence: 1, rollout_id: "r1", payload: {} },
    { kind: "action", event_id: "4", sequence: 4, rollout_id: "r1", payload: {} },
    { kind: "policy", event_id: "2", sequence: 2, rollout_id: "r1", payload: {} },
    { kind: "reward", event_id: "3", sequence: 3, rollout_id: "r1", payload: {} },
  ]);
  assert.equal(state.events.length, 4);
  assert.deepEqual(state.gaps, []);
  assert.equal(state.lastSequenceByScope.get("r1"), 4);
});

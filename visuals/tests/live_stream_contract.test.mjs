import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  LIVE_EVAL_INPUT,
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
    readFileSync(join(root, "families/first_class_example_containers/live.harbor_eval.v1/template.json"), "utf8"),
  );
  assert.deepEqual(
    (meta.inputs ?? meta.slots).map((slot) => slot.name),
    [LIVE_EVAL_INPUT],
  );
  assert.equal(LIVE_EVAL_SLOT, LIVE_EVAL_INPUT);
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

test("numeric string sequences preserve lane-local gaps and recovery", () => {
  const initial = ingestLiveEnvelopeBatch(undefinedState(), [
    { lane: "a", sequence: "1", kind: "observation" },
    { lane: "b", sequence: "1", kind: "observation" },
    { lane: "a", sequence: "3", kind: "observation" },
  ]);
  assert.deepEqual(initial.gaps, [{ scope: "a", after: 1, before: 3 }]);
  const healed = ingestLiveEnvelopeBatch(initial, [
    { lane: "a", sequence: "2", kind: "observation" },
  ]);
  assert.deepEqual(healed.gaps, []);
  assert.equal(healed.lastSequenceByScope.get("a"), 3);
});

test("replay pages normalize every producer shape and never invent a cursor", async () => {
  const { parseReplayPage } = await import("../runtime/replayClient.ts");
  const paged = parseReplayPage(
    { page: { events: [{ sequence: 4 }] }, cursor: { next: 4, high_water: 9, has_more: true, closed: false } },
    0,
  );
  assert.equal(paged.events.length, 1);
  assert.equal(paged.cursor.next, 4);
  assert.equal(paged.cursor.hasMore, true);
  assert.equal(paged.cursor.closed, false);

  // Top-level events with no cursor: the next cursor is the highest sequence
  // seen, never a guess past it.
  const flat = parseReplayPage({ events: [{ sequence_number: 7 }] }, 3);
  assert.equal(flat.cursor.next, 7);
  assert.equal(flat.cursor.hasMore, false);

  // A bare array has no cursor at all, so it is one closed page. Any other
  // reading would silently drop rows or spin forever.
  const bare = parseReplayPage([{ sequence: 1 }], 0);
  assert.equal(bare.cursor.closed, true);

  // An unreadable page is an error, never an empty page.
  assert.throws(() => parseReplayPage({ rows: [] }, 0), /neither page.events nor events/);
  assert.throws(() => parseReplayPage(null, 0), /not an object/);
});

test("declared live bindings become replay streams, and a missing poll authority is reported", async () => {
  const { replayStreamsFromBindings } = await import("../runtime/replayClient.ts");
  const { streams, missingTransport } = replayStreamsFromBindings([
    { slot: "stream", kind: "live_sse", source: "http://127.0.0.1:8114/rollouts/r1/stream", poll_url: "http://127.0.0.1:8114/rollouts/r1/events" },
    { slot: "stream", kind: "live_sse", source: "http://127.0.0.1:8114/rollouts/r2/stream", poll_url: "http://127.0.0.1:8114/rollouts/r2/events" },
    { slot: "stream", kind: "live_sse", source: "http://127.0.0.1:8114/rollouts/r3/stream" },
    { slot: "notes", kind: "inline", source: undefined },
  ]);
  assert.equal(streams.length, 2);
  assert.equal(streams[0].pollUrl, "http://127.0.0.1:8114/rollouts/r1/events");
  // A stream with no durable authority cannot replay after it closes. It is
  // named, not quietly dropped.
  assert.deepEqual(missingTransport, ["http://127.0.0.1:8114/rollouts/r3/stream"]);
});

test("ten declared rollout streams each keep their own durable cursor", async () => {
  const { createReplayClient } = await import("../runtime/replayClient.ts");
  const asked = [];
  const streams = Array.from({ length: 10 }, (_, index) => ({
    streamId: `roll_${index}`,
    pollUrl: `http://127.0.0.1:8114/rollouts/roll_${index}/events`,
  }));
  const client = createReplayClient(streams, async (pollUrl, after, limit) => {
    asked.push({ pollUrl, after, limit });
    return { page: { events: [{ sequence: after + 1 }] }, cursor: { next: after + 1, closed: true } };
  });
  const pages = await Promise.all(streams.map((stream) => client.poll(stream, 0, 500)));
  assert.equal(client.streams.length, 10);
  assert.equal(asked.length, 10);
  assert.equal(new Set(asked.map((call) => call.pollUrl)).size, 10);
  assert.ok(pages.every((page) => page.cursor.closed));
});

test("stream_id plus sequence is the Harbor live identity", () => {
  const first = ingestLiveEnvelopeBatch(undefinedState(), [
    { stream_id: "stream-a", sequence: 1, kind: "trial.planned" },
    { stream_id: "stream-a", sequence: 1, kind: "trial.planned" },
    { stream_id: "stream-b", sequence: 1, kind: "trial.planned" },
  ]);
  assert.equal(first.events.length, 2);
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

test("multi-source slots are explicit and single-source slots reject duplicates", async () => {
  const { bindTemplateSlots } = await import("../runtime/bind.ts");
  const bindings = [
    { slot: "stream", kind: "live_sse", source: "http://127.0.0.1:8098/rollouts/r1/stream", poll_url: "http://127.0.0.1:8098/rollouts/r1/events" },
    { slot: "stream", kind: "live_sse", source: "http://127.0.0.1:8098/rollouts/r2/stream", poll_url: "http://127.0.0.1:8098/rollouts/r2/events" },
  ];
  const accepted = await bindTemplateSlots({
    id: "live.craftax.v1",
    slots: [{ name: "stream", accepts: ["live_sse"], required: true, multiple: true }],
  }, bindings);
  assert.equal(accepted.errors.length, 0);
  assert.equal(accepted.slots.stream.source, "multiple");
  assert.equal(accepted.slots.stream.data.length, 2);

  const rejected = await bindTemplateSlots({
    id: "live.harbor_eval.v1",
    slots: [{ name: "stream", accepts: ["live_sse"], required: true }],
  }, bindings);
  assert.match(rejected.errors[0], /accepts one binding, received 2/);
});

test("a template cannot rest in an unexplained pending state", async () => {
  const streams = await import("../chrome/useLiveEvalStreams.ts");
  const hook = readFileSync(join(root, "chrome/useLiveEvalStreams.ts"), "utf8");
  assert.equal(typeof streams.useLiveEvalStreams, "function");
  // Declared streams that never answer become an error on a bounded deadline.
  // The failure this replaced had no deadline, so "never asked" and "asked and
  // waiting" were the same rendered state for as long as anyone watched.
  assert.match(hook, /REPLAY_FIRST_RESPONSE_TIMEOUT_MS/);
  assert.match(hook, /streamSubscribeTimeout/);
  // Transport arrives as the host's client. The hook must not reach for
  // bindings or construct URLs itself: a template that discovers its own
  // transport can fail to discover one and say nothing.
  assert.doesNotMatch(hook, /from "\.\.\/runtime\/bind\.ts"/);
  assert.doesNotMatch(hook, /new URL\(/);
  assert.doesNotMatch(hook, /fetch\(/);
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
  // A bundled fixture is complete local evidence. It must not wait on a
  // live-only `stream.subscribed` envelope that will never arrive, and it must
  // finish at `terminal` rather than resting in a pending state that reads as
  // a stalled connection.
  assert.match(hook, /ready: Boolean\(fixtureEvents\?\.length\)/);
  assert.match(hook, /setState\("terminal"\)/);
  // A declared stream always wins over a fixture: local example evidence never
  // stands in for the transport a visual actually declared.
  assert.match(hook, /declared \? live : fixture/);
});

test("live Craftax resolves persisted fixture references from packaged template assets", () => {
  const shell = readFileSync(join(root, "families/first_class_example_containers/live.craftax.v1/shell.tsx"), "utf8");
  assert.match(shell, /import\.meta\.glob\("\.\/examples\/\*\.json"/);
  // The declared `stream` input is authoritative. Anonymous `data` remains a
  // direct-preview compatibility fallback, followed by the packaged fixture.
  assert.match(shell, /props\.stream \?\? props\.data \?\? bundledFixtureStream\(bindingList\)/);
  const fixture = JSON.parse(
    readFileSync(join(root, "families/first_class_example_containers/live.craftax.v1/examples/cua-luna-low-10.json"), "utf8"),
  );
  assert.equal(fixture.events.length, 284);
  assert.equal(fixture.events.filter((event) => event.kind === "snapshot").length, 274);
  assert.equal(fixture.events.filter((event) => event.kind === "eval.run.terminal").length, 10);
});

test("live Craftax renders a subscribed optimizer journal immediately instead of fixture-throttling it", () => {
  const shell = readFileSync(join(root, "families/first_class_example_containers/live.craftax.v1/shell.tsx"), "utf8");
  assert.match(shell, /mergeCraftaxOptimizerJournalEvents\(props\.events, props\.enrichmentEvents\)/);
  assert.match(shell, /const events = optimizerEvents \?\? liveStream\.events/);
  assert.match(shell, /data-visual-event-source=\{optimizerEvents \? "optimizer-journal"/);
  assert.match(shell, /optimizerEvents \|\| declaredStreamCount > 0 \? undefined : stream\.events/);
});

test("live Craftax names durable-journal hydration and exposes a run-wide comparison", () => {
  const shell = readFileSync(join(root, "families/first_class_example_containers/live.craftax.v1/shell.tsx"), "utf8");
  const css = readFileSync(join(root, "families/first_class_example_containers/live.craftax.v1/viewer.css"), "utf8");
  assert.match(shell, /optimizerJournalBound && optimizerEvents === undefined/);
  assert.match(shell, /Loading retained rollout journals/);
  assert.match(shell, /Counts and replay controls will appear only after the journal is available/);
  assert.match(shell, /Overall · all rollouts/);
  assert.match(shell, /Achievement coverage/);
  assert.match(shell, /role="table" aria-label="Reward, environment steps, and model calls by rollout"/);
  assert.match(css, /\.cv-overview-grid\{display:grid/);
  assert.match(css, /@media\(max-width:760px\).*\.cv-overview-grid\{grid-template-columns:1fr 1fr\}/s);
});

test("live Craftax loads retained frame CAS through the host and never guesses a relative rollout URL", () => {
  const shell = readFileSync(join(root, "families/first_class_example_containers/live.craftax.v1/shell.tsx"), "utf8");
  assert.match(shell, /props\.media\.warm\(retainedFrameDigests, selectedIndex\)/);
  assert.match(shell, /\? loadedFrame\?\.dataUrl/, "an absent selected frame must remain safe after production minification");
  assert.match(shell, /loadedFrame\?\.digest === selectedMediaDigest && selectedMediaDigest != null/);
  assert.match(shell, /if \(!frameBaseUrl && !\/\^https\?:\|\^data:\/i\.test\(viewer\.frameUrl\)\) return undefined/);
  assert.doesNotMatch(shell, /frameBaseUrl \?\? window\.location\.href/);
  assert.match(shell, /Loading retained gameplay PNG/);
});

test("live Craftax keeps replay-driven call fallback out of passive state effects", () => {
  const shell = readFileSync(join(root, "families/first_class_example_containers/live.craftax.v1/shell.tsx"), "utf8");
  assert.match(shell, /const selectedCall = turns\.calls\.find/);
  assert.match(shell, /reconcileCallSelection\(turns\.calls, selectedCallId, transcriptMode === "focus"\)/);
  assert.doesNotMatch(shell, /setSelectedCallId\(\(current\) => reconcileCallSelection/);
});

test("live Craftax declares optimizer lifecycle authority and makes failure senior to transport", () => {
  const template = JSON.parse(readFileSync(join(root, "families/first_class_example_containers/live.craftax.v1/template.json"), "utf8"));
  const lifecycle = template.inputs.find((input) => input.name === "optimizer_run");
  assert.deepEqual(lifecycle?.accepts, ["optimizer_run"]);
  assert.equal(lifecycle?.required, false);
  const shell = readFileSync(join(root, "families/first_class_example_containers/live.craftax.v1/shell.tsx"), "utf8");
  assert.match(shell, /const visualLive = !lifecycleTerminal && state === "live"/);
  assert.match(shell, /Trace evidence was rejected, not missing/);
  assert.match(shell, /Trace replay retained; evaluation result incomplete/);
  assert.match(shell, /evaluation failure does not reject them/);
  assert.match(shell, /Run cost/);
  assert.match(shell, /run marker/);
  assert.match(shell, /Seal unavailable/);
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

test("payload-carried rollout identity is promoted before multiplexed replay", () => {
  const state = ingestLiveEnvelopes([
    { kind: "observation", event_id: "1", sequence: 1, payload: { rollout_id: "seed-2001", step: 0 } },
    { kind: "observation", event_id: "1", sequence: 1, payload: { rollout_id: "seed-2002", step: 0 } },
    { kind: "reward_signal", event_id: "5", sequence: 5, payload: { rollout_id: "seed-2001", reward: 2 } },
    { kind: "reward_signal", event_id: "5", sequence: 5, payload: { rollout_id: "seed-2002", reward: 1 } },
  ]);
  assert.equal(state.events.length, 4);
  assert.deepEqual(state.conflicts, []);
  assert.deepEqual(state.events.map((event) => event.rollout_id), [
    "seed-2001", "seed-2002", "seed-2001", "seed-2002",
  ]);
  assert.equal(state.lastSequenceByScope.get("seed-2001"), 5);
  assert.equal(state.lastSequenceByScope.get("seed-2002"), 5);
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

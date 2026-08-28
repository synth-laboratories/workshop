import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function loadEvents(rel) {
  const parsed = JSON.parse(readFileSync(join(root, rel), "utf8"));
  return parsed.events ?? parsed;
}

function isControl(event) {
  const kind = String(event.kind ?? event.type ?? "");
  return kind === "stream.subscribed" || kind === "heartbeat" || kind === "stream.heartbeat" || kind === "ping" || event.control === true;
}

function jsonKeys(payload, acc = new Set()) {
  if (!payload || typeof payload !== "object") return acc;
  for (const [key, value] of Object.entries(payload)) {
    acc.add(key);
    jsonKeys(value, acc);
  }
  return acc;
}

function projectLiveEval(events, cutoff) {
  const rows = [];
  const taken = new Map();
  for (const event of events) {
    if (isControl(event)) continue;
    if (cutoff) {
      const stream = event.stream_id ?? event.payload?.stream_id ?? event.rollout_id ?? event.lane ?? event.run_id ?? "run";
      const already = taken.get(stream) ?? 0;
      if (already >= (cutoff[stream] ?? 0)) continue;
      taken.set(stream, already + 1);
    }
    rows.push(event);
  }
  const kinds = rows.map((event) => String(event.kind ?? event.type ?? ""));
  const has_live_frames = kinds.includes("frame");
  const has_reward_txt = rows.some((event) => jsonKeys(event.payload).has("reward.txt"));
  const lastVerifier = [...rows].reverse().find((event) => event.kind === "verifier");
  const lastReward = [...rows].reverse().find((event) => event.kind === "reward_signal" || event.kind === "eval.run.terminal");
  let reward = null;
  const nested = lastVerifier?.payload?.["reward.txt"];
  if (typeof nested === "number") reward = nested;
  if (reward == null && lastReward) {
    for (const key of ["value", "reward", "total"]) {
      if (typeof lastReward.payload?.[key] === "number") {
        reward = lastReward.payload[key];
        break;
      }
    }
  }
  return { events: rows, kinds, has_live_frames, has_reward_txt, reward };
}

function formatMissingNumber(value) {
  return typeof value === "number" && Number.isFinite(value) ? value.toFixed(2) : "—";
}

function assertLiveEvalSlot(slot) {
  if (slot === "live" || slot === "jobs") {
    return `Forbidden live-eval input "${slot}"; bind input "stream"`;
  }
  return null;
}

function isGuessedStreamUrl(source) {
  try {
    const path = new URL(source, "http://127.0.0.1").pathname.replace(/\/+$/, "");
    return path === "/events" || /^\/rollouts\/[^/]+\/stream$/.test(path);
  } catch {
    return source === "/events";
  }
}

function assertDeclaredStreamSource(source, descriptor) {
  const declared = descriptor?.transports?.sse?.url ?? descriptor?.sse_url ?? null;
  if (declared) {
    if (source === declared || source.endsWith(declared) || declared.endsWith(source)) return null;
    return `Stream URL is not the declared stream id/url (got ${source})`;
  }
  if (isGuessedStreamUrl(source)) {
    return `Refusing guessed stream URL "${source}"; bind the declared stream.id from create-rollout`;
  }
  return null;
}

const craftax = loadEvents("families/first_class_example_containers/live.craftax.v1/examples/events.json");
const harbor = loadEvents("families/first_class_example_containers/live.harbor_eval.v1/examples/events.json");

test("C7-W03 missing reward stays em dash", () => {
  const missing = [
    { kind: "trace.opened", sequence: 1, payload: {} },
    { kind: "observation", sequence: 2, payload: { text: "fog" } },
    { kind: "status", sequence: 3, payload: { status: "running" } },
  ];
  const projection = projectLiveEval(missing);
  assert.equal(projection.reward, null);
  assert.equal(formatMissingNumber(projection.reward), "—");
  assert.equal(formatMissingNumber(undefined), "—");
});

test("terminal status is never synthesized into reward", () => {
  for (const status of ["completed", "game_over"]) {
    const projection = projectLiveEval([
      { kind: "status", sequence: 1, payload: { status } },
    ]);
    assert.equal(projection.reward, null);
  }
});

test("C7-W04 same reducer: craftax frames, harbor reward.txt", () => {
  const c = projectLiveEval(craftax);
  const h = projectLiveEval(harbor);
  assert.equal(c.has_live_frames, true);
  assert.equal(c.has_reward_txt, false);
  assert.ok(c.kinds.includes("reward_signal"));
  assert.equal(h.has_live_frames, false);
  assert.equal(h.has_reward_txt, true);
  assert.ok(h.kinds.includes("trial.planned"));
  assert.ok(h.kinds.includes("verifier"));
});

test("C7-W01 forbidden slots live/jobs fail", () => {
  assert.match(assertLiveEvalSlot("live"), /Forbidden live-eval input "live"/);
  assert.match(assertLiveEvalSlot("jobs"), /Forbidden live-eval input "jobs"/);
  assert.equal(assertLiveEvalSlot("stream"), null);
});

test("a cutoff cursor vector hides later events", () => {
  // The cutoff is a prefix length per stream, not a sequence: the real
  // multiplexed capture sequences with opaque strings, so a numeric cutoff
  // cannot address its events at all. See `stream_fold::CursorVector`.
  const all = projectLiveEval(craftax);
  const cut = projectLiveEval(craftax, { "seed:0": 4 });
  assert.equal(cut.events.length, 4);
  assert.deepEqual(cut.kinds, all.kinds.slice(0, 4));
  assert.ok(all.kinds.includes("frame"));
  assert.ok(!cut.kinds.includes("status"));
  assert.deepEqual(projectLiveEval(craftax, {}).events, [], "an unnamed stream is excluded");
});

test("guessed URL /events fails assertDeclaredStreamSource", () => {
  assert.match(assertDeclaredStreamSource("/events"), /Refusing guessed stream URL/);
  assert.match(assertDeclaredStreamSource("http://127.0.0.1:8080/events"), /Refusing guessed stream URL/);
  assert.equal(
    assertDeclaredStreamSource("/rollouts/r1/stream", { transports: { sse: { url: "/rollouts/r1/stream" } } }),
    null,
  );
});

test("/reward maps env status and never fabricates incomplete as zero", async () => {
  const { rewardFromEnvStatus } = await import("../runtime/liveEvalReducer.ts");
  assert.equal(rewardFromEnvStatus("completed"), 1);
  assert.equal(rewardFromEnvStatus("game_over"), 0);
  assert.equal(rewardFromEnvStatus("running"), null);
  assert.equal(rewardFromEnvStatus(null), null);
});

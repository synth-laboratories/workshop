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

function projectLiveEval(events, cutoffSequence) {
  const rows = [];
  for (const event of events) {
    if (isControl(event)) continue;
    const seq = event.sequence_number ?? event.sequence;
    const n = typeof seq === "number" ? seq : typeof seq === "string" && seq !== "" ? Number(seq) : null;
    if (cutoffSequence != null && n != null && Number.isFinite(n) && n > cutoffSequence) continue;
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
    return `Forbidden live-eval slot "${slot}"; bind slot "stream"`;
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

const craftax = loadEvents("templates/live.craftax.v1/examples/events.json");
const harbor = loadEvents("templates/live.harbor_eval.v1/examples/events.json");
const digbench = loadEvents("templates/live.digbench.v1/examples/events.json");

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

test("C7-W04 same reducer: craftax frames, harbor reward.txt, digbench neither", () => {
  const c = projectLiveEval(craftax);
  const h = projectLiveEval(harbor);
  const d = projectLiveEval(digbench);
  assert.equal(c.has_live_frames, true);
  assert.equal(c.has_reward_txt, false);
  assert.ok(c.kinds.includes("reward_signal"));
  assert.equal(h.has_live_frames, false);
  assert.equal(h.has_reward_txt, true);
  assert.ok(h.kinds.includes("trial.planned"));
  assert.ok(h.kinds.includes("verifier"));
  assert.equal(d.has_live_frames, false);
  assert.equal(d.has_reward_txt, false);
  assert.ok(d.kinds.includes("legal_actions"));
  assert.ok(d.kinds.includes("stats"));
});

test("Harbor sibling-container events retain a single DeepSWE attempt and native reward", async () => {
  const { projectHarborAttempts } = await import("../runtime/harborEval.ts");
  const attempts = projectHarborAttempts([
    {
      kind: "env.episode.opened",
      sequence: 1,
      payload: {
        trial_image_id: "deepswe/anko-default-function-arguments",
        environment_release: {
          environment_release_id: "harbor:anko-default-function-arguments:abc123",
          status: "certified",
          prewarm: { state: "required" },
          runnable: false,
        },
      },
    },
    { kind: "nested.workspace.extracted", sequence: 2, payload: {} },
    { kind: "span.policy.opened", sequence: 3, payload: {} },
    { kind: "nested.collected", sequence: 4, payload: { step: 0, exit_code: 0 } },
    { kind: "span.verifier.opened", sequence: 5, payload: {} },
    { kind: "nested.verified", sequence: 6, payload: { exit_code: 0 } },
    { kind: "reward_signal", sequence: 7, payload: { value: 0 } },
    { kind: "status", sequence: 8, payload: { status: "completed" } },
  ]);
  assert.equal(attempts.length, 1);
  assert.equal(attempts[0].environmentReleaseId, "harbor:anko-default-function-arguments:abc123");
  assert.equal(attempts[0].prewarmState, "required");
  assert.equal(attempts[0].runnable, false);
  assert.equal(attempts[0].phase, "scored");
  assert.equal(attempts[0].reward, 0);
});

test("C7-W01 forbidden slots live/jobs fail", () => {
  assert.match(assertLiveEvalSlot("live"), /Forbidden live-eval slot "live"/);
  assert.match(assertLiveEvalSlot("jobs"), /Forbidden live-eval slot "jobs"/);
  assert.equal(assertLiveEvalSlot("stream"), null);
});

test("C8-08 digbench fixture has no live_frames", () => {
  const d = projectLiveEval(digbench);
  assert.equal(d.has_live_frames, false);
  assert.ok(!JSON.stringify(digbench).includes("DIGBENCH_API_TOKEN"));
  assert.ok(!JSON.stringify(digbench).includes("Authorization"));
});

test("cutoff sequence hides later events", () => {
  const all = projectLiveEval(craftax);
  const cut = projectLiveEval(craftax, 4);
  assert.ok(all.kinds.includes("frame"));
  assert.ok(!cut.kinds.includes("frame"));
  assert.ok(cut.kinds.includes("action"));
  assert.ok(!cut.kinds.includes("status"));
});

test("guessed URL /events fails assertDeclaredStreamSource", () => {
  assert.match(assertDeclaredStreamSource("/events"), /Refusing guessed stream URL/);
  assert.match(assertDeclaredStreamSource("http://127.0.0.1:8080/events"), /Refusing guessed stream URL/);
  assert.equal(
    assertDeclaredStreamSource("/rollouts/r1/stream", { transports: { sse: { url: "/rollouts/r1/stream" } } }),
    null,
  );
});

test("dig.bench /reward maps env status and never fabricates incomplete as zero", async () => {
  const { rewardFromEnvStatus } = await import("../runtime/liveEvalReducer.ts");
  assert.equal(rewardFromEnvStatus("completed"), 1);
  assert.equal(rewardFromEnvStatus("game_over"), 0);
  assert.equal(rewardFromEnvStatus("running"), null);
  assert.equal(rewardFromEnvStatus(null), null);
});

test("dig.bench lane projection labels structural smoke as stub evidence", async () => {
  const { projectDigbenchLane } = await import("../runtime/liveEvalReducer.ts");
  const lane = projectDigbenchLane(digbench.filter((event) => event.run_id === "digbench_p1"));
  assert.equal(lane.harness, "react_legal_actions");
  assert.equal(lane.config, "react_legal_actions");
  assert.equal(lane.label, "Basic · react_legal_actions");
  assert.equal(lane.evidence_class, "stub");
  assert.equal(lane.actions, 1);
  assert.equal(lane.invalid_actions, 1);
  assert.equal(lane.unique_observations, 1);
  assert.equal(lane.mcp_calls, 0);
});

test("dig.bench lane projection requires observed non-simulated MCP for live Codex", async () => {
  const { projectDigbenchLane } = await import("../runtime/liveEvalReducer.ts");
  const events = [
    {
      kind: "trace.opened",
      run_id: "agentic",
      payload: { policy_ref: { harness: "codex", config: "agentic_luna_medium" } },
    },
    { kind: "observation", run_id: "agentic", payload: { text: "same room" } },
    { kind: "span.mcp.opened", run_id: "agentic", payload: { tool: "step", server: "digbench-mcp" } },
    { kind: "action", run_id: "agentic", payload: { action: "inspect", action_authority: "policy" } },
    { kind: "span.mcp.closed", run_id: "agentic", payload: { tool: "step" } },
    { kind: "observation", run_id: "agentic", payload: { text: "same room" } },
  ];
  const lane = projectDigbenchLane(events);
  assert.equal(lane.label, "Codex · agentic_luna_medium");
  assert.equal(lane.evidence_class, "live_codex_mcp");
  assert.equal(lane.mcp_calls, 1);
  assert.equal(lane.unique_observations, 1);
});

test("dig.bench lane projection distinguishes authenticated Codex exec from MCP", async () => {
  const { projectDigbenchLane } = await import("../runtime/liveEvalReducer.ts");
  const events = [
    {
      kind: "trace.opened",
      run_id: "codex_exec_lane",
      payload: { policy_ref: { harness: "codex", config: "codex_exec_luna_medium" } },
    },
    {
      kind: "action",
      run_id: "codex_exec_lane",
      payload: { action: "inspect", harness: "codex", action_authority: "codex_exec_live" },
    },
  ];
  const lane = projectDigbenchLane(events);
  assert.equal(lane.evidence_class, "live_codex_exec");
  assert.equal(lane.mcp_calls, 0);
});

test("official P-1 Luna/Terra fixture keeps score and command compliance separate", async () => {
  const { projectDigbenchLane } = await import("../runtime/liveEvalReducer.ts");
  const events = loadEvents("templates/live.digbench.v1/examples/codex-auth-results.json");
  const runIds = [...new Set(events.map((event) => event.run_id))];
  assert.equal(runIds.length, 2);
  const luna = projectDigbenchLane(events.filter((event) => event.run_id === "dig_official_p1_luna"));
  const terra = projectDigbenchLane(events.filter((event) => event.run_id === "dig_official_p1_terra"));
  assert.equal(luna.evidence_class, "live_codex_exec");
  assert.equal(luna.levels_beaten, 8);
  assert.equal(luna.actions, 672);
  assert.equal(luna.command_authority_passed, false);
  assert.equal(luna.malformed_commands, 88);
  assert.equal(terra.evidence_class, "live_codex_exec");
  assert.equal(terra.levels_beaten, 1);
  assert.equal(terra.actions, 269);
  assert.equal(terra.command_authority_passed, true);
  assert.equal(terra.malformed_commands, 0);
});

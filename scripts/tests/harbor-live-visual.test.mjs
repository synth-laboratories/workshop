import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, mkdir, writeFile, appendFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHarborVisualFeed, projectCodexTool } from "../harbor-live-visual.mjs";

test("Codex projection excludes raw commands, outputs and messages", () => {
  const row = { type: "event_msg", timestamp: "2026-09-15T00:00:00Z", payload: {
    type: "item_completed", item: { type: "CommandExecution", status: "completed", exit_code: 0,
      command: "SECRET_COMMAND", aggregated_output: "SECRET_OUTPUT", duration: { secs: 1, nanos: 0 } }
  } };
  const projected = projectCodexTool(row);
  assert.deepEqual(projected, { kind: "tools", ts: row.timestamp,
    payload: { name: "command", status: "completed", elapsed_ms: 1000, exit_code: 0 } });
  assert.equal(projectCodexTool({ type: "event_msg", payload: { type: "item_completed", item: { type: "Reasoning", raw_content: "SECRET" } } }), null);
  assert.doesNotMatch(JSON.stringify(projected), /SECRET/);
  const streamed = projectCodexTool({ type: "item.completed", item: {
    type: "command_execution", status: "completed", exit_code: 0,
    command: "SECRET_COMMAND", aggregated_output: "SECRET_OUTPUT"
  } });
  assert.equal(streamed.kind, "tools");
  assert.equal(streamed.payload.name, "command");
  assert.doesNotMatch(JSON.stringify(streamed), /SECRET/);
});

test("Harbor feed advances a declared cursor and auto-seals reward.txt", async () => {
  const root = await mkdtemp(join(tmpdir(), "workshop-harbor-feed-"));
  const session = join(root, "agent", "sessions", "2026", "09", "15", "rollout.jsonl");
  await mkdir(join(root, "agent", "sessions", "2026", "09", "15"), { recursive: true });
  await mkdir(join(root, "verifier"));
  await writeFile(session, "");
  let visual;
  const server = createServer(async (req, res) => {
    assert.equal(req.headers.authorization, "Bearer test-token");
    if (req.method === "GET") {
      if (!visual) { res.writeHead(404, { "content-type": "application/json" }); res.end(JSON.stringify({ error: "visual not found" })); return; }
      res.writeHead(200, { "content-type": "application/json" }); res.end(JSON.stringify({ visual })); return;
    }
    let body = "";
    for await (const chunk of req) body += chunk;
    const incoming = JSON.parse(body);
    visual = { ...visual, ...incoming, id: "feed-test", bindings: incoming.bindings };
    res.writeHead(200, { "content-type": "application/json" }); res.end(JSON.stringify({ visual }));
  });
  await new Promise((ready) => server.listen(0, "127.0.0.1", ready));
  const feed = createHarborVisualFeed({ trialDir: root, visualId: "feed-test",
    connection: { url: `http://127.0.0.1:${server.address().port}`, token: "test-token" }, intervalMs: 10_000 });
  try {
    await feed.start();
    assert.equal(visual.status, "live");
    assert.equal(visual.bindings.inputs[0].kind, "live_sse");
    const first = await (await fetch(`${feed.pollUrl}?after=0&limit=1`)).json();
    assert.equal(first.events[0].kind, "stream.subscribed");
    assert.equal(first.events[1].kind, "trial.launched");
    assert.equal(first.cursor.has_more, false);
    const row = { type: "event_msg", timestamp: "2026-09-15T00:00:01Z", payload: { type: "item_completed",
      item: { type: "CommandExecution", status: "completed", exit_code: 0, command: "SECRET_COMMAND", aggregated_output: "SECRET_OUTPUT" } } };
    await appendFile(session, `${JSON.stringify(row)}\n`);
    await feed.tick();
    const next = await (await fetch(`${feed.pollUrl}?after=1&limit=1`)).json();
    assert.equal(next.events[0].kind, "tools");
    assert.equal(next.cursor.next, 2);
    assert.doesNotMatch(JSON.stringify(next), /SECRET/);
    await writeFile(join(root, "verifier", "reward.txt"), "0.25\n");
    await writeFile(join(root, "result.json"), JSON.stringify({ finished_at: "2026-09-15T00:00:02Z",
      exception_info: null, verifier_result: { rewards: { reward: 0.25 } },
      agent_result: { n_input_tokens: 10, n_output_tokens: 3, cost_usd: 0.01 } }));
    await feed.tick();
    assert.equal(feed.sealed, true);
    assert.equal(visual.status, "saved");
    assert.equal(visual.bindings.inputs[0].kind, "inline");
    assert.deepEqual(visual.bindings.inputs[0].data.events.map((event) => event.kind),
      ["trial.launched", "tools", "verifier", "status"]);
    assert.equal(visual.bindings.inputs[0].data.events[2].payload["reward.txt"], 0.25);
    assert.doesNotMatch(JSON.stringify(visual), /SECRET/);
  } finally {
    await feed.stop();
    await new Promise((done) => server.close(done));
    await rm(root, { recursive: true, force: true });
  }
});

test("a completed trial waits for a large live stdout backlog before sealing", async () => {
  const root = await mkdtemp(join(tmpdir(), "workshop-harbor-backlog-"));
  await mkdir(join(root, "agent"));
  await mkdir(join(root, "verifier"));
  const ignored = JSON.stringify({ type: "session_meta", blob: "X".repeat(1_048_600) });
  const tool = JSON.stringify({ type: "item.completed", item: { type: "file_change", status: "completed", changes: [] } });
  await writeFile(join(root, "agent", "codex.txt"), `${ignored}\n${tool}\n`);
  await writeFile(join(root, "verifier", "reward.txt"), "0\n");
  await writeFile(join(root, "result.json"), JSON.stringify({ finished_at: "2026-09-15T00:00:02Z",
    exception_info: null, verifier_result: { rewards: { reward: 0 } }, agent_result: { n_input_tokens: 1, n_output_tokens: 1, cost_usd: 0 } }));
  let visual;
  const server = createServer(async (req, res) => {
    if (req.method === "GET" && !visual) {
      res.writeHead(404, { "content-type": "application/json" }); res.end(JSON.stringify({ error: "visual not found" })); return;
    }
    if (req.method !== "GET") {
      let body = "";
      for await (const chunk of req) body += chunk;
      visual = { ...visual, ...JSON.parse(body), id: "backlog-test" };
    }
    res.writeHead(200, { "content-type": "application/json" }); res.end(JSON.stringify({ visual }));
  });
  await new Promise((ready) => server.listen(0, "127.0.0.1", ready));
  const feed = createHarborVisualFeed({ trialDir: root, visualId: "backlog-test",
    connection: { url: `http://127.0.0.1:${server.address().port}`, token: "test-token" }, intervalMs: 10_000 });
  try {
    await feed.start();
    assert.equal(feed.sealed, false);
    assert.equal(visual.status, "live");
    await feed.tick();
    assert.equal(feed.sealed, true);
    assert.equal(visual.status, "saved");
    assert.deepEqual(feed.events.map((event) => event.kind), ["trial.launched", "tools", "verifier", "status"]);
    assert.equal(feed.events[2].payload["reward.txt"], 0);
    assert.doesNotMatch(JSON.stringify(visual), /XXXXX/);
  } finally {
    await feed.stop();
    await new Promise((done) => server.close(done));
    await rm(root, { recursive: true, force: true });
  }
});

test("Harbor infrastructure failure seals failed without inventing reward evidence", async () => {
  const root = await mkdtemp(join(tmpdir(), "workshop-harbor-failed-"));
  await mkdir(join(root, "agent"));
  await writeFile(join(root, "agent", "codex.txt"), "");
  await writeFile(join(root, "result.json"), JSON.stringify({ finished_at: "2026-09-15T00:00:02Z",
    exception_info: { type: "infrastructure" } }));
  let visual;
  const server = createServer(async (req, res) => {
    if (req.method === "GET" && !visual) {
      res.writeHead(404, { "content-type": "application/json" }); res.end(JSON.stringify({ error: "visual not found" })); return;
    }
    if (req.method !== "GET") {
      let body = "";
      for await (const chunk of req) body += chunk;
      visual = { ...visual, ...JSON.parse(body), id: "failed-test" };
    }
    res.writeHead(200, { "content-type": "application/json" }); res.end(JSON.stringify({ visual }));
  });
  await new Promise((ready) => server.listen(0, "127.0.0.1", ready));
  const feed = createHarborVisualFeed({ trialDir: root, visualId: "failed-test",
    connection: { url: `http://127.0.0.1:${server.address().port}`, token: "test-token" }, intervalMs: 10_000 });
  try {
    await feed.start();
    assert.equal(feed.sealed, true);
    assert.equal(visual.status, "failed");
    assert.deepEqual(feed.events.map((event) => event.kind), ["trial.launched", "status"]);
    assert.equal(visual.bindings.inputs[0].kind, "inline");
    assert.doesNotMatch(JSON.stringify(visual), /reward.txt/);
    assert.equal(feed.events[0].payload.instruction, undefined);
  } finally {
    await feed.stop();
    await new Promise((done) => server.close(done));
    await rm(root, { recursive: true, force: true });
  }
});

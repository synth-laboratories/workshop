import test from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createJesterkyVisualFeed, projectJesterkyEvent, projectJesterkySpec } from "../jesterky-live-visual.mjs";

const row = (kind, seq, payload = null) => ({ addr: { run_id: "run-1", node_path: [{ node: "fanout" }, { index: 2 }], iteration: 0, local_seq: seq }, kind: { kind }, payload, wall_ms: 1_789_000_000_000 + seq });

test("Jesterky projection retains structural identity but drops raw outputs", () => {
  const projected = projectJesterkyEvent(row("actor_invoked", 1, { actor: "researcher", prompt: "SECRET", output: "SECRET" }));
  assert.deepEqual(projected.payload.node_path, [{ kind: "node", value: "fanout" }, { kind: "index", value: 2 }]);
  assert.equal(projected.payload.actor, "researcher");
  assert.doesNotMatch(JSON.stringify(projected), /SECRET/);
  const spec = projectJesterkySpec({ name: "demo", entrypoint: ["fanout"], nodes: { fanout: { kind: "map", concurrency: 3, body: { actor: "researcher" }, inputs: { secret: "SECRET" } } }, runplan: { budgets: { caps: [{ kind: "actor_calls", max: 5, hard: true }] } } });
  assert.deepEqual(spec.nodes, [{ id: "fanout", kind: "map", actor: "researcher", concurrency: 3 }]);
  assert.doesNotMatch(JSON.stringify(spec), /SECRET/);
});

test("Jesterky feed advances live then seals the exact manifest event count", async () => {
  const root = await mkdtemp(join(tmpdir(), "workshop-jesterky-"));
  const specPath = join(root, "spec.json");
  const eventsPath = join(root, "events.ndjson");
  const manifestPath = join(root, "manifest.json");
  await writeFile(specPath, JSON.stringify({ name: "demo", entrypoint: ["fanout"], nodes: { fanout: { kind: "map", concurrency: 3, body: { actor: "researcher" } } } }));
  await writeFile(eventsPath, `${JSON.stringify(row("workflow_started", 0))}\n${JSON.stringify(row("node_started", 1))}`);
  let visual;
  const api = createServer(async (req, res) => {
    assert.equal(req.headers.authorization, "Bearer token");
    if (req.method === "GET" && !visual) { res.writeHead(404, { "content-type": "application/json" }); res.end(JSON.stringify({ error: "visual not found" })); return; }
    if (req.method !== "GET") { let body = ""; for await (const chunk of req) body += chunk; visual = { ...visual, ...JSON.parse(body), id: "jesterky-test" }; }
    res.writeHead(200, { "content-type": "application/json" }); res.end(JSON.stringify({ visual }));
  });
  await new Promise((ready) => api.listen(0, "127.0.0.1", ready));
  const feed = createJesterkyVisualFeed({ specPath, eventsPath, manifestPath, visualId: "jesterky-test",
    connection: { url: `http://127.0.0.1:${api.address().port}`, token: "token" }, intervalMs: 10_000 });
  try {
    const poll = await feed.start();
    assert.equal(visual.status, "live");
    assert.equal(visual.templateId, "workflow.jesterky.v1");
    const page = await (await fetch(`${poll}?after=0`)).json();
    assert.deepEqual(page.events.map((event) => event.kind), ["stream.subscribed", "jesterky.transition"]);
    const manifestEvents = [row("workflow_started", 0), row("node_started", 1)];
    await writeFile(manifestPath, JSON.stringify({ run_id: "run-1", workflow_name: "demo", spec_hash: "abc", status: "completed", events: manifestEvents, recorded: [{}], checkpoints: [] }));
    await feed.tick();
    assert.equal(feed.sealed, true);
    assert.equal(visual.status, "saved");
    assert.equal(visual.bindings.inputs[0].kind, "inline");
    assert.equal(visual.bindings.inputs[0].data.events.length, 2);
    assert.equal(visual.bindings.inputs[1].data.eventCount, 2);
    assert.equal(visual.bindings.inputs[1].data.recordedCalls, 1);
  } finally {
    await feed.stop();
    await new Promise((done) => api.close(done));
    await rm(root, { recursive: true, force: true });
  }
});

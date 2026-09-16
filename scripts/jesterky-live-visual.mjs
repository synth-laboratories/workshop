#!/usr/bin/env node
/** Observe one local Jesterky run and project its safe workflow metadata into Workshop. */
import { createServer } from "node:http";
import { open, readFile, stat } from "node:fs/promises";
import { basename, resolve } from "node:path";

const MAX_PAGE = 500;
const MAX_READ_BYTES = 1024 * 1024;
const MAX_EVENTS = 50_000;
const now = () => new Date().toISOString();

function pathSegments(value) {
  if (!Array.isArray(value)) return [];
  return value.flatMap((segment) => {
    if (segment && typeof segment.node === "string") return [{ kind: "node", value: segment.node.slice(0, 128) }];
    if (segment && Number.isInteger(segment.index)) return [{ kind: "index", value: segment.index }];
    return [];
  });
}

export function projectJesterkyEvent(row) {
  const eventKind = typeof row?.kind?.kind === "string" ? row.kind.kind : null;
  const runId = typeof row?.addr?.run_id === "string" ? row.addr.run_id : null;
  if (!eventKind || !runId) return null;
  const payload = {
    event_kind: eventKind,
    node_path: pathSegments(row.addr.node_path),
    iteration: Number.isInteger(row.addr.iteration) ? row.addr.iteration : 0,
    local_seq: Number.isInteger(row.addr.local_seq) ? row.addr.local_seq : 0,
    wall_ms: Number.isFinite(row.wall_ms) ? row.wall_ms : null
  };
  if (eventKind === "actor_invoked" && typeof row.payload?.actor === "string") payload.actor = row.payload.actor.slice(0, 128);
  return { runId, kind: "jesterky.transition", ts: Number.isFinite(row.wall_ms) ? new Date(row.wall_ms).toISOString() : now(), payload };
}

export function projectJesterkySpec(spec) {
  if (!spec || typeof spec !== "object" || typeof spec.name !== "string" || !spec.nodes || typeof spec.nodes !== "object") {
    throw new Error("Jesterky spec is missing name or nodes");
  }
  const entrypoint = Array.isArray(spec.entrypoint) ? spec.entrypoint.filter((id) => typeof id === "string" && spec.nodes[id]) : [];
  const nodes = entrypoint.map((id) => {
    const node = spec.nodes[id] ?? {};
    return { id, kind: typeof node.kind === "string" ? node.kind : "unknown",
      actor: typeof node.actor === "string" ? node.actor : typeof node.body?.actor === "string" ? node.body.actor : null,
      concurrency: Number.isInteger(node.concurrency) ? node.concurrency : null };
  });
  const caps = Array.isArray(spec.runplan?.budgets?.caps) ? spec.runplan.budgets.caps.flatMap((cap) =>
    cap && typeof cap.kind === "string" && Number.isFinite(cap.max)
      ? [{ kind: cap.kind, max: cap.max, hard: cap.hard === true, label: typeof cap.label === "string" ? cap.label : null }]
      : []) : [];
  return { schemaVersion: "synth.jesterky-workflow.v1", workflowName: spec.name, entrypoint, nodes, budgets: caps };
}

export function createJesterkyVisualFeed({ specPath, eventsPath, manifestPath, visualId, title, connection, intervalMs = 350 }) {
  if (!specPath || !eventsPath || !manifestPath || !visualId || !connection?.url || !connection?.token) throw new Error("spec, events, manifest, visual id, and Workshop connection are required");
  const files = { spec: resolve(specPath), events: resolve(eventsPath), manifest: resolve(manifestPath) };
  const events = [];
  let projection;
  let offset = 0;
  let carry = "";
  let origin;
  let server;
  let timer;
  let ticking = false;
  let sealed = false;
  let closed = false;
  const pathId = encodeURIComponent(visualId);

  const requestVisual = async (path, method = "GET", body) => {
    const response = await fetch(`${connection.url}${path}`, { method,
      headers: { authorization: `Bearer ${connection.token}`, "content-type": "application/json" },
      body: body ? JSON.stringify(body) : undefined });
    const value = await response.json();
    if (!response.ok) throw new Error(value.error ?? JSON.stringify(value));
    return value.visual;
  };
  const streamBinding = (final) => final
    ? { input: "stream", kind: "inline", data: { events: [...events] } }
    : { input: "stream", kind: "live_sse", source: `${origin}/jesterky/${pathId}/stream`, poll_url: `${origin}/jesterky/${pathId}/poll`, schema: "synth.trace-stream-event.v1" };
  const workflowBinding = () => ({ input: "workflow", kind: "inline", data: projection, schema: "synth.jesterky-workflow.v1" });
  const bind = async (final = false) => {
    let visual;
    try { visual = await requestVisual(`/v1/visuals/${pathId}`); }
    catch (error) { if (!String(error).includes("not found")) throw error; }
    const bindings = { schemaVersion: "synth.visual-bindings.v1", inputs: [streamBinding(final), workflowBinding()] };
    if (visual) await requestVisual(`/v1/visuals/${pathId}`, "POST", { bindings, status: final ? "saved" : "live", bumpRevision: true });
    else await requestVisual("/v1/visuals", "POST", { id: visualId, title: title ?? `Jesterky · ${projection.workflowName}`,
      templateId: "workflow.jesterky.v1", status: final ? "saved" : "live", bindings,
      metadata: { evidenceMode: "jesterky_live_workflow", spec: basename(files.spec) } });
  };
  const envelope = (projected) => {
    if (events.length >= MAX_EVENTS) throw new Error(`Jesterky feed exceeded ${MAX_EVENTS} events`);
    const sequence = events.length + 1;
    events.push({ schemaVersion: "evals.event-stream.v1", run_id: projected.runId, kind: projected.kind,
      sequence, logical_time: sequence, ts: projected.ts, payload: projected.payload });
  };
  const ingest = async () => {
    let info;
    try { info = await stat(files.events); } catch (error) { if (error.code === "ENOENT") return; throw error; }
    if (info.size < offset) throw new Error("Jesterky event log truncated");
    if (info.size === offset) return;
    const handle = await open(files.events, "r");
    try {
      const bytes = Math.min(info.size - offset, MAX_READ_BYTES);
      const buffer = Buffer.alloc(bytes);
      const read = await handle.read(buffer, 0, bytes, offset);
      offset += read.bytesRead;
      const lines = (carry + buffer.subarray(0, read.bytesRead).toString("utf8")).split("\n");
      carry = lines.pop() ?? "";
      for (const line of lines) {
        if (!line.trim()) continue;
        const projected = projectJesterkyEvent(JSON.parse(line));
        if (projected) envelope(projected);
      }
    } finally { await handle.close(); }
  };
  const finish = async () => {
    let manifest;
    try { manifest = JSON.parse(await readFile(files.manifest, "utf8")); }
    catch (error) { if (error.code === "ENOENT") return; throw error; }
    const total = Array.isArray(manifest.events) ? manifest.events.length : 0;
    const info = await stat(files.events);
    if (offset < info.size) return;
    if (carry.trim()) {
      const projected = projectJesterkyEvent(JSON.parse(carry));
      if (projected) envelope(projected);
      carry = "";
    }
    if (events.length < total) return;
    projection = { ...projection, runId: String(manifest.run_id ?? ""), specHash: String(manifest.spec_hash ?? ""),
      status: String(manifest.status ?? "unknown"), eventCount: total,
      recordedCalls: Array.isArray(manifest.recorded) ? manifest.recorded.length : 0,
      checkpoints: Array.isArray(manifest.checkpoints) ? manifest.checkpoints.length : 0 };
    closed = true;
    await bind(true);
    sealed = true;
    setTimeout(() => { void stop(); }, 2000).unref();
  };
  const tick = async () => {
    if (ticking || sealed) return;
    ticking = true;
    try { await ingest(); await finish(); } finally { ticking = false; }
  };
  const handler = (req, res) => {
    const url = new URL(req.url, origin);
    if (req.method !== "GET" || url.pathname !== `/jesterky/${pathId}/poll`) { res.writeHead(404); res.end(); return; }
    const after = Math.max(0, Number(url.searchParams.get("after")) || 0);
    const limit = Math.min(MAX_PAGE, Math.max(1, Number(url.searchParams.get("limit")) || MAX_PAGE));
    const rows = events.filter((event) => event.sequence > after).slice(0, limit);
    const next = rows.at(-1)?.sequence ?? after;
    res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
    res.end(JSON.stringify({ events: [...(after === 0 ? [{ schemaVersion: "evals.event-stream.v1", run_id: visualId,
      kind: "stream.subscribed", sequence: null, ts: now(), payload: { ready: true, "stream.id": visualId } }] : []), ...rows],
      cursor: { next, high_water: events.length, has_more: next < events.length, closed } }));
  };
  const stop = async () => {
    if (timer) clearInterval(timer);
    if (server?.listening) await new Promise((done) => server.close(done));
  };
  return { get events() { return [...events]; }, get sealed() { return sealed; }, tick, stop,
    async start() {
      projection = projectJesterkySpec(JSON.parse(await readFile(files.spec, "utf8")));
      server = createServer(handler);
      await new Promise((ready, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", ready); });
      origin = `http://127.0.0.1:${server.address().port}`;
      try { await bind(false); await tick(); timer = setInterval(() => void tick().catch((error) => { console.error(`Jesterky live feed stopped: ${error.message}`); clearInterval(timer); }), intervalMs); }
      catch (error) { await stop(); throw error; }
      return `${origin}/jesterky/${pathId}/poll`;
    } };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(new URL(import.meta.url).pathname)) {
  const args = Object.fromEntries(process.argv.slice(2).flatMap((value, index, words) => value.startsWith("--") ? [[value.slice(2), words[index + 1]]] : []));
  if (!["spec", "events", "manifest", "visual-id", "connection-file"].every((key) => args[key])) {
    console.error("usage: node scripts/jesterky-live-visual.mjs --spec ABS --events ABS --manifest ABS --visual-id ID --connection-file ABS [--title TEXT]");
    process.exitCode = 2;
  } else {
    const connection = JSON.parse(await readFile(args["connection-file"], "utf8"));
    const feed = createJesterkyVisualFeed({ specPath: args.spec, eventsPath: args.events, manifestPath: args.manifest,
      visualId: args["visual-id"], title: args.title, connection });
    await feed.start();
    console.log(`Jesterky visual feed observing ${args["visual-id"]} (safe transition metadata only)`);
    const close = () => void feed.stop().then(() => process.exit(0));
    process.on("SIGINT", close); process.on("SIGTERM", close);
  }
}

#!/usr/bin/env node
/** Harbor/Codex trial JSONL -> Workshop's declared, cursor-replayable visual stream.
 *
 * The loopback SSE endpoint publishes only tool identity, status, duration and
 * exit code. Raw commands, tool results, messages and reasoning are never sent
 * to the visual. On verifier completion the bridge replaces its ephemeral SSE
 * binding with the same retained envelopes inline, so reopening needs no
 * running sidecar or manual post-run bind.
 */
import { createServer } from "node:http";
import { open, readFile, readdir, stat, watch } from "node:fs/promises";
import { resolve } from "node:path";

const MAX_READ_BYTES = 1024 * 1024;
const MAX_EVENTS = 20_000;

const now = () => new Date().toISOString();
const durationMs = (value) => value && typeof value === "object"
  ? Math.round((Number(value.secs) || 0) * 1000 + (Number(value.nanos) || 0) / 1e6)
  : null;

export function projectCodexTool(row) {
  const completed = row?.type === "event_msg" && row.payload?.type === "item_completed"
    ? row.payload.item
    : row?.type === "item.completed" ? row.item : null;
  const item = completed;
  const type = {
    command_execution: "CommandExecution", file_change: "FileChange", mcp_tool_call: "McpToolCall"
  }[item?.type] ?? item?.type;
  if (!item || !["CommandExecution", "FileChange", "McpToolCall"].includes(type)) return null;
  const name = type === "CommandExecution" ? "command"
    : type === "FileChange" ? "file change"
      : `${String(item.server ?? "mcp").slice(0, 64)}.${String(item.tool ?? "tool").slice(0, 64)}`;
  const payload = { name, status: String(item.status ?? "completed").slice(0, 32) };
  const elapsed_ms = durationMs(item.duration);
  if (elapsed_ms !== null) payload.elapsed_ms = elapsed_ms;
  if (Number.isInteger(item.exit_code)) payload.exit_code = item.exit_code;
  return { kind: "tools", ts: typeof row.timestamp === "string" ? row.timestamp : now(), payload };
}

async function sessionFiles(root, depth = 0) {
  if (depth > 5) return [];
  let entries;
  try { entries = await readdir(root, { withFileTypes: true }); }
  catch (error) { if (error.code === "ENOENT") return []; throw error; }
  const files = [];
  for (const entry of entries) {
    const path = `${root}/${entry.name}`;
    if (entry.isDirectory()) files.push(...await sessionFiles(path, depth + 1));
    else if (entry.isFile() && entry.name.endsWith(".jsonl")) files.push(path);
  }
  return files.sort();
}

export function createHarborVisualFeed({ trialDir, visualId, title, connection, workspaceSource }) {
  if (!trialDir || !visualId || !connection?.url || !connection?.token) throw new Error("trialDir, visualId and Workshop connection are required");
  const directory = resolve(trialDir);
  const events = [];
  const readers = new Map();
  let closed = false;
  let sealed = false;
  let ticking = false;
  let tickAgain = false;
  let tickPromise;
  let sourceMode;
  let watcherAbort;
  let watcherTask;
  let server;
  let origin;
  let terminalEvent;
  const subscribers = new Set();
  const pathId = encodeURIComponent(visualId);
  const stop = async () => {
    watcherAbort?.abort();
    if (watcherTask) await Promise.allSettled([watcherTask]);
    for (const res of subscribers) res.end();
    subscribers.clear();
    if (server?.listening) await new Promise((done) => server.close(done));
  };

  const writeSse = (res, event) => {
    if (event.sequence != null) res.write(`id: ${event.sequence}\n`);
    res.write(`data: ${JSON.stringify(event)}\n\n`);
  };
  const publish = (event) => { for (const res of subscribers) writeSse(res, event); };
  const envelope = (kind, ts, payload) => {
    if (events.length >= MAX_EVENTS) throw new Error(`visual feed exceeded ${MAX_EVENTS} events`);
    const sequence = events.length + 1;
    const event = { schemaVersion: "evals.event-stream.v1", kind, sequence, ts, logical_time: sequence, payload };
    events.push(event);
    publish(event);
  };

  const requestVisual = async (path, method = "GET", body) => {
    const response = await fetch(`${connection.url}${path}`, {
      method,
      headers: { authorization: `Bearer ${connection.token}`, "content-type": "application/json" },
      body: body ? JSON.stringify(body) : undefined
    });
    const value = await response.json();
    if (!response.ok) throw new Error(value.error ?? JSON.stringify(value));
    return value.visual;
  };

  const liveBinding = () => ({ input: "stream", kind: "live_sse",
    source: `${origin}/rollouts/${pathId}/stream`, schema: "synth.trace-stream-event.v1" });
  const inlineBinding = () => ({ input: "stream", kind: "inline", data: { events: [...events] } });

  const bind = async (final = false, failed = false) => {
    let visual;
    try { visual = await requestVisual(`/v1/visuals/${pathId}`); }
    catch (error) { if (!String(error).includes("not found")) throw error; }
    const inputs = (visual?.bindings?.inputs ?? []).filter((input) => input.input !== "stream");
    if (workspaceSource && !inputs.some((input) => input.input === "workspace")) {
      inputs.push({ input: "workspace", kind: "live_workspace", source: workspaceSource });
    }
    inputs.unshift(final ? inlineBinding() : liveBinding());
    const bindings = { schemaVersion: "synth.visual-bindings.v1", inputs };
    if (visual) {
      await requestVisual(`/v1/visuals/${pathId}`, "POST", { bindings, status: failed ? "failed" : final ? "saved" : "live", bumpRevision: true });
    } else {
      await requestVisual("/v1/visuals", "POST", {
        id: visualId, title: title ?? `Harbor live · ${visualId}`, templateId: "live.harbor_eval.v1",
        status: failed ? "failed" : final ? "saved" : "live", bindings,
        metadata: { evidenceMode: "harbor_live_trace", trialDir: directory }
      });
    }
  };

  const ingestFile = async (path, mixedLog = false) => {
    const state = readers.get(path) ?? { offset: 0, carry: "" };
    const info = await stat(path);
    if (info.size < state.offset) throw new Error(`session JSONL truncated: ${path}`);
    if (info.size === state.offset) return;
    const handle = await open(path, "r");
    try {
      const bytes = Math.min(info.size - state.offset, MAX_READ_BYTES);
      const buffer = Buffer.alloc(bytes);
      const { bytesRead } = await handle.read(buffer, 0, bytes, state.offset);
      state.offset += bytesRead;
      const lines = (state.carry + buffer.subarray(0, bytesRead).toString("utf8")).split("\n");
      state.carry = lines.pop() ?? "";
      for (const line of lines) {
        if (!line.trim()) continue;
        let row;
        try { row = JSON.parse(line); }
        catch {
          // codex.txt interleaves stderr diagnostic lines with the structured
          // stdout JSONL. They are not tool evidence and may include URLs or
          // other operational detail, so skip rather than rendering them.
          if (mixedLog) continue;
          throw new Error(`malformed complete session JSONL line in ${path}`);
        }
        const tool = projectCodexTool(row);
        if (tool) envelope(tool.kind, tool.ts, { ...tool.payload, trial_id: visualId });
      }
      readers.set(path, state);
    } finally { await handle.close(); }
  };

  const finish = async () => {
    if (sealed) return;
    // A result can be written while a large session file still has unread
    // pages. Do not freeze the visual until its declared session files have
    // been fully consumed; otherwise a successful verifier would silently
    // discard the tail of the live tool trace.
    for (const [path, reader] of readers) {
      if ((await stat(path)).size > reader.offset) return;
    }
    let result;
    try { result = JSON.parse(await readFile(`${directory}/result.json`, "utf8")); }
    catch (error) { if (error.code === "ENOENT") return; throw error; }
    if (result.finished_at == null) return;
    const failed = Boolean(result.exception_info);
    if (failed) {
      envelope("status", result.finished_at, { trial_id: visualId, status: "failed", reason: "Harbor trial infrastructure failure" });
    } else {
      let rewardText;
      try { rewardText = await readFile(`${directory}/verifier/reward.txt`, "utf8"); }
      catch (error) { if (error.code === "ENOENT") return; throw error; }
      const reward = Number(rewardText.trim());
      if (!Number.isFinite(reward) || Math.abs(reward - result.verifier_result?.rewards?.reward) > 0.000001) {
        throw new Error("Harbor reward.txt disagrees with verifier result");
      }
      envelope("verifier", result.finished_at, {
        trial_id: visualId, script: "Harbor verifier", "reward.txt": reward,
        usage: {
          prompt_tokens: result.agent_result?.n_input_tokens ?? null,
          completion_tokens: result.agent_result?.n_output_tokens ?? null,
          total_tokens: (result.agent_result?.n_input_tokens ?? 0) + (result.agent_result?.n_output_tokens ?? 0),
          cost_usd: result.agent_result?.cost_usd ?? null
        }
      });
      envelope("status", result.finished_at, { trial_id: visualId, status: "completed" });
    }
    closed = true;
    terminalEvent = { schemaVersion: "evals.event-stream.v1", kind: "stream.closed",
      sequence: events.length + 1, ts: result.finished_at, logical_time: events.length + 1,
      payload: { trial_id: visualId, status: failed ? "failed" : "completed" } };
    publish(terminalEvent);
    for (const res of subscribers) res.end();
    subscribers.clear();
    await bind(true, failed);
    sealed = true;
    // The saved inline binding is durable. Keep the SSE authority briefly
    // for any in-flight renderer read, then release the ephemeral sidecar.
    setTimeout(() => { void stop(); }, 2000).unref();
  };

  const tick = () => {
    if (sealed) return;
    if (ticking) { tickAgain = true; return tickPromise; }
    ticking = true;
    tickPromise = (async () => {
      try {
        do {
          tickAgain = false;
          // Harbor streams codex.txt during execution but copies sessions JSONL
          // to the host only after the agent stops. Choose one authority per trial.
          const liveLog = `${directory}/agent/codex.txt`;
          let hasLiveLog = false;
          try { hasLiveLog = (await stat(liveLog)).isFile(); }
          catch (error) { if (error.code !== "ENOENT") throw error; }
          const archives = sourceMode === "live" ? [] : await sessionFiles(`${directory}/agent/sessions`);
          sourceMode ??= hasLiveLog ? "live" : archives.length ? "archive" : undefined;
          if (sourceMode === "live") await ingestFile(liveLog, true);
          else if (sourceMode === "archive") for (const file of archives) await ingestFile(file);
          await finish();
        } while (tickAgain && !sealed);
      } finally { ticking = false; }
    })();
    return tickPromise;
  };

  const handler = (req, res) => {
    const url = new URL(req.url, origin);
    if (req.method !== "GET" || url.pathname !== `/rollouts/${pathId}/stream`) {
      res.writeHead(404); res.end(); return;
    }
    const after = Math.max(0, Number(req.headers["last-event-id"] ?? url.searchParams.get("after")) || 0);
    res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache, no-transform",
      connection: "keep-alive", "access-control-allow-origin": "*", "x-accel-buffering": "no" });
    res.flushHeaders?.();
    writeSse(res, { schemaVersion: "evals.event-stream.v1", kind: "stream.subscribed", sequence: null,
      ts: now(), payload: { ready: true, "stream.id": visualId, after } });
    for (const event of events) if (event.sequence > after) writeSse(res, event);
    if (closed) { if (terminalEvent && terminalEvent.sequence > after) writeSse(res, terminalEvent); res.end(); return; }
    subscribers.add(res);
    req.on("close", () => subscribers.delete(res));
  };

  return {
    get events() { return [...events]; },
    get sealed() { return sealed; },
    get streamUrl() { return `${origin}/rollouts/${pathId}/stream`; },
    tick,
    async start() {
      server = createServer(handler);
      await new Promise((ready, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", ready); });
      origin = `http://127.0.0.1:${server.address().port}`;
      try {
        envelope("trial.launched", now(), { trial_id: visualId, sandbox: "harbor-docker" });
        await bind(false);
        watcherAbort = new AbortController();
        watcherTask = (async () => {
          try {
            for await (const _change of watch(directory, { recursive: true, signal: watcherAbort.signal })) await tick();
          } catch (error) { if (error?.name !== "AbortError") throw error; }
        })().catch((error) => console.error(`Harbor live feed stopped: ${error.message}`));
        await tick();
      } catch (error) { await this.stop(); throw error; }
      return this.streamUrl;
    },
    stop
  };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(new URL(import.meta.url).pathname)) {
  const args = Object.fromEntries(process.argv.slice(2).flatMap((value, index, words) => value.startsWith("--") ? [[value.slice(2), words[index + 1]]] : []));
  if (!args["trial-dir"] || !args["visual-id"] || !args["connection-file"]) {
    console.error("usage: node scripts/harbor-live-visual.mjs --trial-dir ABS --visual-id ID --connection-file ABS [--title TEXT] [--workspace-source ABS]");
    process.exitCode = 2;
  } else {
    const connection = JSON.parse(await readFile(args["connection-file"], "utf8"));
    const feed = createHarborVisualFeed({ trialDir: args["trial-dir"], visualId: args["visual-id"],
      title: args.title, connection, workspaceSource: args["workspace-source"] });
    await feed.start();
    console.log(`Harbor visual feed observing ${args["visual-id"]} (tool metadata only)`);
    const close = () => { void feed.stop().then(() => process.exit(0)); };
    process.on("SIGINT", close); process.on("SIGTERM", close);
  }
}

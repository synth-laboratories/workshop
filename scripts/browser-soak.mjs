#!/usr/bin/env node
// Sustained headed-Chromium acceptance with semantic actions, bounded
// observations, tab churn, and process/GPU telemetry.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import { performance } from "node:perf_hooks";

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

const app = path.resolve(argument("--app") ?? "apps/synth_desktop/src-tauri/target/release/bundle/macos/Synth Desktop.app");
const durationSeconds = Number(argument("--duration-seconds") ?? 1_800);
const intervalMs = Number(argument("--interval-ms") ?? 2_000);
const receiptPath = path.resolve(argument("--receipt") ?? path.join(os.tmpdir(), "workshop-browser-soak.json"));
assert(Number.isFinite(durationSeconds) && durationSeconds >= 1, "duration must be at least one second");
assert(Number.isFinite(intervalMs) && intervalMs >= 50, "interval must be at least 50ms");

const resources = path.join(app, "Contents", "Resources");
const runtime = path.join(resources, "browser", "runtime");
const node = path.join(runtime, "node", "bin", "node");
const backend = path.join(resources, "browser", "playwright_backend.mjs");
for (const item of [app, runtime, node, backend]) assert(fs.existsSync(item), `missing packaged component: ${item}`);

const html = `<!doctype html><html><body>
<h1>Workshop Chromium soak</h1><p id="counter">0</p>
<label>Cycle <input aria-label="Cycle"></label><button id="apply">Apply cycle</button>
<button id="modal">Open modal</button>
<script>
let mutations=0;
document.querySelector('#apply').onclick=()=>{document.querySelector('#counter').textContent=document.querySelector('[aria-label="Cycle"]').value};
document.querySelector('#modal').onclick=()=>{const old=document.querySelector('[role="dialog"]'); if(old) old.remove(); const modal=document.createElement('section'); modal.setAttribute('role','dialog'); modal.innerHTML='<h2>Mutation '+(++mutations)+'</h2>'; document.body.append(modal)};
</script></body></html>`;
const server = http.createServer((_request, response) => {
  response.setHeader("content-type", "text/html; charset=utf-8");
  response.end(html);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
const profileRoot = fs.mkdtempSync(path.join(os.tmpdir(), "workshop-browser-soak-profile-"));

const child = spawn(node, [backend], {
  env: {
    ...process.env,
    SYNTH_BROWSER_HEADLESS: "0",
    SYNTH_BROWSER_REQUIRE_HOST_APPROVAL: "0",
    SYNTH_BROWSER_ALLOWED_ORIGINS: origin,
    SYNTH_BROWSER_PROFILE_ROOT: profileRoot,
    SYNTH_BROWSER_RUNTIME_ROOT: runtime,
    PLAYWRIGHT_BROWSERS_PATH: path.join(runtime, "browsers"),
  },
  stdio: ["pipe", "pipe", "inherit"],
});
const lines = readline.createInterface({ input: child.stdout });
const pending = new Map();
let nextId = 0;
let unexpectedExit;
lines.on("line", (line) => {
  const message = JSON.parse(line);
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  message.ok ? waiter.resolve(message.response) : waiter.reject(new Error(message.error));
});
child.on("exit", (code, signal) => {
  unexpectedExit = `${signal ?? code}`;
  for (const waiter of pending.values()) waiter.reject(new Error(`browser backend exited (${unexpectedExit})`));
  pending.clear();
});
function call(operation, args = {}) {
  assert.equal(unexpectedExit, undefined, `backend exited unexpectedly (${unexpectedExit})`);
  const id = ++nextId;
  child.stdin.write(`${JSON.stringify({ id, operation, arguments: args })}\n`);
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}
function processes() {
  const rows = execFileSync("ps", ["-axo", "pid=,ppid=,rss=,command="], { encoding: "utf8" })
    .trim().split("\n").map((line) => {
      const match = line.trim().match(/^(\d+)\s+(\d+)\s+(\d+)\s+(.*)$/);
      return match ? { pid: Number(match[1]), ppid: Number(match[2]), rssKiB: Number(match[3]), command: match[4] } : null;
    }).filter(Boolean);
  const ids = new Set([child.pid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const row of rows) if (ids.has(row.ppid) && !ids.has(row.pid)) { ids.add(row.pid); changed = true; }
  }
  return rows.filter((row) => ids.has(row.pid));
}
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const started = performance.now();
const deadline = started + durationSeconds * 1_000;
const gpuPids = new Set();
let peakTreeRssKiB = 0;
let peakGpuRssKiB = 0;
let iterations = 0;
let maxSnapshotChars = 0;
let sessionId;
let tabId;

try {
  const created = await call("browser_create_session", { profile: `soak-${Date.now()}` });
  sessionId = created.result.sessionId;
  tabId = created.result.tabId;
  await call("browser_navigate", { session_id: sessionId, tab_id: tabId, url: origin });
  while (performance.now() < deadline) {
    iterations += 1;
    const value = `cycle-${iterations}`;
    await call("browser_fill", { session_id: sessionId, tab_id: tabId, target: { locator: { role: "textbox", name: "Cycle", exact: true } }, value });
    await call("browser_click", { session_id: sessionId, tab_id: tabId, target: { locator: { role: "button", name: "Apply cycle", exact: true } } });
    await call("browser_click", { session_id: sessionId, tab_id: tabId, target: { locator: { role: "button", name: "Open modal", exact: true } } });
    const snapshot = await call("browser_snapshot", { session_id: sessionId, tab_id: tabId, max_chars: 2_000 });
    assert(snapshot.result.text.length <= 2_000, "snapshot exceeded the observation ceiling");
    maxSnapshotChars = Math.max(maxSnapshotChars, snapshot.result.text.length);
    assert(snapshot.result.text.includes(value), "semantic state did not reflect the latest mutation");
    if (iterations % 10 === 0) {
      const extra = await call("browser_new_tab", { session_id: sessionId });
      await call("browser_navigate", { session_id: sessionId, tab_id: extra.result.tabId, url: origin });
      await call("browser_query", { session_id: sessionId, tab_id: extra.result.tabId, role: "heading", name: "Workshop Chromium soak", max_chars: 1_000 });
      await call("browser_close_tab", { session_id: sessionId, tab_id: extra.result.tabId });
    }
    const tree = processes();
    peakTreeRssKiB = Math.max(peakTreeRssKiB, tree.reduce((sum, row) => sum + row.rssKiB, 0));
    for (const row of tree.filter((entry) => entry.command.includes("--type=gpu-process"))) {
      gpuPids.add(row.pid);
      peakGpuRssKiB = Math.max(peakGpuRssKiB, row.rssKiB);
    }
    await delay(Math.min(intervalMs, Math.max(0, deadline - performance.now())));
  }
  assert(iterations > 0, "soak completed no iterations");
  assert(gpuPids.size > 0, "headed Chromium never exposed a GPU process");
  assert.equal(gpuPids.size, 1, `GPU process restarted during soak (${gpuPids.size} distinct PIDs)`);
  await call("browser_close_session", { session_id: sessionId });
  sessionId = undefined;
  const receipt = {
    schema: "workshop.browser-chromium-soak.v1",
    passed: true,
    headed: true,
    durationSeconds: Math.round((performance.now() - started) / 1_000),
    iterations,
    gpuProcessCount: gpuPids.size,
    gpuProcessRestarts: gpuPids.size - 1,
    peakGpuRssMiB: Math.round(peakGpuRssKiB / 1024 * 10) / 10,
    peakProcessTreeRssMiB: Math.round(peakTreeRssKiB / 1024 * 10) / 10,
    maxSnapshotChars,
    snapshotCeilingChars: 2_000,
    backendUnexpectedExit: false,
    checkedAt: new Date().toISOString(),
  };
  fs.mkdirSync(path.dirname(receiptPath), { recursive: true });
  fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, { mode: 0o600 });
  console.log(JSON.stringify(receipt, null, 2));
} finally {
  if (sessionId) await call("browser_close_session", { session_id: sessionId }).catch(() => {});
  if (child.exitCode === null && child.signalCode === null) {
    child.stdin.end();
    await Promise.race([
      new Promise((resolve) => child.once("exit", resolve)),
      delay(5_000).then(() => child.kill("SIGKILL")),
    ]);
  }
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
  fs.rmSync(profileRoot, { recursive: true, force: true });
}

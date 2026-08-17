#!/usr/bin/env node
// End-to-end acceptance through the public MCP adapter, authenticated Desktop
// IPC, Rust BrowserService approval boundary, and Playwright backend.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import readline from "node:readline";
import { performance } from "node:perf_hooks";

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

const dataRoot = argument("--data-root") ?? process.env.SYNTH_DESKTOP_DATA_ROOT;
const adapter = argument("--adapter") ?? process.env.SYNTH_BROWSER_MCP;
const appPid = Number(argument("--app-pid") ?? process.env.SYNTH_DESKTOP_PID ?? 0);
if (!dataRoot || !adapter) throw new Error("usage: browser-workshop-e2e.mjs --data-root DIR --adapter PATH [--app-pid PID]");
assert(fs.existsSync(path.join(dataRoot, "visuals-ipc.json")), "Workshop IPC descriptor is missing");
assert(fs.existsSync(adapter), "synth-browser-mcp is missing");

const html = `<!doctype html><html><body>
<h1>Workshop full-path acceptance</h1>
<p>${"bounded semantic observation ".repeat(80)}</p>
<p id="persisted"></p>
<label>Display name <input aria-label="Display name"></label>
<label>Password <input type="password" aria-label="Password" value="agent-must-never-see-this"></label>
<button id="apply">Apply profile</button>
<button id="modal">Open modal</button>
<button id="delete">Delete project</button>
<a href="http://example.invalid/escape">Unapproved destination</a>
<a href="/download" download>Download report</a>
<script>
document.querySelector('#persisted').textContent = localStorage.getItem('displayName') || 'No persisted value';
document.querySelector('#apply').onclick = () => { localStorage.setItem('displayName', document.querySelector('[aria-label="Display name"]').value); document.querySelector('#persisted').textContent = localStorage.getItem('displayName'); };
document.querySelector('#modal').onclick = () => { const el=document.createElement('section'); el.setAttribute('role','dialog'); el.innerHTML='<h2>Mutation complete</h2><button>Dismiss modal</button>'; document.body.append(el); };
document.querySelector('#delete').onclick = () => { document.body.dataset.deleted = 'true'; };
</script></body></html>`;

const server = http.createServer((request, response) => {
  if (request.url === "/download") {
    response.writeHead(200, { "content-type": "text/plain", "content-disposition": "attachment; filename=workshop-report.txt" });
    response.end("controlled Workshop download\n");
    return;
  }
  response.setHeader("content-type", "text/html; charset=utf-8");
  response.end(html);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;

const adapterEnv = { ...process.env, SYNTH_DESKTOP_IPC_FILE: path.join(dataRoot, "visuals-ipc.json") };
// The shell running release acceptance may itself belong to an unrelated
// Workshop session. Never borrow that identity for this isolated harness.
delete adapterEnv.SYNTH_SESSION_ID;
const child = spawn(adapter, [], {
  env: adapterEnv,
  stdio: ["pipe", "pipe", "inherit"],
});
const lines = readline.createInterface({ input: child.stdout });
const pending = new Map();
let nextId = 0;
lines.on("line", (line) => {
  const message = JSON.parse(line);
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  message.error ? waiter.reject(new Error(message.error.message)) : waiter.resolve(message.result);
});
child.on("exit", (code, signal) => {
  for (const waiter of pending.values()) waiter.reject(new Error(`browser MCP exited (${signal ?? code})`));
  pending.clear();
});
function rpc(method, params = {}) {
  const id = ++nextId;
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}
async function tool(name, args = {}, expectError = undefined) {
  const response = await rpc("tools/call", { name, arguments: args });
  if (expectError) {
    assert.equal(response.isError, true, `${name} unexpectedly succeeded`);
    assert.match(response.content[0].text, expectError);
    return response;
  }
  assert.equal(response.isError, false, response.content?.[0]?.text ?? `${name} failed`);
  return response.structuredContent;
}

function descendants(rootPid) {
  const rows = spawnSyncPs();
  const found = new Set([rootPid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const row of rows) if (found.has(row.ppid) && !found.has(row.pid)) { found.add(row.pid); changed = true; }
  }
  return rows.filter((row) => found.has(row.pid));
}
function spawnSyncPs() {
  return execFileSync("ps", ["-axo", "pid=,ppid=,command="], { encoding: "utf8" })
    .trim().split("\n").map((line) => {
      const match = line.trim().match(/^(\d+)\s+(\d+)\s+(.*)$/);
      return match ? { pid: Number(match[1]), ppid: Number(match[2]), command: match[3] } : null;
    }).filter(Boolean);
}

const report = { schema: "workshop.browser-full-path-acceptance.v1", origin };
const profileName = `full-path-${Date.now()}`;
let sessionId;
try {
  const initialized = await rpc("initialize");
  assert.equal(initialized.serverInfo.name, "synth-browser-mcp");
  const catalog = await rpc("tools/list");
  assert(catalog.tools.some((entry) => entry.name === "browser_audit"));

  const status = await tool("browser_status");
  assert.equal(status.phase, "ready", status.detail);
  report.runtime = { nodeVersion: status.nodeVersion, backendPresent: status.backendPresent, chromiumPresent: status.chromiumPresent };

  let started = performance.now();
  const created = await tool("browser_create_session", { profile: profileName });
  report.coldSessionMs = Math.round(performance.now() - started);
  sessionId = created.result.sessionId;
  const tabId = created.result.tabId;
  await tool("browser_navigate", { session_id: sessionId, tab_id: tabId, url: origin });

  const snapshot = await tool("browser_snapshot", { session_id: sessionId, tab_id: tabId, max_chars: 512 });
  assert(snapshot.result.text.length <= 512);
  assert.equal(snapshot.meta.truncated, true);
  assert(!JSON.stringify(snapshot).includes("agent-must-never-see-this"));
  assert.equal(snapshot.meta.sessionId, sessionId);
  assert.equal(snapshot.meta.tabId, tabId);
  assert.equal(snapshot.meta.origin, origin);

  const field = await tool("browser_query", { session_id: sessionId, tab_id: tabId, role: "textbox", name: "Display name" });
  const fieldRef = { session_id: sessionId, tab_id: tabId, document_revision: field.meta.documentRevision, element_id: "e1" };
  const secretValue = "E2E persisted value";
  const fill = await tool("browser_fill", { session_id: sessionId, tab_id: tabId, target: { ref: fieldRef }, value: secretValue });
  assert(!JSON.stringify(fill).includes(secretValue), "fill response echoed field contents");
  await tool("browser_click", { session_id: sessionId, tab_id: tabId, target: { locator: { role: "button", name: "Apply profile", exact: true } } });

  const modal = await tool("browser_query", { session_id: sessionId, tab_id: tabId, role: "button", name: "Open modal" });
  const modalRef = { session_id: sessionId, tab_id: tabId, document_revision: modal.meta.documentRevision, element_id: "e1" };
  await tool("browser_click", { session_id: sessionId, tab_id: tabId, target: { ref: modalRef } });
  await tool("browser_click", { session_id: sessionId, tab_id: tabId, target: { ref: modalRef } }, /stale_ref/);
  const mutated = await tool("browser_query", { session_id: sessionId, tab_id: tabId, role: "heading", name: "Mutation complete" });
  assert(mutated.result.text.includes("Mutation complete"), JSON.stringify(mutated));

  await tool("browser_click", { session_id: sessionId, tab_id: tabId, target: { locator: { role: "button", name: "Delete project", exact: true } } }, /require an agent session/);
  const stillPresent = await tool("browser_query", { session_id: sessionId, tab_id: tabId, role: "button", name: "Delete project" });
  assert(stillPresent.result.text.includes("Delete project"), "unapproved destructive action executed");

  const download = await tool("browser_download", { session_id: sessionId, tab_id: tabId, target: { locator: { role: "link", name: "Download report", exact: true } } });
  assert.equal(fs.readFileSync(download.result.path, "utf8"), "controlled Workshop download\n");
  const screenshot = await tool("browser_screenshot", { session_id: sessionId, tab_id: tabId });
  assert(fs.statSync(screenshot.result.path).size > 1_000);
  const audit = await tool("browser_audit", { session_id: sessionId, tab_id: tabId, limit: 100 });
  assert(!JSON.stringify(audit).includes(secretValue), "audit exposed filled field contents");
  assert(audit.result.events.some((entry) => entry.event === "browser_download"));
  // A rejected cross-origin navigation may intentionally abort the in-flight
  // document request. Exercise it after artifact capture so the assertions
  // remain independent.
  await tool("browser_click", { session_id: sessionId, tab_id: tabId, target: { locator: { role: "link", name: "Unapproved destination", exact: true } } }, /navigation_blocked|not approved/);

  const disposable = await tool("browser_new_tab", { session_id: sessionId });
  await tool("browser_close_tab", { session_id: sessionId, tab_id: disposable.result.tabId });
  const replacement = await tool("browser_new_tab", { session_id: sessionId });
  assert.notEqual(replacement.result.tabId, disposable.result.tabId);
  await tool("browser_close_session", { session_id: sessionId });
  sessionId = undefined;

  const reopened = await tool("browser_create_session", { profile: profileName });
  sessionId = reopened.result.sessionId;
  await tool("browser_navigate", { session_id: sessionId, tab_id: reopened.result.tabId, url: origin });
  const persisted = await tool("browser_query", { session_id: sessionId, tab_id: reopened.result.tabId, name: secretValue });
  assert(persisted.result.text.includes(secretValue));
  await tool("browser_close_session", { session_id: sessionId });
  sessionId = undefined;

  if (appPid > 0) {
    const crashSession = await tool("browser_create_session", { profile: "crash-recovery" });
    sessionId = crashSession.result.sessionId;
    const tree = descendants(appPid);
    const backend = tree.find((row) => row.command.includes("playwright_backend.mjs"));
    assert(backend, "could not identify the isolated Workshop browser backend");
    const backendTree = descendants(backend.pid).map((row) => row.pid);
    process.kill(backend.pid, "SIGKILL");
    for (const pid of backendTree.filter((pid) => pid !== backend.pid)) { try { process.kill(pid, "SIGTERM"); } catch {} }
    await tool("browser_list_tabs", { session_id: sessionId }, /backend crashed/);
    sessionId = undefined;
    const afterCrash = await tool("browser_status");
    assert(afterCrash.crashCount >= 1);
    assert.equal(afterCrash.serviceRunning, false);
    const recovered = await tool("browser_create_session", { profile: "crash-recovery" });
    sessionId = recovered.result.sessionId;
    await tool("browser_navigate", { session_id: sessionId, tab_id: recovered.result.tabId, url: origin });
    await tool("browser_close_session", { session_id: sessionId });
    sessionId = undefined;
    report.lifecycle = { crashCount: afterCrash.crashCount, recovered: true };
  }

  report.protocolVersion = snapshot.protocolVersion;
  report.snapshot = { chars: snapshot.result.text.length, truncated: snapshot.meta.truncated };
  report.security = { passwordRedacted: true, consequentialActionRefused: true, crossOriginBlocked: true, auditRedacted: true };
  report.profilePersistence = true;
  report.downloadAndScreenshot = true;
  report.passed = true;
  console.log(JSON.stringify(report, null, 2));
} finally {
  if (sessionId) await tool("browser_close_session", { session_id: sessionId }).catch(() => {});
  child.kill("SIGTERM");
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
}

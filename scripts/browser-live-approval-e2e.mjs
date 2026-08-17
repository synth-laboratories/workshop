#!/usr/bin/env node
// Drive a consequential browser action up to the real Workshop approval card.
// A macOS UI driver must click "Approve once"; only then does this process
// verify that the exact prepared action committed and was audited.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import readline from "node:readline";

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}
const dataRootArgument = argument("--data-root");
const adapterArgument = argument("--adapter");
assert(dataRootArgument, "--data-root is required");
assert(adapterArgument, "--adapter is required");
const dataRoot = path.resolve(dataRootArgument);
const adapter = path.resolve(adapterArgument);
const receiptPath = path.resolve(argument("--receipt") ?? path.join(dataRoot, "browser-live-approval-receipt.json"));
assert(dataRoot && fs.existsSync(dataRoot), "--data-root must name a running named Workshop instance");
assert(adapter && fs.existsSync(adapter), "--adapter must name synth-browser-mcp");

function descriptor(name) {
  const file = path.join(dataRoot, name);
  assert(fs.existsSync(file), `missing live Workshop descriptor: ${file}`);
  const parsed = JSON.parse(fs.readFileSync(file, "utf8"));
  assert.equal(typeof parsed.url, "string");
  assert.equal(typeof parsed.token, "string");
  return parsed;
}
const evalDriver = descriptor("eval-driver.json");
const ipc = descriptor("visuals-ipc.json");
const sessionId = `approval-e2e-${Date.now()}`;
async function request(connection, route, body) {
  const response = await fetch(`${connection.url}${route}`, {
    method: "POST",
    headers: { authorization: `Bearer ${connection.token}`, "content-type": "application/json", "x-synth-eval-driver": "synth.eval-driver.v1" },
    body: JSON.stringify(body),
  });
  const payload = await response.json();
  assert(response.ok, `${route} failed: ${JSON.stringify(payload)}`);
  return payload;
}

await request(evalDriver, "/v1/sessions", {
  sessionId,
  model: "openai/gpt-5.6-luna",
  provider: "openrouter",
  approvalPolicy: "on-request",
  sandbox: "workspace-write",
});
await request(ipc, "/v1/sessions/present", { sessionId, title: "Browser approval E2E" });

const html = `<!doctype html><html><body>
<h1>Consequential approval fixture</h1>
<button id="delete">Delete test project</button>
<script>document.querySelector('#delete').onclick=()=>{const h=document.createElement('h2');h.textContent='Approved action executed';document.body.append(h)}</script>
</body></html>`;
const server = http.createServer((_request, response) => {
  response.setHeader("content-type", "text/html; charset=utf-8");
  response.end(html);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;

const child = spawn(adapter, [], {
  env: { ...process.env, SYNTH_DESKTOP_IPC_FILE: path.join(dataRoot, "visuals-ipc.json"), SYNTH_SESSION_ID: sessionId },
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
async function tool(name, args = {}) {
  const response = await rpc("tools/call", { name, arguments: args });
  assert.equal(response.isError, false, response.content?.[0]?.text ?? `${name} failed`);
  return response.structuredContent;
}

let browserSession;
try {
  await rpc("initialize");
  const created = await tool("browser_create_session", { profile: `approval-${Date.now()}` });
  browserSession = created.result.sessionId;
  const tabId = created.result.tabId;
  await tool("browser_navigate", { session_id: browserSession, tab_id: tabId, url: origin });
  const pendingClick = tool("browser_click", {
    session_id: browserSession,
    tab_id: tabId,
    target: { locator: { role: "button", name: "Delete test project", exact: true } },
  });
  console.log(JSON.stringify({ event: "ready_for_ui_approval", sessionId, title: "Browser approval E2E", expectedButton: "Approve once", expectedDetail: "Delete test project" }));
  const click = await Promise.race([
    pendingClick,
    new Promise((_, reject) => setTimeout(() => reject(new Error("timed out waiting for the live UI approval click")), 120_000)),
  ]);
  const executed = await tool("browser_query", { session_id: browserSession, tab_id: tabId, role: "heading", name: "Approved action executed", max_chars: 2_000 });
  assert.equal(executed.result.elementCount, 1, "the approved exact action did not execute exactly once");
  const audit = await tool("browser_audit", { session_id: browserSession, tab_id: tabId, limit: 100 });
  const committed = audit.result.events.filter((event) => event.event === "browser_click" && JSON.stringify(event).includes("Delete test project"));
  assert.equal(committed.length, 1, "the consequential action was not audited exactly once");
  const receipt = {
    schema: "workshop.browser-live-approval-e2e.v1",
    passed: true,
    sessionId,
    origin,
    renderedTitle: "Confirm this action",
    renderedExactAction: "Delete test project",
    clickedControl: "Approve once",
    committedExactlyOnce: committed.length === 1,
    resultMetadata: click.meta,
    checkedAt: new Date().toISOString(),
  };
  fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, { mode: 0o600 });
  console.log(JSON.stringify(receipt, null, 2));
} finally {
  if (browserSession) await tool("browser_close_session", { session_id: browserSession }).catch(() => {});
  child.kill("SIGTERM");
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
}

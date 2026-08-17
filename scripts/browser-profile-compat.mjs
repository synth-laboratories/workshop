#!/usr/bin/env node
// Prove that a persistent managed-browser profile created by one packaged
// Workshop build can be reopened by another packaged build.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

const beforeArgument = argument("--before");
const afterArgument = argument("--after");
const rollbackArgument = argument("--rollback");
const requireVersionChange = process.argv.includes("--require-version-change");
const receiptPath = argument("--receipt") ?? process.env.SYNTH_ACCEPTANCE_RECEIPT;
if (!beforeArgument || !afterArgument) throw new Error("usage: browser-profile-compat.mjs --before BEFORE.app --after AFTER.app [--rollback ROLLBACK.app] [--require-version-change] [--receipt FILE]");
const beforeApp = path.resolve(beforeArgument);
const afterApp = path.resolve(afterArgument);
const rollbackApp = rollbackArgument ? path.resolve(rollbackArgument) : undefined;

function packaged(app) {
  const resources = path.join(app, "Contents", "Resources");
  const runtime = path.join(resources, "browser", "runtime");
  const node = path.join(runtime, "node", "bin", "node");
  const backend = path.join(resources, "browser", "playwright_backend.mjs");
  const info = path.join(app, "Contents", "Info.plist");
  for (const item of [app, runtime, node, backend, info]) assert(fs.existsSync(item), `packaged browser component is missing: ${item}`);
  const version = execFileSync("/usr/libexec/PlistBuddy", ["-c", "Print :CFBundleShortVersionString", info], { encoding: "utf8" }).trim();
  return { app, resources, runtime, node, backend, version };
}

const before = packaged(beforeApp);
const after = packaged(afterApp);
const rollback = rollbackApp ? packaged(rollbackApp) : undefined;
if (requireVersionChange) assert.notEqual(before.version, after.version, "updater acceptance requires distinct app versions");
if (rollback) assert.equal(rollback.version, before.version, "rollback bundle must restore the original version");

const html = `<!doctype html><html><body>
<h1 id="persisted">No persisted updater value</h1>
<label>Updater value <input aria-label="Updater value"></label>
<button id="save">Save updater value</button>
<script>
const saved = localStorage.getItem('workshopUpdaterValue');
if (saved) document.querySelector('#persisted').textContent = saved;
document.querySelector('#save').onclick = () => {
  const value = document.querySelector('[aria-label="Updater value"]').value;
  localStorage.setItem('workshopUpdaterValue', value);
  document.querySelector('#persisted').textContent = value;
};
</script></body></html>`;
const server = http.createServer((_request, response) => {
  response.setHeader("content-type", "text/html; charset=utf-8");
  response.end(html);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
const profileRoot = fs.mkdtempSync(path.join(os.tmpdir(), "workshop-browser-update-profile-"));
const profileName = "updater-compatibility";
const persistedValue = `Workshop profile ${Date.now()}`;
const clients = new Set();

function backend(build) {
  const child = spawn(build.node, [build.backend], {
    env: {
      ...process.env,
      SYNTH_BROWSER_HEADLESS: "1",
      SYNTH_BROWSER_REQUIRE_HOST_APPROVAL: "0",
      SYNTH_BROWSER_ALLOWED_ORIGINS: origin,
      SYNTH_BROWSER_PROFILE_ROOT: profileRoot,
      SYNTH_BROWSER_RUNTIME_ROOT: build.runtime,
      PLAYWRIGHT_BROWSERS_PATH: path.join(build.runtime, "browsers"),
    },
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
    message.ok ? waiter.resolve(message.response) : waiter.reject(new Error(message.error));
  });
  child.on("exit", (code, signal) => {
    for (const waiter of pending.values()) waiter.reject(new Error(`browser backend exited (${signal ?? code})`));
    pending.clear();
  });
  const client = {
    child,
    call(operation, args = {}) {
      const id = ++nextId;
      child.stdin.write(`${JSON.stringify({ id, operation, arguments: args })}\n`);
      return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
    },
    async stop() {
      if (!clients.has(client)) return;
      if (child.exitCode !== null || child.signalCode !== null) {
        clients.delete(client);
        return;
      }
      child.stdin.end();
      await new Promise((resolve) => {
        const timer = setTimeout(() => child.kill("SIGKILL"), 5_000);
        child.once("exit", () => { clearTimeout(timer); resolve(); });
      });
      clients.delete(client);
    },
  };
  clients.add(client);
  return client;
}

try {
  const writer = backend(before);
  const created = await writer.call("browser_create_session", { profile: profileName });
  const target = { session_id: created.result.sessionId, tab_id: created.result.tabId };
  await writer.call("browser_navigate", { ...target, url: origin });
  await writer.call("browser_fill", { ...target, target: { locator: { role: "textbox", name: "Updater value", exact: true } }, value: persistedValue });
  await writer.call("browser_click", { ...target, target: { locator: { role: "button", name: "Save updater value", exact: true } } });
  await writer.call("browser_close_session", { session_id: target.session_id });
  await writer.stop();

  async function verifyPersisted(build, phase) {
    const reader = backend(build);
    const reopened = await reader.call("browser_create_session", { profile: profileName });
    const reopenedTarget = { session_id: reopened.result.sessionId, tab_id: reopened.result.tabId };
    await reader.call("browser_navigate", { ...reopenedTarget, url: origin });
    const persisted = await reader.call("browser_query", { ...reopenedTarget, role: "heading", name: persistedValue, max_chars: 2_000 });
    assert(persisted.result.text.includes(persistedValue), `${phase} build did not reopen persisted browser state`);
    await reader.call("browser_close_session", { session_id: reopenedTarget.session_id });
    await reader.stop();
  }

  await verifyPersisted(after, "post-update");
  if (rollback) await verifyPersisted(rollback, "post-rollback");

  const receipt = {
    schema: "workshop.browser-profile-compatibility.v1",
    passed: true,
    productionEligible: false,
    beforeApp: before.app,
    beforeVersion: before.version,
    afterApp: after.app,
    afterVersion: after.version,
    versionChanged: before.version !== after.version,
    profilePersisted: true,
    rollbackApp: rollback?.app ?? null,
    rollbackVersion: rollback?.version ?? null,
    rollbackProfilePersisted: Boolean(rollback),
    checkedAt: new Date().toISOString(),
  };
  if (receiptPath) {
    fs.mkdirSync(path.dirname(path.resolve(receiptPath)), { recursive: true });
    fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, { mode: 0o600 });
  }
  console.log(JSON.stringify(receipt, null, 2));
} finally {
  await Promise.all([...clients].map((client) => client.stop().catch(() => {})));
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
  fs.rmSync(profileRoot, { recursive: true, force: true });
}

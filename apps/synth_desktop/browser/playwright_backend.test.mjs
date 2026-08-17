import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import test from "node:test";

const html = `<!doctype html><html><body>
<h1>Workshop Browser Test</h1>
<p>${"bounded observation content ".repeat(30)}</p>
<p id="persisted"></p>
<label>Display name <input aria-label="Display name"></label>
<label>Password <input type="password" aria-label="Password" value="never-expose-me"></label>
<button id="apply">Apply</button>
<button class="duplicate">Duplicate</button><button class="duplicate">Duplicate</button>
<button id="modal">Open modal</button>
<a href="/next">Next page</a>
<script>
persisted.textContent = localStorage.getItem('displayName') || 'No persisted value';
apply.onclick = () => { localStorage.setItem('displayName', document.querySelector('[aria-label="Display name"]').value); persisted.textContent = localStorage.getItem('displayName'); };
modal.onclick = () => { const dialog=document.createElement('div');dialog.setAttribute('role','dialog');dialog.innerHTML='<h2>Dynamic modal</h2><button>Dismiss modal</button>';document.body.append(dialog); };
</script></body></html>`;

function serve() {
  return new Promise((resolve) => {
    const server = http.createServer((request, response) => {
      response.setHeader("content-type", "text/html");
      response.end(request.url === "/next" ? "<h1>Next page</h1><a href='/'>Back home</a>" : html);
    });
    server.listen(0, "127.0.0.1", () => resolve(server));
  });
}

function backend(origin, root) {
  const child = spawn(process.execPath, [new URL("./playwright_backend.mjs", import.meta.url).pathname], {
    env: { ...process.env, SYNTH_BROWSER_HEADLESS: "1", SYNTH_BROWSER_ALLOWED_ORIGINS: origin, SYNTH_BROWSER_PROFILE_ROOT: root },
    stdio: ["pipe", "pipe", "inherit"],
  });
  const lines = readline.createInterface({ input: child.stdout });
  let nextId = 0;
  const pending = new Map();
  lines.on("line", (line) => {
    const message = JSON.parse(line);
    const waiter = pending.get(message.id);
    if (!waiter) return;
    pending.delete(message.id);
    message.ok ? waiter.resolve(message.response) : waiter.reject(new Error(message.error));
  });
  return {
    child,
    call(operation, callArgs = {}) {
      const id = ++nextId;
      child.stdin.write(`${JSON.stringify({ id, operation, arguments: callArgs })}\n`);
      return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
    },
  };
}

test("Playwright backend satisfies bounded, stale-ref, tab, and profile invariants", async (context) => {
  const server = await serve();
  const address = server.address();
  const origin = `http://127.0.0.1:${address.port}`;
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "workshop-browser-test-"));
  const client = backend(origin, root);
  context.after(async () => {
    client.child.kill("SIGTERM");
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(root, { recursive: true, force: true });
  });

  const created = await client.call("browser_create_session", { profile: "integration" });
  const session_id = created.result.sessionId;
  const tab_id = created.result.tabId;
  await client.call("browser_navigate", { session_id, tab_id, url: origin });

  const snapshot = await client.call("browser_snapshot", { session_id, tab_id, max_chars: 256 });
  assert(snapshot.result.text.length <= 256);
  assert.equal(snapshot.meta.maxChars, undefined);
  assert.equal(snapshot.meta.truncated, true);
  assert(!JSON.stringify(snapshot).includes("never-expose-me"));
  assert(snapshot.result.text.includes("Workshop Browser Test"));

  const modal = await client.call("browser_query", { session_id, tab_id, role: "button", name: "Open modal" });
  const modalRef = {
    session_id,
    tab_id,
    document_revision: modal.meta.documentRevision,
    element_id: "e1",
  };
  await client.call("browser_click", { session_id, tab_id, target: { ref: modalRef } });
  await assert.rejects(
    client.call("browser_click", { session_id, tab_id, target: { ref: modalRef } }),
    /stale_ref/,
  );
  const dialog = await client.call("browser_query", { session_id, tab_id, role: "dialog", name: "Dynamic modal" });
  assert(dialog.result.text.includes("Dynamic modal"));

  await assert.rejects(
    client.call("browser_click", { session_id, tab_id, target: { locator: { role: "button", name: "Duplicate", exact: true } } }),
    /ambiguous_locator/,
  );

  await client.call("browser_fill", { session_id, tab_id, target: { locator: { role: "textbox", name: "Display name", exact: true } }, value: "Persistent Workshop" });
  await client.call("browser_click", { session_id, tab_id, target: { locator: { role: "button", name: "Apply", exact: true } } });
  const extra = await client.call("browser_new_tab", { session_id });
  const extraId = extra.result.tabId;
  await client.call("browser_close_tab", { session_id, tab_id: extraId });
  const replacement = await client.call("browser_new_tab", { session_id });
  assert.notEqual(replacement.result.tabId, extraId);
  await client.call("browser_close_session", { session_id });

  const reopened = await client.call("browser_create_session", { profile: "integration" });
  await client.call("browser_navigate", { session_id: reopened.result.sessionId, tab_id: reopened.result.tabId, url: origin });
  const persisted = await client.call("browser_query", { session_id: reopened.result.sessionId, tab_id: reopened.result.tabId, name: "Persistent Workshop" });
  assert(persisted.result.text.includes("Persistent Workshop"));
  const screenshot = await client.call("browser_screenshot", { session_id: reopened.result.sessionId, tab_id: reopened.result.tabId });
  assert(fs.statSync(screenshot.result.path).size > 0);
  await client.call("browser_close_session", { session_id: reopened.result.sessionId });
});

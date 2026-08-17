import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import test from "node:test";
const require = createRequire(import.meta.url);
const runtimePlaywright = new URL("./runtime/node_modules/playwright", import.meta.url).pathname;
const { chromium } = require(fs.existsSync(runtimePlaywright) ? runtimePlaywright : "playwright");

function bundledChromiumExecutable() {
  const root = new URL("./runtime/browsers/", import.meta.url).pathname;
  if (!fs.existsSync(root)) return chromium.executablePath();
  const pending = [root];
  while (pending.length) {
    const dir = pending.pop();
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const item = path.join(dir, entry.name);
      if (entry.isDirectory()) pending.push(item);
      else if (item.endsWith("/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing")) return item;
    }
  }
  return chromium.executablePath();
}

const html = `<!doctype html><html><body>
<h1>Workshop Browser Test</h1>
<p>${"bounded observation content ".repeat(30)}</p>
<p id="persisted"></p>
<label>Display name <input aria-label="Display name"></label>
<label>Password <input type="password" aria-label="Password" value="never-expose-me"></label>
<label>Attachment <input type="file" aria-label="Attachment"></label>
<button id="apply">Apply</button>
<button class="duplicate">Duplicate</button><button class="duplicate">Duplicate</button>
<button id="modal">Open modal</button>
<button id="native-dialog">Open native dialog</button>
<a href="/next">Next page</a>
<script>
persisted.textContent = localStorage.getItem('displayName') || 'No persisted value';
apply.onclick = () => { localStorage.setItem('displayName', document.querySelector('[aria-label="Display name"]').value); persisted.textContent = localStorage.getItem('displayName'); };
modal.onclick = () => { const dialog=document.createElement('div');dialog.setAttribute('role','dialog');dialog.innerHTML='<h2>Dynamic modal</h2><button>Dismiss modal</button>';document.body.append(dialog); };
document.querySelector('#native-dialog').onclick = () => confirm('Delete the staged project?');
</script></body></html>`;

function serve(host = "127.0.0.1") {
  return new Promise((resolve) => {
    const server = http.createServer((request, response) => {
      if (request.url === "/cross-origin") {
        const address = server.address();
        response.writeHead(302, { location: `http://localhost:${address.port}/next` });
        response.end();
        return;
      }
      response.setHeader("content-type", "text/html");
      response.end(request.url === "/next" ? "<h1>Next page</h1><a href='/'>Back home</a>" : html);
    });
    server.listen(0, host, () => resolve(server));
  });
}

function backend(origin, root, policyFile = undefined, extraEnv = {}) {
  const runtimeRoot = new URL("./runtime", import.meta.url).pathname;
  const runtimeEnv = fs.existsSync(path.join(runtimeRoot, "manifest.json"))
    ? { SYNTH_BROWSER_RUNTIME_ROOT: runtimeRoot, PLAYWRIGHT_BROWSERS_PATH: path.join(runtimeRoot, "browsers") }
    : {};
  const child = spawn(process.execPath, [new URL("./playwright_backend.mjs", import.meta.url).pathname], {
    env: { ...process.env, ...runtimeEnv, SYNTH_BROWSER_HEADLESS: "1", SYNTH_BROWSER_ALLOWED_ORIGINS: origin, SYNTH_BROWSER_PROFILE_ROOT: root, SYNTH_BROWSER_UPLOAD_ROOTS: root, ...(policyFile ? { SYNTH_BROWSER_POLICY_FILE: policyFile } : {}), ...extraEnv },
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
      if (process.env.SYNTH_BROWSER_TEST_TRACE === "1") process.stderr.write(`call ${id} ${operation}\n`);
      child.stdin.write(`${JSON.stringify({ id, operation, arguments: callArgs })}\n`);
      return new Promise((resolve, reject) => pending.set(id, {
        resolve: (value) => {
          if (process.env.SYNTH_BROWSER_TEST_TRACE === "1") process.stderr.write(`done ${id} ${operation}\n`);
          resolve(value);
        },
        reject,
      }));
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

  const prepared = await client.call("browser_prepare_action", { operation: "browser_click", arguments: { session_id, tab_id, target: { locator: { role: "button", name: "Apply", exact: true } } } });
  await client.call("browser_click", { session_id, tab_id, target: { locator: { role: "button", name: "Open modal", exact: true } } });
  await assert.rejects(client.call("browser_commit_action", { action_token: prepared.result.actionToken }), /stale_action_token/);

  await client.call("browser_click", { session_id, tab_id, target: { locator: { role: "button", name: "Open native dialog", exact: true } } });
  const nativeDialogs = await client.call("browser_list_dialogs", { session_id, tab_id });
  assert.equal(nativeDialogs.result.dialogs.length, 1);
  const nativeDialog = nativeDialogs.result.dialogs[0];
  assert.match(nativeDialog.message, /Delete the staged project/);
  const preparedDialog = await client.call("browser_prepare_action", { operation: "browser_handle_dialog", arguments: { session_id, tab_id, dialog_id: nativeDialog.dialogId, accept: true } });
  assert.equal(preparedDialog.result.consequential, true);
  await client.call("browser_commit_action", { action_token: preparedDialog.result.actionToken });
  await assert.rejects(client.call("browser_commit_action", { action_token: preparedDialog.result.actionToken }), /stale_action_token/);

  const uploadPath = path.join(root, "explicit-upload.txt");
  fs.writeFileSync(uploadPath, "selected by the acceptance test");
  const uploadField = await client.call("browser_query", { session_id, tab_id, name: "Attachment" });
  const uploadRef = { session_id, tab_id, document_revision: uploadField.meta.documentRevision, element_id: "e1" };
  const preparedUpload = await client.call("browser_prepare_action", { operation: "browser_upload", arguments: { session_id, tab_id, target: { ref: uploadRef }, file_paths: [uploadPath] } });
  assert.equal(preparedUpload.result.consequential, true);
  await client.call("browser_commit_action", { action_token: preparedUpload.result.actionToken });
  const refreshedUploadField = await client.call("browser_query", { session_id, tab_id, name: "Attachment" });
  const refreshedUploadRef = { session_id, tab_id, document_revision: refreshedUploadField.meta.documentRevision, element_id: "e1" };
  const refusedUpload = await client.call("browser_prepare_action", { operation: "browser_upload", arguments: { session_id, tab_id, target: { ref: refreshedUploadRef }, file_paths: ["/etc/hosts"] } });
  await assert.rejects(client.call("browser_commit_action", { action_token: refusedUpload.result.actionToken }), /upload_refused/);

  await assert.rejects(
    client.call("browser_click", { session_id, tab_id, target: { locator: { role: "button", name: "Duplicate", exact: true } } }),
    /ambiguous_locator/,
  );

  const preparedFill = await client.call("browser_prepare_action", { operation: "browser_fill", arguments: { session_id, tab_id, target: { locator: { role: "textbox", name: "Display name", exact: true } }, value: "Persistent Workshop" } });
  assert.equal(preparedFill.result.actionDetails.valueLength, 19);
  assert(!JSON.stringify(preparedFill).includes("Persistent Workshop"));
  await client.call("browser_commit_action", { action_token: preparedFill.result.actionToken });
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

test("origin policy reloads between navigations and revocation fails closed", async (context) => {
  const server = await serve("0.0.0.0");
  const address = server.address();
  const origin = `http://0.0.0.0:${address.port}`;
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "workshop-browser-policy-test-"));
  const policyFile = path.join(root, "policy.json");
  fs.writeFileSync(policyFile, JSON.stringify({ allowedOrigins: [origin] }));
  const client = backend("", root, policyFile);
  context.after(async () => {
    client.child.kill("SIGTERM");
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(root, { recursive: true, force: true });
  });

  const created = await client.call("browser_create_session", { profile: "policy" });
  const target = { session_id: created.result.sessionId, tab_id: created.result.tabId, url: origin };
  await client.call("browser_navigate", target);
  await client.call("browser_navigate", { ...target, url: `${origin}/cross-origin` }).then(
    (blockedRedirect) => assert(!blockedRedirect.result.url.startsWith(`http://localhost:${address.port}`)),
    (error) => assert.match(error.message, /origin_not_approved|not approved|blocked|aborted|ERR_FAILED/i),
  );
  fs.writeFileSync(policyFile, JSON.stringify({ allowedOrigins: [] }));
  await assert.rejects(client.call("browser_navigate", target), /origin_not_approved/);
  await client.call("browser_close_session", { session_id: created.result.sessionId });
});

test("existing Chrome claims are disabled by default", async (context) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "workshop-browser-claim-test-"));
  const client = backend("", root);
  context.after(() => {
    client.child.kill("SIGTERM");
    fs.rmSync(root, { recursive: true, force: true });
  });
  await assert.rejects(
    client.call("browser_claim_chrome", { url_contains: "example.com" }),
    /chrome_claim_disabled/,
  );
});

test("an enabled Chrome claim preserves the exact user tab", async (context) => {
  const server = await serve();
  const origin = `http://127.0.0.1:${server.address().port}`;
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "workshop-browser-live-claim-test-"));
  const chromeProfile = path.join(root, "chrome-profile");
  const chrome = spawn(bundledChromiumExecutable(), [
    "--headless=new",
    "--remote-debugging-port=0",
    `--user-data-dir=${chromeProfile}`,
    "--no-first-run",
    "--no-default-browser-check",
    origin,
  ], { stdio: "ignore" });
  const portFile = path.join(chromeProfile, "DevToolsActivePort");
  for (let attempt = 0; attempt < 100 && !fs.existsSync(portFile); attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  assert(fs.existsSync(portFile), "Chrome did not expose its loopback CDP endpoint");
  const port = fs.readFileSync(portFile, "utf8").split("\n")[0];
  const endpoint = `http://127.0.0.1:${port}`;
  const client = backend(origin, root, undefined, { SYNTH_BROWSER_ENABLE_CHROME_CLAIM: "1" });
  context.after(async () => {
    const exited = (child) => child.exitCode !== null || child.signalCode !== null
      ? Promise.resolve()
      : new Promise((resolve) => child.once("exit", resolve));
    client.child.stdin.end();
    await exited(client.child);
    chrome.kill("SIGTERM");
    await exited(chrome);
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  });

  const claimed = await client.call("browser_claim_chrome", { cdp_endpoint: endpoint, url_contains: origin });
  assert.equal(claimed.result.claimed, true);
  await assert.rejects(client.call("browser_close_tab", { session_id: claimed.result.sessionId, tab_id: claimed.result.tabId }), /claimed_tab_preserved/);
  await client.call("browser_close_session", { session_id: claimed.result.sessionId });
  const targets = await (await fetch(`${endpoint}/json/list`)).json();
  assert(targets.some((target) => target.url.startsWith(origin)), "claimed user tab was closed");
});

test("stdin shutdown releases a persistent profile for immediate restart", async (context) => {
  const server = await serve();
  const origin = `http://127.0.0.1:${server.address().port}`;
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "workshop-browser-shutdown-test-"));
  context.after(async () => {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(root, { recursive: true, force: true });
  });

  const first = backend(origin, root);
  const created = await first.call("browser_create_session", { profile: "restartable" });
  await first.call("browser_navigate", { session_id: created.result.sessionId, tab_id: created.result.tabId, url: origin });
  first.child.stdin.end();
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("backend did not exit after stdin EOF")), 5_000);
    first.child.once("exit", () => { clearTimeout(timeout); resolve(); });
  });

  const second = backend(origin, root);
  const reopened = await second.call("browser_create_session", { profile: "restartable" });
  await second.call("browser_close_session", { session_id: reopened.result.sessionId });
  second.child.stdin.end();
  await new Promise((resolve) => second.child.once("exit", resolve));
});

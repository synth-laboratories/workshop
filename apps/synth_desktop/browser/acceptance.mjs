#!/usr/bin/env node
// Live smoke check for the two public acceptance pages. It is intentionally
// separate from deterministic CI because those sites and the network are not.
import { spawn, execFileSync } from "node:child_process";
import readline from "node:readline";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";

const profileRoot = fs.mkdtempSync(path.join(os.tmpdir(), "workshop-browser-acceptance-"));
const runtimeRoot = new URL("./runtime", import.meta.url).pathname;
const child = spawn(process.execPath, [new URL("./playwright_backend.mjs", import.meta.url).pathname], {
  env: {
    ...process.env,
    SYNTH_BROWSER_RUNTIME_ROOT: runtimeRoot,
    PLAYWRIGHT_BROWSERS_PATH: path.join(runtimeRoot, "browsers"),
    SYNTH_BROWSER_HEADLESS: process.env.SYNTH_BROWSER_HEADLESS ?? "1",
    SYNTH_BROWSER_ALLOWED_ORIGINS: "https://example.com,https://www.iana.org,https://iana.org,https://www.usesynth.ai",
    SYNTH_BROWSER_PROFILE_ROOT: profileRoot,
  },
  stdio: ["pipe", "pipe", "inherit"],
});
const reader = readline.createInterface({ input: child.stdout });
const pending = new Map();
let nextId = 0;
reader.on("line", (line) => {
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
function call(operation, callArgs = {}) {
  const id = ++nextId;
  child.stdin.write(`${JSON.stringify({ id, operation, arguments: callArgs })}\n`);
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}

function processTreeRssMiB(rootPid) {
  const rows = execFileSync("ps", ["-axo", "pid=,ppid=,rss="], { encoding: "utf8" })
    .trim().split("\n").map((line) => line.trim().split(/\s+/).map(Number));
  const wanted = new Set([rootPid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const [pid, ppid] of rows) if (wanted.has(ppid) && !wanted.has(pid)) { wanted.add(pid); changed = true; }
  }
  return Math.round((rows.filter(([pid]) => wanted.has(pid)).reduce((sum, row) => sum + row[2], 0) / 1024) * 10) / 10;
}

const report = {};
try {
  let started = performance.now();
  const created = await call("browser_create_session", { profile: "acceptance" });
  report.coldStartMs = Math.round(performance.now() - started);
  const session_id = created.result.sessionId;
  const tab_id = created.result.tabId;

  await call("browser_navigate", { session_id, tab_id, url: "https://example.com/" });
  const heading = await call("browser_query", { session_id, tab_id, role: "heading", name: "Example Domain", max_chars: 2_000 });
  if (!heading.result.text.includes("Example Domain")) throw new Error("example.com heading was not found");
  await call("browser_click", { session_id, tab_id, target: { locator: { role: "link", name: "Learn more", exact: true } } });
  await call("browser_back", { session_id, tab_id });
  const back = await call("browser_query", { session_id, tab_id, role: "heading", name: "Example Domain", max_chars: 2_000 });
  if (!back.result.text.includes("Example Domain")) throw new Error("browser back did not return to example.com");
  report.example = { passed: true, snapshotChars: heading.result.text.length };

  started = performance.now();
  await call("browser_navigate", { session_id, tab_id, url: "https://www.usesynth.ai/evals/craftax" });
  report.warmNavigationMs = Math.round(performance.now() - started);
  const craftax = await call("browser_query", { session_id, tab_id, name: "Craftax", max_chars: 4_000 });
  const trajectories = await call("browser_query", { session_id, tab_id, name: "Trajectories", max_chars: 4_000 });
  if (!craftax.result.text.toLowerCase().includes("craftax")) throw new Error("Craftax text was not found");
  if (!trajectories.result.text.toLowerCase().includes("trajectories")) throw new Error("Trajectories text was not found");
  report.craftax = {
    passed: true,
    craftaxChars: craftax.result.text.length,
    trajectoriesChars: trajectories.result.text.length,
    ceiling: 4_000,
    craftaxTruncated: craftax.meta.truncated,
    trajectoriesTruncated: trajectories.meta.truncated,
  };
  const rssKb = Number(execFileSync("ps", ["-o", "rss=", "-p", String(child.pid)], { encoding: "utf8" }).trim());
  report.backendRssMiB = Math.round((rssKb / 1024) * 10) / 10;
  report.browserProcessTreeRssMiB = processTreeRssMiB(child.pid);
  await call("browser_close_session", { session_id });
  console.log(JSON.stringify(report, null, 2));
} finally {
  child.kill("SIGTERM");
  if (child.exitCode === null) await new Promise((resolve) => child.once("exit", resolve));
  fs.rmSync(profileRoot, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}

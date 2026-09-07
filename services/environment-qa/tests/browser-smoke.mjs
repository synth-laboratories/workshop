import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { spawn } from "node:child_process";
import { chromium } from "@playwright/test";

const root = await mkdtemp(join(tmpdir(), "workshop-qa-ui-"));
const task = join(root, "task");
await mkdir(join(task, "tests"), { recursive: true });
await writeFile(join(task, "task.toml"), 'version="1.0"\n');
await writeFile(join(task, "instruction.md"), "Write a program. Compilation byproducts are allowed.");
await writeFile(join(task, "tests/test.sh"), "#!/bin/sh\nexit 0\n");
await writeFile(join(task, "tests/test_outputs.py"), 'import os\ndef test_files():\n    files = os.listdir("/app")\n    assert files == ["main.c"]\n');
const port = 17338;
const origin = `http://127.0.0.1:${port}`;
let service;
async function start() {
  service = spawn("python3", ["-m", "environment_qa", "--store", join(root, "store"), "serve", "--task-root", root, "--port", String(port)],
    {env: {...process.env, PYTHONPATH: resolve("services/environment-qa")}, stdio: ["ignore", "pipe", "pipe"]});
  for (let i = 0; i < 100; i++) {
    try { const r = await fetch(origin + "/health"); if (r.ok) return; } catch {}
    await new Promise(r => setTimeout(r, 100));
  }
  throw new Error("QA service did not start");
}
async function stop() {
  if (!service || service.exitCode !== null) return;
  service.kill("SIGINT");
  await new Promise(r => service.once("exit", r));
}
const browser = await chromium.launch({headless:true});
try {
  await start();
  const page = await browser.newPage({viewport:{width:1100,height:850}});
  const errors=[]; page.on("pageerror", e=>errors.push(e.message));
  await page.goto(origin);
  await page.getByLabel("Task directory").fill(task);
  await page.getByLabel("Decision mode").selectOption("hitl");
  await page.getByRole("button",{name:"Run QA",exact:true}).click();
  await page.getByRole("heading",{name:"Review required"}).waitFor();
  await page.getByRole("button",{name:"tests/test_outputs.py:4",exact:true}).click();
  await page.getByRole("dialog").waitFor();
  assert.match(await page.locator("#source-text").textContent(), /assert files/);
  await page.getByRole("button",{name:"Close",exact:true}).click();
  const runHash = new URL(page.url()).hash;
  await stop();
  await start();
  await page.goto(origin + "/" + runHash);
  await page.getByRole("heading",{name:"Review required"}).waitFor();
  // A second client resolves the exact same durable interaction.
  const other = await browser.newPage();
  other.on("pageerror", e=>errors.push(e.message));
  await other.goto(origin + "/" + runHash);
  await other.getByRole("heading",{name:"Review required"}).waitFor();
  await other.getByLabel("Reason",{exact:true}).fill("UI test: inspect source and confirm the candidate finding.");
  const [decisionResponse] = await Promise.all([
    other.waitForResponse(r => r.url().endsWith("/decision"), {timeout:5000}).catch(async e => { throw new Error(e.message + " UI error: " + await other.locator("#error").textContent() + " JS: " + errors.join(";")); }),
    other.getByRole("button",{name:"Confirm assessment"}).click()
  ]);
  assert.equal(decisionResponse.status(), 200, await decisionResponse.text());
  await page.getByRole("button",{name:"Export sealed result"}).waitFor();
  await page.getByRole("button",{name:"View eval comparison"}).click();
  await page.getByText("No eval receipt yet.",{exact:false}).waitFor();
  await page.getByRole("button",{name:"Review repaired version"}).click();
  assert.match(await page.locator("#parent-label").textContent(), /New snapshot linked/);
  const denied=await fetch(origin+"/api/runs",{method:"POST",headers:{"Content-Type":"application/json",Origin:"https://example.com"},body:"{}"});
  assert.equal(denied.status,403);
  const unauth=await fetch(origin+"/api/runs"); assert.equal(unauth.status,401);
  for(const width of [1100,360]) {
    await page.setViewportSize({width,height:900});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth > innerWidth),false);
  }
  assert.deepEqual(errors,[]);
  console.log("PASS: two-client review, service restart, source evidence, seal, follow-up, access checks, desktop/mobile layout");
} finally {
  await browser.close(); await stop(); await rm(root,{recursive:true,force:true});
}

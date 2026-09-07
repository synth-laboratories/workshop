import assert from "node:assert/strict";
import { mkdtemp, rm, mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";
import { chromium, expect } from "@playwright/test";
import { createServer } from "vite";
import react from "@vitejs/plugin-react";

const root = await mkdtemp(join(tmpdir(), "qa-workshop-hitl-"));
const tasks = resolve(process.env.QA_TBENCH_TASK_ROOT || "../workshop-release/evals/environment-qa-terminal-bench/local-corpus/tasks");
const origin = "http://127.0.0.1:17340";
let service, vite, browser;
async function start() {
  service = spawn(process.env.QA_PYTHON || "python3", ["services/environment-qa/tests/fixtures/seed_hitl.py", root,
    join(tasks,"instance-a"), join(tasks,"instance-b")],
    {env:{...process.env, PYTHONPATH:resolve("services/environment-qa")}, stdio:["ignore","pipe","pipe"]});
  let stderr = ""; service.stderr.on("data", d => stderr += d);
  for(let i=0;i<100;i++) {
    if(service.exitCode !== null) throw Error(`Fixture exited: ${stderr}`);
    try { if((await fetch(origin+"/health")).ok) return; } catch {}
    await new Promise(r=>setTimeout(r,100));
  }
  throw Error(`Fixture failed: ${stderr}`);
}
async function stop() {
  if(!service || service.exitCode !== null) return;
  const exited = new Promise(r=>service.once("exit",r)); service.kill("SIGINT"); await exited;
}
try {
  await start();
  vite = await createServer({configFile:false, root:process.cwd(), plugins:[react()],
    optimizeDeps:{entries:["services/environment-qa/tests/fixtures/workshop-hitl.html"]},
    server:{host:"127.0.0.1", port:17341, strictPort:true}, logLevel:"error"});
  await vite.listen();
  browser = await chromium.launch();
  const page = await browser.newPage({viewport:{width:1280,height:900}});
  const errors=[]; page.on("pageerror",e=>errors.push(e.message));
  await page.goto("http://127.0.0.1:17341/services/environment-qa/tests/fixtures/workshop-hitl.html");
  await page.getByRole("status").filter({hasText:"2 pending decisions"}).waitFor();
  const frame = page.frameLocator('iframe[title="Environment QA review"]');
  assert.equal(await frame.getByLabel("Decision mode").inputValue(),"hitl");
  await expect(frame.locator('select[name="profile_id"]')).toHaveValue('legacy');
  await expect(frame.locator('select[name="profile_id"] option')).toHaveCount(1);
  await frame.getByLabel("Review queue").selectOption("pending");
  assert.equal(await frame.locator(".run").count(),2);
  await frame.getByRole("heading",{name:"Review required",exact:true}).waitFor();
  await frame.getByLabel("Reason",{exact:true}).fill("UNSUBMITTED TEST DRAFT: not human adjudication");
  const first = await frame.locator('.run[aria-pressed="true"]').getAttribute("data-id");
  await frame.locator(`.run:not([data-id="${first}"])`).click();
  await expect(frame.getByLabel("Reason",{exact:true})).toHaveValue("");
  await frame.locator(`[data-id="${first}"]`).click();
  await expect(frame.getByLabel("Reason",{exact:true})).toHaveValue("UNSUBMITTED TEST DRAFT: not human adjudication");
  // Real service restart: auth-token refresh and local draft survival.
  await stop(); await start();
  await page.getByRole("button",{name:"Reconnect",exact:true}).click();
  await page.getByRole("status").filter({hasText:"2 pending decisions"}).waitFor();
  await frame.locator(`[data-id="${first}"]`).click();
  await expect(frame.getByLabel("Reason",{exact:true})).toHaveValue("UNSUBMITTED TEST DRAFT: not human adjudication");
  // Test-only automated decision on the disposable store, never a real human vote.
  await frame.getByLabel("Reason",{exact:true}).fill("AUTOMATED UI TEST ONLY: request evidence; this is not human adjudication.");
  const [decision] = await Promise.all([page.waitForResponse(r=>r.url().endsWith("/decision")),
    frame.getByRole("button",{name:"More evidence needed",exact:true}).click()]);
  assert.equal(decision.status(),200);
  await frame.getByRole("button",{name:"Export sealed result"}).waitFor();
  await page.getByRole("status").filter({hasText:"1 pending decision"}).waitFor();
  await frame.getByText("Decision history · 1",{exact:true}).click();
  await frame.getByText("AUTOMATED UI TEST ONLY: request evidence; this is not human adjudication.",{exact:true}).waitFor();
  if(process.env.QA_TEST_ARTIFACT_DIR) {
    await mkdir(process.env.QA_TEST_ARTIFACT_DIR,{recursive:true});
    await page.screenshot({path:join(process.env.QA_TEST_ARTIFACT_DIR,"workshop-hitl.png"),fullPage:true});
  }
  assert.deepEqual(errors,[]);
  console.log("PASS: actual Workshop component + two real TBench source-check fixtures; pending queue, HITL default, no direct-AI fallback, per-context drafts, restart, test-only decision and sealed history. No Codex/provider calls; not live human adjudication.");
} finally {
  await browser?.close(); await vite?.close(); await stop(); await rm(root,{recursive:true,force:true});
}

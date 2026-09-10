import assert from "node:assert/strict";
import {mkdtemp, rm} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join, resolve} from "node:path";
import {spawn} from "node:child_process";
import {chromium} from "@playwright/test";

const root = await mkdtemp(join(tmpdir(),"qa-dag-browser-"));
const program = `
import sys
from pathlib import Path
from environment_qa.core import Store
from environment_qa.bundles import export_bundle
from environment_qa.policy import full_policy
from environment_qa.server import serve
import environment_qa.executors as executors
root=Path(sys.argv[1]); task=root/'task';task.mkdir(exist_ok=True)
(task/'instruction.md').write_text('Synthetic browser fixture, not historical QA evidence')
(task/'task.toml').write_text('version="1"')
store=Store(root/'store');bundle=export_bundle(task,store.root,[root])
store.create(bundle,mode='hitl',reviewer='ai',pipeline=full_policy(),request_key='fixture')
def fake(store,run,gate,path):
 return {'interaction':True} if gate['executor']=='interaction' else {'findings':[],'limitations':[]}
executors.execute_gate=fake
serve(root/'store',[root],17339)
`;
let service;
async function start(){
  service=spawn("python3",["-c",program,root],{env:{...process.env,PYTHONPATH:resolve("services/environment-qa")},stdio:"pipe"});
  for(let n=0;n<100;n++){
    try{if((await fetch("http://127.0.0.1:17339/health")).ok)return;}catch{}
    await new Promise(r=>setTimeout(r,100));
  }
  throw Error("Fixture service failed to start");
}
async function stop(){service.kill("SIGINT");await new Promise(r=>service.once("exit",r));}
const browser=await chromium.launch(process.env.QA_BROWSER_EXECUTABLE ? {executablePath:process.env.QA_BROWSER_EXECUTABLE} : {});
try{
 await start();const page=await browser.newPage();const errors=[];page.on("pageerror",e=>errors.push(e.message));
 await page.goto("http://127.0.0.1:17339/");
 await page.getByRole("heading",{name:"Review required"}).waitFor();
 assert.equal(await page.locator(".stage").count(),26);
 await stop();await start();await page.reload();
 for(let i=0;i<4;i++){
  await page.getByRole("heading",{name:"Review required"}).waitFor();
  await page.getByLabel("Reason",{exact:true}).fill("Synthetic UI test approval; not a real task decision");
  const [response]=await Promise.all([page.waitForResponse(r=>r.url().endsWith("/decision")),page.getByRole("button",{name:"Confirm assessment"}).click()]);
  assert.equal(response.status(),200);
  await new Promise(r=>setTimeout(r,800));
 }
 await page.getByRole("button",{name:"Export sealed result"}).waitFor();
 assert.deepEqual(errors,[]);
 console.log("PASS: full DAG UI, four distinct persisted interaction gates, restart and terminal seal (synthetic executors, no paid calls)");
}finally{await browser.close();if(service?.exitCode===null)await stop();await rm(root,{recursive:true,force:true});}

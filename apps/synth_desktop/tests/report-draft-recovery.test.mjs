import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { chromium } from "playwright";

// Exercise the real Report editor with an in-memory bridge. No native app,
// credentials, network provider, or published Report is involved.
test("Report drafts survive navigation/reload and asynchronous saves", async () => {
  const root = fileURLToPath(new URL("../../..", import.meta.url));
  const result = await build({
    absWorkingDir: root, bundle: true, write: false, format: "iife", jsx: "automatic",
    stdin: { resolveDir: root, loader: "tsx", contents: `
      import React from 'react';
      import { createRoot } from 'react-dom/client';
      import { ReportsPage } from './apps/synth_desktop/src/renderer/src/components/ReportsPage';
      const records = ['a','b'].map(id => ({ id, title: 'Report '+id, currentRevision: 1, status: 'draft' }));
      const revisions = Object.fromEntries(records.map(r => [r.id, {
        reportId:r.id, revision:1, schemaVersion:'report.v1', title:r.title, summary:'',
        blocks:[{blockId:'findings',anchor:'findings',kind:'report.prose.v1',title:'Findings',payload:{markdown:''}},
                {blockId:'methods',anchor:'methods',kind:'report.prose.v1',title:'Methods',payload:{markdown:''}}],
        claims:[], limitations:[]
      }]));
      window.fixture = { wait:false, fail:false, seals:0, updates:0, resolveWait:false, resolveCount:0, catalog:[], inventory:{
        listTraces:async()=>window.fixture.catalog,
        resolveTraceProjection:async digest=>{
          window.fixture.resolveCount++;
          if(window.fixture.resolveWait) await new Promise(resolve=>window.fixture.finishResolve=resolve);
          return {traceDigest:digest,payload:{items:[]}};
        }
      }, bridge:{
        list:async()=>records, listSeals:async()=>[], getRevision:async id=>structuredClone(revisions[id]),
        listExperiments:async()=>[], listLog:async()=>[], listVisibilityRequests:async()=>[],
        validate:async()=>({sealable:true,findings:[]}), listComments:async()=>[], onEvent:()=>()=>{},
        update:async(id,input)=>{
          window.fixture.updates++;
          if(window.fixture.fail) throw new Error('save failed');
          if(window.fixture.wait) await new Promise(resolve=>window.fixture.finish=resolve);
          if(input.expectedRevision!==revisions[id].revision) throw new Error('revision conflict');
          Object.assign(revisions[id],input,{revision:revisions[id].revision+1});
          const row=records.find(r=>r.id===id); row.currentRevision=revisions[id].revision;
          if(input.title!==undefined) row.title=input.title;
          return {...row};
        }, seal:async()=>{window.fixture.seals++; throw new Error('seal should not be reached');}
      }};
      createRoot(document.getElementById('root')).render(<ReportsPage onBack={()=>{}} initialReportId="a"/>);
    ` },
    plugins: [{ name: "local-fixtures", setup(builder) {
      builder.onResolve({ filter: /desktopBridge$|DocumentContent$|^@synth\/visual-templates\// }, args => ({ path: args.path, namespace: "fixture" }));
      builder.onLoad({ filter: /.*/, namespace: "fixture" }, args => ({ contents:
        args.path.endsWith("desktopBridge") ? "export const bridges={get reports(){return window.fixture.bridge},get inventory(){return window.fixture.inventory}};" :
        args.path.endsWith("DocumentContent") ? "export const Markdown=()=>null;" : "import React from 'react'; export default ()=>React.createElement('p',{'data-testid':'trace-preview'},'Trace preview');", loader: "js", resolveDir: root }));
    }}]
  });
  const server = createServer((req, res) => {
    res.setHeader("Content-Type", req.url === "/bundle.js" ? "text/javascript" : "text/html");
    res.end(req.url === "/bundle.js" ? result.outputFiles[0].text : '<div id="root"></div><script src="/bundle.js"></script>');
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  let browser;
  try {
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage();
    await page.route("**/*", route => new URL(route.request().url()).hostname === "127.0.0.1" ? route.continue() : route.abort());
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const findings = page.getByTestId("reports-findings");
    const selectReport = id => page.getByTestId("reports-grid").getByRole("button").filter({hasText:`Report ${id}`}).click();
    await findings.fill("retained draft");
    await selectReport("b");
    await selectReport("a");
    assert.equal(await findings.inputValue(), "retained draft");
    await page.reload();
    await findings.waitFor();
    assert.equal(await findings.inputValue(), "retained draft");

    await page.evaluate(() => { window.fixture.wait = true; });
    await page.getByRole("button", { name: "Save draft", exact: true }).click();
    await page.waitForFunction(() => typeof window.fixture.finish === "function");
    await findings.fill("typed while saving");
    await page.evaluate(() => { window.fixture.finish(); });
    await page.waitForFunction(() => document.body.textContent.includes("Unsaved changes") && !document.body.textContent.includes("Saving"));
    assert.equal(await findings.inputValue(), "typed while saving");

    await page.getByRole("button", { name: "Save draft", exact: true }).click();
    await page.waitForFunction(() => window.fixture.updates === 2);
    await selectReport("b");
    await page.evaluate(() => { window.fixture.finish(); });
    await page.waitForTimeout(50);
    assert.equal(await findings.inputValue(), "");
    assert.ok((await page.locator("input").evaluateAll(inputs => inputs.map(input => input.value))).includes("Report b"));

    await selectReport("a");
    await findings.fill("must not seal after failure");
    await page.evaluate(() => { window.fixture.wait = false; window.fixture.fail = true; });
    await page.getByTestId("reports-seal").click();
    await page.getByTestId("reports-error").waitFor();
    assert.equal(await page.evaluate(() => window.fixture.seals), 0);
    assert.equal(await findings.inputValue(), "must not seal after failure");

    // Inventory empty state and picker selection use no provider or native bridge.
    await selectReport("b");
    await page.evaluate(() => { window.fixture.fail = false; });
    await page.getByRole("button", { name: "Choose retained trace" }).click();
    await page.getByText("No retained traces yet.", { exact:false }).waitFor();
    await page.getByRole("button", { name: "Choose retained trace" }).click();
    await page.evaluate(() => { window.fixture.catalog = [{digest:'sha256:trace-one',title:'Retained evidence',createdAt:'2026-09-10'}]; });
    await page.getByRole("button", { name: "Choose retained trace" }).click();
    await page.getByRole("combobox", { name: "Available retained traces" }).selectOption('sha256:trace-one');
    assert.equal(await page.getByLabel("Trace evidence label").inputValue(), "Retained evidence");

    // A pending projection must not attach to the old report after navigation.
    const before = await page.evaluate(() => window.fixture.updates);
    await page.evaluate(() => { window.fixture.resolveWait = true; });
    await page.getByTestId("reports-attach-trace").click();
    await page.waitForFunction(() => typeof window.fixture.finishResolve === 'function');
    assert.equal(await page.getByTestId("reports-attach-trace").isDisabled(), true);
    await selectReport("a");
    await page.evaluate(() => window.fixture.finishResolve());
    await page.waitForTimeout(50);
    assert.equal(await page.evaluate(() => window.fixture.updates), before);
    assert.equal(await findings.inputValue(), "must not seal after failure");

    await selectReport("b");
    await page.evaluate(() => { window.fixture.resolveWait = false; window.fixture.wait = true; });
    await page.getByLabel("Retained trace digest").fill('sha256:trace-one');
    await page.getByTestId("reports-attach-trace").click();
    await page.waitForFunction(count => window.fixture.updates === count + 1, before);
    await findings.fill('edited during attachment');
    await page.evaluate(() => window.fixture.finish());
    await page.getByRole("button", { name: "Inspect attached trace" }).waitFor();
    assert.equal(await findings.inputValue(), 'edited during attachment');
    assert.equal(await page.getByTestId('trace-preview').count(), 0);
    await page.getByRole("button", { name: "Inspect attached trace" }).click();
    await page.getByTestId('trace-preview').waitFor();
    await page.getByRole("button", { name: "Hide attached trace" }).click();
    assert.equal(await page.getByTestId('trace-preview').count(), 0);
    await page.evaluate(() => { window.fixture.wait = false; });
    await page.getByRole("button", { name: "Save draft", exact:true }).click();
    await page.waitForFunction(() => document.body.textContent.includes('Saved · rev'));
    assert.equal(await findings.inputValue(), 'edited during attachment');
  } finally {
    await browser?.close();
    await new Promise(resolve => server.close(resolve));
  }
});

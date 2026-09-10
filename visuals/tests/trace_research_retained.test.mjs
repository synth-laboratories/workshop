import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {build} from 'esbuild';
import {chromium} from 'playwright';

// Real production projection from the native retained-archive test. The scale
// archive itself is explicitly synthetic; no provider output is fabricated.
test('retained long projection renders bounded rows and preserves exact selection', {skip:!process.env.SYNTH_RESEARCH_RETAINED_VISUAL}, async()=>{
 const root=fileURLToPath(new URL('../../',import.meta.url));
 const retained=JSON.parse(await fs.readFile(process.env.SYNTH_RESEARCH_RETAINED_VISUAL,'utf8'));
 const projection=retained.visual ?? retained;
 assert.ok(projection.items.length>=10000);
 const actors=new Set(projection.items.map(i=>i.actor_id).filter(Boolean));assert.ok(actors.size>=4);
 const bundle=await build({stdin:{contents:`import React from 'react';import{createRoot}from'react-dom/client';import{AgentTraceInspector}from'./visuals/components/agent_trace.v1/AgentTraceInspector.tsx';window.started=performance.now();createRoot(document.getElementById('root')).render(<AgentTraceInspector projection={window.projection} onAnnotate={target=>window.annotationTarget=target}/>);`,resolveDir:root,loader:'tsx'},bundle:true,write:false,format:'iife',platform:'browser',jsx:'automatic',define:{'process.env.NODE_ENV':'"production"'}});
 const browser=await chromium.launch();
 try {
  const page=await browser.newPage({viewport:{width:1280,height:900}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent('<div id="root"></div>');await page.evaluate(p=>{window.projection=p;},projection);
  await page.addScriptTag({content:bundle.outputFiles[0].text});
  await page.locator('[data-trace-item-id]').first().waitFor();
  const measurement=await page.evaluate(()=>({firstUsableRenderMs:performance.now()-window.started,renderedRows:document.querySelectorAll('[data-trace-item-id]').length,heapBytes:performance.memory?.usedJSHeapSize??null}));
  assert.ok(measurement.renderedRows<500,'initial DOM must stay bounded');
  const item=page.locator('[data-trace-item-id]').first();const id=await item.getAttribute('data-trace-item-id');
  await item.getByRole('button',{name:'Annotate',exact:true}).click();
  const target=await page.evaluate(()=>window.annotationTarget);const original=projection.items.find(i=>i.item_id===id);
  assert.equal(target.entity_id,original.source_selector.entity_id);assert.equal(target.trace_digest,original.source_selector.trace_digest);
  assert.deepEqual(errors,[]);
  const out=path.dirname(process.env.SYNTH_RESEARCH_RETAINED_VISUAL);
  await page.screenshot({path:path.join(out,'long-visual.png')});
  await fs.writeFile(path.join(out,'long-render-measurement.json'),JSON.stringify({...measurement,projectionItems:projection.items.length,actors:actors.size,status:'passed',scope:'production component in Chromium; not installed native app'},null,2));
 }finally{await browser.close();}
});

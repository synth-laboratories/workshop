import assert from 'node:assert/strict';
import test from 'node:test';
import { build } from 'esbuild';
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';

test('horizontal scrollbar selects and reveals distant trace items in both directions', async () => {
 const projection = {trace_id:'t',trace_digest:'sha',items:Array.from({length:150},(_,i)=>({item_id:`e${i}`,actor_id:'agent',session_id:'session',kind:'model_call.finished',source_selector:{trace_id:'t',trace_digest:'sha',kind:'event',entity_id:`e${i}`},detail:{kind:'policy.call',call_index:i,prompt:`Observation ${i}`,reply:`THOUGHT: Thought ${i}.\nACTIONS: do`}}))};
 const root = fileURLToPath(new URL('../../', import.meta.url));
 const bundle = await build({stdin:{contents:`import React from 'react';import {createRoot} from 'react-dom/client';import {AgentTraceInspector,craftaxTraceExtension} from './visuals/components/agent_trace.v1/AgentTraceInspector.tsx';createRoot(document.getElementById('root')).render(<AgentTraceInspector projection={${JSON.stringify(projection)}} initialView="craftax" extensions={[craftaxTraceExtension]} onAnnotate={t=>window.target=t}/>);`,resolveDir:root,loader:'tsx'},bundle:true,write:false,format:'iife',platform:'browser',jsx:'automatic'});
 const browser = await chromium.launch({headless:true});
 try {
  const page = await browser.newPage();await page.setContent('<div id="root"></div>');await page.addScriptTag({content:bundle.outputFiles[0].text});
  const bar=page.getByRole('navigation',{name:'Trace event timeline'});
  for (const [position,id] of [['end','e149'],['start','e0']]) {
    await bar.evaluate((node,position)=>{node.scrollLeft=position==='end'?node.scrollWidth:0;},position);
    await page.locator(`[data-trace-item-id="${id}"][aria-current="true"]`).waitFor();
    assert.equal(await bar.locator(`[data-marker-id="${id}"][aria-current="true"]`).count(),1);
    const visible=await page.locator(`[data-trace-item-id="${id}"]`).evaluate(node=>{const p=node.closest('.atv-transcript').getBoundingClientRect();const r=node.getBoundingClientRect();return r.top>=p.top&&r.top<p.bottom;});
    assert.ok(visible,`${id} should be visible after horizontal scroll`);
  }
 } finally {await browser.close();}
});

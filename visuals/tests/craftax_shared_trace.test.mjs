import assert from 'node:assert/strict';
import test from 'node:test';
import { build } from 'esbuild';
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';

test('Craftax decisions isolate interleaved actors and keep annotated hidden records accessible', async () => {
 const target = id => ({trace_id:'t',trace_digest:'sha',kind:'event',entity_id:id});
 const row = (id,actor,kind,detail) => ({item_id:id,actor_id:actor,session_id:actor,kind,source_selector:target(id),detail});
 const projection = {trace_id:'t',trace_digest:'sha',items:[
  row('a','alice','model_call.finished',{kind:'policy.call',call_index:1,prompt:'Alice observation',reply:'THOUGHT: Gather wood.\nACTIONS: do, do'}),
  row('b','bob','model_call.finished',{kind:'policy.call',call_index:1,prompt:'Bob observation',reply:'THOUGHT: Find water.\nACTIONS: left'}),
  row('ar','alice','environment.action_executed',{action:'do',step_index:1,transition:'harvest'}),
  row('br','bob','environment.action_executed',{action:'left',step_index:1,transition:'move'}),
  row('hidden','alice','craftax.transcript',{message:'Retained plumbing'}),
  {item_id:'note',kind:'evidence.annotation',source_selector:target('hidden'),detail:{rationale:'Review source',labels:['note'],review_state:'accepted'}},
 ]};
 const root = fileURLToPath(new URL('../../', import.meta.url));
 const bundle = await build({stdin:{contents:`import React from 'react';import {createRoot} from 'react-dom/client';import {AgentTraceInspector,craftaxTraceExtension} from './visuals/components/agent_trace.v1/AgentTraceInspector.tsx';createRoot(document.getElementById('root')).render(<AgentTraceInspector projection={${JSON.stringify(projection)}} initialView="craftax" extensions={[craftaxTraceExtension]} onAnnotate={t=>window.target=t}/>);`,resolveDir:root,loader:'tsx'},bundle:true,write:false,format:'iife',platform:'browser',jsx:'automatic'});
 const browser = await chromium.launch({headless:true});
 try {
  const page = await browser.newPage();await page.setContent('<div id="root"></div>');await page.addScriptTag({content:bundle.outputFiles[0].text});
  const alice=page.locator('.atv-turn').filter({has:page.locator('[data-trace-item-id="a"]')});
  assert.equal(await alice.locator('[data-trace-item-id="ar"]').count(),1);
  assert.equal(await alice.locator('[data-trace-item-id="br"]').count(),0);
  assert.equal(await page.getByText('Gather wood.',{exact:true}).count(),1);
  assert.equal(await page.locator('[data-trace-item-id="hidden"]').count(),1);
  await page.locator('[data-trace-item-id="a"]').getByRole('button',{name:'Annotate',exact:true}).click();
  assert.deepEqual(await page.evaluate(()=>window.target),target('a'));
  await page.getByLabel('Trace presentation').selectOption('general');
  assert.equal(await page.locator('[data-trace-item-id="hidden"]').count(),1);
 } finally {await browser.close();}
});

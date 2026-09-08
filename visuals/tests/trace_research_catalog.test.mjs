import assert from 'node:assert/strict';
import test from 'node:test';
import { build } from 'esbuild';
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';

test('research catalog pages 450 results, preserves selectors, and renders aggregates without traces', async () => {
 const root = fileURLToPath(new URL('../../', import.meta.url));
 const snapshot = {snapshotId:'saved',querySchemaVersion:'synth.trace-query.v2',resultCount:450,resultDigest:'digest',resultIds:Array.from({length:450},(_,i)=>`r${i}`),facets:{rows:Array.from({length:450},(_,i)=>({trialId:`trial-${i}`,reward:i===0?0:null,traceDigest:`trace-${i}`,traceAvailability:i===0?'unavailable':'available',selector:{trace_id:`trace-${i}`,kind:'event',entity_id:`e${i}`}}))}};
 const bundle = await build({stdin:{contents:`import React from 'react';import {createRoot} from 'react-dom/client';import Shell from './visuals/families/analysis/trace.catalog.v1/shell.tsx';const root=createRoot(document.getElementById('root'));window.render=(data)=>root.render(<Shell data={data}/>);`,resolveDir:root,loader:'tsx'},bundle:true,write:false,outfile:'research-test.js',format:'iife',platform:'browser',jsx:'automatic'});
 const browser = await chromium.launch({headless:true});
 try {
  const page = await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.route('http://catalog.test/',route=>route.fulfill({body:'<div id="root"></div>',contentType:'text/html'}));await page.goto('http://catalog.test/');await page.addStyleTag({content:bundle.outputFiles.find(f=>f.path.endsWith('.css')).text});await page.addScriptTag({content:bundle.outputFiles.find(f=>f.path.endsWith('.js')).text});await page.evaluate(snapshot=>window.render(snapshot),snapshot);
  await page.getByRole('cell',{name:'trial-0',exact:true}).waitFor();
  assert.equal(await page.locator('tbody tr').count(),50);
  assert.equal(await page.locator('tbody tr').nth(1).getByRole('button',{name:'Open trace',exact:true}).getAttribute('data-reference-value'),'trace-1');
  assert.equal(await page.locator('tbody tr').nth(1).getByRole('button',{name:'Open trace',exact:true}).getAttribute('data-reference-kind'),'trace');
  assert.equal(await page.locator('tbody tr').first().getByRole('button',{name:'Open trace',exact:true}).count(),0);
  assert.equal(await page.locator('tbody tr').first().getByRole('cell',{name:'0',exact:true}).count(),1);
  for(let i=0;i<8;i++) {await page.getByRole('button',{name:'Next',exact:true}).click();await page.getByRole('cell',{name:`trial-${(i+1)*50}`,exact:true}).waitFor();}
  await page.getByRole('cell',{name:'trial-449',exact:true}).waitFor();
  assert.equal(await page.locator('tbody tr').count(),50);
  assert.equal(await page.getByRole('button',{name:'Next',exact:true}).isDisabled(),true);
  await page.locator('tbody tr').last().getByRole('button',{name:'Inspect result'}).click();
  assert.match(await page.getByRole('region',{name:'Selected evidence'}).innerText(),/e449/);
  await page.evaluate(()=>window.render({snapshotId:'aggregate',querySchemaVersion:'synth.trace-query.v2',resultCount:1,resultIds:['aggregate-1'],facets:{rows:[{rewardMean:null,missingCount:2,measuredCount:0}]}}));
  await page.getByRole('cell',{name:'2',exact:true}).waitFor();
  assert.equal(await page.getByRole('button',{name:'Open trace',exact:true}).count(),0);
  await page.evaluate(snapshot=>window.render(snapshot),snapshot);
  await page.getByRole('cell',{name:'trial-449',exact:true}).waitFor();
  assert.match(await page.getByRole('region',{name:'Selected evidence'}).innerText(),/e449/);
  assert.deepEqual(errors,[]);
 } finally {await browser.close();}
});

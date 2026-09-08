import assert from 'node:assert/strict';
import test from 'node:test';
import {build} from 'esbuild';
import {chromium} from 'playwright';
import {fileURLToPath} from 'node:url';

test('inspector requests pinned bounded windows and rejects identity drift',async()=>{
 const root=fileURLToPath(new URL('../../',import.meta.url));
 const bundle=await build({stdin:{contents:`import React from 'react';import{createRoot}from'react-dom/client';import Shell from './visuals/families/analysis/trace.rollout_inspector.v1/shell.tsx';
 const packet=(offset)=>({schema_version:'synth.trace-projection.rollout-inspector-window.v1',trace_id:'trace',trace_digest:'sha256:trace',annotation_view:{records:[{id:'note',target:{trace_id:'trace',trace_digest:'sha256:trace',kind:'event',entity_id:'event-0'},body:'Independent finding',labels:['recorded'],author:'ordinary',reviewState:'accepted',evidence:[]}],truncated:false,scope:'pinned'},view_window:{snapshotDigest:'sha256:snapshot',sourceProjectionDigest:'sha256:projection',offset,limit:200,total:402,nextOffset:offset+200<402?offset+200:null},visual:{state:'sealed',lanes:[],items:Array.from({length:Math.min(200,402-offset)},(_,i)=>({item_id:'event-'+(offset+i),kind:'action',title:'action '+(offset+i),source_selector:{trace_digest:'sha256:trace',kind:'event',entity_id:'event-'+(offset+i)}}))}});
 window.calls=[];window.drift=false;const client={request:async(op,args)=>{window.calls.push({op,args});const next=packet(args.offset);if(window.drift)next.view_window.snapshotDigest='other';return next;}};
 createRoot(document.getElementById('root')).render(<Shell data={packet(0)} traceResearch={client}/>);`,resolveDir:root,loader:'tsx'},bundle:true,write:false,format:'iife',platform:'browser',jsx:'automatic',outfile:'window.js'});
 const browser=await chromium.launch();try {
  const page=await browser.newPage();await page.setContent('<div id="root"></div>');
  const css=bundle.outputFiles.find(f=>f.path.endsWith('.css'));if(css)await page.addStyleTag({content:css.text});
  await page.addScriptTag({content:bundle.outputFiles.find(f=>f.path.endsWith('.js')).text});
  await page.getByText(/Showing events 1–200 of 402/).waitFor();
  assert.deepEqual(await page.evaluate(()=>window.calls),[]);
  await page.getByText(/1 current annotations/).waitFor();
  await page.getByText(/Independent annotations are pinned/).waitFor();
  await page.getByRole('button',{name:'Next window',exact:true}).click();await page.getByText(/Showing events 201–400 of 402/).waitFor();
  assert.deepEqual((await page.evaluate(()=>window.calls))[0],{op:'window',args:{trace_digest:'sha256:trace',snapshot_digest:'sha256:snapshot',offset:200,limit:200}});
  await page.evaluate(()=>window.drift=true);await page.getByRole('button',{name:'Next window',exact:true}).click();await page.getByRole('alert').filter({hasText:'pinned source'}).waitFor();
  await page.getByText(/Showing events 201–400 of 402/).waitFor();
  await page.evaluate(()=>window.drift=false);await page.getByRole('button',{name:'Next window',exact:true}).click();await page.getByText(/Showing events 401–402 of 402/).waitFor();assert.equal(await page.getByRole('button',{name:'Next window',exact:true}).isDisabled(),true);
  await page.getByRole('button',{name:'Previous window',exact:true}).click();await page.getByText(/Showing events 201–400 of 402/).waitFor();
 } finally {await browser.close();}
});

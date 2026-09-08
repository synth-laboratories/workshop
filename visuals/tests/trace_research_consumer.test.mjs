import assert from 'node:assert/strict';
import test from 'node:test';
import {build} from 'esbuild';
import {chromium} from 'playwright';
import {fileURLToPath} from 'node:url';

test('query consumer pins pages, reads exact evidence, prepares without compute, and restores snapshot',async()=>{
 const root=fileURLToPath(new URL('../../',import.meta.url));
 const bundle=await build({stdin:{contents:`import React from 'react';import{createRoot}from'react-dom/client';import{TraceResearchPanel}from'./visuals/components/agent_trace.v1/TraceResearch.tsx';
 const row={jobId:'existing-job',trialId:'episode',actorId:'actor-a',traceDigest:'sha256:trace',traceAvailability:'available',selector:{trace_id:'trace',trace_digest:'sha256:trace',kind:'event',entity_id:'event-a'}};
 window.calls=[];const client={request:async(op,args)=>{window.calls.push({op,args});if(op==='query'||op==='page'){const offset=args.offset||0;return{snapshotId:window.drift?'wrong':'snapshot-a',resultDigest:'sha256:result',resultCount:51,rows:[row],resultIds:[offset?'row-51':'row-1'],nextOffset:offset?null:50};}if(op==='source')return{resolved:true,resolved_text:args.offset?'second':'first',textDigest:'sha256:body',nextOffset:args.offset?null:5};if(op==='prepare_annotations')return{startsCompute:false,targets:[{trace_digest:'sha256:trace',selectors:[row.selector]}]};throw Error('unsupported');}};
 createRoot(document.getElementById('root')).render(<TraceResearchPanel client={client} jobIds={['existing-job']} stateKey="test" onInspect={s=>window.selection=s}/>);`,resolveDir:root,loader:'tsx'},bundle:true,write:false,format:'iife',platform:'browser',jsx:'automatic'});
 const browser=await chromium.launch();try{
  const p=await browser.newPage();await p.route('http://trace-review.test/',route=>route.fulfill({body:'<div id="root"></div>',contentType:'text/html'}));await p.goto('http://trace-review.test/');await p.addScriptTag({content:bundle.outputFiles[0].text});
  assert.deepEqual(await p.evaluate(()=>window.calls),[]);
  await p.getByRole('button',{name:'Run query / refresh'}).click();await p.getByRole('button',{name:'Read exact source'}).click();await p.getByText('first',{exact:true}).waitFor();await p.getByRole('button',{name:'Read next source page'}).click();await p.getByText('firstsecond',{exact:true}).waitFor();await p.getByRole('button',{name:'Open in replay'}).click();assert.equal(await p.evaluate(()=>window.selection.selector.entity_id),'event-a');
  await p.getByLabel('Select result 1').check();await p.getByRole('button',{name:'Prepare annotations (1)'}).click();await p.getByText('Annotation preparation · no compute started').waitFor();
  const prep=await p.evaluate(()=>window.calls.find(c=>c.op==='prepare_annotations'));assert.deepEqual(prep.args,{snapshot_id:'snapshot-a',result_ids:['row-1']});
  await p.evaluate(()=>window.drift=true);await p.getByRole('button',{name:'Next page'}).click();await p.getByRole('alert').filter({hasText:'changed snapshot'}).waitFor();
  await p.evaluate(()=>window.drift=false);await p.getByRole('button',{name:'Next page'}).click();await p.getByText('51 results · showing 51–51').waitFor();
  await p.reload();await p.addScriptTag({content:bundle.outputFiles[0].text});await p.getByText('51 results · showing 51–51').waitFor();assert.deepEqual((await p.evaluate(()=>window.calls))[0],{op:'page',args:{snapshot_id:'snapshot-a',offset:50,limit:50}});
 }finally{await browser.close();}
});

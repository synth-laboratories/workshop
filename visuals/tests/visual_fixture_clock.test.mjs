import assert from 'node:assert/strict';
import test from 'node:test';
import {build} from 'esbuild';
import {chromium} from 'playwright';
import {fileURLToPath} from 'node:url';
import {retainedStreamInput} from '../../packages/workshop-visuals/runtime/retainedStreamInput.ts';

test('multiple retained stream bindings preserve events without inventing shared metadata',()=>{
 const first={run_id:'a',scope:{rollout_ids:['a']},events:[{kind:'trace.opened',run_id:'a',sequence:1,payload:{}}]};
 const second={run_id:'b',events:[{kind:'trace.opened',run_id:'b',sequence:1,payload:{}}]};
 assert.equal(retainedStreamInput([first]),first);
 assert.deepEqual(retainedStreamInput([first,second]),{events:[...first.events,...second.events]});
 assert.deepEqual(retainedStreamInput(undefined),{});
});

test('hosted finite fixtures expose one complete cut while unhosted demos animate arrival',async()=>{
 const root=fileURLToPath(new URL('../../',import.meta.url));
 const bundle=await build({stdin:{resolveDir:root,loader:'tsx',contents:String.raw`
 import React from 'react';import {createRoot} from 'react-dom/client';
 import {createVisualClient,initialSession} from './packages/visuals-sdk/src/index.ts';
 import {VisualSessionProvider} from './packages/visuals-react/src/session.ts';
 import {useLiveEvalStream} from './packages/workshop-visuals/chrome/useLiveEvalStream.ts';
 const fixture=[{kind:'run.started',run_id:'fixture',sequence:1,payload:{}},{kind:'run.finished',run_id:'fixture',sequence:2,payload:{}}];
 const identity={visualId:'fixture-clock',revision:1,viewKey:'default'};
 const client=createVisualClient(identity,{id:'fixture',version:'1'},{request:async()=>({state:initialSession(identity,{id:'fixture',version:'1'})})});
 function Pane({id}){const cut=useLiveEvalStream({fixtureEvents:fixture,replayMs:150});return <p id={id}>{cut.state}:{cut.events.length}</p>;}
 createRoot(document.getElementById('root')).render(<><VisualSessionProvider client={client}><Pane id="first"/><Pane id="second"/></VisualSessionProvider><Pane id="demo"/></>);
 `},bundle:true,write:false,format:'iife',jsx:'automatic'});
 const browser=await chromium.launch();try{
  const page=await browser.newPage();await page.setContent('<div id="root"></div>');await page.addScriptTag({content:bundle.outputFiles[0].text});
  await page.locator('#first').waitFor();
  assert.equal(await page.locator('#first').textContent(),'terminal:2');
  assert.equal(await page.locator('#second').textContent(),'terminal:2');
  await page.waitForFunction(()=>document.getElementById('demo')?.textContent==='terminal:2');
  assert.equal(await page.locator('#first').textContent(),'terminal:2');
 }finally{await browser.close();}
});

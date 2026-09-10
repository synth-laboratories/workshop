import test from 'node:test';
import assert from 'node:assert/strict';
import {build} from 'esbuild';
import {chromium} from 'playwright';
const built=await build({entryPoints:[new URL('../src/renderer/src/visuals/templateEvidencePages.ts',import.meta.url).pathname],bundle:true,write:false,platform:'node',format:'esm'});
const {retainTemplatePages,restoreTemplatePages}=await import(`data:text/javascript;base64,${Buffer.from(built.outputFiles[0].text).toString('base64')}`);
const digest=async value=>'sha256:'+Buffer.from(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(JSON.stringify(['string',value])))).toString('hex');
function store(){
 const blobs=new Map();let reads=0;
 return {blobs,get reads(){return reads;},async request(r){
  if(r.operation==='evidence.put'){
   assert.ok(Buffer.byteLength(JSON.stringify(r.value))<=1_500_000);
   const hash=await digest(r.value);blobs.set(hash,r.value);return {digest:hash};
  }
  assert.equal(r.operation,'evidence.read');reads++;return {value:blobs.get(r.digest)};
 }};
}
test('large retained trace props round-trip through bounded immutable pages',async()=>{
 const backend=store();const original={events:Array.from({length:600},(_,n)=>({n,payload:'\\"\n🧪'.repeat(900)})),reward:0.6};
 const manifest=await retainTemplatePages(original,backend.request);
 assert.ok(manifest.pages.length>1);
 assert.deepEqual(await restoreTemplatePages(manifest,backend.request),original);
 assert.equal(backend.reads,manifest.pages.length);
 // Restoring the old manifest remains independent of a newer live payload.
 await retainTemplatePages({events:[],reward:0},backend.request);
 assert.deepEqual(await restoreTemplatePages(manifest,backend.request),original);
});
test('missing, corrupt, reordered, and oversized evidence fail closed',async()=>{
 const backend=store();const manifest=await retainTemplatePages({events:['a'.repeat(130000),'b'.repeat(130000)]},backend.request);
 await assert.rejects(restoreTemplatePages({...manifest,pages:[...manifest.pages].reverse()},backend.request));
 backend.blobs.set(manifest.pages[0],'corrupt');
 await assert.rejects(restoreTemplatePages(manifest,backend.request),/missing or corrupt/);
 backend.blobs.delete(manifest.pages[0]);
 await assert.rejects(restoreTemplatePages(manifest,backend.request),/missing or corrupt/);
 await assert.rejects(retainTemplatePages('x'.repeat(16_000_001),backend.request),/exceeds 16 MB/);
 await assert.rejects(restoreTemplatePages({...manifest,pages:Array(135).fill(manifest.pages[0])},backend.request),/Invalid/);
});
test('legacy direct checkpoints remain readable and forged writes are rejected',async()=>{
 assert.deepEqual(await restoreTemplatePages({events:[1]},()=>{throw Error('unexpected read');}),{events:[1]});
 await assert.rejects(retainTemplatePages({events:[1]},async()=>({digest:'sha256:wrong'})),/disagrees/);
});

test('hosted template pins large props and replays the old cut without source writes',async()=>{
 const root=new URL('../../../',import.meta.url).pathname;
 const bundle=await build({absWorkingDir:root,stdin:{resolveDir:root,loader:'tsx',contents:`
 import React,{useState} from 'react';import {createRoot} from 'react-dom/client';
 import {VisualSession,VisualSessionClient,initialSession,canonicalDigest,checkpoint} from '@synth/visuals-sdk';
 import {VisualSessionProvider} from '@synth/visuals-react';
 import {useTemplateEvidence} from './apps/synth_desktop/src/renderer/src/visuals/useTemplateEvidence';
 const s=new VisualSession(initialSession({visualId:'test',revision:1,viewKey:'default'},{id:'test',version:'1'}));const blobs=new Map();window.writes=0;
 const client=new VisualSessionClient(s.state,{evidenceCuts:true,async request(r){
  if(r.operation==='attach'){for(const c of r.controls??[])s.register(c,r.defaults[c.id]);return {state:s.state};}
  if(r.operation==='evidence.put'){if(new TextEncoder().encode(JSON.stringify(r.value)).length>1500000)throw Error('oversize');window.writes++;const digest=await canonicalDigest(r.value);blobs.set(digest,r.value);return {digest};}
  if(r.operation==='evidence.read')return {value:blobs.get(r.digest)};
  if(r.operation==='act')return s.execute(r.action);return {state:s.state};
 }});
 let saved;window.save=async()=>{saved=await checkpoint(s.state);};window.restore=async()=>{await s.restore(saved);await client.sync();};
 function App(){const [n,setN]=useState(1);window.change=()=>setN(2);const cut=useTemplateEvidence({n,payload:'x'.repeat(2000000)});return <output data-ready={cut.ready}>{cut.error??(cut.ready?cut.value.n:'loading')}</output>;}
 createRoot(document.getElementById('root')).render(<VisualSessionProvider client={client}><App/></VisualSessionProvider>);
 `},bundle:true,write:false,format:'iife',jsx:'automatic'});
 const browser=await chromium.launch();try{
  const page=await browser.newPage();await page.route('http://localhost/**',route=>route.fulfill({contentType:'text/html',body:'<div id="root"></div>'}));await page.goto('http://localhost/');await page.addScriptTag({content:bundle.outputFiles[0].text});
  await page.waitForFunction(()=>document.querySelector('output')?.textContent==='1');
  await page.evaluate(()=>window.save());await page.evaluate(()=>window.change());
  await page.waitForFunction(()=>document.querySelector('output')?.textContent==='2');
  const writes=await page.evaluate(()=>window.writes);await page.evaluate(()=>window.restore());
  await page.waitForFunction(()=>document.querySelector('output')?.textContent==='1');
  assert.equal(await page.evaluate(()=>window.writes),writes);
 }finally{await browser.close();}
});

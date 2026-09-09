import test from 'node:test';
import assert from 'node:assert/strict';
import {build} from 'esbuild';
import {chromium} from 'playwright';
import {fileURLToPath} from 'node:url';
import {readFileSync} from 'node:fs';

test('managed runtime registers after retained empty payload and restores sandbox controls',async()=>{
 const root=fileURLToPath(new URL('../../',import.meta.url));
 const source=readFileSync(new URL('./fixtures/accept.managed-session.v1/renderer.html',import.meta.url),'utf8');
 const bundle=await build({absWorkingDir:root,stdin:{resolveDir:root,loader:'tsx',contents:`
 import React from 'react';import {createRoot} from 'react-dom/client';
 import {VisualSession,VisualSessionClient,initialSession,canonicalDigest,checkpoint} from '@synth/visuals-sdk';
 import {VisualSessionProvider} from '@synth/visuals-react';
 import {ManagedHtmlFrame} from '@synth/workshop-visuals/components/ManagedHtmlFrame.tsx';
 const s=new VisualSession(initialSession({visualId:'managed',revision:1,viewKey:'default'},{id:'managed',version:'1'}));const blobs=new Map();
 const client=new VisualSessionClient(s.state,{evidenceCuts:true,async request(r){
  if(r.operation==='attach'){for(const c of r.controls??[])s.register(c,r.defaults[c.id]);return {state:s.state};}
  if(r.operation==='evidence.put'){const digest=await canonicalDigest(r.value);blobs.set(digest,r.value);return {digest};}
  if(r.operation==='evidence.read')return {value:blobs.get(r.digest)};
  if(r.operation==='act')return s.execute(r.action);return {state:s.state};
 }});
 let saved;window.save=async()=>{saved=await checkpoint(s.state);};window.restore=async()=>{await s.restore(saved);await client.sync();};
 createRoot(document.getElementById('root')).render(<VisualSessionProvider client={client}><ManagedHtmlFrame source={${JSON.stringify(source)}} payload={undefined} formatError={String}/></VisualSessionProvider>);
 `},bundle:true,write:false,format:'iife',jsx:'automatic'});
 const browser=await chromium.launch();try{
  const page=await browser.newPage();page.setDefaultTimeout(10000);
  await page.route('http://localhost/**',route=>route.fulfill({contentType:'text/html',body:'<div id="root"></div>'}));await page.goto('http://localhost/');await page.addScriptTag({content:bundle.outputFiles[0].text});
  const frame=page.frameLocator('iframe');
  await frame.locator('output').filter({hasText:/^0$/}).waitFor();
  await page.evaluate(()=>window.save());await frame.getByRole('button',{name:'Next logical step'}).click();
  await frame.locator('output').filter({hasText:/^1$/}).waitFor();
  await page.evaluate(()=>window.restore());await frame.locator('output').filter({hasText:/^0$/}).waitFor();
  assert.equal(await frame.locator('output').textContent(),'0');
 }finally{await browser.close();}
});

test('rendered evidence restores its saved cut instead of following newer input',async()=>{
 const bundle=await build({absWorkingDir:fileURLToPath(new URL('../../',import.meta.url)),stdin:{resolveDir:fileURLToPath(new URL('../../',import.meta.url)),loader:'tsx',contents:`
 import React,{useState} from 'react';import {createRoot} from 'react-dom/client';
 import {VisualSession,VisualSessionClient,initialSession,canonicalDigest,checkpoint} from '@synth/visuals-sdk';
 import {VisualSessionProvider,useVisualEvidence} from '@synth/visuals-react';
 const s=new VisualSession(initialSession({visualId:'test',revision:1,viewKey:'default'},{id:'test',version:'1'}));const blobs=new Map();
 const client=new VisualSessionClient(s.state,{evidenceCuts:true,async request(r){
  if(r.operation==='attach'){for(const c of r.controls??[])s.register(c,r.defaults[c.id]);return {state:s.state};}
  if(r.operation==='evidence.put'){const digest=await canonicalDigest(r.value);blobs.set(digest,r.value);return {digest};}
  if(r.operation==='evidence.read')return {value:blobs.get(r.digest)};
  if(r.operation==='act')return s.execute(r.action);return {state:s.state};
 }});
 let saved;window.save=async()=>{saved=await checkpoint(s.state);};window.restore=async()=>{await s.restore(saved);await client.sync();};
 function App(){const [input,setInput]=useState({count:1});window.change=()=>setInput({count:2});const cut=useVisualEvidence('test',input);return <output data-ready={cut.ready}>{cut.error??(cut.ready?cut.value.count:'loading')}</output>;}
 createRoot(document.getElementById('root')).render(<VisualSessionProvider client={client}><App/></VisualSessionProvider>);
 `},bundle:true,write:false,format:'iife'});
 const browser=await chromium.launch();
 try{
  const page=await browser.newPage();await page.route('http://localhost/**',route=>route.fulfill({contentType:'text/html',body:'<div id="root"></div>'}));await page.goto('http://localhost/');await page.addScriptTag({content:bundle.outputFiles[0].text});
  await page.waitForFunction(()=>document.querySelector('output')?.textContent==='1');
  await page.evaluate(()=>window.save());await page.evaluate(()=>window.change());
  await page.waitForFunction(()=>document.querySelector('output')?.textContent==='2');
  await page.evaluate(()=>window.restore());await page.waitForFunction(()=>document.querySelector('output')?.textContent==='1');
  assert.equal(await page.locator('output').getAttribute('data-ready'),'true');
 }finally{await browser.close();}
});

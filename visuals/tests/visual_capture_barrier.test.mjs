import assert from 'node:assert/strict';
import test from 'node:test';
import {build} from 'esbuild';
import {chromium} from 'playwright';
import {fileURLToPath} from 'node:url';

test('pixel barrier verifies DOM, committed versions and explicit renderer adapters',async()=>{
 const bundle=await build({entryPoints:[fileURLToPath(new URL('../../packages/visuals-react/src/captureBarrier.ts',import.meta.url))],bundle:true,write:false,format:'iife',globalName:'Capture'});
 const browser=await chromium.launch();
 try{
  const page=await browser.newPage();
  await page.setContent('<div data-visual-session-id="v" data-visual-session-revision="1" data-visual-session-version="2" data-visual-session-ready="true"><div style="width:100px">Evidence</div></div>');
  await page.addScriptTag({content:bundle.outputFiles[0].text});
  await page.evaluate(()=>Capture.installVisualCaptureBarrier());
  const begin=()=>page.evaluate(()=>window.__synthVisualCapture.begin('v',1,2));
  const ready=()=>page.waitForFunction(()=>window.__synthVisualCapture.read()?.ready);
  const read=()=>page.evaluate(()=>window.__synthVisualCapture.read());
  await begin();await ready();assert.equal((await read()).mutations,0);
  await page.evaluate(()=>document.querySelector('[data-visual-session-id] div').textContent='Changed evidence');
  assert.ok((await read()).mutations>0);
  assert.ok((await read()).mutationTargets.some(target=>target==='childList:DIV:'));
  await page.evaluate(()=>window.__synthVisualCapture.release());assert.equal(await read(),null);
  await page.evaluate(()=>document.querySelector('[data-visual-session-id] div').style.backgroundImage='url("data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7")');
  await begin();await page.waitForFunction(()=>window.__synthVisualCapture.read()?.error);
  assert.match((await read()).error,/CSS image/);
  await page.evaluate(()=>{window.__synthVisualCapture.release();document.querySelector('[data-visual-session-id] div').style.backgroundImage='none';});
  await page.evaluate(()=>document.querySelector('[data-visual-session-id]').append(document.createElement('canvas')));
  await begin();await page.waitForFunction(()=>window.__synthVisualCapture.read()?.error);
  assert.match((await read()).error,/freeze adapter/);
  await page.evaluate(()=>{
   window.prepared=0;window.released=0;window.dirty=false;
   Capture.registerVisualPixelFreezeAdapter(document.querySelector('canvas'),{
    async prepare(signal){if(signal.aborted)throw new Error('Cancelled');window.prepared++;},
    verify(){if(window.dirty)throw new Error('Canvas changed');},release(){window.released++;}
   });
  });
  await begin();await ready();assert.equal(await page.evaluate(()=>window.prepared),1);
  await page.evaluate(()=>{window.dirty=true;window.__synthVisualCapture.verify();});
  await page.waitForFunction(()=>window.__synthVisualCapture.read()?.error);assert.equal((await read()).error,'Canvas changed');
  await page.evaluate(()=>window.__synthVisualCapture.release());assert.equal(await page.evaluate(()=>window.released),1);
  await page.evaluate(()=>document.querySelector('[data-visual-session-id]').dataset.visualSessionVersion='3');
  await begin();await page.waitForFunction(()=>window.__synthVisualCapture.read()?.error);
  assert.match((await read()).error,/committed session version|changed version/);
 }finally{await browser.close();}
});

test('offscreen native paint preparation does not depend on foreground animation frames',async()=>{
 const bundle=await build({entryPoints:[fileURLToPath(new URL('../../packages/visuals-react/src/captureBarrier.ts',import.meta.url))],bundle:true,write:false,format:'iife',globalName:'Capture'});
 const browser=await chromium.launch();
 try{
  const page=await browser.newPage();
  await page.setContent('<div data-visual-session-id="v" data-visual-session-revision="1" data-visual-session-version="2" data-visual-session-ready="true"><div style="width:100px">Evidence</div></div>');
  await page.addScriptTag({content:bundle.outputFiles[0].text});
  await page.evaluate(()=>{window.requestAnimationFrame=()=>{throw new Error('Occluded');};Capture.installVisualCaptureBarrier({nativeSnapshotPaint:true});window.__synthVisualCapture.begin('v',1,2);});
  await page.waitForFunction(()=>window.__synthVisualCapture.read()?.ready);
  assert.equal(await page.evaluate(()=>window.__synthVisualCapture.read().mutations),0);
  await page.evaluate(()=>document.querySelector('[data-visual-session-id] div').textContent='Changed');
  assert.ok(await page.evaluate(()=>window.__synthVisualCapture.read().mutations>0));
 }finally{await browser.close();}
});

test('React identical input writes are harmless but changed-and-restored attributes invalidate capture',async()=>{
 const bundle=await build({entryPoints:[fileURLToPath(new URL('../../packages/visuals-react/src/captureBarrier.ts',import.meta.url))],bundle:true,write:false,format:'iife',globalName:'Capture'});
 const browser=await chromium.launch();
 try{
  const page=await browser.newPage();
  await page.setContent('<div data-visual-session-id="v" data-visual-session-revision="1" data-visual-session-version="2" data-visual-session-ready="true"><input type="range" name="frame"></div>');
  await page.addScriptTag({content:bundle.outputFiles[0].text});
  await page.evaluate(()=>{Capture.installVisualCaptureBarrier({nativeSnapshotPaint:true});window.__synthVisualCapture.begin('v',1,2);});
  await page.waitForFunction(()=>window.__synthVisualCapture.read()?.ready);
  await page.evaluate(()=>{const input=document.querySelector('input');input.setAttribute('type','range');input.setAttribute('name','frame');});
  assert.equal(await page.evaluate(()=>window.__synthVisualCapture.read().mutations),0);
  await page.evaluate(()=>{const input=document.querySelector('input');input.setAttribute('type','text');input.setAttribute('type','range');});
  assert.ok(await page.evaluate(()=>window.__synthVisualCapture.read().mutations)>0);
 }finally{await browser.close();}
});

test('static opaque frames freeze and verify without executing authored scripts',async()=>{
 const root=fileURLToPath(new URL('../../',import.meta.url));
 const bundle=await build({stdin:{contents:String.raw`import React from 'react';import {createRoot} from 'react-dom/client';import {StaticVisualDocument} from './packages/visuals-react/src/staticDocument.tsx';import {installVisualCaptureBarrier} from './packages/visuals-react/src/captureBarrier.ts';
 installVisualCaptureBarrier();window.authored=false;addEventListener('message',event=>{if(event.data==='authored-executed')window.authored=true;});
 createRoot(document.getElementById('root')).render(<div data-visual-session-id="v" data-visual-session-revision="1" data-visual-session-version="2" data-visual-session-ready="true"><StaticVisualDocument title="Opaque static" source={'<style>@keyframes move {from {opacity:1}to{opacity:.2}}p{animation:move 1s infinite}</style><p>Frozen document</p><script>parent.postMessage("authored-executed","*")</script><img src="data:image/png;base64,broken" onerror="parent.postMessage(\'authored-executed\',\'*\')">'} /></div>);`,resolveDir:root,loader:'tsx'},bundle:true,write:false,format:'iife',jsx:'automatic'});
 const browser=await chromium.launch();try{
  const page=await browser.newPage();page.setDefaultTimeout(5000);
  await page.route('http://localhost/**',route=>route.fulfill({contentType:'text/html',body:'<div id="root"></div>'}));await page.goto('http://localhost/capture');
  await page.addScriptTag({content:bundle.outputFiles[0].text});
  await page.frameLocator('iframe').getByText('Frozen document').waitFor();
  assert.equal(await page.evaluate(()=>window.authored),false);
  assert.equal(await page.locator('iframe').getAttribute('sandbox'),'allow-scripts');
  assert.equal(await page.evaluate(()=>document.querySelector('iframe').contentDocument),null);
  // Unavailable assets reject capture rather than silently disappearing.
  await page.evaluate(()=>window.__synthVisualCapture.begin('v',1,2));
  await page.waitForFunction(()=>window.__synthVisualCapture.read()?.error);
  assert.match(await page.evaluate(()=>window.__synthVisualCapture.read().error),/image unavailable|freeze adapter/);
  await page.evaluate(()=>window.__synthVisualCapture.release());
  await page.frameLocator('iframe').locator('img').evaluate(image=>image.remove());
  await page.evaluate(()=>window.__synthVisualCapture.begin('v',1,2));
  await page.waitForFunction(()=>window.__synthVisualCapture.read()?.ready);
  await page.evaluate(()=>window.__synthVisualCapture.verify());
  await page.waitForFunction(()=>window.__synthVisualCapture.read()?.verified);
  assert.equal(await page.evaluate(()=>window.__synthVisualCapture.read().mutations),0);
  await page.frameLocator('iframe').locator('p').evaluate(node=>node.textContent='Changed');
  await page.evaluate(()=>window.__synthVisualCapture.verify());
  await page.waitForFunction(()=>window.__synthVisualCapture.read()?.error);
  assert.match(await page.evaluate(()=>window.__synthVisualCapture.read().error),/changed during capture/);
  await page.evaluate(()=>window.__synthVisualCapture.release());
 }finally{await browser.close();}
});

test('managed HTML shares the opaque capture barrier and rejects live changes',async()=>{
 const root=fileURLToPath(new URL('../../',import.meta.url));
 const bundle=await build({stdin:{contents:String.raw`import React from 'react';import {createRoot} from 'react-dom/client';import {ManagedHtmlFrame} from './packages/workshop-visuals/components/ManagedHtmlFrame.tsx';import {installVisualCaptureBarrier} from './packages/visuals-react/src/captureBarrier.ts';
 installVisualCaptureBarrier();const host=createRoot(document.getElementById('root'));
 const source='<h1 id="value">Waiting</h1><script>addEventListener("message",event=>{if(event.data?.type==="synth.visual.update.v1")document.getElementById("value").textContent=event.data.payload.label;});</script>';
 window.renderVisual=label=>host.render(<div data-visual-session-id="v" data-visual-session-revision="1" data-visual-session-version="2" data-visual-session-ready="true"><ManagedHtmlFrame title="Managed" source={source} payload={{label}} formatError={String}/></div>);window.renderVisual('First cut');`,resolveDir:root,loader:'tsx'},bundle:true,write:false,format:'iife',jsx:'automatic'});
 const browser=await chromium.launch();try{
  const page=await browser.newPage();page.setDefaultTimeout(5000);
  await page.route('http://localhost/**',route=>route.fulfill({contentType:'text/html',body:'<div id="root"></div>'}));await page.goto('http://localhost/managed-capture');
  await page.addScriptTag({content:bundle.outputFiles[0].text});
  await page.frameLocator('iframe').getByText('First cut').waitFor();
  assert.equal(await page.evaluate(()=>document.querySelector('iframe').contentDocument),null);
  await page.evaluate(()=>window.__synthVisualCapture.begin('v',1,2));
  await page.waitForFunction(()=>window.__synthVisualCapture.read()?.ready);
  await page.evaluate(()=>window.__synthVisualCapture.verify());
  await page.waitForFunction(()=>window.__synthVisualCapture.read()?.verified);
  await page.evaluate(()=>window.renderVisual('Later cut'));
  await page.frameLocator('iframe').getByText('Later cut').waitFor();
  await page.evaluate(()=>window.__synthVisualCapture.verify());
  await page.waitForFunction(()=>window.__synthVisualCapture.read()?.error);
  assert.match(await page.evaluate(()=>window.__synthVisualCapture.read().error),/changed during capture/);
  await page.evaluate(()=>window.__synthVisualCapture.release());
 }finally{await browser.close();}
});

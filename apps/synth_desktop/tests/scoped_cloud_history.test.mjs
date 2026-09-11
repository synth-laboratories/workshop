import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import { transformSync } from 'esbuild';
const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const output = join(root, 'node_modules/.cache/synth-desktop-tests/scoped-cloud-history.mjs');
mkdirSync(dirname(output), {recursive:true});
writeFileSync(output, transformSync(readFileSync(join(root,'src/renderer/src/stores/scopedCloudHistory.ts'),'utf8'), {loader:'ts',format:'esm',target:'es2022'}).code);
const {emptyScopedHistory,observeScope,acceptHistory,acceptEvents,connectScopedHistory} = await import(pathToFileURL(output));
const native = {id:'local',kind:'codex',target:{kind:'cloud',model:'hosted'}};
const cloud = {id:'a-private',kind:'intern',target:{kind:'intern'}};
const view = (generation,availability='ready')=>({generation,availability});
const deferred = () => {let resolve;const promise=new Promise(r=>resolve=r);return {promise,resolve};};
const tick = () => new Promise(r=>setImmediate(r));
test('scope replacement clears cloud history and keeps native hosted-provider history',()=>{
 let state=observeScope(emptyScopedHistory(),view(1));
 state=acceptHistory(state,{generation:1,sessions:[native,cloud]});
 state=acceptEvents(state,'local',{generation:1,events:[{sessionId:'local',sequence:1}]});
 state=acceptEvents(state,'a-private',{generation:1,events:[{sessionId:'a-private',sequence:2,payload:'private'}]});
 state=observeScope(state,view(2,'signed_out'));
 assert.deepEqual(state.sessions,[native]);assert.deepEqual(Object.keys(state.events),['local']);
 assert.equal(acceptHistory(state,{generation:1,sessions:[cloud]}),state);
 assert.equal(acceptEvents(state,'a-private',{generation:1,events:[]}),state);
 assert.equal(observeScope(state,view(1)),state);
});
test('gated history rejects legacy cloud rows, unknown ownership, and cross-session events',()=>{
 let state=acceptHistory(emptyScopedHistory(),{generation:0,sessions:[native,cloud,{id:'unknown',kind:'other'}]});
 assert.deepEqual(state.sessions,[native]);
 assert.equal(acceptEvents(state,'local',{generation:0,events:[{sessionId:'a-private',sequence:3}]}),state);
});
test('subscription precedes snapshot; late snapshot and history cannot resurrect the previous account',async()=>{
 const snapshot=deferred(),oldHistory=deferred();let listener;let calls=0;let latest;let detached=false;
 const stop=connectScopedHistory({observe:async cb=>{listener=cb;return ()=>{detached=true}},view:()=>snapshot.promise,history:()=>++calls===1?oldHistory.promise:Promise.resolve({generation:2,sessions:[native]})},s=>{latest=s});
 await tick();listener(view(1));await tick();listener(view(2,'signed_out'));await tick();
 oldHistory.resolve({generation:1,sessions:[cloud]});snapshot.resolve(view(1));await tick();
 assert.equal(latest.scope.generation,2);assert.deepEqual(latest.sessions,[native]);stop();assert.equal(detached,true);
});
test('dispose during listener attachment detaches and never fetches a snapshot',async()=>{
 const attached=deferred();let detached=false;let published=0;
 const stop=connectScopedHistory({observe:()=>attached.promise,view:async()=>{throw Error('must not fetch')},history:async()=>{throw Error('must not fetch')}},()=>published++);
 stop();attached.resolve(()=>{detached=true});await tick();assert.equal(detached,true);assert.equal(published,0);
});

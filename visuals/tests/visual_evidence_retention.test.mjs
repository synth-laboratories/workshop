import test from 'node:test';
import assert from 'node:assert/strict';
import {VisualSession,VisualSessionClient,initialSession,canonicalDigest,checkpoint,retainVisualRead} from '@synth/visuals-sdk';
import {retainWorkshopPorts} from '@synth/workshop-visuals/runtime/retainedPorts.ts';

test('collection subscriptions discard superseded notifications and stop after cancellation',async()=>{
 let publish,stopped=false;const seen=[];
 const ports=retainWorkshopPorts(null,{collections:{subscribePage(...args){publish=args.at(-1);return()=>{stopped=true;};}}});
 const stop=ports.collections.subscribePage('rollouts',{},state=>seen.push(state));
 publish({status:'ready',page:{revision:1}});
 publish({status:'ready',page:{revision:2}});
 await new Promise(resolve=>setTimeout(resolve,0));
 assert.deepEqual(seen.map(state=>state.page.revision),[2]);
 publish({status:'ready',page:{revision:3}});stop();
 await new Promise(resolve=>setTimeout(resolve,0));
 assert.equal(stopped,true);assert.equal(seen.length,1);
});

test('retained read ports replay the original cut offline and never invoke new reads',async()=>{
 const session=new VisualSession(initialSession({visualId:'evidence',revision:1,viewKey:'default'},{id:'test',version:'1'}));
 const blobs=new Map();
 const client=new VisualSessionClient(session.state,{evidenceCuts:true,async request(request){
  if(request.operation==='attach'){for(const control of request.controls??[])session.register(control,request.defaults[control.id]);return {state:session.state};}
  if(request.operation==='evidence.put'){const digest=await canonicalDigest(request.value);blobs.set(digest,structuredClone(request.value));return {digest};}
  if(request.operation==='evidence.read'){if(!blobs.has(request.digest))throw new Error('Offline source unavailable');return {value:blobs.get(request.digest)};}
  if(request.operation==='act')return session.execute(request.action);
  return {state:session.state};
 }});
 try{
  await client.start();let reads=0;
  const read=async()=>({sequence:++reads,score:reads});
  assert.deepEqual(await retainVisualRead(client,'collection.page',['rollouts',0],read),{sequence:1,score:1});
  const saved=await checkpoint(session.state);
  await retainVisualRead(client,'collection.page',['rollouts',0],read);
  assert.equal(reads,2);
  await session.restore(saved);await client.sync();
  assert.deepEqual(await retainVisualRead(client,'collection.page',['rollouts',0],read),{sequence:1,score:1});
  assert.equal(reads,2);
  await assert.rejects(()=>retainVisualRead(client,'collection.page',['rollouts',20],read),/not retained/);
  assert.equal(reads,2);
  const digest=Object.values(session.state.values)[0];blobs.set(digest,{tampered:true});
  await assert.rejects(()=>retainVisualRead(client,'collection.page',['rollouts',0],read),/digest mismatch/);
 }finally{client.dispose();}
});

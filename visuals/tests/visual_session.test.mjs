import assert from "node:assert/strict";
import test from "node:test";
import { VisualSession, VisualSessionClient, initialSession, checkpoint, replayRecording, canonicalDigest, validateSession } from "@synth/visuals-sdk";

function session() {
  const session=new VisualSession(initialSession({visualId:"diagram",revision:1,viewKey:"default"},{id:"diagram",version:"1"}));
  session.register({id:"source",label:"Source",type:"boolean"},false);
  session.register({id:"step",label:"Step",type:"number",minimum:0,maximum:100},0);
  return session;
}
const action=(version,values,id=crypto.randomUUID())=>({id,kind:"presentation.patch",payload:{values},expectedStateVersion:version});

test("static diagrams need no corpus; commands are atomic, optimistic and idempotent",()=>{
  const s=session();const command=action(0,{source:true,step:8},"first");
  const first=s.execute(command);assert.equal(first.state.stateVersion,1);assert.equal(first.state.corpus,undefined);
  assert.equal(s.execute(command).duplicate,true);assert.equal(s.state.stateVersion,1);
  assert.throws(()=>s.execute({...command,payload:{values:{step:4}}}),/different input/);
  assert.throws(()=>s.execute(action(0,{step:4})),/Stale/);
  assert.throws(()=>s.execute(action(1,{source:false,step:1000})),/range/);
  assert.deepEqual(s.state.values,{source:true,step:8});
  const exposed=s.state;exposed.values.step=50;assert.equal(s.state.values.step,8);
});

test("complete checkpoints restore and semantic replay verifies every transition",async()=>{
  const s=session();const saved=await checkpoint(s.state);
  await s.startRecording();s.execute(action(0,{step:2}));s.execute(action(1,{source:true,step:7}));
  const recorded=s.stopRecording();assert.equal(recorded.events.length,2);
  assert.deepEqual((await replayRecording(recorded)).values,s.state.values);
  assert.equal((await replayRecording(recorded,1)).values.step,2);
  const corrupt=structuredClone(recorded);corrupt.events[0].state.values.step=6;
  await assert.rejects(()=>replayRecording(corrupt),/transition mismatch/);
  await s.restore(saved);assert.deepEqual(s.state.values,{source:false,step:0});
  assert.deepEqual(s.state.replay,{checkpointId:saved.id});
  s.execute(action(s.state.stateVersion,{step:1}));assert.equal(s.state.replay,undefined);
  const other=structuredClone(saved);other.state.revision=2;other.digest=await canonicalDigest(other.state);
  await assert.rejects(()=>s.restore(other),/original visual revision/);
});

test("replay cursors are bounded, explicit and reject ambiguous modes",()=>{
  const state=session().state;
  for(const replay of [null,{},[],{checkpointId:""},{recordingId:"r",sequence:-1},{recordingId:"r",sequence:1.5},{checkpointId:"c",sequence:0}])
    assert.throws(()=>validateSession({...state,replay}),/replay/);
  validateSession({...state,replay:{checkpointId:"c"}});
  validateSession({...state,replay:{recordingId:"r",sequence:0}});
  for(const intervalMs of [0,15,60001,1.5,"300",null])assert.throws(()=>validateSession({...state,replay:{recordingId:"r",sequence:0,intervalMs}}),/replay/);
  validateSession({...state,replay:{recordingId:"r",sequence:0,intervalMs:75}});
});

test("recording speed is committed, preserved by stepping, and validated atomically",async()=>{
 const s=session();await s.startRecording();s.execute(action(0,{step:1}));const recording=s.stopRecording();
 await s.seekRecording(recording,0);
 s.playRecording(true,s.state.stateVersion,75);
 assert.equal(s.state.replay.intervalMs,75);
 const before=s.state;
 assert.throws(()=>s.playRecording(true,s.state.stateVersion,0),/interval/);
 assert.deepEqual(s.state,before);
 await s.seekRecording(recording,1);
 assert.equal(s.state.replay.intervalMs,75);assert.equal(s.state.replay.playing,false);
});

test("native-compatible canonical digests normalize numbers and object ordering",async()=>{
  assert.equal(await canonicalDigest({z:-0,a:1e-7}),await canonicalDigest({a:0.0000001,z:0}));
  assert.notEqual(await canonicalDigest({n:1}),await canonicalDigest({n:"1"}));
  assert.equal(await canonicalDigest({a:1,z:1e-7}),"sha256:31e8dfc1125300b8321f191a16cd8805806503f828ae6d612c4b2dd897c31ed1");
});

test("structured controls reject malformed nested values atomically",()=>{
  const s=session();s.register({id:"viewport",label:"Viewport",type:"object",required:["scale"],additionalProperties:false,properties:{scale:{type:"number",minimum:.25,maximum:4}}},{scale:1});
  assert.throws(()=>s.execute(action(0,{step:4,viewport:{}})),/required/);
  assert.throws(()=>s.execute(action(0,{viewport:{scale:100}})),/range/);
  assert.throws(()=>s.execute(action(0,{viewport:{scale:1,extra:true}})),/not allowed/);
  assert.equal(s.state.values.step,0);assert.equal(s.state.stateVersion,0);
});

test("nullable discriminated controls reject partial and ambiguous variants",()=>{
 const s=session();s.register({id:"filter",label:"Filter",type:"object",nullable:true,oneOf:[
  {type:"object",required:["kind","name"],additionalProperties:false,properties:{kind:{type:"string",options:["label"]},name:{type:"string"}}},
  {type:"object",required:["kind","low","high"],additionalProperties:false,properties:{kind:{type:"string",options:["range"]},low:{type:"number"},high:{type:"number"}}}
 ]},null);
 for(const filter of [{},{kind:"range",low:1},{kind:"label",name:"a",low:1}])assert.throws(()=>s.execute(action(0,{filter})),/variant/);
 s.execute(action(0,{filter:{kind:"range",low:1,high:2}}));
 s.execute(action(1,{filter:null}));assert.equal(s.state.values.filter,null);
});

test("recording preserves controls first mounted after recording started",async()=>{
  const s=session();await s.startRecording();
  s.register({id:"detail.tab",label:"Detail tab",type:"string",options:["summary","trace"]},"summary");
  s.execute(action(0,{"detail.tab":"trace"}));
  const record=s.stopRecording();assert.equal((await replayRecording(record)).values["detail.tab"],"trace");
  record.events[0].before.values.step=99;
  await assert.rejects(()=>replayRecording(record),/changed committed presentation/);
});

test("client batches one human gesture and observes another client's committed state",async()=>{
  const s=session();const calls=[];let changed=()=>{};
  const client=new VisualSessionClient(initialSession({visualId:"diagram",revision:1,viewKey:"default"},{id:"diagram",version:"1"}),{
    async request(request){calls.push(request);if(request.operation==="act")return s.execute(request.action);return {state:s.state};},
    subscribe(callback){changed=callback;return()=>{changed=()=>{};};},
  });
  client.register({id:"source",label:"Source",type:"boolean"},false);
  client.register({id:"step",label:"Step",type:"number",minimum:0,maximum:100},0);
  await client.start();await Promise.all([client.set("step",5),client.set("source",true)]);
  assert.throws(()=>{client.getSnapshot().state.values.step=500;},TypeError);
  assert.equal(calls.filter(c=>c.operation==="act").length,1);
  s.execute(action(1,{step:9}));changed();await client.sync();
  await new Promise(resolve=>setTimeout(resolve,0));assert.equal(client.getSnapshot().state.values.step,9);
  client.dispose();await assert.rejects(()=>client.set("step",10),/disposed/);
});

test("StrictMode restart cannot install a stale subscription",async()=>{
  const pending=[];let subscriptions=0;
  const state=initialSession({visualId:"diagram",revision:1,viewKey:"default"},{id:"diagram",version:"1"});
  const client=new VisualSessionClient(state,{
    request(){return new Promise(resolve=>pending.push(resolve));},
    subscribe(){subscriptions++;return()=>{subscriptions--;};},
  });
  const first=client.start();client.dispose();const second=client.start();
  pending[1]({state});await second;
  pending[0]({state});await first;
  assert.equal(subscriptions,1);client.dispose();assert.equal(subscriptions,0);
});

test("client registration batches reject atomically and do not retain caller mutations",async()=>{
 const state=initialSession({visualId:'diagram',revision:1,viewKey:'default'});let attached;
 const client=new VisualSessionClient(state,{request:async request=>{attached=request;return {state:{...state,controls:request.controls,values:request.defaults}};}});
 const good={id:'frame.good',label:'Good',type:'number'};
 assert.throws(()=>client.registerMany([{control:good,initial:1},{control:{id:'frame.bad',label:'Bad',type:'number'},initial:'wrong'}]),/requires number/);
 await client.start();assert.deepEqual(attached.controls,[]);
 client.registerMany([{control:good,initial:1}]);good.label='Mutated';
 await new Promise(resolve=>setTimeout(resolve,0));
 assert.equal(attached.controls[0].label,'Good');client.dispose();
});

test("incompatible mounted controls cannot become capture-ready on inspection",async()=>{
 const state=initialSession({visualId:"diagram",revision:1,viewKey:"default"});
 state.controls=[{id:"cursor",label:"cursor",type:"number",nullable:true}];state.values.cursor=null;
 const client=new VisualSessionClient(initialSession({visualId:"diagram",revision:1,viewKey:"default"}),{request:async()=>({state})});
 client.register({id:"cursor",label:"cursor",type:"object",nullable:true},null);
 await client.start();assert.equal(client.getSnapshot().ready,false);assert.match(client.getSnapshot().error,/schema changed/);
 await client.sync();assert.equal(client.getSnapshot().ready,false);client.dispose();
});

test("domain and control scene publication converges instead of oscillating",async()=>{
 const s=session();let publishes=0;
 const client=new VisualSessionClient(initialSession({visualId:"diagram",revision:1,viewKey:"default"},{id:"diagram",version:"1"}),{request:async request=>{if(request.operation==='publish'){publishes++;const state=s.state;state.scene=request.scene;return {state};}return {state:s.state};}});
 await client.start();
 const scene={visualId:'diagram',revision:1,stateVersion:0,clocks:{},selection:{members:[]},truth:{},diagnostics:[],landmarks:[{ref:{kind:'evidence',id:'event'},role:'region',label:'Evidence',actions:[]}]};
 client.publishScene(scene);await new Promise(resolve=>setTimeout(resolve,0));
 for(let i=0;i<20;i++){client.publishScene(client.getSnapshot().state.scene);client.publishScene(scene);}
 await new Promise(resolve=>setTimeout(resolve,0));
 assert.equal(publishes,1);assert.equal(client.getSnapshot().state.scene.landmarks.filter(item=>item.ref.kind==='control').length,2);client.dispose();
});

test("namespaced host observations converge with independent family publishers",async()=>{
 const s=session();let publishes=0;
 const client=new VisualSessionClient(initialSession({visualId:"diagram",revision:1,viewKey:"default"},{id:"diagram",version:"1"}),{request:async request=>{if(request.operation==='publish'){publishes++;const state=s.state;state.scene=request.scene;return {state};}return {state:s.state};}});
 await client.start();const scene={visualId:'diagram',revision:1,stateVersion:0,clocks:{},selection:{members:[]},truth:{count:{state:'observed',value:10}},diagnostics:['Domain diagnostic'],landmarks:[]};
 client.publishScene(scene);await new Promise(resolve=>setTimeout(resolve,0));
 const contribution={truth:{frames:{state:'observed',value:2}},diagnostics:['Host diagnostic']};
 client.publishSceneContribution('rendered',0,contribution);await new Promise(resolve=>setTimeout(resolve,0));
 for(let i=0;i<20;i++){client.publishScene(scene);client.publishSceneContribution('rendered',0,contribution);client.publishScene(client.getSnapshot().state.scene);}
 await new Promise(resolve=>setTimeout(resolve,0));assert.equal(publishes,2);
 assert.deepEqual(client.getSnapshot().state.scene.truth,{'count':{state:'observed',value:10},'rendered.frames':{state:'observed',value:2}});
 assert.deepEqual(client.getSnapshot().state.scene.diagnostics,['Domain diagnostic','[rendered] Host diagnostic']);client.dispose();
});

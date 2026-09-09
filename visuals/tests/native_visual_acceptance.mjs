// Explicitly pointed at an isolated native Workshop instance. Never reads the
// canonical profile, provider credentials, or macOS Keychain.
import {readFileSync,existsSync,readdirSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve,join,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
const repo=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const root=process.argv[2],family=process.argv[3];
if(!root?.startsWith('/tmp/workshop-visuals-native-')||!family)throw new Error('Usage: node native_visual_acceptance.mjs /tmp/workshop-visuals-native-... family-id');
const connection=JSON.parse(readFileSync(join(root,'visuals-ipc.json'),'utf8'));
if(!/^http:\/\/127\.0\.0\.1:\d+$/.test(connection.url))throw new Error('Expected isolated loopback connection');
async function request(path,body,method=body===undefined?'GET':'POST'){
 const response=await fetch(connection.url+path,{method,headers:{Authorization:'Bearer '+connection.token,'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(45_000)});
 const value=await response.json();if(!response.ok)throw new Error(JSON.stringify(value));return value;
}
function manifests(path){return readdirSync(path,{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?manifests(join(path,entry.name)):entry.name==='template.json'?[join(path,entry.name)]:[]);}
const manifestPath=manifests(join(repo,'packages/workshop-visuals/families')).find(path=>JSON.parse(readFileSync(path,'utf8')).id===family);
if(!manifestPath)throw new Error('Unknown catalog family');
const exampleName=process.argv.find(argument=>argument.startsWith('--example='))?.slice('--example='.length)??'fixture_binding.json';
if(!/^[a-z0-9_]+\.json$/.test(exampleName))throw new Error('Expected a packaged example filename');
const examplePath=join(dirname(manifestPath),'examples',exampleName);
let example=existsSync(examplePath)?JSON.parse(readFileSync(examplePath,'utf8')):{};
let content=example.content;
if(family==='diagram.mermaid.v1')content='flowchart LR\n  Human[Human intent] --> Engine[Visual session]\n  AI[AI intent] --> Engine\n  Engine --> Pixels[Coherent pixels]';
const bindings={schemaVersion:'synth.visual-bindings.v1',inputs:(Array.isArray(example.bindings)?example.bindings:example.bindings?.inputs ?? example.inputs ?? example.slots ?? []).map(({slot,...binding})=>({...binding,input:binding.input??slot}))};
const trajectoryCount=process.argv.find(argument=>argument.startsWith('--trajectory-count='));
if(trajectoryCount){
 assert.equal(family,'analysis.swarm_trajectories.v1');
 assert.equal(Number(trajectoryCount.split('=')[1]),1000,'Use the registered, versioned 1000-row fixture');
 bindings.inputs=[{input:'trajectories',kind:'fixture',schema:'synth.workshop.agent-trajectory.v1',source:'fixtures/trajectory-swarm-1000.v1'}];
}
if(example.importOptimizerFixture){
 const source=resolve(repo,'packages/workshop-visuals',example.importOptimizerFixture);
 if(!source.startsWith(join(repo,'packages/workshop-visuals')+'/'))throw new Error('Fixture import must be packaged');
 const fixture=JSON.parse(readFileSync(source,'utf8'));
 assert.ok(Array.isArray(fixture.events)&&fixture.events.length);
 // Importing the same legacy run id can reset its header while journal
 // deduplication skips the old events. Every acceptance import owns a fresh run.
 {
  const original=fixture.events[0].optimizerRunId;
  assert.equal(typeof original,'string');
  const isolated='accept-recorded-'+crypto.randomUUID();
  fixture.events=fixture.events.map(event=>JSON.parse(JSON.stringify(event).replaceAll(original,isolated)));
 }
 const path=join(root,'acceptance-eval-events.jsonl');
 writeFileSync(path,fixture.events.map(event=>JSON.stringify({...event,_seq:event.sequenceNumber,optimizer_run_id:event.optimizerRunId,algorithm_id:event.algorithmId})).join('\n')+'\n');
 const imported=await request('/v1/optimizers/import_local',{path,openVisual:false});
 assert.ok(imported.run?.id);
 for(const binding of bindings.inputs)if(binding.kind==='optimizer_run')binding.source=imported.run.id;
}
const id='accept-'+family.replaceAll('.','-')+(exampleName==='fixture_binding.json'?'':'-'+exampleName.replace('.json',''));
let visual;
try{visual=(await request('/v1/visuals/'+id)).visual;}catch{
 visual=(await request('/v1/visuals',{id,templateId:family,title:'Acceptance · '+family,workspaceOwned:true,bindings,content,metadata:{acceptanceFixture:true,displayName:family}})).visual;
}
assert.ok(visual?.id);
if(process.argv.includes('--refresh-fixture')){
 assert.equal(visual.metadata?.acceptanceFixture,true,'Only test-owned fixtures may be refreshed');
 visual=(await request('/v1/visuals/'+id,{bindings,content,bumpRevision:true})).visual;
}
await request('/v1/visuals/'+id+'/show',{presentation:'pane'});
let capture;
if(process.argv.includes('--capture')){
 for(let attempt=0;attempt<4;attempt++){
  try{capture=await request('/v1/visuals/'+id+'/engine',{operation:'capture.pixels',revision:visual.currentRevision});break;}
  catch(error){if(attempt===3)throw error;await new Promise(resolve=>setTimeout(resolve,500));}
 }
}
let session;
for(let attempt=0;attempt<80;attempt++){
 try{session=await request('/v1/visuals/'+id+'/engine',{operation:'inspect',revision:visual.currentRevision});if(session.state?.scene)break;}catch{}
 await new Promise(resolve=>setTimeout(resolve,250));
}
assert.ok(session?.state,'Mounted native session did not attach');
const evidence={family,visualId:id,revision:visual.currentRevision,stateVersion:session.state.stateVersion,controls:session.state.controls.map(control=>control.id),scene:session.state.scene};
if(process.argv.includes('--exercise')){
 const engine=(operation,fields={})=>request('/v1/visuals/'+id+'/engine',{operation,revision:visual.currentRevision,...fields});
 const initial=session.state;
 let specific;
 const optimizerBinding=bindings.inputs.find(binding=>binding.input==='optimizer_run'&&binding.kind==='fixture');
 if(optimizerBinding&&initial.controls.some(control=>control.id==='optimizer.sequence')){
  const fixture=optimizerBinding.data??JSON.parse(readFileSync(resolve(repo,'packages/workshop-visuals',optimizerBinding.source),'utf8'));
  const sequences=(fixture.events??[]).map(event=>event.sequenceNumber??event.sequence_number??event.sequence).filter(Number.isFinite);
  if(sequences.length)specific={id:'optimizer.sequence',value:Math.max(0,Math.floor(Math.max(...sequences)/2))};
 }
 if(family==='analysis.annotation_workbench.v1')specific={id:'annotation.view',value:'findings'};
 if(family==='live.container_rollouts.v1')specific={id:'container.cursor',value:0};
 if(process.argv.includes('--domain')){
  const cursors={'live.annotated_rollouts.v1':'annotated.globalCursor','live.craftax.v1':'craftax.evaluationCutoff','live.harbor_eval.v1':'harbor.eventCutoff'};
  if(cursors[family])specific={id:cursors[family],value:1};
 }
 const control=(specific?initial.controls.find(control=>control.id===specific.id):undefined)
   ?? initial.controls.find(control=>control.options?.some(value=>JSON.stringify(value)!==JSON.stringify(initial.values[control.id])))
   ?? initial.controls.find(control=>control.type==='boolean'&&!/playing|followLive|following/i.test(control.id))
   ?? initial.controls.find(control=>control.type==='number'&&Number.isFinite(initial.values[control.id])&&((control.maximum??Infinity)>initial.values[control.id]||(control.minimum??-Infinity)<initial.values[control.id]));
 if(control){
  const current=initial.values[control.id];
  const value=(specific?.id===control.id?specific.value:undefined) ?? control.options?.find(value=>JSON.stringify(value)!==JSON.stringify(current)) ?? (control.type==='number'?(current<(control.maximum??Infinity)?Math.min(current+1,control.maximum??Infinity):Math.max(current-1,control.minimum??-Infinity)):!current);
  const recording=await engine('record.start');
  const action={id:crypto.randomUUID(),kind:'presentation.set',target:{id:control.id},expectedStateVersion:initial.stateVersion,payload:{value}};
  const changed=await engine('act',{action});assert.deepEqual(changed.state.values[control.id],value);
  await assert.rejects(()=>engine('act',{action:{...action,id:crypto.randomUUID()}}),/stale/);
  if(process.argv.includes('--domain')){
   let selectedCapture;
   for(let attempt=0;attempt<4;attempt++){
    try{selectedCapture=await engine('capture.pixels');break;}catch(error){if(attempt===3)throw error;await new Promise(resolve=>setTimeout(resolve,500));}
   }
   assert.deepEqual(selectedCapture.pixelCut.checkpoint.state.values[control.id],value);
   assert.equal(selectedCapture.pixelCut.paint.verified,true);
   evidence.domain={control:control.id,value,pixelPath:selectedCapture.path,pixelCut:selectedCapture.pixelCut};
  }
  await engine('record.stop');
  const beforeReplay=await engine('inspect');
  const replayed=await engine('record.seek',{recordingId:recording.recordingId,sequence:0,expectedStateVersion:beforeReplay.state.stateVersion});
  assert.deepEqual(replayed.state.values,initial.values);
  evidence.interaction={control:control.id,recordingId:recording.recordingId,staleRejected:true,replayRestored:true};
 }else evidence.interaction={reason:'No safely toggleable registered control; family-specific seek/selection acceptance required'};
}
if(process.argv.includes('--capture')){
 assert.equal(capture.pixelCut.checkpoint.state.visualId,id);
 assert.equal(capture.pixelCut.paint.mutations,0);
 assert.ok(existsSync(capture.path));
 evidence.capture=capture;
}
mkdirSync(join(root,'acceptance'),{recursive:true});
const evidencePath=join(root,'acceptance',family+(exampleName==='fixture_binding.json'?'':'.'+exampleName.replace('.json',''))+'.json');
writeFileSync(evidencePath,JSON.stringify(evidence,null,2));
console.log(JSON.stringify({family,visualId:id,controls:evidence.controls,interaction:evidence.interaction,pixelPath:evidence.capture?.path,evidencePath}));

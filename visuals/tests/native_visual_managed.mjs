// Only import the reviewed, networkless test package into an isolated instance.
import {readFileSync,writeFileSync} from 'node:fs';
import {resolve,join,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
const root=process.argv[2];
assert.ok(root?.startsWith('/tmp/workshop-visuals-native-'));
const connection=JSON.parse(readFileSync(join(root,'visuals-ipc.json'),'utf8'));
assert.match(connection.url,/^http:\/\/127\.0\.0\.1:\d+$/);
async function request(path,body){
 const response=await fetch(connection.url+path,{method:'POST',headers:{Authorization:'Bearer '+connection.token,'Content-Type':'application/json'},body:JSON.stringify(body),signal:AbortSignal.timeout(45000)});
 const result=await response.json();assert.ok(response.ok,JSON.stringify(result));return result;
}
const sourcePath=resolve(dirname(fileURLToPath(import.meta.url)),'fixtures/accept.managed-session.v1');
const {template}=await request('/v1/visuals/templates/import',{sourcePath});
const id='accept-managed-'+crypto.randomUUID();
await request('/v1/visuals',{id,templateId:template.id,title:'Managed session native acceptance',workspaceOwned:true,bindings:{schemaVersion:'synth.visual-bindings.v1',inputs:[]},metadata:{acceptanceFixture:true}});
await request('/v1/visuals/'+id+'/show',{presentation:'pane'});
const engine=(operation,fields={})=>request('/v1/visuals/'+id+'/engine',{operation,revision:1,...fields});
let state;
for(let attempt=0;attempt<80;attempt++){
 try{state=(await engine('inspect')).state;}catch(error){if(!String(error).includes('open this visual revision first'))throw error;}
 if(state?.values['frame.step']!==undefined)break;
 await new Promise(resolve=>setTimeout(resolve,250));
}
assert.ok(state,'Managed visual did not mount within the acceptance deadline');
assert.equal(state.values['frame.step'],0,'Frame must register through its sandbox bridge');
const before=await engine('capture.pixels');
await engine('act',{action:{id:crypto.randomUUID(),kind:'presentation.set',target:{id:'frame.step'},expectedStateVersion:state.stateVersion,payload:{value:3}}});
const after=await engine('capture.pixels');
assert.equal(after.pixelCut.checkpoint.state.values['frame.step'],3);
assert.equal(after.pixelCut.paint.verified,true);
await engine('restore',{checkpointId:before.pixelCut.checkpoint.id,expectedStateVersion:after.pixelCut.checkpoint.state.stateVersion});
const restored=await engine('capture.pixels');
assert.equal(restored.pixelCut.checkpoint.state.values['frame.step'],0);
const evidence={visualId:id,before,after,restored};
writeFileSync(join(root,'acceptance/managed-session.json'),JSON.stringify(evidence,null,2));
console.log(JSON.stringify({visualId:id,before:before.path,after:after.path,restored:restored.path}));

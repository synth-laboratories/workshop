import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {join} from 'node:path';
import assert from 'node:assert/strict';
const root=process.argv[2];
if(!root?.startsWith('/tmp/workshop-visuals-native-'))throw new Error('Explicit isolated native acceptance root required');
const connection=JSON.parse(readFileSync(join(root,'visuals-ipc.json')));
if(!/^http:\/\/127\.0\.0\.1:\d+$/.test(connection.url))throw new Error('Expected isolated loopback connection');
async function request(path,body){
 const response=await fetch(connection.url+path,{method:'POST',headers:{Authorization:'Bearer '+connection.token,'Content-Type':'application/json'},body:JSON.stringify(body),signal:AbortSignal.timeout(45_000)});
 const value=await response.json();if(!response.ok)throw new Error(JSON.stringify(value));return value;
}
const id='accept-source-pin-'+crypto.randomUUID();
await request('/v1/visuals',{id,templateId:'diagram.mermaid.v1',title:'Native source revision acceptance',workspaceOwned:true,content:'flowchart LR\n A[Original source] --> B[Revision one]',metadata:{acceptanceFixture:true}});
const engine=(revision,operation,fields={})=>request('/v1/visuals/'+id+'/engine',{revision,operation,...fields});
await request('/v1/visuals/'+id+'/show',{presentation:'pane'});
const before=await engine(1,'capture.pixels');
const updated=await request('/v1/visuals/'+id,{content:'flowchart LR\n A[Changed source] --> B[Revision two] --> C[Independent state]',bumpRevision:true});
assert.equal(updated.visual.currentRevision,2);
await request('/v1/visuals/'+id+'/show',{presentation:'pane'});
const after=await engine(2,'capture.pixels');
const stateBefore=(await engine(2,'inspect')).state;
await assert.rejects(()=>engine(2,'restore',{checkpointId:before.pixelCut.checkpoint.id,expectedStateVersion:stateBefore.stateVersion}),/revision|identity|different|belong/);
assert.deepEqual((await engine(2,'inspect')).state,stateBefore);
assert.equal(before.pixelCut.checkpoint.state.revision,1);assert.equal(after.pixelCut.checkpoint.state.revision,2);
assert.equal(before.pixelCut.paint.verified,true);assert.equal(after.pixelCut.paint.verified,true);
const receipt={visualId:id,oldRevision:1,newRevision:2,foreignRevisionRestoreRejected:true,newStateUnchanged:true,beforePixelPath:before.path,afterPixelPath:after.path,beforeCheckpoint:before.pixelCut.checkpoint.id,afterCheckpoint:after.pixelCut.checkpoint.id};
mkdirSync(join(root,'revision-acceptance'),{recursive:true});
writeFileSync(join(root,'revision-acceptance',id+'.json'),JSON.stringify(receipt,null,2));
console.log(JSON.stringify(receipt));

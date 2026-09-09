import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
const repo=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const [root,visualId,operation='inspect',fields='{}']=process.argv.slice(2);
if(!root?.startsWith('/tmp/workshop-visuals-native-')||!visualId)throw new Error('Expected an isolated native acceptance root and visual id');
const process_=spawn(join(repo,'apps/synth_desktop/src-tauri/target/debug/synth-visuals-mcp'),[],{
 env:{...process.env,SYNTH_VISUALS_IPC_FILE:join(root,'visuals-ipc.json')},stdio:['pipe','pipe','pipe'],
});
const pending=new Map();let sequence=0;
const lines=createInterface({input:process_.stdout});
lines.on('line',line=>{try{const response=JSON.parse(line);if(response.id)pending.get(response.id)?.(response);}catch{}});
function rpc(method,params){
 const id=++sequence;
 return new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>{pending.delete(id);reject(new Error('MCP request timed out'));},45_000);
  pending.set(id,response=>{clearTimeout(timer);pending.delete(id);response.error?reject(new Error(JSON.stringify(response.error))):resolve(response.result);});
  process_.stdin.write(JSON.stringify({jsonrpc:'2.0',id,method,params})+'\n');
 });
}
try{
 await rpc('initialize',{protocolVersion:'2024-11-05',capabilities:{},clientInfo:{name:'visuals-native-acceptance',version:'0.10.0'}});
 process_.stdin.write(JSON.stringify({jsonrpc:'2.0',method:'notifications/initialized'})+'\n');
 const manage=operation.startsWith('manage:');
 const result=await rpc('tools/call',{name:'visual_manage',arguments:manage
  ? {operation:operation.slice('manage:'.length),arguments:{visual_id:visualId,...JSON.parse(fields)}}
  : {operation:'session',arguments:{visual_id:visualId,revision:1,operation,...JSON.parse(fields)}}});
 if(result.isError)throw new Error(JSON.stringify(result.content));
 const text=result.content.find(item=>item.type==='text')?.text;
 console.log(text ?? JSON.stringify(result));
}finally{lines.close();process_.stdin.end();process_.kill();}

import {BotSDK} from '/app/sdk/index';
import {BotActions} from '/app/sdk/actions';
import {appendFileSync,writeFileSync} from 'node:fs';
import {randomBytes} from 'node:crypto';
import scenario from './scenario.json';
scenario.evidence_kind=process.env.MA_EVIDENCE_KIND||'scripted';
const names=scenario.actors.map(a=>a.id), observers=new Map<string,BotSDK>(), controllers=new Map<string,{sdk:BotSDK,bot:BotActions}>();
const tokens=new Map(names.map(n=>[n,randomBytes(24).toString('hex')]));
const busy=new Set<string>(), recorders:any[]=[];
let latest:any={ready:false,bots:{}}, clock:any={phase:'idle'}, startAt=0, sequence=0, started=false;
const seen=new Set<string>();
function event(kind:string,actor:string|null,payload:any){
 appendFileSync('/logs/ma/events.jsonl',JSON.stringify({schema_version:'evals.event-stream.v1',kind:'trace.raw',source:'local',occurred_at:new Date().toISOString(),run_id:process.env.MA_RUN_ID||'local-ma',sequence:String(++sequence),actor_id:actor,payload:{kind,elapsedMs:startAt?Date.now()-startAt:0,...payload}})+'\n');
}
async function engine(path:string,body?:any){
 const r=await fetch('http://127.0.0.1:8792'+path,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined});
 if(!r.ok)throw Error(await r.text());return r.json();
}
function privateView(name:string){
 const s=latest.bots[name];if(!s)return null;
 return {actor:scenario.actors.find(a=>a.id===name),remainingMs:clock.remainingMs,phase:clock.phase,tick:s.tick,player:s.player,skills:s.skills,inventory:s.inventory,nearbyPlayers:s.nearbyPlayers,nearbyLocs:s.nearbyLocs?.slice(0,45),gameMessages:s.gameMessages?.slice(-18),menu:s.menu,dialog:s.dialog};
}
Bun.serve({port:8790,idleTimeout:120,fetch:async req=>{
 try{
  const path=new URL(req.url).pathname;
  if(req.method==='GET'&&path==='/health')return Response.json({ready:latest.ready,names,phase:clock.phase},{status:latest.ready?200:503});
  if(req.method==='GET'&&path==='/state')return Response.json({...latest,clock,scenario});
  if(req.method==='POST'&&path==='/arm'){
   if(started||!latest.ready)return new Response('Not ready or already started',{status:409});
   started=true;
   for(const name of names){
    const sdk=new BotSDK({botUsername:name,password:'test',gatewayUrl:'ws://localhost:7780',connectionMode:'control',autoLaunchBrowser:false,autoReconnect:false,actionTimeout:20000});
    await sdk.connect();controllers.set(name,{sdk,bot:new BotActions(sdk)});
   }
   const b=await req.json();const result=await engine('/arm',{duration_seconds:b.duration_seconds});scenario.duration_seconds=b.duration_seconds;startAt=Date.now();clock=await engine('/status');
   event('episode.started',null,{scenario,baseline:result.baseline});
   for(const [i,name] of names.entries()){
    const recorder=Bun.spawn(['ffmpeg','-nostdin','-y','-f','x11grab','-framerate','2','-video_size','800x600','-i',`:${99+i}`,'-an','-c:v','libx264','-threads','1','-preset','ultrafast','-crf','27','-pix_fmt','yuv420p','-movflags','+frag_keyframe+empty_moov+default_base_moof',`/logs/ma/${name}.mp4`],{stdout:'ignore',stderr:Bun.file(`/logs/ma/record-${name}.log`)});
    recorders.push(recorder);event('media.started',name,{path:`${name}.mp4`,offsetMs:Date.now()-startAt});
   }
   return Response.json({scenario,...result,tokens:Object.fromEntries(tokens)});
  }
  if(req.method==='POST'&&path==='/stop'){const result=await engine('/stop',{});return Response.json(result);}
  const match=path.match(/^\/actors\/([a-z0-9]+)\/(observation|action)$/);
  if(match){
   const [,name,op]=match;
   if(!tokens.has(name)||req.headers.get('authorization')!==`Bearer ${tokens.get(name)}`)return new Response('Wrong actor capability',{status:403});
   if(op==='observation'&&req.method==='GET')return Response.json(privateView(name));
   if(op==='action'&&req.method==='POST'){
    clock=await engine('/status');
    if(clock.phase!=='running'||clock.remainingMs<=0)return new Response('Episode closed',{status:409});
    if(busy.has(name))return new Response('Actor busy',{status:409});
    const a=await req.json();
    if(!['say','chop','walk','drop','wait'].includes(a.type))return new Response('Unsupported action',{status:400});
    const c=controllers.get(name)!;
    const s=c.sdk.getState();
    if(a.type==='say'&&(typeof a.text!=='string'||a.text.length>80))return new Response('Chat must be at most 80 characters',{status:400});
    if(a.type==='walk'&&(!Number.isInteger(a.x)||!Number.isInteger(a.z)||Math.abs(a.x-s.player.worldX)>32||Math.abs(a.z-s.player.worldZ)>32))return new Response('Walk within 32 tiles',{status:400});
    if(a.type==='drop'&&(!Number.isInteger(a.slot)||!s.inventory.some((i:any)=>i.slot===a.slot&&/logs/i.test(i.name))))return new Response('Only logs may be dropped',{status:400});
    let tree:any;
    if(a.type==='chop'){
     tree=a.x!==undefined?s.nearbyLocs.find((l:any)=>/^tree$/i.test(l.name)&&l.x===a.x&&l.z===a.z):c.sdk.findNearbyLoc(/^tree$/i);
     if(!tree)return Response.json({success:false,message:'No normal tree at requested location'});
    }
    busy.add(name);event('action.started',name,{action:a,target:tree?{x:tree.x,z:tree.z,id:tree.id}:null,privateObservation:privateView(name)});
    try{
     const result=a.type==='say'?await c.sdk.say(a.text):a.type==='walk'?await c.bot.walkTo(a.x,a.z,1):a.type==='chop'?await c.bot.chopTree(tree):a.type==='drop'?await c.sdk.sendDropItem(a.slot):await c.sdk.sendWait(2);
     event('action.completed',name,{action:a,result});return Response.json(result);
    }finally{busy.delete(name);}
   }
  }
  return new Response('RuneBench MA: /health /state',{status:path==='/'?200:404});
 }catch(e){return Response.json({error:String(e)},{status:500});}
}});
await Promise.all(names.map(async name=>{
 const sdk=new BotSDK({botUsername:name,password:'test',gatewayUrl:'ws://localhost:7780',connectionMode:'observe',autoLaunchBrowser:false,autoReconnect:true});
 await sdk.connect();observers.set(name,sdk);
}));
let sealed=false;
async function sample(){
 try{
  clock=await engine('/status');
  const snapshots=Object.fromEntries(names.map(name=>[name,observers.get(name)?.getState()??null]));
  latest={timestamp:new Date().toISOString(),elapsedMs:startAt?Date.now()-startAt:0,ready:names.every(n=>snapshots[n]?.inGame),bots:snapshots};
  if(clock.phase==='running'){
   appendFileSync('/logs/ma/states.jsonl',JSON.stringify(latest)+'\n');
   for(const [name,s] of Object.entries<any>(snapshots))for(const m of s?.gameMessages??[]){
    if(!m.sender)continue;
    const key=`${name}|${m.sender}|${m.tick}|${m.type}|${m.text}`;if(seen.has(key))continue;seen.add(key);
    event('message.observed',name,{sender:m.sender,text:m.text,clientTick:m.tick,messageType:m.type,visibility:'actor-observation'});
   }
  }
  writeFileSync('/logs/ma/latest.json',JSON.stringify(latest));
  if(clock.phase==='frozen'&&!sealed){
   sealed=true;event('episode.frozen',null,{cutoff:clock.cutoff});
   for(const p of recorders)p.kill('SIGINT');
   await Promise.all([...controllers.values()].map(c=>c.sdk.disconnect()));
  }
 }catch(e){console.error('Observer:',String(e));}
}
await sample();setInterval(sample,1000);

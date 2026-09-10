import {readFileSync,writeFileSync,appendFileSync,renameSync} from 'node:fs';
import {performance} from 'node:perf_hooks';
const scenario=JSON.parse(readFileSync(process.env.MA_SCENARIO_FILE||'/app/ma/scenario.json','utf8'));
const logDir=process.env.MA_LOG_DIR||'/logs/ma';
// Imported in the engine process before World.start(). Never available to policies.
export function installMaClock(world:any) {
 let phase='idle', start=0, deadline=0, baseline:any=null, last:any=null, cutoff:any=null;
 const atomic=(name:string,value:any)=>{writeFileSync(`${logDir}/${name}.tmp`,JSON.stringify(value));renameSync(`${logDir}/${name}.tmp`,`${logDir}/${name}`)};
 function snapshot(){
  const players=world.players.filter(Boolean);
  const actors=Object.fromEntries(scenario.actors.map(a=>{
   const p=players.find((p:any)=>p.username===a.id);
   return [a.id,p?{xpTenths:p.stats[8],xp:p.stats[8]/10,x:p.x,z:p.z,level:p.level,loggedIn:true}:null];
  }));
  return {tick:world.currentTick,elapsedMs:start?performance.now()-start:0,at:new Date().toISOString(),actors};
 }
 function finish(reason:string){
  if(phase!=='running')return;
  phase='frozen';
  const scores=Object.fromEntries(scenario.actors.map(a=>[a.id,last?.actors[a.id]&&baseline?.actors[a.id]?(last.actors[a.id].xpTenths-baseline.actors[a.id].xpTenths)/10:null]));
  const teams=Object.fromEntries([...new Set(scenario.actors.map(a=>a.team))].map(t=>[t,scenario.actors.filter(a=>a.team===t).every(a=>scores[a.id]!==null)?scenario.actors.filter(a=>a.team===t).reduce((s,a)=>s+scores[a.id],0):null]));
  cutoff={phase,reason,rule:'last-complete-engine-tick-at-or-before-deadline',durationMs:deadline-start,freezeElapsedMs:performance.now()-start,baseline,final:last,scores,teams,valid:reason==='deadline'&&Object.values(scores).every(v=>v!==null)};
  atomic('cutoff.json',cutoff);
 }
 const original=world.cycle.bind(world);
 world.cycle=function(){
  if(phase==='frozen')return;
  if(phase==='running'&&performance.now()>=deadline){finish('deadline');return;}
  original();
  if(phase==='running'){
   const s=snapshot();
   if(performance.now()<=deadline){last=s;appendFileSync(`${logDir}/engine-states.jsonl`,JSON.stringify(s)+'\n');}
   else finish('deadline');
  }
 };
 return Bun.serve({hostname:'127.0.0.1',port:Number(process.env.MA_CLOCK_PORT??8792),fetch:async req=>{
  const path=new URL(req.url).pathname;
  if(path==='/status')return Response.json({phase,remainingMs:phase==='running'?Math.max(0,deadline-performance.now()):0,snapshot:snapshot(),cutoff});
  if(req.method==='POST'&&path==='/arm'){
   if(phase!=='idle')return new Response('Already armed',{status:409});
   const b=await req.json();const seconds=b.duration_seconds;
   if(!Number.isInteger(seconds)||seconds<10||seconds>600)return new Response('Duration must be 10–600 seconds',{status:400});
   baseline=snapshot();if(Object.values(baseline.actors).some(v=>!v))return new Response('Players not ready',{status:409});
   // The pinned engine uses level/10 in its XP curve, not modern RS level/7.
   if(scenario.actors.some(a=>baseline.actors[a.id].x!==a.position.x||baseline.actors[a.id].z!==a.position.z||baseline.actors[a.id].xp!==980))return new Response('Scenario save mismatch: expected start positions and level-10 Woodcutting (980 XP in this engine)',{status:409});
   start=performance.now();deadline=start+seconds*1000;baseline.elapsedMs=0;last=baseline;phase='running';
   atomic('baseline.json',baseline);return Response.json({phase,durationMs:seconds*1000,baseline});
  }
  if(req.method==='POST'&&path==='/stop'){finish('operator-stop');return Response.json(cutoff);}
  return new Response('Not found',{status:404});
 }});
}

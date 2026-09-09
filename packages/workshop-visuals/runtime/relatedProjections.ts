type ObjectValue=Record<string,unknown>;
const object=(value:unknown):ObjectValue=>value!==null && typeof value==="object" && !Array.isArray(value)?value as ObjectValue:{};
const check=(signal:AbortSignal)=>{if(signal.aborted)throw new DOMException("Projection resolution cancelled","AbortError");};
export type SealedTrialProjection={trialId:string;rolloutId:string|null;digest:string;projection:unknown};
export type TraceProjectionReadPort=(digest:string)=>Promise<{traceDigest:string;projectionKind:string;projectionSchema:string;payload:unknown}>;

/** Resolve only explicitly inspectable terminal references. Shared digests are
 * loaded once and concurrency is bounded; no guessed trace or trial identities. */
export async function resolveSealedTrialProjections(events:unknown[],read:TraceProjectionReadPort,signal:AbortSignal):Promise<SealedTrialProjection[]>{
 check(signal);
 const refs:Omit<SealedTrialProjection,"projection">[]=[];
 const seen=new Set<string>();
 for(const value of events){
  const event=object(value);if((event.type??event.eventType)!=="eval.trial.terminal")continue;
  const item=object(event.item),record=object(item.raw??item),sealed=object(record.sealedTrace??record.sealed_trace);
  if(sealed.inspectable!==true||!Array.isArray(sealed.traces))continue;
  const trial=object(event.delta).trial_id??record.trialId??item.id;
  if(typeof trial!=="string"||!trial)continue;
  for(const value of sealed.traces){
   const digest=object(value).digest;if(typeof digest!=="string"||!digest)continue;
   const rolloutId=typeof record.rolloutId==="string"?record.rolloutId:null;
   const key=JSON.stringify([trial,rolloutId,digest]);if(seen.has(key))continue;seen.add(key);
   refs.push({trialId:trial,rolloutId,digest});
  }
 }
 const digests=[...new Set(refs.map(ref=>ref.digest))],payloads=new Map<string,unknown>();
 let cursor=0;
 await Promise.all(Array.from({length:Math.min(4,digests.length)},async()=>{
  while(cursor<digests.length){
   check(signal);const digest=digests[cursor++]!;const result=await read(digest);check(signal);
   if(result.traceDigest!==digest||result.projectionKind!=="rollout-inspector"||result.projectionSchema!=="synth.trace-projection.rollout-inspector.v1")throw new Error(`Sealed trace projection identity changed for ${digest}`);
   payloads.set(digest,result.payload);
  }
 }));
 check(signal);return refs.map(ref=>({...ref,projection:payloads.get(ref.digest)}));
}

export async function resolveComparisonProjection<T extends {id:string;createdAt?:string|null},V>(runId:string,ports:{list:()=>Promise<T[]>;view:(id:string)=>Promise<V>},signal:AbortSignal):Promise<{run:T;runViewV2:V}|null>{
 check(signal);const prefix=(id:string)=>id.split("_").slice(0,2).join("_");
 const runs=await ports.list();check(signal);
 const sibling=runs.filter(run=>run.id!==runId&&prefix(run.id)===prefix(runId))
  .sort((a,b)=>(Date.parse(b.createdAt??"")||0)-(Date.parse(a.createdAt??"")||0)||a.id.localeCompare(b.id))[0];
 if(!sibling)return null;
 const runViewV2=await ports.view(sibling.id);check(signal);return {run:sibling,runViewV2};
}

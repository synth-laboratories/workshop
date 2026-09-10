import { useEffect, useMemo, useState } from "react";
import type { AggregateResult, CohortRef, CorpusRef, SamplingReceipt, SamplingStrategy } from "@synth/visuals-protocol";
import { RemoteCorpus, stableValueDigest, type VisualSessionClient } from "@synth/visuals-sdk";
import { swarmQuery, type AgentTrajectory, type SwarmFilters } from "./swarm.ts";

type Projection={cohort:CohortRef;outcomes:AggregateResult;behaviors:AggregateResult;models:AggregateResult;rewards:AggregateResult;sample:{rows:AgentTrajectory[];receipt:SamplingReceipt};selected?:AgentTrajectory};
const hydration=new WeakMap<VisualSessionClient,Map<string,Promise<void>>>();

export function useSwarmProjection(input:{client:VisualSessionClient|null;source:CorpusRef;current:CorpusRef;rows:AgentTrajectory[];filters:SwarmFilters;label:string;history:Array<{filters:SwarmFilters;label:string}>;strategy:SamplingStrategy;selectedId?:string}) {
  const {client,source,current,rows,filters,label,history,strategy,selectedId}=input;
  const key=stableValueDigest({source,filters,label,history,strategy,selectedId});
  const [result,setResult]=useState<{key:string;projection?:Projection;error?:string}>();
  const backend=useMemo(()=>client?new RemoteCorpus<AgentTrajectory>(source,client.analyticalRequest):null,[client,source.id,source.revision]);
  useEffect(()=>{
    if(!client||!backend)return;
    const abort=new AbortController();
    void(async()=>{
      let uploads=hydration.get(client);if(!uploads){uploads=new Map();hydration.set(client,uploads);}
      let uploaded=uploads.get(current.revision);
      if(!uploaded){
        uploaded=new RemoteCorpus<AgentTrajectory>(current,client.analyticalRequest).ingest(rows);
        uploads.set(current.revision,uploaded);
        void uploaded.catch(()=>uploads!.delete(current.revision));
        if(uploads.size>8)uploads.delete(uploads.keys().next().value!);
      }
      await uploaded;abort.signal.throwIfAborted();
      // History is navigation intent. Counts and membership always come from the
      // pinned read model, not mutable copies embedded in presentation state.
      let parent:CohortRef|undefined;
      for(const view of history)parent=await backend.cohort(view.label,swarmQuery(view.filters),parent,abort.signal);
      const cohort=await backend.cohort(label,swarmQuery(filters),parent,abort.signal);
      const [outcomes,behaviors,models,rewards,sample]=await Promise.all([
        backend.aggregate(cohort,"outcome",abort.signal),backend.aggregate(cohort,"behaviors",abort.signal),backend.aggregate(cohort,"model",abort.signal),backend.aggregate(cohort,"rewardBand",abort.signal),
        backend.sample(cohort,strategy,8,{seed:10,scoreField:"reward",failureField:"failed"},abort.signal),
      ]);
      let selected=sample.rows.find(row=>row.id===selectedId)??sample.rows[0];
      if(selectedId&&!sample.rows.some(row=>row.id===selectedId)){
        const query={...cohort.query,where:{op:"and" as const,expressions:[cohort.query.where,{op:"eq" as const,field:"id",value:selectedId}]}};
        selected=(await backend.query(query,{offset:0,limit:1},abort.signal)).rows[0]??selected;
      }
      if(!abort.signal.aborted)setResult({key,projection:{cohort,outcomes,behaviors,models,rewards,sample,selected}});
    })().catch(error=>{if(!abort.signal.aborted)setResult({key,error:error instanceof Error?error.message:String(error)});});
    return()=>abort.abort();
  },[client,backend,key,current.revision]);
  return result?.key===key?result:undefined;
}

import {useEffect,useMemo,useRef,useState} from "react";
import {canonicalDigest,stableSerialize} from "@synth/visuals-sdk";
import {useVisualState,useVisualSessionClient,useVisualSessionSnapshot} from "./session.ts";

/** Immutable derived render cuts. Original domain stores remain authoritative.
 * The digest is ordinary committed presentation intent, so checkpoints and
 * recording transitions retain it without copying evidence into each event. */
export function useVisualEvidence<T>(owner:string,input:T|undefined):{value:T|undefined;ready:boolean;error?:string}{
 if(!/^[a-zA-Z0-9_.-]{1,80}$/.test(owner))throw new Error("Invalid evidence owner");
 const client=useVisualSessionClient(),session=useVisualSessionSnapshot();
 const [cut]=useVisualState<string|null>(`source.${owner}`,null,{type:"string",nullable:true});
 // Standalone viewers have no evidence-cut store. Serializing their entire
 // retained journal on every render does no work for them and stalls replay.
 const key=useMemo(()=>!client?.supportsEvidenceCuts||input===undefined?undefined:stableSerialize(input),[client?.supportsEvidenceCuts,input]);
 const value=useMemo(()=>input,[key]);
 const [loaded,setLoaded]=useState<{digest:string;value:T}>();
 const [error,setError]=useState<string>();
 const pending=useRef<{client:NonNullable<typeof client>;key:string;value:T}|null>(null);
 const draining=useRef(false);
 const activeClient=useRef(client);
 activeClient.current=client;
 const replay=Boolean(session?.state.replay);
 const capture=document.documentElement.hasAttribute('data-synth-review-capture')
  || document.documentElement.hasAttribute('data-synth-capture');
 useEffect(()=>{
  // A coherent pixel cut is pinned to the native checkpoint's evidence
  // digest. It must read that cut, never commit a fresher live-poll value while
  // the capture barrier holds the presentation frozen.
  if(!client?.supportsEvidenceCuts||!session?.ready||value===undefined||replay||capture)return;
  pending.current={client,key:key!,value};setError(undefined);
  if(draining.current)return;
  draining.current=true;
  void(async()=>{
   // Live streams can advance faster than a native evidence write. Keep one
   // write in flight and replace the queued value with the newest fold instead
   // of building an unbounded queue of obsolete prefixes.
   while(pending.current){
    const next=pending.current;pending.current=null;
    const nextClient=next.client;
    try{
     const digest=await canonicalDigest(next.value);
     if(nextClient.getSnapshot().state.values[`source.${owner}`]!==digest){
      const committed=await nextClient.commitEvidence(`source.${owner}`,next.value);
      if(committed===undefined)continue;
      if(committed!==digest)throw new Error("Evidence digest disagrees with native host");
     }
     if(activeClient.current===nextClient&&!nextClient.getSnapshot().state.replay)setLoaded({digest,value:next.value});
    }catch(reason){
     if(activeClient.current===nextClient)setError(String(reason));
    }
   }
  })().finally(()=>{draining.current=false;});
 },[client,session?.ready,owner,key,replay,capture]);
 useEffect(()=>{
  if(!client?.supportsEvidenceCuts||!cut||loaded?.digest===cut)return;
  let cancelled=false;setError(undefined);
  void(async()=>{
   const answer=await client.analyticalRequest({operation:"evidence.read",digest:cut});
   const value=answer.value as T;
   if(await canonicalDigest(value)!==cut)throw new Error("Restored evidence digest mismatch");
   if(!cancelled)setLoaded({digest:cut,value});
  })().catch(reason=>{if(!cancelled)setError(String(reason));});
  return()=>{cancelled=true;};
 },[client,cut,loaded?.digest]);
 if(!client?.supportsEvidenceCuts)return {value:input,ready:true};
 const restored=loaded?.digest===cut?loaded.value:undefined;
 return {value:restored,ready:!error&&restored!==undefined&&(replay||capture||key===stableSerialize(restored)),error};
}

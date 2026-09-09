import {useEffect,useMemo,useState} from "react";
import {canonicalDigest,stableSerialize} from "@synth/visuals-sdk";
import {useVisualState,useVisualSessionClient,useVisualSessionSnapshot} from "./session.ts";

/** Immutable derived render cuts. Original domain stores remain authoritative.
 * The digest is ordinary committed presentation intent, so checkpoints and
 * recording transitions retain it without copying evidence into each event. */
export function useVisualEvidence<T>(owner:string,input:T|undefined):{value:T|undefined;ready:boolean;error?:string}{
 if(!/^[a-zA-Z0-9_.-]{1,80}$/.test(owner))throw new Error("Invalid evidence owner");
 const client=useVisualSessionClient(),session=useVisualSessionSnapshot();
 const [cut]=useVisualState<string|null>(`source.${owner}`,null,{type:"string",nullable:true});
 const key=input===undefined?undefined:stableSerialize(input);
 const value=useMemo(()=>input,[key]);
 const [loaded,setLoaded]=useState<{digest:string;value:T}>();
 const [error,setError]=useState<string>();
 const replay=Boolean(session?.state.replay);
 useEffect(()=>{
  if(!client?.supportsEvidenceCuts||!session?.ready||value===undefined||replay)return;
  let cancelled=false;setError(undefined);
  void(async()=>{
   const digest=await canonicalDigest(value);
   if(cancelled)return;
   if(client.getSnapshot().state.values[`source.${owner}`]===digest){setLoaded({digest,value});return;}
   const committed=await client.commitEvidence(`source.${owner}`,value);
   if(committed===undefined)return;
   if(committed!==digest)throw new Error("Evidence digest disagrees with native host");
   if(cancelled||client.getSnapshot().state.replay)return;
   setLoaded({digest,value});
  })().catch(reason=>{if(!cancelled)setError(String(reason));});
  return()=>{cancelled=true;};
 },[client,session?.ready,owner,key,replay]);
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
 return {value:restored,ready:!error&&restored!==undefined&&(replay||key===stableSerialize(restored)),error};
}

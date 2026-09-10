import {useEffect,useState} from "react";
import {stableSerialize} from "@synth/visuals-sdk";
import {useVisualEvidence,useVisualSessionClient,useVisualSessionSnapshot} from "@synth/visuals-react";
import {publicError} from "../runtime/publicError";
import {retainTemplatePages,restoreTemplatePages} from "./templateEvidencePages";

export function useTemplateEvidence<T>(input:T):{value?:T;ready:boolean;error?:string}{
 const client=useVisualSessionClient(),session=useVisualSessionSnapshot();
 const replay=Boolean(session?.state.replay),enabled=Boolean(client?.supportsEvidenceCuts);
 const key=stableSerialize(input);
 const [prepared,setPrepared]=useState<{key:string;value:unknown}>();
 const [restored,setRestored]=useState<{key:string;value:T}>();
 const [error,setError]=useState<string>();
 useEffect(()=>{
  if(!client||!enabled||!session?.ready||replay)return;
  let cancelled=false;setError(undefined);
  void retainTemplatePages(input,client.analyticalRequest).then(value=>{
   if(!cancelled)setPrepared({key,value});
  }).catch(reason=>{if(!cancelled)setError(publicError(reason));});
  return()=>{cancelled=true;};
 },[client,enabled,session?.ready,replay,key]);
 const cut=useVisualEvidence("template",prepared?.key===key?prepared.value:undefined);
 const cutKey=cut.value===undefined?undefined:stableSerialize(cut.value);
 useEffect(()=>{
  if(!client||!cut.ready||cutKey===undefined)return;
  let cancelled=false;setError(undefined);
  void restoreTemplatePages<T>(cut.value,client.analyticalRequest).then(value=>{
   if(!cancelled)setRestored({key:cutKey,value});
  }).catch(reason=>{if(!cancelled)setError(publicError(reason));});
  return()=>{cancelled=true;};
 },[client,cut.ready,cutKey]);
 if(!enabled)return {value:input,ready:true};
 return {value:restored?.key===cutKey?restored?.value:undefined,
  ready:!error&&cut.ready&&cutKey!==undefined&&restored?.key===cutKey,
  error:error??cut.error};
}

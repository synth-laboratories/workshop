import {useEffect,type RefObject} from "react";
import type {JsonValue,VisualAction,VisualControl} from "@synth/visuals-protocol";
import {assertJson} from "@synth/visuals-sdk";
import {useVisualSessionClient,useVisualSessionSnapshot} from "./session.ts";

/** Capability-limited bridge for an opaque-origin sandbox. Source-window
 * identity, a bounded message envelope and a control namespace are mandatory.
 * No host transport, credentials, bindings, filesystem or effect ports cross it. */
export function useFrameSession(frame:RefObject<HTMLIFrameElement|null>,loaded:boolean,namespace="frame."){
  const client=useVisualSessionClient(),snapshot=useVisualSessionSnapshot();
  useEffect(()=>{
    if(!client||!loaded)return;
    const receive=(event:MessageEvent)=>{
      const target=frame.current?.contentWindow;
      if(!target||event.source!==target)return;
      const data=event.data;
      if(!data||data.type!=="synth.visual.session.request.v1")return;
      const answer=(result:unknown,error?:string)=>target.postMessage({type:"synth.visual.session.response.v1",requestId:typeof data.requestId==="string"?data.requestId:null,result,error},"*");
      try{
        assertJson(data);
        if (!data || typeof data !== "object" || Array.isArray(data)) throw new Error("Frame session request must be an object");
        if(!data||typeof data!=="object"||Array.isArray(data))throw new Error("Frame message must be an object");
        if(JSON.stringify(data).length>65536)throw new Error("Frame session message exceeds 64 KiB");
        if(typeof data.requestId!=="string"||data.requestId.length>200)throw new Error("Frame request identity required");
        if(data.operation==="register"){
          if(!Array.isArray(data.controls)||data.controls.length>64)throw new Error("At most 64 frame controls may be registered");
          const defaults=data.defaults as Record<string,JsonValue>;
          if(!defaults||Array.isArray(defaults)||typeof defaults!=="object")throw new Error("Frame control defaults required");
          const entries=(data.controls as VisualControl[]).map(control=>{
            if(typeof control?.id!=="string"||!control.id.startsWith(namespace))throw new Error("Frame control outside namespace");
            return {control,initial:defaults[control.id]!};
          });
          client.registerMany(entries);
          answer({accepted:true});
        }else if(data.operation==="act"){
          const action=data.action as VisualAction;
          const ids=action.kind==="presentation.set"?[action.target?.id]:action.kind==="presentation.patch"?Object.keys(action.payload?.values??{}):[];
          if(ids.length===0||ids.some(id=>!id?.startsWith(namespace)))throw new Error("Frame action outside presentation namespace");
          void client.dispatch(action).then(receipt=>answer({commandId:receipt.commandId}),error=>answer(null,String(error)));
        }else throw new Error("Unsupported frame session operation");
      }catch(error){answer(null,error instanceof Error?error.message:String(error));}
    };
    window.addEventListener("message",receive);return()=>window.removeEventListener("message",receive);
  },[client,loaded,frame,namespace]);
  useEffect(()=>{
    if(!loaded||!snapshot?.ready)return;
    const {visualId,revision,viewKey,stateVersion,values,controls}=snapshot.state;
    frame.current?.contentWindow?.postMessage({type:"synth.visual.session.state.v1",state:{visualId,revision,viewKey,stateVersion,values:Object.fromEntries(Object.entries(values).filter(([id])=>id.startsWith(namespace))),controls:controls.filter(control=>control.id.startsWith(namespace))}},"*");
  },[loaded,snapshot,frame,namespace]);
}

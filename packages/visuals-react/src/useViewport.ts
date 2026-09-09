import { useRef, useState, type PointerEvent, type KeyboardEvent } from "react";
import { useVisualState } from "./session.ts";

export type ViewportTransform = { scale:number; x:number; y:number };

/** Motion stays ephemeral. A completed gesture commits exactly one action. */
export function useViewport({id="viewport",initial={scale:1,x:0,y:0},minScale=.25,maxScale=4,onChange}: {
  id?:string;initial?:ViewportTransform;minScale?:number;maxScale?:number;onChange?:(value:ViewportTransform)=>void;
} = {}) {
  const [value,setValue]=useVisualState(id,initial,{label:"Viewport transform",required:["x","y","scale"],additionalProperties:false,properties:{x:{type:"number"},y:{type:"number"},scale:{type:"number",minimum:minScale,maximum:maxScale}}});
  const [draft,setDraft]=useState<ViewportTransform>();
  const drag=useRef<{pointerId:number;clientX:number;clientY:number;start:ViewportTransform}|undefined>(undefined);
  const commit=(next:ViewportTransform)=>{setValue(next);onChange?.(next);};
  const zoom=(delta:number)=>setValue(previous=>{const next={...previous,scale:Math.max(minScale,Math.min(maxScale,previous.scale+delta))};onChange?.(next);return next;});
  const fit=()=>commit({scale:1,x:0,y:0});
  const position=(event:PointerEvent<HTMLDivElement>):ViewportTransform|undefined=>{
    const start=drag.current;if(!start||start.pointerId!==event.pointerId)return;
    return {...start.start,x:start.start.x+event.clientX-start.clientX,y:start.start.y+event.clientY-start.clientY};
  };
  const cancel=()=>{drag.current=undefined;setDraft(undefined);};
  return {transform:draft??value,zoom,fit,stageProps:{
    tabIndex:0,
    onKeyDown(event:KeyboardEvent<HTMLDivElement>){if(event.key==="+"||event.key==="="){event.preventDefault();zoom(.15);}else if(event.key==="-"){event.preventDefault();zoom(-.15);}else if(event.key==="0"){event.preventDefault();fit();}},
    onPointerDown(event:PointerEvent<HTMLDivElement>){if(event.button!==0)return;drag.current={pointerId:event.pointerId,clientX:event.clientX,clientY:event.clientY,start:value};event.currentTarget.setPointerCapture(event.pointerId);},
    onPointerMove(event:PointerEvent<HTMLDivElement>){const next=position(event);if(next)setDraft(next);},
    onPointerUp(event:PointerEvent<HTMLDivElement>){const next=position(event);if(next)commit(next);cancel();if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);},
    onPointerCancel:cancel,onLostPointerCapture:cancel,
  }};
}

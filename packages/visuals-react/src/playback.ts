import { useEffect, useRef } from "react";
import type { JsonValue } from "@synth/visuals-protocol";
import { useVisualSessionClient } from "./session.ts";

/** A renderer proposes a logical tick; the host serializes clocks across panes.
 * The projection is pure and all cursor/stop updates commit in one event. Never
 * use a React state updater to trigger another state update during playback. */
export function useVisualPlaybackDriver(options:{
  clock:string; playingControl:string; playing:boolean; intervalMs:number;
  project:(values:Readonly<Record<string,JsonValue>>)=>Record<string,JsonValue>;
  fallback:()=>void;
}) {
  const client=useVisualSessionClient();
  const latest=useRef(options); latest.current=options;
  const intervalMs=Math.max(16,Math.min(60_000,Math.round(options.intervalMs)));
  useEffect(()=>{
    if(!options.playing || !Number.isFinite(intervalMs))return;
    let pending=false, disposed=false;
    const timer=setInterval(()=>{
      if(pending||disposed)return;
      if(!client){latest.current.fallback();return;}
      pending=true;
      void client.playbackTick(options.clock,options.playingControl,intervalMs,values=>latest.current.project(values))
        .catch(()=>{}) // Shared toolbar reports transport/persistence failures.
        .finally(()=>{pending=false;});
    },intervalMs);
    return()=>{disposed=true;clearInterval(timer);};
  },[client,options.clock,options.playingControl,options.playing,intervalMs]);
}

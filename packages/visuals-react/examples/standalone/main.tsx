import React,{useRef,useState} from "react";
import { createRoot } from "react-dom/client";
import { checkpoint, initialSession, VisualSession, VisualSessionClient, InMemoryCorpus, allQuery } from "@synth/visuals-sdk";
import { VisualSessionProvider, VisualSessionToolbar, VisualViewport, useVisualState,useFrameSession } from "@synth/visuals-react";
import type { SessionCheckpoint, SessionRecording, VisualAction, VisualControl, JsonValue } from "@synth/visuals-protocol";

// This deliberately has no Workshop imports, native bridge, credentials, or
// filesystem authority. Reload resets its in-memory reference host.
const identity={visualId:"standalone",revision:1,viewKey:"default"};
const definition={id:"portable-demo",version:"1"};
const session=new VisualSession(initialSession(identity,definition));
const snapshots=new Map<string,SessionCheckpoint>();
const recordings=new Map<string,SessionRecording>();
let active:string|undefined;
let lastReplayTick=0;
let replayTickPending=false;
const transport={
  subscribe:session.subscribe,
  async request(request:Record<string,unknown>):Promise<Record<string,unknown>>{
    switch(request.operation){
      case "attach":for(const control of request.controls as VisualControl[])session.register(control,(request.defaults as Record<string,JsonValue>)[control.id]!);break;
      case "inspect":case "publish":break;
      case "act":return session.execute(request.action as VisualAction);
      case "capture":{const saved=await checkpoint(session.state);snapshots.set(saved.id,saved);return {checkpoint:saved};}
      case "checkpoints":return {items:[...snapshots.values()]};
      case "checkpoint.read":return {checkpoint:snapshots.get(String(request.checkpointId))};
      case "checkpoint.import":{const saved=request.checkpoint as SessionCheckpoint;await import("@synth/visuals-sdk").then(api=>api.verifyCheckpoint(saved));snapshots.set(saved.id,saved);return {checkpoint:saved};}
      case "restore":await session.restore(snapshots.get(String(request.checkpointId))!);break;
      case "record.start":{const record=await session.startRecording();active=record.id;recordings.set(record.id,record);return {recordingId:record.id};}
      case "record.stop":{const record=session.stopRecording();if(record)recordings.set(record.id,record);active=undefined;break;}
      case "recordings":return {items:[...recordings.values()].map(record=>({id:record.id,createdAt:record.initial.capturedAt,eventCount:record.events.length}))};
      case "record.read":return {recording:recordings.get(String(request.recordingId))};
      case "record.seek":await session.seekRecording(recordings.get(String(request.recordingId))!,Number(request.sequence),Number(request.expectedStateVersion));break;
      case "record.play":session.playRecording(Boolean(request.playing),Number(request.expectedStateVersion));break;
      case "record.tick":{
        const replay=session.state.replay;
        if(!replayTickPending && performance.now()-lastReplayTick>=300 && replay && "recordingId" in replay && replay.playing && request.expectedStateVersion===session.state.stateVersion){
          replayTickPending=true;
          try{await session.seekRecording(recordings.get(replay.recordingId)!,replay.sequence+1,Number(request.expectedStateVersion),true);lastReplayTick=performance.now();}
          finally{replayTickPending=false;}
        }
        break;
      }
      default:throw new Error(`Unsupported demonstration operation ${request.operation}`);
    }
    return {state:session.state,activeRecording:active};
  }
};
const clients=[0,1].map(()=>new VisualSessionClient(initialSession(identity,definition),transport));
const rows=Array.from({length:1000},(_,index)=>({id:`row-${index}`,group:index%4===0?"flagged":"ordinary",score:index/1000}));
const corpus=new InMemoryCorpus({id:"example",schema:"example.row.v1",rows});

function FrameExample(){
  const frame=useRef<HTMLIFrameElement>(null);const [loaded,setLoaded]=useState(false);useFrameSession(frame,loaded);
  return <iframe ref={frame} title="Sandbox control" sandbox="allow-scripts" onLoad={()=>setLoaded(true)} srcDoc={`<!doctype html><button disabled>Increment sandbox counter</button><output>Waiting</output><script>
    let state;
    const send=(operation,fields)=>parent.postMessage({type:"synth.visual.session.request.v1",requestId:String(Math.random()),operation,...fields},"*");
    addEventListener("message",event=>{if(event.source!==parent)return;const data=event.data;if(data.type==="synth.visual.session.state.v1"){
      state=data.state;if(Object.hasOwn(state.values,"frame.count")){document.querySelector("output").textContent=String(state.values["frame.count"]);document.querySelector("button").disabled=false;}
      else send("register",{controls:[{id:"frame.count",label:"Sandbox count",type:"number",minimum:0}],defaults:{"frame.count":0}});
    }});
    document.querySelector("button").onclick=()=>send("act",{action:{id:String(Math.random()),kind:"presentation.patch",expectedStateVersion:state.stateVersion,payload:{values:{"frame.count":state.values["frame.count"]+1}}}});
  </script>`}/>;
}

function Example({name}:{name:string}){
  const [filter,setFilter]=useVisualState("filter","all",{options:["all","flagged"]});
  const [step,setStep]=useVisualState("step",0,{minimum:0,maximum:999,clock:"example.sequence"});
  const query=filter==="all"?allQuery():{...allQuery(),where:{op:"eq" as const,field:"group",value:"flagged"}};
  const result=corpus.query(query,{offset:0,limit:8});
  return <section aria-label={name} style={{border:"1px solid #bbb",padding:16,flex:1,minWidth:350}}>
    <h2>{name}</h2><VisualSessionToolbar/>
    <label>Population <select aria-label="Population" value={filter} onChange={event=>setFilter(event.target.value)}><option value="all">All</option><option value="flagged">Flagged</option></select></label>
    <p role="status">{result.total} of 1,000 rows; displaying {result.rows.length}</p>
    <label>Logical step <input aria-label="Logical step" type="range" min={0} max={999} value={step} onChange={event=>setStep(Number(event.target.value))}/></label><output>{step}</output>
    <VisualViewport><svg width="300" height="100" role="img" aria-label="Static diagram"><rect x="5" y="20" width="90" height="40" fill="#b8d8f0"/><path d="M95 40H180" stroke="black"/><rect x="180" y="20" width="110" height="40" fill="#bee6c5"/><text x="15" y="45">Input</text><text x="190" y="45">Projection</text></svg></VisualViewport>
    <ul>{result.rows.map(row=><li key={row.id}>{row.id}: {row.group}</li>)}</ul><FrameExample/>
  </section>;
}
createRoot(document.getElementById("root")!).render(<React.StrictMode><main style={{fontFamily:"system-ui",padding:24}}><h1>Portable visuals engine</h1><p>Two independent clients, one authoritative in-memory session. No Workshop dependencies. Reload resets this demo.</p><div style={{display:"flex",gap:20,flexWrap:"wrap"}}>{clients.map((client,index)=><VisualSessionProvider key={index} client={client}><Example name={`Client ${index+1}`}/></VisualSessionProvider>)}</div></main></React.StrictMode>);

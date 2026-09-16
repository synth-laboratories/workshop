import { useMemo, useState } from "react";
import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../../chrome/useLiveEvalStream.ts";
import type { LiveTemplateProps } from "../../../runtime/replayClient.ts";
import type { LiveEvalEvent } from "../../../runtime/types.ts";
import "./shell.css";

type NodeSpec={id:string;kind:string;actor?:string|null;concurrency?:number|null};
type Workflow={schemaVersion?:string;workflowName?:string;entrypoint?:string[];nodes?:NodeSpec[];budgets?:Array<{kind:string;max:number;hard:boolean;label?:string|null}>;runId?:string;specHash?:string;status?:string;eventCount?:number;recordedCalls?:number;checkpoints?:number};
type StreamPayload={events?:LiveEvalEvent[];replay_ms?:number};
type NodeState={state:"queued"|"running"|"completed"|"failed";events:number;actorCalls:number;shards:Map<number,string>};
export type ShellProps=LiveTemplateProps&{title?:string;lede?:string;workflow?:Workflow;stream?:StreamPayload};

const pathOf=(payload:Record<string,unknown>)=>Array.isArray(payload.node_path)?payload.node_path.flatMap((part)=>{
  if(!part||typeof part!=="object")return[];const row=part as Record<string,unknown>;
  return row.kind==="node"&&typeof row.value==="string"?[row.value]:[];
}):[];
const indexOfPath=(payload:Record<string,unknown>)=>Array.isArray(payload.node_path)?payload.node_path.flatMap((part)=>{
  if(!part||typeof part!=="object")return[];const row=part as Record<string,unknown>;
  return row.kind==="index"&&typeof row.value==="number"?[row.value]:[];
}).at(-1):undefined;

function fold(nodes:NodeSpec[],events:LiveEvalEvent[]){
  const states=new Map<string,NodeState>(nodes.map((node)=>[node.id,{state:"queued",events:0,actorCalls:0,shards:new Map()}]));
  for(const event of events){if(event.kind!=="jesterky.transition")continue;const payload=event.payload??{};const nodeId=pathOf(payload)[0];if(!nodeId)continue;
    const current=states.get(nodeId)??{state:"queued",events:0,actorCalls:0,shards:new Map()};current.events++;
    const kind=String(payload.event_kind??"");const shard=indexOfPath(payload);
    if(kind==="node_started"||kind==="map_item_started")current.state="running";
    if(kind==="node_completed"||kind==="map_completed")current.state="completed";
    if(kind==="map_item_failed")current.state="failed";
    if(kind==="actor_invoked")current.actorCalls++;
    if(shard!==undefined){if(kind==="map_item_started")current.shards.set(shard,"running");if(kind==="map_item_completed")current.shards.set(shard,"completed");if(kind==="map_item_failed")current.shards.set(shard,"failed");}
    states.set(nodeId,current);
  }return states;
}

export function Shell(props:ShellProps){
  const workflow=props.workflow??{};const fixture=props.replay?.streams.length?undefined:props.stream?.events;
  const liveStream=useLiveEvalStream({replay:props.replay,fixtureEvents:fixture,replayMs:props.stream?.replay_ms,visualId:props.visualId,revision:props.revision});
  const max=Math.max(0,liveStream.events.length-1);const [cut,setCut]=useState<number|null>(null);const effective=cut==null?max:Math.min(cut,max);
  const visible=useMemo(()=>liveStream.events.slice(0,liveStream.events.length?effective+1:0),[liveStream.events,effective]);
  const nodes=workflow.nodes??[];const states=useMemo(()=>fold(nodes,visible),[nodes,visible]);const [selected,setSelected]=useState<string|null>(nodes[0]?.id??null);
  const selectedNode=nodes.find((node)=>node.id===selected)??nodes[0];const selectedState=selectedNode?states.get(selectedNode.id):undefined;
  const transitions=visible.filter((event)=>event.kind==="jesterky.transition");const completed=[...states.values()].filter((value)=>value.state==="completed").length;
  const terminal=["completed","failed"].includes(String(workflow.status??"").toLowerCase());
  return <VisualChrome kicker="Jesterky · Workflow" live={liveStream.state==="live"&&!terminal} title={props.title??workflow.workflowName??"Jesterky workflow"}
    lede={props.lede??"Replayable program structure, transitions, and retained execution evidence."} testId="visual-workflow-jesterky"
    captureBlocked={Boolean(props.replay?.streams.length)&&!liveStream.ready} footer="workflow.jesterky.v1 · structural metadata only; raw prompts and outputs stay out of the visual">
    <div className="jk-summary">
      <div className="jk-stat"><small>Status</small><strong>{workflow.status??(liveStream.ready?liveStream.state:"connecting")}</strong></div>
      <div className="jk-stat"><small>Nodes</small><strong>{completed}/{nodes.length} complete</strong></div>
      <div className="jk-stat"><small>Transitions</small><strong>{transitions.length}{workflow.eventCount?` / ${workflow.eventCount}`:""}</strong></div>
      <div className="jk-stat"><small>Recorded calls</small><strong>{workflow.recordedCalls??[...states.values()].reduce((sum,row)=>sum+row.actorCalls,0)}</strong></div>
    </div>
    <section className="sv-section jk-program" aria-label="Workflow graph"><div className="sv-section-head"><h3>Program</h3><span className="sv-mono">{workflow.runId??workflow.specHash?.slice(0,12)??"awaiting run"}</span></div>
      <div className="jk-graph">{nodes.map((node)=>{const state=states.get(node.id)??{state:"queued",events:0,actorCalls:0,shards:new Map()};return <button type="button" className="jk-node" key={node.id} data-state={state.state} data-selected={selected===node.id} onClick={()=>setSelected(node.id)}>
        <span className="jk-node-head"><strong>{node.id}</strong><span className="jk-state">{state.state}</span></span><p>{node.kind}{node.actor?` · ${node.actor}`:""}{node.concurrency?` · ×${node.concurrency}`:""}</p>
        {state.shards.size>0?<span className="jk-shards" aria-label={`${state.shards.size} shards`}>{[...state.shards].sort(([a],[b])=>a-b).map(([id,value])=><span key={id} className="jk-shard" data-state={value} title={`shard ${id}: ${value}`}/>)}</span>:null}
      </button>})}</div>
    </section>
    {liveStream.events.length>1?<div className="jk-scrub"><span className="sv-mono">0</span><input aria-label="Transition cutoff" type="range" min="0" max={max} value={effective} onChange={(event)=>setCut(Number(event.target.value))}/><button className="sv-button" type="button" onClick={()=>setCut(null)}>Live edge</button></div>:null}
    <div className="jk-inspector">
      <section className="jk-detail" aria-label="Selected node"><strong>{selectedNode?.id??"No node selected"}</strong>{selectedNode?<dl><dt>Kind</dt><dd>{selectedNode.kind}</dd><dt>Actor</dt><dd>{selectedNode.actor??"—"}</dd><dt>State</dt><dd>{selectedState?.state??"queued"}</dd><dt>Events</dt><dd>{selectedState?.events??0}</dd><dt>Actor calls</dt><dd>{selectedState?.actorCalls??0}</dd></dl>:null}</section>
      <section className="jk-detail" aria-label="Recent transitions"><strong>Recent transitions</strong><div className="jk-event-list">{transitions.slice(-10).reverse().map((event,index)=><div className="jk-event" key={`${event.logical_time??index}`}><span>{String(event.payload?.event_kind??"event")}</span><span>{pathOf(event.payload??{}).join(" / ")||"workflow"}</span><time>{event.logical_time??"—"}</time></div>)}</div></section>
    </div>
  </VisualChrome>;
}
export default Shell;

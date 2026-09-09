import {useEffect,useMemo,useState} from "react";
import type {AggregateResult,CorpusRef,QuerySpec,QueryResult,JsonValue} from "@synth/visuals-protocol";
import {RemoteCorpus,stableValueDigest} from "@synth/visuals-sdk";
import {useVisualSessionClient,useVisualSessionSnapshot,useVisualState} from "@synth/visuals-react";

type Row={id:string;kind:string;status:string|null;label:string|null;score:number|null;sequence:number;[key:string]:unknown};
type Filters=Record<string,string|null>;
const fields=["status","kind","label"];
const collections=["rollouts","evaluations","candidates","metric_points","proposer_calls"];
const filterSchema={type:"object" as const,additionalProperties:false,properties:Object.fromEntries(fields.map(field=>[field,{type:"string" as const,nullable:true}]))};

/** Human/AI exploration over the same pinned native read model. Only a bounded
 * metadata page crosses IPC; details are loaded on selection, never on mount. */
export function RecordedCollectionExplorer({runId,title}:{runId:string;title?:string}){
 const client=useVisualSessionClient(),session=useVisualSessionSnapshot();
 const [collection,setCollection]=useVisualState("recorded.collection","rollouts",{options:collections});
 const [source,setSource]=useVisualState<CorpusRef|null>("recorded.source",null,{type:"object",nullable:true,required:["id","revision","schema","count"],additionalProperties:false,properties:{id:{type:"string"},revision:{type:"string"},schema:{type:"string"},count:{type:"number",minimum:0,maximum:1000000}}});
 const [filters,setFilters]=useVisualState<Filters>("recorded.filters",{},filterSchema);
 const [history,setHistory]=useVisualState<Filters[]>("recorded.history",[],{type:"array",maxItems:32,items:filterSchema});
 const [offset,setOffset]=useVisualState("recorded.offset",0,{minimum:0,maximum:1000000});
 const [selected,setSelected]=useVisualState<string|null>("recorded.selected",null,{type:"string",nullable:true});
 const [result,setResult]=useState<{key:string;page:QueryResult<Row>;facets:AggregateResult[]}>();
 const [detail,setDetail]=useState<{key:string;value:unknown}>();
 const [error,setError]=useState<string>();
 const [loadingSource,setLoadingSource]=useState(false);
 const sourceMatches=source?.id===`optimizer:${runId}:${collection}`;
 const backend=useMemo(()=>client&&source&&sourceMatches?new RemoteCorpus<Row>(source,client.analyticalRequest):null,[client,source?.id,source?.revision,sourceMatches]);
 const query:QuerySpec=useMemo(()=>({schemaVersion:"synth.visuals-core.v1",where:{op:"and",expressions:Object.entries(filters).map(([field,value])=>({op:"eq",field,value}))},orderBy:[{field:"ordinal",direction:"asc"}]}),[stableValueDigest(filters)]);
 const key=stableValueDigest({source,query,offset});
 const detailKey=stableValueDigest({source,selected});
 useEffect(()=>{
  if(!client||!session?.ready||sourceMatches||session.state.replay)return;
  let cancelled=false;setLoadingSource(true);setError(undefined);
  void client.analyticalRequest({operation:"corpus.from_collection",runId,collection}).then(answer=>{
   if(!cancelled)return setSource(answer.corpus as CorpusRef);
  }).catch(reason=>{if(!cancelled)setError(String(reason));}).finally(()=>{if(!cancelled)setLoadingSource(false);});
  return()=>{cancelled=true;};
 },[client,session?.ready,sourceMatches,runId,collection,Boolean(session?.state.replay)]);
 useEffect(()=>{
  if(!backend)return;
  const abort=new AbortController();setError(undefined);
  void(async()=>{
   const cohort=await backend.cohort("Selected records",query,undefined,abort.signal);
   const [page,...facets]=await Promise.all([backend.query(query,{offset,limit:20},abort.signal),...fields.map(field=>backend.aggregate(cohort,field,abort.signal))]);
   if(!abort.signal.aborted)setResult({key,page:page as QueryResult<Row>,facets:facets as AggregateResult[]});
  })().catch(reason=>{if(!abort.signal.aborted)setError(String(reason));});
  return()=>abort.abort();
 },[backend,key]);
 useEffect(()=>{
  if(!client||!source||!selected||!sourceMatches)return;
  let cancelled=false;
  void client.analyticalRequest({operation:"corpus.detail",corpus:source,rowId:selected}).then(answer=>{if(!cancelled)setDetail({key:detailKey,value:answer.details});}).catch(reason=>{if(!cancelled)setError(String(reason));});
  return()=>{cancelled=true;};
 },[client,detailKey,sourceMatches]);
 const current=result?.key===key?result:undefined;
 function filter(field:string,value:JsonValue|undefined){
  if(history.length>=32){setError("History limit reached; return to a prior filter first.");return;}
  if(value!==null&&typeof value!=="string")return;
  setHistory([...history,filters]);setFilters({...filters,[field]:value});setOffset(0);setSelected(null);
 }
 if(!client)return <p role="alert">Recorded-run analytics requires the Workshop native read-model host.</p>;
 return <section data-visual-capture-blocked={error||!current||(selected&&detail?.key!==detailKey)?true:undefined} data-visual-observation="analysis.swarm_trajectories.v1" data-visual-rollout-count={source?.count??0} data-visual-terminal="true" style={{padding:24}}>
  <h2>{title??"Recorded run explorer"}</h2>
  <p>Exact metadata aggregates over a pinned recorded-run collection. Scores retain their source meaning; absent rewards, outcomes, and behavior labels are not inferred.</p>
  <label>Collection <select aria-label="Recorded collection" value={collection} onChange={event=>{setCollection(event.target.value);setFilters({});setHistory([]);setSelected(null);setOffset(0);}}>{collections.map(name=><option key={name}>{name}</option>)}</select></label>
  <button onClick={()=>{setSource(null);setFilters({});setHistory([]);setSelected(null);setOffset(0);}}>Pin latest source cut</button>
  {sourceMatches&&source&&<p>Source cut: {source.revision} · {current?.page.total??"…"} selected / {source.count} total records</p>}
  {history.length>0&&<button onClick={()=>{setFilters(history.at(-1)!);setHistory(history.slice(0,-1));setOffset(0);setSelected(null);}}>Back to prior aggregate</button>}
  {error&&<p role="alert">{error}</p>}
  {!current&&!error&&<p role="status">{loadingSource?"Pinning recorded source…":"Reading pinned collection…"}</p>}
  {current&&<>
   <div style={{display:"flex",gap:24,flexWrap:"wrap"}}>{current.facets.map(facet=><section key={facet.field}><h3>{facet.field}</h3>{facet.buckets.map((bucket,index)=><button key={index} style={{display:"block"}} onClick={()=>filter(facet.field,bucket.value)}>{bucket.key}: {bucket.count} / {bucket.denominator}</button>)}</section>)}</div>
   <h3>{current.page.total===0?"No records in this source cut":`Records (${offset+1}–${offset+current.page.rows.length})`}</h3>
   {current.page.rows.map(row=><button key={row.id} style={{display:"block"}} onClick={()=>setSelected(row.id)}>{row.id} · {row.kind} · {row.status??"status unavailable"} · score {row.score??"unavailable"}</button>)}
   <button disabled={offset===0} onClick={()=>setOffset(Math.max(0,offset-20))}>Previous page</button>
   <button disabled={offset+20>=current.page.total} onClick={()=>setOffset(offset+20)}>Next page</button>
  </>}
  {selected&&<section><h3>Recorded detail: {selected}</h3>{detail?.key===detailKey?<><pre style={{maxHeight:400,overflow:"auto",whiteSpace:"pre-wrap"}}>{JSON.stringify(detail.value,null,2).slice(0,16000)}</pre><p>Display limited to 16,000 characters; full bounded detail is available through corpus.detail.</p></>:<p role="status">Loading selected detail…</p>}</section>}
 </section>;
}

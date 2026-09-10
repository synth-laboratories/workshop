import { useEffect, useRef, useState } from 'react';
import { useVisualState, useVisualSessionClient } from '@synth/visuals-react';
import type { VisualControl } from '@synth/visuals-protocol';

export type TraceResearchOperation = 'query' | 'snapshot' | 'page' | 'source' | 'prepare_annotations' | 'window';
export type TraceResearchClient = { request: (operation: TraceResearchOperation, arguments_: Record<string, unknown>) => Promise<any> };
export type ResearchSelection = { snapshotId: string; resultId: string; row: Record<string, any>; selector: Record<string, any> };
type Page = { snapshotId: string; resultDigest: string; resultCount: number; rows: Record<string, any>[]; resultIds: string[]; nextOffset: number | null; offset?: number };
export function researchPage(value: any, expected?: Pick<Page, 'snapshotId' | 'resultDigest'>): Page {
 if (value?.error) throw new Error(String(value.error));
 if (!value || typeof value.snapshotId !== 'string' || typeof value.resultDigest !== 'string' || !Array.isArray(value.rows) || !Array.isArray(value.resultIds) || value.rows.length !== value.resultIds.length || !Number.isInteger(value.resultCount) || value.resultCount < value.rows.length || !(value.nextOffset === null || Number.isInteger(value.nextOffset) && value.nextOffset >= 0)) throw new Error('Invalid trace query page');
 if (expected && (value.snapshotId !== expected.snapshotId || value.resultDigest !== expected.resultDigest)) throw new Error('Query page changed snapshot; refresh explicitly');
 return value;
}
/** Persist navigation only, never source bodies, credentials, or query results. */
export function useTraceViewState<T>(key: string, initial: T, schema:Partial<Omit<VisualControl,'id'>>={}) {
 const session = useVisualSessionClient();
 const storageKey = `synth.trace-view.v1:${key}`;
 const [state, setState] = useVisualState<T>(`research.${key}`, () => {
  if(session)return initial;
  try { const raw = localStorage.getItem(storageKey); if (raw && raw.length < 64000) { const value = JSON.parse(raw); if (value != null && typeof value === typeof initial && Array.isArray(value) === Array.isArray(initial)) return value; } } catch {}
  return initial;
 },schema);
 useEffect(() => { if(session)return;try { const raw = JSON.stringify(state); if (raw.length < 64000) localStorage.setItem(storageKey, raw); } catch {} }, [storageKey, state,session]);
 return [state, setState] as const;
}
export function TraceResearchPanel({ client, jobIds, stateKey = 'research', onInspect }: {
 client?: TraceResearchClient; jobIds: string[]; stateKey?: string; onInspect?: (selection: ResearchSelection) => void;
}) {
 const [saved, setSaved] = useTraceViewState(stateKey, { query: JSON.stringify({ schemaVersion: 'synth.trace-query.v2', evalJobIds: jobIds, grain: 'entities', where: [{ field: 'eventType', op: 'eq', value: 'tool.result' }], limit: 50 }, null, 2), snapshotId: '', offset: 0 });
 const [page, setPage] = useState<Page | null>(null), [error, setError] = useState('');
 const [pageBusy,setPageBusy]=useState(false),[sourceBusy,setSourceBusy]=useState(false),[preparing,setPreparing]=useState(false);
 const busy=pageBusy||sourceBusy||preparing;
 const [selection,setSelection]=useTraceViewState(stateKey+'.selection',{snapshotId:'',resultId:''});
 const [chosen, setChosen] = useState<ResearchSelection | null>(null), [source, setSource] = useState<any>(null), [preparation, setPreparation] = useState<any>(null);
 const [checks,setChecks]=useTraceViewState(stateKey+'.checked',{snapshotId:'',ids:[] as string[]},
   {type:'object',required:['snapshotId','ids'],additionalProperties:false,properties:{snapshotId:{type:'string'},ids:{type:'array',items:{type:'string'},maxItems:50}}});
 const checked=checks.snapshotId===saved.snapshotId?checks.ids:[];
 const setChecked=(update:string[]|((ids:string[])=>string[]))=>setChecks(previous=>({snapshotId:saved.snapshotId,ids:typeof update==='function'?update(previous.snapshotId===saved.snapshotId?previous.ids:[]):update}));
 const generation = useRef(0);
 const sourceGeneration=useRef(0),preparationGeneration=useRef(0);
 const checksIdentity=JSON.stringify(checks);
 useEffect(()=>{preparationGeneration.current++;setPreparing(false);setPreparation(null);return()=>{preparationGeneration.current++;};},[page,checksIdentity]);
 async function load(operation: 'query' | 'page', arguments_: Record<string, unknown>, offset = 0) {
  if (!client) return;
  const token = ++generation.current; sourceGeneration.current++;setPageBusy(true); setError('');
  try {
   const next = researchPage(await client.request(operation, arguments_), operation === 'page' && page?.snapshotId===arguments_.snapshot_id ? page ?? undefined : undefined);
   if (operation === 'page' && next.snapshotId !== arguments_.snapshot_id) throw new Error('Query page changed requested snapshot');
   if (token !== generation.current) return;
   setPage({...next,offset}); setSaved(v => ({ ...v, snapshotId: next.snapshotId, offset })); setChosen(null); setSource(null); setPreparation(null); setChecked([]);setSelection({snapshotId:'',resultId:''});
  } catch (e) { if (token === generation.current) setError(String(e)); }
  finally { if (token === generation.current) setPageBusy(false); }
 }
 useEffect(() => {
  if(!client || !saved.snapshotId){setPage(null);setPageBusy(false);return;}
  if(page?.snapshotId===saved.snapshotId && page.offset===saved.offset)return;
  const token=++generation.current;setPageBusy(true);setError('');setPage(null);
  void client.request('page',{snapshot_id:saved.snapshotId,offset:saved.offset,limit:50}).then(payload=>{
   const next=researchPage(payload);
   if(next.snapshotId!==saved.snapshotId)throw new Error('Query page changed requested snapshot');
   if(token===generation.current)setPage({...next,offset:saved.offset});
  }).catch(error=>{if(token===generation.current)setError(String(error));})
    .finally(()=>{if(token===generation.current)setPageBusy(false);});
  return()=>{generation.current++;};
 },[client,saved.snapshotId,saved.offset]);
 useEffect(()=>{
  setChosen(null);setSource(null);setPreparation(null);setSourceBusy(false);
  if(!page || selection.snapshotId!==page.snapshotId || !selection.resultId)return;
  const index=page.resultIds.indexOf(selection.resultId);
  if(index<0){setError('Selected evidence is outside the restored page');return;}
  void inspect(page.rows[index],selection.resultId);
  return()=>{sourceGeneration.current++;};
 },[client,page,selection.snapshotId,selection.resultId]);
 async function inspect(row: Record<string, any>, resultId: string) {
  if (!client || !page) return;
  const token = ++sourceGeneration.current;
  const selection = { snapshotId: page.snapshotId, resultId, row, selector: row.selector };
  setChosen(selection); setSource(null); setPreparation(null); setSourceBusy(true); setError('');
  try {
   const response = await client.request('source', { snapshot_id: page.snapshotId, result_id: resultId, source_limit: 4000 });
   if (token !== sourceGeneration.current) return;
   if (!response.resolved) throw new Error(response.reason || response.error || 'Evidence unavailable');
   setSource(response);
  } catch (e) { if (token === sourceGeneration.current) setError(String(e)); }
  finally { if (token === sourceGeneration.current) setSourceBusy(false); }
 }
 async function moreSource() {
  if (!client || !chosen || source?.nextOffset == null) return;
  const token = ++sourceGeneration.current; setSourceBusy(true); setError('');
  try {
   const next = await client.request('source', { snapshot_id: chosen.snapshotId, result_id: chosen.resultId, offset: source.nextOffset, source_limit: 4000 });
   if (token !== sourceGeneration.current) return;
   if (!next.resolved || next.textDigest !== source.textDigest) throw new Error('Source revision changed');
   setSource({ ...next, resolved_text: source.resolved_text + next.resolved_text });
  } catch (e) { if (token === sourceGeneration.current) setError(String(e)); }
  finally { if (token === sourceGeneration.current) setSourceBusy(false); }
 }
 const columns = ['jobId','trialId','model','effort','actorId','eventType','reward','rewardMean','episodeCount','measuredCount','missingCount','label','reviewState','traceAvailability'].filter(k => page?.rows.some(r => r[k] != null));
 return <section aria-label="Query retained traces" className="trace-research">
 <style>{`.trace-research{padding:12px;border:1px solid #cbd6c6;border-radius:8px;margin:12px 0;font:13px/1.5 system-ui}.trace-research textarea{width:100%;min-height:160px;font:12px monospace}.trace-research table{border-collapse:collapse;width:100%}.trace-research th,.trace-research td{padding:7px;border-bottom:1px solid #cbd6c6;text-align:left;max-width:260px;overflow-wrap:anywhere}.trace-research pre{white-space:pre-wrap;overflow-wrap:anywhere;max-height:350px;overflow:auto}.trace-research button{margin:4px}.trace-research small{overflow-wrap:anywhere}`}</style>
 <h2>Query retained traces</h2><p>Query existing eval jobs, page a saved result, and read its exact source. Querying and preparing annotations start no model work.</p>
 {!client && <p role="status">Query transport is available inside Workshop. Retained replay remains available here.</p>}
 <details><summary>Edit typed query</summary><textarea aria-label="Trace query" value={saved.query} onChange={e => setSaved(v => ({ ...v, query: e.target.value }))}/></details>
 <button disabled={!client || busy} onClick={() => { try { const query = JSON.parse(saved.query); void load('query', { query }); } catch (e) { setError(String(e)); } }}>Run query / refresh</button>
 {error && <p role="alert">{error}{page ? ' Previous saved results remain below.' : ' No query results loaded. Check that these job IDs are registered in Workshop.'}</p>}
 {busy && <p role="status">Loading evidence…</p>}
 {page && <><p>{page.resultCount} results · showing {saved.offset + (page.rows.length ? 1 : 0)}–{saved.offset + page.rows.length}</p><small>Snapshot {page.snapshotId} · {page.resultDigest}</small>
 <div style={{ overflowX: 'auto' }}><table><thead><tr><th>Select</th>{columns.map(c => <th key={c}>{c}</th>)}<th>Evidence</th></tr></thead><tbody>{page.rows.map((row, i) => <tr key={page.resultIds[i]}><td><input type="checkbox" aria-label={`Select result ${i + 1}`} disabled={!row.selector || row.traceAvailability !== 'available'} checked={checked.includes(page.resultIds[i])} onChange={e => {const selected=e.target.checked;setChecked(v => selected ? [...new Set([...v, page.resultIds[i]])] : v.filter(id => id !== page.resultIds[i]));}}/></td>{columns.map(c => <td key={c}>{row[c] == null ? '—' : typeof row[c] === 'object' ? JSON.stringify(row[c]) : String(row[c])}</td>)}<td>{row.selector ? <button disabled={busy} onClick={() => setSelection({snapshotId:page.snapshotId,resultId:page.resultIds[i]})}>Read exact source</button> : <span>Aggregate · inspect episode-grain results</span>}</td></tr>)}</tbody></table></div>
 <button disabled={busy || saved.offset === 0} onClick={() => void load('page', { snapshot_id: page.snapshotId, offset: Math.max(0, saved.offset - 50), limit: 50 }, Math.max(0, saved.offset - 50))}>Previous page</button><button disabled={busy || page.nextOffset == null} onClick={() => void load('page', { snapshot_id: page.snapshotId, offset: page.nextOffset, limit: 50 }, page.nextOffset!)}>Next page</button>
 <button disabled={busy || !checked.length} onClick={async () => { if (!client) return; const token=++preparationGeneration.current;setPreparing(true); setError(''); try { const result = await client.request('prepare_annotations', { snapshot_id: page.snapshotId, result_ids: checked }); if(token!==preparationGeneration.current)return;if(result.error) throw new Error(result.error); setPreparation(result); } catch(e) {if(token===preparationGeneration.current)setError(String(e));} finally {if(token===preparationGeneration.current)setPreparing(false);} }}>Prepare annotations ({checked.length})</button>
 </>}
 {chosen && source && <section aria-label="Exact query evidence"><h3>Exact source</h3><small>{chosen.resultId} · {source.textDigest}</small><pre>{source.resolved_text}</pre>{source.nextOffset != null && <button disabled={busy} onClick={() => void moreSource()}>Read next source page</button>}{onInspect && <button onClick={() => { try { onInspect(chosen); } catch(e) {setError(String(e));} }}>Open in replay</button>}</section>}
 {preparation && <details open><summary>Annotation preparation · no compute started</summary><pre>{JSON.stringify(preparation, null, 2)}</pre></details>}
 </section>;
}

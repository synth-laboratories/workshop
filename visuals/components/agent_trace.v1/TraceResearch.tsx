import { useEffect, useRef, useState } from 'react';

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
export function useTraceViewState<T>(key: string, initial: T) {
 const storageKey = `synth.trace-view.v1:${key}`;
 const [state, setState] = useState<T>(() => {
  try { const raw = localStorage.getItem(storageKey); if (raw && raw.length < 64000) { const value = JSON.parse(raw); if (value != null && typeof value === typeof initial && Array.isArray(value) === Array.isArray(initial)) return value; } } catch {}
  return initial;
 });
 useEffect(() => { try { const raw = JSON.stringify(state); if (raw.length < 64000) localStorage.setItem(storageKey, raw); } catch {} }, [storageKey, state]);
 return [state, setState] as const;
}
export function TraceResearchPanel({ client, jobIds, stateKey = 'research', onInspect }: {
 client?: TraceResearchClient; jobIds: string[]; stateKey?: string; onInspect?: (selection: ResearchSelection) => void;
}) {
 const [saved, setSaved] = useTraceViewState(stateKey, { query: JSON.stringify({ schemaVersion: 'synth.trace-query.v2', evalJobIds: jobIds, grain: 'entities', where: [{ field: 'eventType', op: 'eq', value: 'tool.result' }], limit: 50 }, null, 2), snapshotId: '', offset: 0 });
 const [page, setPage] = useState<Page | null>(null), [error, setError] = useState(''), [busy, setBusy] = useState(false);
 const [chosen, setChosen] = useState<ResearchSelection | null>(null), [source, setSource] = useState<any>(null), [preparation, setPreparation] = useState<any>(null);
 const [checked, setChecked] = useState<string[]>([]);
 const generation = useRef(0);
 async function load(operation: 'query' | 'page', arguments_: Record<string, unknown>, offset = 0) {
  if (!client) return;
  const token = ++generation.current; setBusy(true); setError('');
  try {
   const next = researchPage(await client.request(operation, arguments_), operation === 'page' && page ? page : undefined);
   if (operation === 'page' && next.snapshotId !== arguments_.snapshot_id) throw new Error('Query page changed requested snapshot');
   if (token !== generation.current) return;
   setPage(next); setSaved(v => ({ ...v, snapshotId: next.snapshotId, offset })); setChosen(null); setSource(null); setPreparation(null); setChecked([]);
  } catch (e) { if (token === generation.current) setError(String(e)); }
  finally { if (token === generation.current) setBusy(false); }
 }
 useEffect(() => { if (client && saved.snapshotId) void load('page', { snapshot_id: saved.snapshotId, offset: saved.offset, limit: 50 }, saved.offset); return () => { generation.current++; }; }, [client]);
 async function inspect(row: Record<string, any>, resultId: string) {
  if (!client || !page) return;
  const token = ++generation.current;
  const selection = { snapshotId: page.snapshotId, resultId, row, selector: row.selector };
  setChosen(selection); setSource(null); setPreparation(null); setBusy(true); setError('');
  try {
   const response = await client.request('source', { snapshot_id: page.snapshotId, result_id: resultId, source_limit: 4000 });
   if (token !== generation.current) return;
   if (!response.resolved) throw new Error(response.reason || response.error || 'Evidence unavailable');
   setSource(response);
  } catch (e) { if (token === generation.current) setError(String(e)); }
  finally { if (token === generation.current) setBusy(false); }
 }
 async function moreSource() {
  if (!client || !chosen || source?.nextOffset == null) return;
  const token = ++generation.current; setBusy(true); setError('');
  try {
   const next = await client.request('source', { snapshot_id: chosen.snapshotId, result_id: chosen.resultId, offset: source.nextOffset, source_limit: 4000 });
   if (token !== generation.current) return;
   if (!next.resolved || next.textDigest !== source.textDigest) throw new Error('Source revision changed');
   setSource({ ...next, resolved_text: source.resolved_text + next.resolved_text });
  } catch (e) { if (token === generation.current) setError(String(e)); }
  finally { if (token === generation.current) setBusy(false); }
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
 <div style={{ overflowX: 'auto' }}><table><thead><tr><th>Select</th>{columns.map(c => <th key={c}>{c}</th>)}<th>Evidence</th></tr></thead><tbody>{page.rows.map((row, i) => <tr key={page.resultIds[i]}><td><input type="checkbox" aria-label={`Select result ${i + 1}`} disabled={!row.selector || row.traceAvailability !== 'available'} checked={checked.includes(page.resultIds[i])} onChange={e => setChecked(v => e.target.checked ? [...v, page.resultIds[i]] : v.filter(id => id !== page.resultIds[i]))}/></td>{columns.map(c => <td key={c}>{row[c] == null ? '—' : typeof row[c] === 'object' ? JSON.stringify(row[c]) : String(row[c])}</td>)}<td>{row.selector ? <button disabled={busy} onClick={() => void inspect(row, page.resultIds[i])}>Read exact source</button> : <span>Aggregate · inspect episode-grain results</span>}</td></tr>)}</tbody></table></div>
 <button disabled={busy || saved.offset === 0} onClick={() => void load('page', { snapshot_id: page.snapshotId, offset: Math.max(0, saved.offset - 50), limit: 50 }, Math.max(0, saved.offset - 50))}>Previous page</button><button disabled={busy || page.nextOffset == null} onClick={() => void load('page', { snapshot_id: page.snapshotId, offset: page.nextOffset, limit: 50 }, page.nextOffset!)}>Next page</button>
 <button disabled={busy || !checked.length} onClick={async () => { if (!client) return; setBusy(true); setError(''); try { const result = await client.request('prepare_annotations', { snapshot_id: page.snapshotId, result_ids: checked }); if(result.error) throw new Error(result.error); setPreparation(result); } catch(e) {setError(String(e));} finally {setBusy(false);} }}>Prepare annotations ({checked.length})</button>
 </>}
 {chosen && source && <section aria-label="Exact query evidence"><h3>Exact source</h3><small>{chosen.resultId} · {source.textDigest}</small><pre>{source.resolved_text}</pre>{source.nextOffset != null && <button disabled={busy} onClick={() => void moreSource()}>Read next source page</button>}{onInspect && <button onClick={() => { try { onInspect(chosen); } catch(e) {setError(String(e));} }}>Open in replay</button>}</section>}
 {preparation && <details open><summary>Annotation preparation · no compute started</summary><pre>{JSON.stringify(preparation, null, 2)}</pre></details>}
 </section>;
}

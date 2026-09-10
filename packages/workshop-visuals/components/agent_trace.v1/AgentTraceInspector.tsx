import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useVisualState } from '@synth/visuals-react';
import { eventActor, eventFamily, eventTime, traceText, type TraceItem, type TraceVisual, type TraceSection } from './model.ts';
import { GeneralTraceView, type TraceViewKind, type TraceViewExtension } from './TraceViews.tsx';
import type { TraceAnnotation } from './annotations.ts';
import type { TraceSelector } from './model.ts';
export type { TraceItem, TraceVisual } from './model.ts';
const labels = { messages: 'Messages', rollout: 'Agent rollout', evidence: 'Rewards & annotations', events: 'All events' };
const time = (ms: number | null) => ms == null ? '—' : `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, '0')}`;
function Field({ label, value, missing }: { label: string; value: unknown; missing?: string }) {
  const [open, setOpen] = useState(false);
  return value == null ? <p className="ati-muted"><strong>{label}:</strong> {missing ?? 'Not recorded'}</p> :
    <details className="ati-field" onToggle={e => setOpen(e.currentTarget.open)}><summary>{label}</summary>{open && <pre>{traceText(value)}</pre>}</details>;
}
export function TraceEventCard({ item, selected, onSelect, sourceLink }: {
  item: TraceItem; selected?: boolean; onSelect?: () => void; sourceLink?: string;
}) {
  const d = item.detail ?? {}; const family = eventFamily(item);
  const output = d.content ?? d.output ?? d.text ?? d.message;
  return <article className={`ati-event ${selected ? 'ati-selected' : ''}`} data-testid={`agent-trace-event-${item.item_id}`}>
    <button className="ati-event-head" type="button" onClick={onSelect} aria-pressed={selected}>
      <span>{family} · {item.title ?? item.kind}</span><small>{item.status}</small>
    </button>
    <div className="ati-muted">{d.native_actor_id ?? item.actor_id ?? 'Shared'} · {time(eventTime(item, []))} · {item.kind}</div>
    {family === 'input' && <><Field label="Input messages" value={d.input_messages ?? d.messages} missing="Exact request messages not recorded; retained observation below."/><Field label="Observation" value={d.observation ?? d.input}/></>}
    {family === 'output' && <><Field label="Assistant response" value={output}/><Field label="Thinking / reasoning" value={d.reasoning ?? d.thinking} missing={d.reasoning_availability ?? 'Reasoning text not recorded. Token counts do not contain the reasoning.'}/><Field label="Usage" value={d.usage}/></>}
    {family === 'thinking' && <Field label="Recorded reasoning" value={output ?? d.reasoning} missing={item.status}/ >}
    {family === 'tool' && <><Field label={/result|completed/.test(item.kind) ? 'Tool result' : 'Tool arguments'} value={d.result ?? d.action ?? d.arguments ?? d}/>{d.action?.reason && <p><strong>Reason:</strong> {d.action.reason}</p>}</>}
    {family === 'message' && <><p>{d.sender ?? d.sender_actor_id ?? 'Sender not recorded'} → {d.native_actor_id ?? d.recipient_actor_ids?.join(', ') ?? 'Recipients not recorded'}</p><p>{typeof output === 'string' ? output : traceText(output)}</p><small>{/observed/.test(item.kind) ? 'Observed by recipient; use is not established.' : item.status}</small></>}
    {family === 'reward' && <><p className="ati-reward">{d.value ?? 'Unavailable'} {d.units ?? 'reward'}{d.cumulative != null && <small> · cumulative {d.cumulative}</small>}</p><p>{d.provenance}</p><Field label="Reward components" value={d.components}/></>}
    {family === 'annotation' && <><p>{d.rationale ?? d.summary ?? item.title}</p><p className="ati-muted">{d.author_kind ?? d.producer?.kind ?? 'Producer not recorded'} · {d.review_state ?? item.status} · {item.occurred_at}</p><Field label="Annotation / evaluation evidence" value={d}/></>}
    {item.kind === 'span.model_call' && <><Field label="Input messages" value={d.input_messages}/><Field label="Output messages" value={d.output_messages}/><Field label="Usage" value={d.usage}/></>}
    {family === 'event' && <Field label="Event payload" value={d}/ >}
    {sourceLink && <a href={sourceLink} target="_blank" rel="noreferrer">Original record ↗</a>}
    <Field label="Source & provenance" value={{ selector: item.source_selector, digest: item.source_digest, source: d.source }}/>
  </article>;
}
export function AgentTraceInspector({ projection, actorId, onActorChange, cursorMs, onSelectEvent, renderEnvironment, sourceUrl, annotations, onAnnotate, onOpenAnnotation, extensions = [], initialView = 'general', selection }: {
  projection: TraceVisual; actorId?: string | null; onActorChange?: (id: string) => void; cursorMs?: number;
  onSelectEvent?: (item: TraceItem, ms: number | null) => void;
  renderEnvironment?: (actorId: string | null, cursorMs?: number) => ReactNode;
  sourceUrl?: (item: TraceItem) => string | undefined;
  annotations?: TraceAnnotation[];
  onAnnotate?: (target: TraceSelector) => void;
  onOpenAnnotation?: (annotation: TraceAnnotation) => void;
  extensions?: TraceViewExtension[];
  initialView?: string;
  selection?: { itemId: string; revision: number };
}) {
  const lanes = projection.lanes ?? []; const items = projection.items ?? [];
  const identity = projection.trace_id ?? projection.capture_id;
  const scope=`agent.${identity??'trace'}`;
  const [localActor, setActor] = useVisualState(`${scope}.actor`,'all'); const actor = actorId ?? localActor;
  const [section, setSection] = useVisualState<TraceSection>(`${scope}.section`,'rollout',{options:['messages','rollout','evidence','events']}); const [query, setQuery] = useVisualState(`${scope}.query`,'');
  const [selectedId, select] = useVisualState<string | null>(`${scope}.selected`,null); const [pin, setPin] = useVisualState(`${scope}.pin`,'');
  const [limit, setLimit] = useVisualState(`${scope}.limit`,60,{minimum:1,maximum:10000});
  const [view, setView] = useVisualState<string>(`${scope}.view`,initialView);
  const previousFilter=useRef(`${actor}:${section}:${query}`);
  useEffect(() => { const filter=`${actor}:${section}:${query}`;if(filter!==previousFilter.current){previousFilter.current=filter;setLimit(60);} }, [actor, section, query]);
  useEffect(() => { if (selection) { setSection('events'); setQuery(''); changeActor('all'); } }, [selection?.itemId, selection?.revision]);
  const filtered = useMemo(() => items.filter(item => {
    const family = eventFamily(item);
    const inSection = section === 'events' || (section === 'messages' ? family === 'message' : section === 'evidence' ? family === 'reward' || family === 'annotation' : family !== 'annotation');
    return inSection && (!query || `${item.title} ${item.kind} ${traceText(item.detail)}`.toLowerCase().includes(query.toLowerCase()));
  }), [items, section, query]);
  const agents = lanes.filter(lane => lane.actor_id && !['orchestrator','environment'].includes(lane.role ?? ''));
  const agentName = (id: string) => { const lane = lanes.find(l => l.actor_id === id); return [lane?.display_name ?? id, lane?.role].filter(Boolean).join(' · '); };
  function changeActor(id: string) { setActor(id); setPin(''); onActorChange?.(id); }
  function choose(item: TraceItem) { select(item.item_id); onSelectEvent?.(item, eventTime(item, items)); }
  function laneContent(id: string) {
    const rows = filtered.filter(item => id === 'all' || eventActor(item, items) === id || (item.source_selector?.kind === 'actor' && item.source_selector.entity_id === id));
    if (section === 'rollout' || section === 'events') return <GeneralTraceView key={`${identity}:${id}:${section}`} projection={projection} items={rows} view={(['general', 'react', 'codex'].includes(view) ? view : 'general') as TraceViewKind} extension={extensions.find(extension => extension.id === view)} annotations={annotations} selection={selection} cursorMs={cursorMs} context={renderEnvironment?.(id === 'all' ? null : id, cursorMs)} initialFull={section === 'events'} sourceUrl={sourceUrl} onSelect={choose} onAnnotate={onAnnotate} onOpenAnnotation={onOpenAnnotation}/>;
    const groups = rows.map(item => ({ id: item.item_id, items: [item] }));
    return <div className="ati-scroll" aria-label={`${lanes.find(l => l.actor_id === id)?.display_name ?? 'All agents'} ${labels[section]}`}>
      {!groups.length && <p className="ati-muted">{section === 'evidence' ? 'No rewards or annotations recorded for this selection.' : section === 'messages' ? 'No messages recorded for this selection.' : 'No events match this selection.'}</p>}
      {groups.slice(0, limit).map(group => <section className="ati-decision" key={group.id}>
        {group.items.map(item => <TraceEventCard key={item.item_id} item={item} selected={selectedId === item.item_id} onSelect={() => choose(item)} sourceLink={sourceUrl?.(item)}/>)}
      </section>)}
      {groups.length > limit && <button onClick={() => setLimit(n => n + 60)}>Show next 60 ({groups.length - limit} remaining)</button>}
    </div>;
  }
  return <section className="agent-trace-inspector" data-testid="agent-trace-inspector">
    <style>{`.agent-trace-inspector{font:13px/1.5 -apple-system,BlinkMacSystemFont,sans-serif;color:var(--sv-text,#28382e);min-width:0;container-type:inline-size}.agent-trace-inspector *{box-sizing:border-box}.ati-toolbar{display:flex;gap:8px;flex-wrap:wrap;margin:12px 0}.ati-toolbar button,.ati-toolbar select,.ati-toolbar input,.ati-scroll>button{font:inherit;padding:7px 10px;border:1px solid var(--sv-border,#cfd9cf);border-radius:6px;background:var(--sv-surface,#fff);color:inherit}.ati-toolbar button[aria-pressed=true]{background:var(--sv-text,#294f3b);color:var(--sv-surface,#fff)}.ati-toolbar input{flex:1;min-width:180px}.ati-columns{display:grid;grid-template-columns:minmax(0,1fr);gap:12px}.ati-columns.ati-compare{grid-template-columns:repeat(2,minmax(0,1fr))}.ati-scroll{max-height:600px;overflow:auto;overscroll-behavior:contain;min-width:0;padding:2px}.ati-decision{border:1px solid var(--sv-border,#d4ded2);border-radius:8px;margin-bottom:12px;overflow:hidden}.ati-decision h4{margin:0;padding:10px;background:var(--sv-surface-muted,#eef2e9);overflow-wrap:anywhere}.ati-decision h4 small{float:right;font-weight:400}.ati-event{padding:12px;border-top:1px solid var(--sv-border,#d4ded2);overflow-wrap:anywhere;background:var(--sv-surface,#fff)}.ati-selected{box-shadow:inset 3px 0 var(--sv-accent,#387958)}.ati-event-head{display:flex;justify-content:space-between;gap:8px;width:100%;text-align:left;font:inherit;font-weight:650;color:inherit;background:transparent;border:0;padding:0;cursor:pointer}.ati-muted{color:var(--sv-text-muted,#667468);font-size:12px}.ati-field{margin-top:8px}.ati-field summary{cursor:pointer;font-weight:550}.ati-field pre{font:12px/1.5 ui-monospace,monospace;white-space:pre-wrap;overflow-wrap:anywhere;max-height:350px;overflow:auto;background:var(--sv-surface-muted,#f0f3ed);padding:10px}.ati-reward{font-size:20px;font-weight:650}.ati-reward small{font-size:12px;font-weight:400}@container(max-width:620px){.ati-columns.ati-compare{grid-template-columns:1fr}.ati-scroll{max-height:480px}}`}</style>
    <div className="ati-toolbar" role="group" aria-label="Trace sections">{(Object.keys(labels) as TraceSection[]).map(s => <button key={s} aria-pressed={section === s} onClick={() => setSection(s)}>{labels[s]}</button>)}<label>Presentation <select aria-label="Trace presentation" value={view} onChange={e => setView(e.target.value)}><option value="general">General trace</option><option value="react">ReAct</option><option value="codex">Codex app server</option>{extensions.map(extension => <option key={extension.id} value={extension.id}>{extension.label}</option>)}</select></label></div>
    <div className="ati-toolbar" role="group" aria-label="Agent viewing mode">
      <button aria-pressed={actor === 'all'} onClick={() => changeActor('all')}>All agents</button>
      <button aria-pressed={actor !== 'all'} onClick={() => changeActor(actor === 'all' ? agents[0]?.actor_id ?? 'all' : actor)}>One agent</button>
    </div>
    <div style={{padding:'10px 12px',background:'#e9f0e8',borderRadius:6,marginBottom:8}} role="status" aria-label="Current agent scope">
      <strong>{actor === 'all' ? `Showing all ${agents.length} agents together` : `Showing one agent: ${agentName(actor)}`}</strong>
      <div>{actor === 'all' ? 'Each timeline marker and transcript card identifies its agent.' : 'Timeline, transcript and replay follow this agent.'}</div>
    </div>
    <div className="ati-toolbar" aria-label="Agent roster">{agents.map(lane => <button key={lane.lane_id} style={{borderLeft:`4px solid ${lane.color ?? '#547c92'}`}} aria-pressed={actor === lane.actor_id} onClick={() => changeActor(lane.actor_id!)}>{agentName(lane.actor_id!)}</button>)}</div>
    <div className="ati-toolbar"><label>Agent <select aria-label="Trace agent" value={actor} onChange={e => { changeActor(e.target.value); }}><option value="all">All agents</option>{lanes.map(l => <option key={l.lane_id} value={l.actor_id ?? l.lane_id}>{l.display_name ?? l.lane_id} {l.role ? `· ${l.role}` : ''}</option>)}</select></label>
      <label>Compare <select aria-label="Compare agent" disabled={actor === 'all'} title={actor === 'all' ? 'Select one agent before comparing' : 'Compare with another agent'} value={actor === 'all' ? '' : pin} onChange={e => setPin(e.target.value)}><option value="">None</option>{lanes.filter(l => l.actor_id !== actor).map(l => <option key={l.lane_id} value={l.actor_id ?? l.lane_id}>{l.display_name ?? l.lane_id}</option>)}</select></label>
      {actor === 'all' ? <span className="ati-muted">Select one agent before comparing.</span> : null}
      <input aria-label="Search agent trace" placeholder="Search messages, tools, rewards…" value={query} onChange={e => setQuery(e.target.value)}/></div>
    <p className="ati-muted">{items.length} recorded items · {projection.state ?? 'Unknown state'}{cursorMs != null ? ` · replay ${time(cursorMs)}` : ''}. History retained while scrubbing.</p>
    {projection.losses?.length ? <details><summary>Capture coverage</summary>{projection.losses.map(loss => <p key={loss}>{loss}</p>)}</details> : null}
    <div className={`ati-columns ${pin && actor !== 'all' ? 'ati-compare' : ''}`}><div>{laneContent(actor)}</div>{pin && actor !== 'all' && <div><strong>{lanes.find(l => l.actor_id === pin)?.display_name}</strong>{laneContent(pin)}</div>}</div>
  </section>;
}

export { useTraceArchive } from "./archive.ts";
export { GeneralTraceView, ReActTraceView, CodexAppServerTraceView, ContainerTraceView } from './TraceViews.tsx';
export type { TraceViewProps, TraceViewExtension, TraceViewKind } from './TraceViews.tsx';
export { traceAnnotations, annotationMatches } from './annotations.ts';
export type { TraceAnnotation } from './annotations.ts';
export { RuneBenchTraceView, runeBenchTraceExtension } from './RuneBenchTraceView.tsx';

export { useTraceEvidence, AnnotationEditor } from './EvidenceClient.tsx';
export type { EvidenceService } from './EvidenceClient.tsx';
export { craftaxTraceExtension } from './CraftaxTraceView.tsx';

export { TraceResearchPanel, useTraceViewState } from "./TraceResearch.tsx";
export type { TraceResearchClient, ResearchSelection } from "./TraceResearch.tsx";

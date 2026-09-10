import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import {useVisualState} from '@synth/visuals-react';
import { decisionGroups, eventFamily, eventTime, traceText, type TraceItem, type TraceSelector, type TraceVisual } from './model.ts';
import { annotationMatches, annotationTargetLabel, itemSelector, traceAnnotations, type TraceAnnotation } from './annotations.ts';

export type TraceViewKind = 'general' | 'react' | 'codex';
export type TraceViewExtension = {
  id: string; label: string; base: TraceViewKind;
  /** Return null to use the protocol/general renderer. The shared anchor surrounds both. */
  renderItem: (item: TraceItem) => ReactNode;
  renderContext?: (item: TraceItem | null) => ReactNode;
  filterItems?: (items: TraceItem[]) => TraceItem[];
  groupItems?: (items: TraceItem[]) => { id: string; items: TraceItem[] }[];
  focusItems?: (items: TraceItem[]) => TraceItem[];
};
export type TraceViewProps = {
  projection: TraceVisual; items?: TraceItem[]; view?: TraceViewKind;
  extension?: TraceViewExtension; annotations?: TraceAnnotation[];
  onSelect?: (item: TraceItem, ms: number | null) => void;
  onAnnotate?: (target: TraceSelector) => void;
  onOpenAnnotation?: (annotation: TraceAnnotation) => void;
  initialFull?: boolean;
  sourceUrl?: (item: TraceItem) => string | undefined;
  cursorMs?: number;
  selection?: { itemId: string; revision: number };
  context?: ReactNode;
};

const elapsed = (ms: number | null) => ms == null ? '' : `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, '0')}`;
function eventLabel(item: TraceItem): string {
  if (item.title && item.title !== item.kind) return item.title;
  const family = eventFamily(item); const d = item.detail ?? {};
  if (d.kind === 'policy.call') return `Decision ${d.call_index}`;
  if (item.kind === 'environment.action_executed') return `${d.action ?? 'Action'} · ${d.transition ?? 'result'}`;
  if (family === 'input') return 'Observation';
  if (family === 'output') return 'Assistant response';
  if (family === 'thinking') return d.reasoning_details?.some((part:any) => part.type === 'reasoning.summary') ? 'Reasoning summary' : 'Reasoning';
  if (family === 'message') return 'Message';
  if (family === 'reward') return 'Reward';
  if (family === 'tool') return d.result != null ? 'Tool result' : d.action?.type ?? d.tool_name ?? 'Tool call';
  return item.title ?? item.kind;
}
function Text({ value }: { value: unknown }) {
  return value == null ? null : <pre className="atv-text">{traceText(value)}</pre>;
}
function LazyDetails({ title, children }: { title: string; children: () => ReactNode }) {
  const [open, setOpen] = useState(false);
  return <details onToggle={e => setOpen(e.currentTarget.open)}><summary>{title}</summary>{open && children()}</details>;
}
function Messages({ messages }: { messages: any[] | undefined }) {
  return <>{messages?.map((message, index) => <section key={message.message_id ?? index}>
    <small>{message.role}</small>
    {message.parts?.map((part: any, n: number) => <div key={part.part_id ?? n}>
      {part.type === 'reasoning' && <strong>Recorded reasoning</strong>}
      {part.type === 'tool_call' && <strong>Tool call · {part.tool_name}</strong>}
      {part.type === 'tool_result' && <strong>Tool result</strong>}
      <Text value={part.text ?? part.arguments_json ?? part.structured}/>
    </div>)}
  </section>)}</>;
}
function GeneralContent({ item }: { item: TraceItem }) {
  const d = item.detail ?? {};
  const value = d.content ?? d.text ?? d.message ?? d.output ?? d.result ?? d.arguments ?? d.action ?? d.observation;
  if (item.kind === 'span.model_call') return <><strong>Input</strong><LazyDetails title="Recorded messages">{() => Array.isArray(d.input_messages) ? <Messages messages={d.input_messages}/> : <Text value={d.input}/>}</LazyDetails><strong>Assistant response</strong>{Array.isArray(d.output_messages) ? <Messages messages={d.output_messages}/> : <Text value={d.output}/>}</>;
  return <Text value={value ?? d}/>;
}
function ReActContent({ item }: { item: TraceItem }) {
  const d = item.detail ?? {}; const family = eventFamily(item);
  if (family === 'thinking') return <><strong>Recorded reasoning</strong><Text value={d.reasoning ?? d.thinking ?? d.content ?? d.text}/></>;
  if (family === 'input') return <LazyDetails title="Observation supplied to agent">{() => <Text value={d.observation ?? d.input_messages ?? d.messages ?? d.input ?? d}/>}</LazyDetails>;
  if (family === 'tool') return <>
    {d.action?.reason && <><strong>Reason</strong><Text value={d.action.reason}/></>}
    <strong>{/result|completed/.test(item.kind) ? 'Result' : 'Action'}</strong>
    <Text value={d.result ?? d.arguments ?? d.action ?? d}/>
  </>;
  return <GeneralContent item={item}/>;
}
/** Consumes already projected Codex events, retaining unknown native items verbatim. */
function CodexContent({ item }: { item: TraceItem }) {
  const d = item.detail ?? {}; const native = d.native ?? d;
  const n = native.params?.item ?? native.item ?? native;
  const kind = n.type ?? item.kind;
  if (/reasoning/.test(kind)) return <><strong>Recorded reasoning</strong><Text value={n.text ?? n.summary ?? n.content ?? d.content ?? native}/></>;
  if (/command/.test(kind)) return <><strong>Command</strong><Text value={n.command ?? native.params?.delta ?? native}/><Text value={n.aggregatedOutput ?? n.aggregated_output ?? n.output ?? n.stdout}/>{(n.exitCode ?? n.exit_code) != null && <p>Exit code {n.exitCode ?? n.exit_code}</p>}</>;
  if (/agent_message|agentMessage/.test(kind)) return <Text value={n.text ?? d.content ?? native}/>;
  if (kind === 'mcpToolCall') return <><strong>{n.server ? `${n.server} / ` : ''}{n.tool ?? 'MCP tool'}</strong><Text value={n.arguments ?? native}/><Text value={n.result ?? n.error}/></>;
  if (kind === 'fileChange') return <><strong>File changes</strong><Text value={n.changes ?? native}/></>;
  return <ReActContent item={item}/>;
}

function AnnotationNote({ annotation, superseded, onOpen }: { annotation: TraceAnnotation; superseded: boolean; onOpen?: (note: TraceAnnotation) => void }) {
  return <article className="atv-note">
    <strong>{annotation.labels.join(' · ') || 'Annotation'}</strong>
    <p>{annotation.body}</p>
    <small>{annotation.author} · {annotation.reviewState}{superseded ? ' · superseded' : ''}{annotation.createdAt ? ` · ${annotation.createdAt}` : ''}</small>
    <details><summary>Exact target & evidence</summary><p>{annotationTargetLabel(annotation.target)}</p><Text value={{ target: annotation.target, evidence: annotation.evidence }}/></details>
    {annotation.grounding && <small>Review basis: {annotation.grounding === 'summary_only' ? 'rendered projection' : annotation.grounding}</small>}
    {onOpen && !superseded && <button type="button" onClick={() => onOpen(annotation)}>Open annotation</button>}
  </article>;
}

export function GeneralTraceView({ projection, items = projection.items ?? [], view = 'general', extension, annotations, onSelect, onAnnotate, onOpenAnnotation, initialFull = false, sourceUrl, cursorMs, context, selection }: TraceViewProps) {
  const notes = useMemo(() => annotations ?? traceAnnotations(projection.items ?? []), [annotations, projection.items]);
  const key=`trace.detail.${projection.trace_id}.${extension?.id??view}`;
  const [selectedId, setSelectedId] = useVisualState<string | null>(`${key}.selected`,null);
  const [onlyAnnotated, setOnlyAnnotated] = useVisualState(`${key}.annotated`,false);
  const [full, setFull] = useVisualState(`${key}.full`,initialFull);
  const [limit, setLimit] = useVisualState(`${key}.limit`,60,{minimum:1});
  const [follow, setFollow] = useVisualState(`${key}.follow`,true);
  const [markerKind, setMarkerKind] = useVisualState(`${key}.markers`,'all');
  const transcript = useRef<HTMLDivElement>(null);
  const chosenCursor = useRef<number | null>(null);
  const markerClickUntil = useRef(0);
  const selected = items.find(item => item.item_id === selectedId) ?? null;
  const superseded = new Set(notes.flatMap(note => note.supersedesId ? [note.supersedesId] : []));
  const attached = (item: TraceItem) => notes.filter(note => annotationMatches(note, item, projection));
  const active = (item: TraceItem) => attached(item).filter(note => !superseded.has(note.id));
  const focused = !full && extension?.filterItems ? extension.filterItems(items) : items;
  const rows = items.filter(item => full || focused.includes(item) || active(item).length > 0 || item.item_id === selectedId).filter(item => (item.kind !== 'evidence.annotation' || item.item_id === selectedId) &&
    (full || !/^(eval\.|media\.|policy\.finished)/.test(item.kind) || active(item).length > 0) && (!onlyAnnotated || active(item).length > 0));
  const base = extension?.base ?? view;
  const groups = base === 'general' || full ? rows.map(item => ({ id: item.item_id, items: [item] })) : extension?.groupItems?.(rows) ?? decisionGroups(rows);
  const timed = rows.map(item => ({ item, ms: eventTime(item, projection.items ?? []) })).filter(row => row.ms != null).sort((a,b) => a.ms! - b.ms!);
  useEffect(() => {
    if (cursorMs == null) return;
    if (chosenCursor.current === cursorMs) { chosenCursor.current = null; return; }
    chosenCursor.current = null;
    const row = timed.filter(row => row.ms! <= cursorMs).at(-1);
    if (row) setSelectedId(row.item.item_id);
  }, [cursorMs, projection.trace_id]);
  useEffect(() => {
    if (selection && items.some(item => item.item_id === selection.itemId)) { setSelectedId(selection.itemId); setFollow(true); setFull(true); setOnlyAnnotated(false); }
  }, [selection?.itemId, selection?.revision]);
  useEffect(() => {
    if (!selectedId || !follow) return;
    const index = groups.findIndex(group => group.items.some(item => item.item_id === selectedId));
    if (index >= limit) { setLimit(Math.ceil((index + 1) / 60) * 60); return; }
    const selectedNode = Array.from(transcript.current?.querySelectorAll<HTMLElement>('[data-trace-item-id]') ?? []).find(node => node.dataset.traceItemId === selectedId);
    const node = selectedNode;
    if (node && transcript.current) {
      const parent = transcript.current; const offset = node.getBoundingClientRect().top - parent.getBoundingClientRect().top;
      if (offset < 0 || offset > parent.clientHeight - 120) parent.scrollTop += offset - 20;
    }
  }, [selectedId, follow, limit, base]);
  const unresolved = notes.filter(note => !(projection.items ?? []).some(item => item.kind !== 'evidence.annotation' && annotationMatches(note, item, projection)));
  function choose(item: TraceItem) { const time = eventTime(item, projection.items ?? []); chosenCursor.current = time; setFollow(true); setSelectedId(item.item_id); onSelect?.(item, time); }
  const markers = rows.filter(item => markerKind === 'annotations' ? active(item).length > 0 : markerKind === 'failure' ? item.detail?.result?.success === false || item.status === 'error' : markerKind === 'all' ? eventFamily(item) !== 'input' && item.kind !== 'span.model_call' : eventFamily(item) === markerKind);
  function scrubTimeline(bar: HTMLElement) {
    if (performance.now() < markerClickUntil.current) return;
    const buttons = Array.from(bar.querySelectorAll<HTMLElement>('[data-marker-id]'));
    const edge = bar.getBoundingClientRect().left;
    const atEnd = bar.scrollLeft >= bar.scrollWidth - bar.clientWidth - 2;
    const button = atEnd ? buttons.at(-1) : buttons.find(node => node.getBoundingClientRect().right > edge + 2);
    const item = markers.find(row => row.item_id === button?.dataset.markerId);
    if (item && item.item_id !== selectedId) choose(item);
  }
  function jump(direction: number) { const index = markers.findIndex(item => item.item_id === selectedId); const next = index < 0 ? (direction > 0 ? 0 : markers.length - 1) : index + direction; if (markers[next]) choose(markers[next]); }
  const Content = base === 'codex' ? CodexContent : base === 'react' ? ReActContent : GeneralContent;
  const extensionContext = extension?.renderContext?.(selected);
  const showNotes = notes.length > 0 || Boolean(context) || Boolean(extensionContext);
  return <section className="atv" data-trace-view={extension?.id ?? base}>
    <style>{`.atv{font:13px/1.5 system-ui;min-width:0}.atv button{font:inherit;cursor:pointer}.atv-controls,.atv-header{display:flex;gap:10px;align-items:center;flex-wrap:wrap}.atv-controls{margin:8px 0}.atv-layout{display:grid;grid-template-columns:minmax(0,1fr) minmax(200px,28%);gap:16px}.atv-transcript{max-height:650px;overflow:auto}.atv-turn{border-top:1px solid #a0a0a055;padding:10px 0}.atv-item{padding:6px 8px;border-left:3px solid transparent;overflow-wrap:anywhere}.atv-item[aria-current=true]{border-color:#327f69;background:#327f690c}.atv-header{justify-content:space-between;color:var(--sv-text-muted,#667468)}.atv-text{font:12px/1.6 ui-monospace,monospace;white-space:pre-wrap;overflow-wrap:anywhere;max-height:240px;overflow:auto;margin:8px 0}.atv-note{padding:10px;margin:8px 0;border:1px solid #ba963c77;border-radius:6px}.atv-note p{margin:5px 0}.atv-item p{margin:5px 0}.atv-item>button{padding:3px 6px;font-size:11px}.atv-item details{margin-top:4px}.atv-item a{font-size:11px}.atv-item .atv-header{font-size:11px}.atv-note small{opacity:.75}.atv-notes{border-left:1px solid #a0a0a055;padding-left:12px;max-height:650px;overflow:auto}.atv details{margin-top:8px}.atv-timeline{display:flex;gap:4px;overflow:auto;padding:8px 0}.atv-timeline button{flex-shrink:0;font-size:11px}.atv-timeline button[aria-current=true]{background:#327f69;color:white}.atv h4{margin:0 0 8px}@container(max-width:650px){.atv-layout{grid-template-columns:1fr}.atv-notes{border-left:0;padding-left:0}}`}</style>
    <div className="atv-controls">
      <button type="button" aria-pressed={!full} onClick={() => setFull(false)}>Focus</button>
      <button type="button" aria-pressed={full} onClick={() => setFull(true)}>Full</button>
      <label><input type="checkbox" checked={onlyAnnotated} onChange={e => setOnlyAnnotated(e.target.checked)}/> Annotated only</label>
      <label><input type="checkbox" checked={follow} onChange={e => setFollow(e.target.checked)}/> Follow replay</label>
      <button type="button" onClick={() => jump(-1)}>Previous</button><button type="button" onClick={() => jump(1)}>Next</button>
      <label>Jump to <select aria-label="Trace marker type" value={markerKind} onChange={e => setMarkerKind(e.target.value)}>{['all','failure','message','reward','annotations','thinking','tool'].map(kind => <option key={kind} value={kind}>{kind}</option>)}</select></label>
      <small>{rows.length} items · {notes.filter(note => !superseded.has(note.id)).length} current annotations</small>
    </div>
    <nav className="atv-timeline" aria-label="Trace event timeline" onScroll={e => scrubTimeline(e.currentTarget)}>{markers.map(item => <button key={item.item_id} data-marker-id={item.item_id} style={{borderTop:`3px solid ${projection.lanes?.find(lane => lane.actor_id === item.actor_id)?.color ?? '#547c92'}`}} type="button" aria-current={selectedId === item.item_id} title={item.kind} onClick={() => { markerClickUntil.current = performance.now() + 200; choose(item); }}>{projection.lanes?.find(lane => lane.actor_id === item.actor_id)?.display_name ?? item.detail?.native_actor_id ?? item.actor_id ?? 'Shared'} · {elapsed(eventTime(item, projection.items ?? [])) || 'untimed'} {item.detail?.result?.success === false ? '✕ ' : ''}{eventLabel(item)}{active(item).length ? ` · ${active(item).length} notes` : ''}</button>)}</nav>
    <div className="atv-layout" style={!showNotes ? { gridTemplateColumns: 'minmax(0,1fr)' } : undefined}>
      <div className="atv-transcript" ref={transcript}>
        {!groups.length && <p>No matching trace items.</p>}
        {groups.slice(0, limit).map((group, index) => <section className="atv-turn" key={group.id}>
          {base !== 'general' && !full && <h4>{group.items[0].detail?.decision_id ?? `Step ${index + 1}`} · {projection.lanes?.find(lane => lane.actor_id === group.items[0].actor_id)?.display_name ?? group.items[0].detail?.native_actor_id ?? 'Shared'} <small>{elapsed(eventTime(group.items[0], projection.items ?? []))}</small></h4>}
          {(full ? group.items : extension?.focusItems?.(group.items) ?? group.items).concat(group.items.filter(item => !full && extension?.focusItems && !extension.focusItems(group.items).includes(item) && (active(item).length > 0 || item.item_id === selectedId))).map(item => <article key={item.item_id} className="atv-item" style={{borderLeftColor:projection.lanes?.find(lane => lane.actor_id === item.actor_id)?.color}} aria-current={selectedId === item.item_id} data-trace-item-id={item.item_id} data-trace-selector={JSON.stringify(itemSelector(item, projection))}>
            <div className="atv-header"><button type="button" title={item.kind} onClick={() => choose(item)}>{eventLabel(item)}</button><small>{projection.lanes?.find(lane => lane.actor_id === item.actor_id)?.display_name ?? item.detail?.native_actor_id ?? 'Shared'} · {elapsed(eventTime(item, projection.items ?? []))} · {item.detail?.result?.success === false ? 'unsuccessful' : item.status}</small></div>
            {extension?.renderItem(item) ?? <Content item={item}/>}
            {active(item).length > 0 && <button type="button" onClick={() => choose(item)}>{active(item).length} annotations · {active(item).flatMap(note => note.labels).join(', ')}</button>}
            {onAnnotate && item.source_selector?.entity_id && <button type="button" onClick={() => { choose(item); onAnnotate(itemSelector(item, projection)); }}>Annotate</button>}
            {sourceUrl?.(item) && <a href={sourceUrl(item)} target="_blank" rel="noreferrer">Original record ↗</a>}
            <LazyDetails title="Source & provenance">{() => <Text value={{ selector: itemSelector(item, projection), digest: item.source_digest, detail: item.detail }}/>}</LazyDetails>
          </article>)}
          {!full && group.items.length > 1 && <LazyDetails title={`All ${group.items.length} source records for this decision`}>{() => group.items.map(item => <div key={item.item_id}><button type="button" onClick={() => choose(item)}>{item.kind}</button><Text value={item.detail}/>{onAnnotate && <button type="button" onClick={() => onAnnotate(itemSelector(item, projection))}>Annotate source</button>}</div>)}</LazyDetails>}
        </section>)}
        {groups.length > limit && <button type="button" onClick={() => setLimit(n => n + 60)}>Show next 60</button>}
      </div>
      {showNotes ? <aside className="atv-notes" aria-label="Trace annotations">
        {context}
        {extensionContext}
        <h4>{selected ? `Annotations · ${selected.title ?? selected.kind}` : 'Annotations'}</h4>
        {!selected && <p>Select a trace item or annotation marker to inspect its notes.</p>}
        {selected && attached(selected).length === 0 && <p>No annotations on this item.</p>}
        {selected && attached(selected).filter(note => !superseded.has(note.id)).map(note => <AnnotationNote key={note.id} annotation={note} superseded={false} onOpen={onOpenAnnotation}/>)}
        {selected && attached(selected).some(note => superseded.has(note.id)) && <details><summary>Superseded annotation history</summary>{attached(selected).filter(note => superseded.has(note.id)).map(note => <AnnotationNote key={note.id} annotation={note} superseded={true}/>)}</details>}
        {unresolved.length > 0 && <details><summary>{unresolved.length} unresolved annotation targets</summary><p>These targets are absent from this projection or refer to a different trace version.</p>{unresolved.map(note => <AnnotationNote key={note.id} annotation={note} superseded={superseded.has(note.id)} onOpen={onOpenAnnotation}/>)}</details>}
      </aside> : null}
    </div>
  </section>;
}

export function ReActTraceView(props: Omit<TraceViewProps, 'view'>) { return <GeneralTraceView {...props} view="react"/>; }
export function CodexAppServerTraceView(props: Omit<TraceViewProps, 'view'>) { return <GeneralTraceView {...props} view="codex"/>; }
export function ContainerTraceView(props: TraceViewProps & { extension: TraceViewExtension }) { return <GeneralTraceView {...props}/>; }

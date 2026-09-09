/** Read-only consumer of containers' synth.trace-visual.v1. No eval-specific parsing. */
export type TraceSelector = { entity_id?: string; kind?: string; trace_digest?: string; [key: string]: unknown };
export type TraceItem = {
  item_id: string; kind: string; title?: string; occurred_at?: string; sequence?: number;
  actor_id?: string | null; session_id?: string | null; lane_id?: string | null;
  status?: string; source_selector?: TraceSelector | null; source_digest?: string | null;
  detail?: Record<string, any>;
};
export type TraceVisual = {
  schema_version?: string; capture_id?: string; trace_id?: string | null; trace_digest?: string | null;
  state?: string; lanes?: { lane_id: string; actor_id?: string; session_id?: string; display_name?: string; color?: string; role?: string }[];
  items?: TraceItem[]; losses?: string[]; summary?: Record<string, any>;
};
export type TraceSection = 'messages' | 'rollout' | 'evidence' | 'events';
export function eventFamily(item: TraceItem): 'input' | 'thinking' | 'tool' | 'output' | 'message' | 'reward' | 'annotation' | 'event' {
  const kind = item.kind;
  if (kind === 'model_call.completed' && (item.detail?.reasoning || item.detail?.reasoning_details?.length)) return 'thinking';
  if (/reward/.test(kind)) return 'reward';
  if (/annotation|judgment|verifier|rubric|evidence/.test(kind)) return 'annotation';
  if (/coordination.*message/.test(kind)) return 'message';
  if (/reasoning|thinking|thought/.test(kind)) return 'thinking';
  if (/tool|action\.|environment.action_executed/.test(kind)) return 'tool';
  if (/model_call.started|input|observation/.test(kind)) return 'input';
  if (/model_call.completed|model_call.finished|output/.test(kind)) return 'output';
  if (/message/.test(kind)) return 'message';
  return 'event';
}
export function eventActor(item: TraceItem, items: TraceItem[]): string | null {
  return item.actor_id ?? item.detail?.actor_id ?? items.find(i => i.item_id === item.source_selector?.entity_id)?.actor_id ?? null;
}
export function eventTime(item: TraceItem, items: TraceItem[]): number | null {
  const target = items.find(i => i.item_id === item.source_selector?.entity_id && i.item_id !== item.item_id);
  const ms = item.detail?.elapsed_ms ?? item.detail?.elapsedMs ?? target?.detail?.elapsed_ms;
  return typeof ms === 'number' && Number.isFinite(ms) ? ms : null;
}
export function decisionKey(item: TraceItem): string | null {
  const d = item.detail ?? {};
  const id = d.decision_id ?? d.decisionId ?? d.call_id ?? d.span_id;
  return id == null ? null : JSON.stringify([item.actor_id ?? item.lane_id, item.session_id, id]);
}
export function decisionGroups(items: TraceItem[]): { id: string; items: TraceItem[] }[] {
  const groups = new Map<string, TraceItem[]>();
  for (const item of items) {
    if (item.kind === 'span.model_call' && item.detail?.event_ids?.length) continue;
    const id = decisionKey(item) ?? item.item_id;
    const group = groups.get(id) ?? []; group.push(item); groups.set(id, group);
  }
  return [...groups].map(([id, items]) => ({ id, items }));
}
export function traceText(value: unknown): string { return typeof value === 'string' ? value : JSON.stringify(value, null, 2) ?? ''; }

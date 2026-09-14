import type { TraceItem, TraceSelector, TraceVisual } from './model.ts';

/** Annotations remain evidence records; views never rewrite their selectors. */
export type TraceAnnotation = {
  id: string; target: TraceSelector; body: string; labels: string[];
  author: string; reviewState: string; createdAt?: string; supersedesId?: string;
  evidence: TraceSelector[];
  grounding?: string;
};

export function traceAnnotations(items: TraceItem[]): TraceAnnotation[] {
  return items.filter(item => item.kind === 'evidence.annotation').map(item => {
    const d = item.detail ?? {};
    return {
      id: item.item_id, target: d.target ?? item.source_selector ?? {},
      body: d.rationale ?? d.summary ?? item.title ?? '', labels: d.labels ?? [],
      author: d.producer?.name ?? d.producer?.producer_id ?? d.producer?.id ?? d.author_kind ?? 'Unknown author',
      reviewState: d.review_state || item.status || 'Unreviewed', createdAt: item.occurred_at,
      supersedesId: d.supersedes_id, evidence: d.evidence ?? [],
      grounding: d.grounding,
    };
  });
}

export function itemSelector(item: TraceItem, trace: TraceVisual): TraceSelector {
  return { trace_id: trace.trace_id ?? undefined, trace_digest: trace.trace_digest ?? undefined, ...item.source_selector };
}

export function annotationMatches(annotation: TraceAnnotation, item: TraceItem, trace: TraceVisual): boolean {
  const a = annotation.target; const b = itemSelector(item, trace);
  // Missing identity is unresolved, never permission to match by display label.
  return Boolean(a.trace_id && a.trace_digest && a.entity_id && a.kind &&
    a.trace_id === b.trace_id && a.trace_digest === b.trace_digest &&
    a.entity_id === b.entity_id && a.kind === b.kind);
}

export function annotationTargetLabel(target: TraceSelector): string {
  const suffix = [target.part_id, target.json_pointer, target.range ? 'text range' : null].filter(Boolean).join(' · ');
  return `${target.kind ?? 'Unresolved target'} ${target.entity_id ?? ''}${suffix ? ` · ${suffix}` : ''}`;
}

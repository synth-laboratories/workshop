/**
 * W0 live-eval bind + reducer contract.
 *
 * Slot `stream` only (not `live` or `jobs`). Bind a declared stream URL.
 * Missing reward / usage / cost stay missing. Heartbeats and
 * `stream.subscribed` do not become evidence.
 */

export const LIVE_EVAL_SLOT = "stream";
export const FORBIDDEN_LIVE_EVAL_SLOTS = ["live", "jobs"] as const;

const LIVE_EVAL_TEMPLATE_PREFIXES = [
  "live.harbor",
  "live.container",
  "live.eval",
  "live.craftax",
  "live.digbench"
] as const;

export type DeclaredStreamDescriptor = {
  id?: string;
  sse_url?: string;
  transports?: {
    poll?: { url?: string | null };
    sse?: { url?: string | null };
    websocket?: { url?: string | null };
  };
};

export type LiveEnvelope = {
  event_id?: string | null;
  sequence?: number | string | null;
  sequence_number?: number | null;
  kind?: string | null;
  type?: string | null;
  ts?: string;
  occurred_at?: string;
  run_id?: string;
  rollout_id?: string;
  lane?: string | null;
  control?: boolean;
  payload?: Record<string, unknown>;
  [key: string]: unknown;
};

export type LiveIngestState = {
  events: LiveEnvelope[];
  ready: boolean;
  ids: Set<string>;
  digests: Map<string, string>;
  lastSequenceByScope: Map<string, number>;
  receivedSequencesByScope: Map<string, Set<number>>;
  gaps: Array<{ scope: string; after: number; before: number }>;
  conflicts: string[];
};

export function isLiveEvalTemplate(templateId: string): boolean {
  return LIVE_EVAL_TEMPLATE_PREFIXES.some((prefix) => templateId.startsWith(prefix));
}

export function assertLiveEvalSlot(slot: string, templateId?: string): string | null {
  if (FORBIDDEN_LIVE_EVAL_SLOTS.includes(slot as (typeof FORBIDDEN_LIVE_EVAL_SLOTS)[number])) {
    return `Forbidden live-eval slot "${slot}"; bind slot "${LIVE_EVAL_SLOT}"`;
  }
  if (templateId && isLiveEvalTemplate(templateId) && slot !== LIVE_EVAL_SLOT) {
    return `Live eval template "${templateId}" must bind slot "${LIVE_EVAL_SLOT}", not "${slot}"`;
  }
  return null;
}

export function declaredSseUrl(descriptor: DeclaredStreamDescriptor | null | undefined): string | null {
  if (!descriptor) return null;
  const fromTransport = descriptor.transports?.sse?.url;
  if (typeof fromTransport === "string" && fromTransport.length > 0) return fromTransport;
  if (typeof descriptor.sse_url === "string" && descriptor.sse_url.length > 0) return descriptor.sse_url;
  return null;
}

/** True when `source` looks like a caller-constructed Craftax/Harbor guess. */
export function isGuessedStreamUrl(source: string): boolean {
  try {
    const path = new URL(source, "http://127.0.0.1").pathname.replace(/\/+$/, "");
    if (path === "/events") return true;
    if (/^\/rollouts\/[^/]+\/stream$/.test(path)) return true;
    return false;
  } catch {
    return source === "/events" || /\/rollouts\/[^/]+\/stream$/.test(source);
  }
}

/** `/events` was never echoed by create-rollout. */
export function isNeverDeclaredStreamUrl(source: string): boolean {
  try {
    return new URL(source, "http://127.0.0.1").pathname.replace(/\/+$/, "") === "/events";
  } catch {
    return source === "/events" || source.replace(/\/+$/, "").endsWith("/events");
  }
}

export function assertDeclaredStreamSource(
  source: string,
  descriptor?: DeclaredStreamDescriptor | null
): string | null {
  const declared = declaredSseUrl(descriptor ?? undefined);
  if (declared) {
    if (source === declared || source.endsWith(declared) || declared.endsWith(source)) return null;
    return `Stream URL is not the declared stream id/url (got ${source})`;
  }
  if (isNeverDeclaredStreamUrl(source)) {
    return `Refusing guessed stream URL "${source}"; bind the declared stream.id from create-rollout`;
  }
  return null;
}

export function isControlEnvelope(event: LiveEnvelope): boolean {
  const kind = String(event.kind ?? event.type ?? "");
  return (
    kind === "stream.subscribed" ||
    kind === "heartbeat" ||
    kind === "stream.heartbeat" ||
    kind === "ping"
  );
}

export function envelopeIdentity(event: LiveEnvelope, index: number): string {
  // Sequence/event_id is monotonic only within a rollout. A multiplexed run
  // legitimately contains ten `event_id: "1"` records, so identity must keep
  // the producer lane. Treating event_id as globally unique silently drops
  // all but one lane while still making the aggregate lane count look valid.
  const scope = event.rollout_id ?? event.lane ?? event.run_id ?? "run";
  if (typeof event.event_id === "string" && event.event_id.length > 0) {
    return `${scope}:${event.event_id}`;
  }
  const sequence = event.sequence_number ?? event.sequence;
  if (sequence != null && String(sequence).length > 0) {
    return `${scope}:${sequence}`;
  }
  return `${scope}:${event.kind ?? event.type ?? "event"}:${event.occurred_at ?? event.ts ?? index}`;
}

export function emptyLiveIngest(): LiveIngestState {
  return {
    events: [], ready: false, ids: new Set(), digests: new Map(),
    lastSequenceByScope: new Map(), receivedSequencesByScope: new Map(), gaps: [], conflicts: []
  };
}

/** Append one envelope. Control records can set ready; they are not evidence. */
export function ingestLiveEnvelope(state: LiveIngestState, event: LiveEnvelope): LiveIngestState {
  const id = envelopeIdentity(event, state.events.length);
  const digest = typeof event.digest === "string" ? event.digest : JSON.stringify(event);
  if (state.ids.has(id)) {
    const previous = state.digests.get(id);
    if (previous === digest) return state;
    return { ...state, conflicts: [...state.conflicts, `Conflicting duplicate envelope ${id}`] };
  }
  const ids = new Set(state.ids);
  ids.add(id);
  const digests = new Map(state.digests);
  digests.set(id, digest);
  if (isControlEnvelope(event)) {
    const kind = String(event.kind ?? event.type ?? "");
    return {
      ...state,
      events: state.events,
      ids,
      digests,
      ready: state.ready || kind === "stream.subscribed"
    };
  }
  const scope = String(event.rollout_id ?? event.lane ?? event.run_id ?? "run");
  const rawSequence = event.sequence_number ?? event.sequence;
  const sequence = typeof rawSequence === "number" ? rawSequence : Number(rawSequence);
  const lastSequenceByScope = new Map(state.lastSequenceByScope);
  const receivedSequencesByScope = new Map(state.receivedSequencesByScope);
  let gaps = [...state.gaps];
  if (Number.isFinite(sequence)) {
    const received = new Set(receivedSequencesByScope.get(scope) ?? []);
    received.add(sequence);
    receivedSequencesByScope.set(scope, received);
    const ordered = [...received].sort((a, b) => a - b);
    lastSequenceByScope.set(scope, ordered.at(-1) ?? sequence);
    gaps = gaps.filter((gap) => gap.scope !== scope);
    for (let index = 1; index < ordered.length; index++) {
      if (ordered[index] > ordered[index - 1] + 1) {
        gaps.push({ scope, after: ordered[index - 1], before: ordered[index] });
      }
    }
  }
  return { ...state, events: [...state.events, event], ids, digests, lastSequenceByScope, receivedSequencesByScope, gaps, ready: state.ready };
}

export function ingestLiveEnvelopes(events: LiveEnvelope[]): LiveIngestState {
  return events.reduce(ingestLiveEnvelope, emptyLiveIngest());
}

export function missingNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

export function formatMissingNumber(value: unknown, digits = 2): string {
  const n = missingNumber(value);
  return n == null ? "—" : n.toFixed(digits);
}

export function formatMissingUsd(value: unknown): string {
  const n = missingNumber(value);
  return n == null ? "—" : `$${n.toFixed(n >= 0.01 ? 2 : 4)}`;
}

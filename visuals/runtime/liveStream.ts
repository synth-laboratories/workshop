/**
 * W0 live-eval bind + reducer contract.
 *
 * Input `stream` only (not `live` or `jobs`). Bind a declared stream URL.
 * Missing reward / usage / cost stay missing. Heartbeats and
 * `stream.subscribed` do not become evidence.
 */

export const LIVE_EVAL_INPUT = "stream";
/** Alias of `LIVE_EVAL_INPUT` so existing imports compile. */
export const LIVE_EVAL_SLOT = LIVE_EVAL_INPUT;
export const FORBIDDEN_LIVE_EVAL_SLOTS = ["live", "jobs"] as const;

const LIVE_EVAL_TEMPLATE_PREFIXES = [
  "live.harbor",
  "live.container",
  "live.eval",
  "live.craftax"
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
    return `Forbidden live-eval input "${slot}"; bind input "${LIVE_EVAL_INPUT}"`;
  }
  if (templateId && isLiveEvalTemplate(templateId) && slot !== LIVE_EVAL_INPUT) {
    return `Live eval template "${templateId}" must bind input "${LIVE_EVAL_INPUT}", not "${slot}"`;
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

/**
 * The single definition of "control" for the whole live-eval pipeline: the
 * ingest fold, the projector (`liveEvalReducer.projectLiveEval`) and the
 * optimizer compose path all decide control-ness here and nowhere else.
 *
 * An explicit `control: true` flag counts, not just a known control kind. The
 * projector already honoured the flag while the fold checked kind only, so an
 * envelope flagged `control: true` under an ordinary kind was evidence to one
 * and not the other — it became a row in `LiveIngestState.events` that the
 * projection then silently dropped.
 */
export function isControlEnvelope(event: LiveEnvelope): boolean {
  if (event.control === true) return true;
  const kind = String(event.kind ?? event.type ?? "");
  return (
    kind === "stream.subscribed" ||
    kind === "heartbeat" ||
    kind === "stream.heartbeat" ||
    kind === "ping"
  );
}

/** Placement `includeKinds` matches envelope `kind` or `type`. */
export function eventMatchesIncludeKinds(
  event: { kind?: string | null; type?: string | null },
  includeKinds?: string[]
): boolean {
  if (!includeKinds?.length) return true;
  const labels = [event.kind, event.type].filter(
    (value): value is string => typeof value === "string" && value.length > 0
  );
  return includeKinds.some((kind) => labels.includes(kind));
}

function payloadString(event: LiveEnvelope, ...keys: string[]): string {
  const payload = event.payload;
  if (!payload) return "";
  for (const key of keys) {
    const value = payload[key];
    if (typeof value === "string" && value.length > 0) return value;
  }
  return "";
}

/**
 * Producers may carry transport identity in the envelope payload. Promote that
 * declared identity at the ingestion boundary so every viewer gets the same
 * rollout-local de-duplication and lane projection without knowing a producer's
 * wire shape.
 */
export function envelopeScope(event: LiveEnvelope): string {
  const streamId = typeof event.stream_id === "string" && event.stream_id.length > 0
    ? event.stream_id
    : payloadString(event, "stream_id", "stream.id");
  return streamId
    || event.rollout_id
    || payloadString(event, "rollout_id")
    || event.lane
    || payloadString(event, "lane")
    || event.run_id
    || payloadString(event, "run_id")
    || "run";
}

function normalizeEnvelopeIdentity(event: LiveEnvelope): LiveEnvelope {
  const rolloutId = event.rollout_id || payloadString(event, "rollout_id");
  const lane = event.lane || payloadString(event, "lane") || rolloutId;
  const runId = event.run_id || payloadString(event, "run_id");
  const streamId = typeof event.stream_id === "string" && event.stream_id.length > 0
    ? event.stream_id
    : payloadString(event, "stream_id", "stream.id");
  if (!rolloutId && !lane && !runId && !streamId) return event;
  return {
    ...event,
    ...(rolloutId ? { rollout_id: rolloutId } : {}),
    ...(lane ? { lane } : {}),
    ...(runId ? { run_id: runId } : {}),
    ...(streamId ? { stream_id: streamId } : {}),
  };
}

export function envelopeIdentity(event: LiveEnvelope, index: number): string {
  // Sequence/event_id is monotonic only within a rollout. A multiplexed run
  // legitimately contains ten `event_id: "1"` records, so identity must keep
  // the producer lane. Treating event_id as globally unique silently drops
  // all but one lane while still making the aggregate lane count look valid.
  const streamId = typeof event.stream_id === "string" && event.stream_id.length > 0
    ? event.stream_id
    : payloadString(event, "stream_id", "stream.id");
  const sequence = event.sequence_number ?? event.sequence;
  if (streamId && sequence != null && String(sequence).length > 0) {
    return `${streamId}:${sequence}`;
  }
  const scope = envelopeScope(event);
  if (typeof event.event_id === "string" && event.event_id.length > 0) {
    return `${scope}:${event.event_id}`;
  }
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

/**
 * Append a batch while cloning each collection only once. Live transports can
 * deliver thousands of messages in one browser task; applying the single-row
 * reducer for each message made 100k-envelope runs quadratic in both array
 * copies and sequence-gap scans.
 */
export function ingestLiveEnvelopeBatch(
  state: LiveIngestState,
  incoming: LiveEnvelope[]
): LiveIngestState {
  if (incoming.length === 0) return state;
  const ids = new Set(state.ids);
  const digests = new Map(state.digests);
  const events = [...state.events];
  const lastSequenceByScope = new Map(state.lastSequenceByScope);
  const receivedSequencesByScope = new Map(state.receivedSequencesByScope);
  const clonedSequenceScopes = new Set<string>();
  const touchedSequenceScopes = new Set<string>();
  const conflicts = [...state.conflicts];
  let ready = state.ready;

  for (const event of incoming) {
    const id = envelopeIdentity(event, events.length);
    const digest = typeof event.digest === "string" ? event.digest : JSON.stringify(event);
    if (ids.has(id)) {
      const previous = digests.get(id);
      if (previous !== digest) conflicts.push(`Conflicting duplicate envelope ${id}`);
      continue;
    }
    ids.add(id);
    digests.set(id, digest);
    const control = isControlEnvelope(event);
    if (control) {
      ready ||= String(event.kind ?? event.type ?? "") === "stream.subscribed";
    } else {
      // Only non-control envelopes are evidence: they alone become rows, and
      // they alone advance the per-scope evidence high-water mark.
      events.push(normalizeEnvelopeIdentity(event));
    }
    const scope = envelopeScope(event);
    const rawSequence = event.sequence_number ?? event.sequence;
    // `Number(null)` is 0 and `Number("")` is 0; an absent sequence must read as
    // absent, not as sequence zero, or it manufactures a gap before sequence 1.
    const sequence = typeof rawSequence === "number"
      ? rawSequence
      : rawSequence != null && String(rawSequence).length > 0
        ? Number(rawSequence)
        : Number.NaN;
    if (!Number.isFinite(sequence)) continue;
    if (!clonedSequenceScopes.has(scope)) {
      receivedSequencesByScope.set(scope, new Set(receivedSequencesByScope.get(scope) ?? []));
      clonedSequenceScopes.add(scope);
    }
    // A control envelope that carries a sequence consumed a number in the
    // producer's stream. Recording it keeps the scope's numbering contiguous;
    // omitting it made every sequenced heartbeat a permanent phantom gap.
    receivedSequencesByScope.get(scope)!.add(sequence);
    touchedSequenceScopes.add(scope);
    if (control) continue;
    lastSequenceByScope.set(scope, Math.max(lastSequenceByScope.get(scope) ?? sequence, sequence));
  }

  let gaps = state.gaps.filter((gap) => !touchedSequenceScopes.has(gap.scope));
  for (const scope of touchedSequenceScopes) {
    const ordered = [...(receivedSequencesByScope.get(scope) ?? [])].sort((a, b) => a - b);
    for (let index = 1; index < ordered.length; index++) {
      if (ordered[index] > ordered[index - 1] + 1) {
        gaps.push({ scope, after: ordered[index - 1], before: ordered[index] });
      }
    }
  }
  return { events, ids, digests, lastSequenceByScope, receivedSequencesByScope, gaps, conflicts, ready };
}

/** Append one envelope. Control records can set ready; they are not evidence. */
export function ingestLiveEnvelope(state: LiveIngestState, event: LiveEnvelope): LiveIngestState {
  return ingestLiveEnvelopeBatch(state, [event]);
}

export function ingestLiveEnvelopes(events: LiveEnvelope[], state = emptyLiveIngest()): LiveIngestState {
  return ingestLiveEnvelopeBatch(state, events);
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

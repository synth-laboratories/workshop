/**
 * W0 live-eval bind + reducer contract — the TypeScript mirror of the fold.
 *
 * Input `stream` only (not `live` or `jobs`). Bind a declared stream URL.
 * Missing reward / usage / cost stay missing. Heartbeats and
 * `stream.subscribed` do not become evidence.
 *
 * # This is a mirror, not the fold
 *
 * The authoritative fold is `src-tauri/src/stream_fold.rs`. It owns identity,
 * scope, dedupe, conflict detection, gap scanning and the projection, and it
 * is what the receipt, the readiness gate, the spool and the seal read.
 *
 * What survives here is what a host with no Rust underneath it cannot render
 * without — browser preview, fixture replay, and the two shipped shells still
 * have to draw a pane. So: identity, dedupe, the control predicate, the
 * evidence high-water mark, and the projection next door.
 *
 * What does **not** survive here is the sequence-gap scan. A gap is a claim
 * about a producer's sequence space, it is read by the readiness gate and by
 * agents rather than drawn, and the host already observes it at the poll seam
 * and emits `STREAM_REPLAY_GAP` from there — server-side, correlated to the
 * visual and its revision, and not reported by the thing under test. Two
 * implementations of that claim is two answers to a question that must have
 * one. See `stream_fold.rs` for the gap rules and their tests.
 *
 * The two sides are pinned together by `visuals/fixtures/live_fold_golden.json`
 * over every checked-in fixture, asserted from both languages. A mirror is
 * honest exactly as long as something checks it; do not edit one side alone.
 */

export const LIVE_EVAL_INPUT = "stream";
/** Alias of `LIVE_EVAL_INPUT` so existing imports compile. */
export const LIVE_EVAL_SLOT = LIVE_EVAL_INPUT;
export const FORBIDDEN_LIVE_EVAL_SLOTS = ["live", "jobs"] as const;

function isDeclaredLiveEvalAuxiliary(slot: string, templateId: string): boolean {
  // Craftax keeps `stream` as its only gameplay transport. The optional
  // optimizer input contributes run lifecycle, evidence disposition, and
  // proxy usage; it must never be interpreted as a replacement stream.
  return templateId === "live.craftax.v1" && slot === "optimizer_run";
}

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
  /**
   * Highest sequence *evidence* reached per scope. Control envelopes hold
   * their place in the producer's numbering for the host's gap scan, but they
   * never advance this: a stream carrying nothing but sequenced heartbeats has
   * not made progress, and a reader of this number must not be told it has.
   */
  lastSequenceByScope: Map<string, number>;
  conflicts: string[];
  /** Envelopes handed over, duplicates included. Names an envelope that carries
   * no identity of its own; see `envelopeIdentity`. */
  delivered: number;
};

function normalizeLiveEnvelope(value: unknown): LiveEnvelope | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const event = value as LiveEnvelope;
  const kind = typeof event.kind === "string" && event.kind.length > 0
    ? event.kind
    : typeof event.type === "string" && event.type.length > 0
      ? event.type
      : null;
  if (!kind) return null;
  return event.kind === kind ? event : { ...event, kind };
}

export function isLiveEvalTemplate(templateId: string): boolean {
  return LIVE_EVAL_TEMPLATE_PREFIXES.some((prefix) => templateId.startsWith(prefix));
}

export function assertLiveEvalSlot(slot: string, templateId?: string): string | null {
  if (FORBIDDEN_LIVE_EVAL_SLOTS.includes(slot as (typeof FORBIDDEN_LIVE_EVAL_SLOTS)[number])) {
    return `Forbidden live-eval input "${slot}"; bind input "${LIVE_EVAL_INPUT}"`;
  }
  if (templateId && isLiveEvalTemplate(templateId) && slot !== LIVE_EVAL_INPUT && !isDeclaredLiveEvalAuxiliary(slot, templateId)) {
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

/**
 * The stream an envelope was delivered on, for a cutoff cursor vector.
 *
 * The declared stream when the producer names one, the lane otherwise. A
 * cutoff addresses arrival order *within a stream*, which is the one total
 * order that exists whatever a producer does with its sequence numbers.
 */
export function envelopeStream(event: LiveEnvelope): string {
  const streamId = typeof event.stream_id === "string" && event.stream_id.length > 0
    ? event.stream_id
    : payloadString(event, "stream_id", "stream.id");
  return streamId || envelopeScope(event);
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

export function envelopeIdentity(event: LiveEnvelope, ordinal: number): string {
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
  return `${scope}:${event.kind ?? event.type ?? "event"}:${event.occurred_at ?? event.ts ?? ordinal}`;
}

/**
 * A body digest that does not depend on key order.
 *
 * Only equality matters here: this decides whether one identity arrived twice
 * with two different bodies. `JSON.stringify` preserves insertion order, so
 * two byte-equivalent envelopes whose producer emitted their keys in a
 * different order read as a conflict — and the Rust fold, whose map is sorted,
 * reads them as the same record. Sorting is the answer both sides can give;
 * porting the accident is not.
 */
function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    const record = value as Record<string, unknown>;
    return `{${Object.keys(record).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(record[key])}`).join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

export function emptyLiveIngest(): LiveIngestState {
  return {
    events: [], ready: false, ids: new Set(), digests: new Map(),
    lastSequenceByScope: new Map(), conflicts: [], delivered: 0
  };
}

/**
 * Append a batch while cloning each collection only once. Live transports can
 * deliver thousands of messages in one browser task; applying the single-row
 * reducer for each message made 100k-envelope runs quadratic in array copies.
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
  const conflicts = [...state.conflicts];
  let delivered = state.delivered;
  let ready = state.ready;

  for (const candidate of incoming as unknown[]) {
    delivered += 1;
    const event = normalizeLiveEnvelope(candidate);
    if (!event) {
      conflicts.push("Malformed live-eval envelope: expected an object with a non-empty kind or type");
      continue;
    }
    const id = envelopeIdentity(event, delivered);
    const digest = typeof event.digest === "string" ? event.digest : canonicalJson(event);
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
      // A control envelope holds its place in the producer's numbering — the
      // host's gap scan needs that — but it is never evidence, so it becomes
      // no row and advances no high-water mark.
      continue;
    }
    events.push(normalizeEnvelopeIdentity(event));
    const scope = envelopeScope(event);
    const rawSequence = event.sequence_number ?? event.sequence;
    // `Number(null)` is 0 and `Number("")` is 0; an absent sequence must read
    // as absent, not as sequence zero.
    const sequence = typeof rawSequence === "number"
      ? rawSequence
      : rawSequence != null && String(rawSequence).length > 0
        ? Number(rawSequence)
        : Number.NaN;
    if (!Number.isFinite(sequence)) continue;
    lastSequenceByScope.set(scope, Math.max(lastSequenceByScope.get(scope) ?? sequence, sequence));
  }
  return { events, ids, digests, lastSequenceByScope, conflicts, delivered, ready };
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

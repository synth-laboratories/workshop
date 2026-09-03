/**
 * Pure projection for live.annotated_rollouts.v1.
 *
 * Two event families arrive on the same fold, keyed by rollout_id:
 *
 * - the rollout's own stream (`observation`, `action`, `reward_signal`,
 *   `span.policy.*`, lifecycle) — the underlying evidence;
 * - the annotation stream a bound protocol publishes beside it
 *   (`annotation.finding`, `annotation.finding.retracted`, `annotation.metric`,
 *   `annotation.model.*`, `annotation.protocol.*`, `annotation.closed`) — the
 *   provisional summary layer.
 *
 * Relayed optimizer envelopes (`eval.trial.event` / `eval.trial.annotation`)
 * are unwrapped to the same shapes so a durable run journal replays through
 * the identical reducer. Nothing here is authoritative: findings stay labelled
 * provisional until a post-hoc annotator confirms them against the sealed
 * trace, and a retraction is shown, never erased.
 */
import type { LiveEvalEvent } from "../../../runtime/types.ts";

export type FindingStatus = "provisional" | "superseded" | "retracted";

export type Finding = {
  findingId: string;
  kind: string;
  label: string;
  status: FindingStatus;
  step?: number;
  confidence?: number;
  sequences: number[];
  sourceSequence?: number;
  supersedes?: string;
  supersededBy?: string;
  retractedReason?: string;
  basis?: string;
  detail: Record<string, unknown>;
  ts: string;
  logicalTime?: number;
};

export type Marker = { sequence: number; logicalTime?: number; step?: number; kind: string; label: string; status: FindingStatus; findingId: string };

export type LogicalEvent = {
  /** Shared, one-based replay tick across every rollout and annotation stream. */
  logicalTime: number;
  event: LiveEvalEvent;
  lane: string;
  stream: "rollout" | "annotation";
  /** Sequence in the event's own producer stream. */
  streamSequence?: number;
  /** Rollout sequence this annotation had observed when it emitted. */
  sourceSequence?: number;
  occurredAt: string;
};

export type LaneEvent = {
  kind: string;
  stream: "rollout" | "annotation";
  sequence?: number;
  sourceSequence?: number;
  logicalTime?: number;
  occurredAt: string;
  detail: string;
  payload: Record<string, unknown>;
  verifier: boolean;
};

export type LlmCall = {
  callId: string;
  role: "policy" | "verifier" | "annotator";
  model?: string;
  provider?: string;
  status: "running" | "completed" | "failed";
  startedAt?: number;
  endedAt?: number;
  sourceSequences: number[];
  events: LaneEvent[];
  findings: Finding[];
};

export type Lane = {
  name: string;
  /** Task family is advisory presentation metadata, never evidence. */
  family?: string;
  status: "starting" | "running" | "finished" | "failed";
  done: number;
  total?: number;
  reward?: number;
  achievements: string[];
  health?: number;
  food?: number;
  drink?: number;
  energy?: number;
  calls: number;
  planLength?: number;
  frameUrl?: string;
  last: string;
  rolloutEvents: number;
  /** Evidence-preserving event rows used by the per-rollout trace views. */
  trace: LaneEvent[];
  // --- annotation layer
  protocol?: { revisionId?: string; protocolId?: string; model?: string | null };
  /** Consumer -> annotator history: acknowledged controls and hot-swaps, in stream order. */
  controls: Array<{ sequence: number; op?: string; controlId?: string; accepted: boolean; reason?: string; sourceSequence?: number }>;
  rebinds: number;
  findings: Finding[];
  markers: Marker[];
  metrics: Record<string, number>;
  metricSeries: Record<string, Array<{ step?: number; value: number; sequence: number }>>;
  model: { requested: number; completed: number; failed: number };
  protocolErrors: number;
  annotationOutcome?: string;
  annotationClosed: boolean;
  annotationEvents: number;
  lastAnnotation: string;
  /** Task-specific facts retained for adapters without coupling the shared fold. */
  task: Record<string, unknown>;
};

export const FINDING_KIND_ORDER = ["achievement", "milestone", "failure_mode", "intent", "note"] as const;

function num(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() !== "" && Number.isFinite(Number(value))) return Number(value);
  return undefined;
}
function obj(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}
function str(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

/**
 * Select the rollout envelopes retained by the optimizer journal.
 *
 * The journal also contains run lifecycle, usage, and selection records. Those
 * are useful to the host, but feeding them to the rollout reducer invents a
 * synthetic lane named after the optimizer run. Keep only the relayed producer
 * envelopes and de-duplicate by the journal's durable identity before replay.
 */
export function durableAnnotatedRolloutEvents(
  terminalEvents: unknown[] | undefined,
  enrichmentEvents: unknown[] | undefined,
): LiveEvalEvent[] | undefined {
  const combined = [...(terminalEvents ?? []), ...(enrichmentEvents ?? [])];
  const seen = new Set<string>();
  const retained = combined.filter((candidate) => {
    const host = obj(candidate);
    const payload = obj(host.payload);
    const type = str(host.type) ?? str(host.kind) ?? str(payload.type);
    if (type !== "eval.trial.event") return false;
    const eventId = str(host.eventId) ?? str(host.event_id);
    const optimizerRunId = str(host.optimizerRunId) ?? str(host.optimizer_run_id);
    const sequence = num(host.sequenceNumber) ?? num(host.sequence_number);
    const delta = Object.keys(obj(host.delta)).length ? obj(host.delta) : obj(payload.delta);
    const inner = obj(delta.container_event ?? delta.containerEvent);
    const fallback = [
      str(inner.rollout_id) ?? str(inner.rolloutId) ?? str(delta.trial_id) ?? str(delta.trialId) ?? "rollout",
      str(inner.stream_id) ?? str(inner.streamId) ?? str(delta.stream) ?? "rollout",
      num(inner.sequence) ?? num(inner.sequence_number) ?? num(inner.sequenceNumber) ?? "—",
      str(inner.kind) ?? str(inner.type) ?? "event",
    ].join(":");
    const identity = eventId
      ? `event:${eventId}`
      : optimizerRunId && sequence != null
        ? `journal:${optimizerRunId}:${sequence}`
        : `producer:${fallback}`;
    if (seen.has(identity)) return false;
    seen.add(identity);
    return Object.keys(inner).length > 0;
  });
  if (!retained.length) return undefined;
  return retained.sort((left, right) => {
    const leftRecord = obj(left);
    const rightRecord = obj(right);
    return (num(leftRecord.sequenceNumber) ?? num(leftRecord.sequence_number) ?? Number.MAX_SAFE_INTEGER)
      - (num(rightRecord.sequenceNumber) ?? num(rightRecord.sequence_number) ?? Number.MAX_SAFE_INTEGER);
  }) as LiveEvalEvent[];
}

export function timestamp(event: LiveEvalEvent): string {
  const host = obj(event);
  return event.occurred_at ?? event.ts ?? str(host.occurredAt) ?? "";
}
export function eventLogicalTime(event: LiveEvalEvent): number | undefined {
  return num((event as LiveEvalEvent & { logical_time?: unknown }).logical_time);
}
export function laneName(event: LiveEvalEvent): string {
  const extra = event as LiveEvalEvent & { rollout_id?: string };
  return extra.rollout_id ?? event.lane ?? str(event.payload.rollout_id) ?? event.run_id ?? "rollout";
}

/**
 * A relayed optimizer envelope carries the container/annotation envelope in
 * its delta. Unwrap so the reducer sees one wire shape.
 */
export function unwrapRelayed(event: LiveEvalEvent): LiveEvalEvent {
  const host = obj(event);
  const payload = obj(host.payload);
  const type = str(host.type) ?? str(host.kind) ?? str(payload.type);
  if (type !== "eval.trial.event") return event;
  const delta = Object.keys(obj(host.delta)).length ? obj(host.delta) : obj(payload.delta);
  const inner = obj(delta.container_event ?? delta.containerEvent);
  if (!Object.keys(inner).length) return event;
  const kind = str(inner.kind) ?? str(inner.type) ?? event.kind;
  // Annotation rows ride the same carrier, tagged by `delta.stream` and the
  // envelope's own stream identity (`stream:<rollout>:annotations`).
  const rolloutId = str(inner.rollout_id) ?? str(inner.rolloutId) ?? str(delta.rollout_id) ?? str(delta.rolloutId);
  const streamId = str(inner.stream_id) ?? str(inner.streamId) ?? (delta.stream === "annotation" && rolloutId ? `stream:${rolloutId}:annotations` : undefined);
  return {
    ...event,
    kind,
    sequence: num(inner.sequence) ?? num(inner.sequence_number) ?? num(inner.sequenceNumber) ?? event.sequence,
    occurred_at: str(inner.occurred_at) ?? str(inner.occurredAt) ?? event.occurred_at ?? str(host.occurredAt),
    payload: obj(inner.payload),
    lane: rolloutId ?? event.lane,
    ...(rolloutId ? { rollout_id: rolloutId } : {}),
    ...(streamId ? { stream_id: streamId } : {}),
  } as LiveEvalEvent;
}

/**
 * Give all producer streams one deterministic replay clock.
 *
 * The ingest-assigned logical clock is authoritative because it records what
 * the viewer actually saw. Producer timestamps remain provenance and only
 * order legacy events that predate logical-time stamping. This means a late
 * backfill cannot jump into the past and renumber an already-visible replay.
 */
export function logicalTimeline(events: LiveEvalEvent[]): LogicalEvent[] {
  const ordered = events.map((raw, observedIndex) => {
    const event = unwrapRelayed(raw);
    const occurredAt = timestamp(event);
    const parsed = Date.parse(occurredAt);
    const annotation = isAnnotationEvent(event);
    const ownSequence = num(event.sequence);
    const acceptedLogicalTime = eventLogicalTime(event);
    return {
      event,
      observedIndex,
      acceptedLogicalTime,
      occurredAt,
      physicalTime: Number.isFinite(parsed) ? parsed : Number.POSITIVE_INFINITY,
      lane: laneName(event),
      stream: annotation ? "annotation" as const : "rollout" as const,
      streamSequence: ownSequence,
      sourceSequence: annotation ? num(event.payload.source_sequence) : ownSequence,
    };
  }).sort((left, right) =>
    (left.acceptedLogicalTime != null && right.acceptedLogicalTime != null
      ? left.acceptedLogicalTime - right.acceptedLogicalTime
      : left.acceptedLogicalTime != null
        ? -1
        : right.acceptedLogicalTime != null
          ? 1
          : 0)
    || left.physicalTime - right.physicalTime
    || left.occurredAt.localeCompare(right.occurredAt)
    || left.lane.localeCompare(right.lane)
    || (left.sourceSequence ?? Number.MAX_SAFE_INTEGER) - (right.sourceSequence ?? Number.MAX_SAFE_INTEGER)
    || (left.stream === right.stream ? 0 : left.stream === "rollout" ? -1 : 1)
    || (left.streamSequence ?? Number.MAX_SAFE_INTEGER) - (right.streamSequence ?? Number.MAX_SAFE_INTEGER)
    || left.observedIndex - right.observedIndex
  );
  return ordered.map((row, index) => {
    const logicalTime = row.acceptedLogicalTime ?? index + 1;
    return {
      logicalTime,
      event: { ...row.event, logical_time: logicalTime } as LiveEvalEvent,
      lane: row.lane,
      stream: row.stream,
      streamSequence: row.streamSequence,
      sourceSequence: row.sourceSequence,
      occurredAt: row.occurredAt,
    };
  });
}

/**
 * Annotation rows are recognised by their declared stream identity first
 * (`stream:<rollout>:annotations`), so the stream's own `capture.*` closing
 * records are attributed to the annotation layer and never to the rollout.
 */
export function isAnnotationEvent(event: LiveEvalEvent): boolean {
  const streamId = (event as LiveEvalEvent & { stream_id?: string }).stream_id;
  return event.kind.startsWith("annotation.")
    || (typeof streamId === "string" && streamId.endsWith(":annotations"));
}

export function eventDetail(event: LiveEvalEvent): string {
  const p = event.payload;
  const kind = event.kind;
  if (kind === "annotation.finding") return `${str(p.kind) ?? "finding"} · ${str(p.label) ?? ""}`.trim();
  if (kind === "annotation.finding.retracted") return `retracted ${str(p.finding_id) ?? ""} · ${str(p.reason) ?? ""}`.trim();
  if (kind === "annotation.metric") return `${str(p.name) ?? "metric"} = ${num(p.value) ?? "?"}`;
  if (kind === "annotation.model.requested") return `judge asked · ${str(p.model) ?? ""}`.trim();
  if (kind === "annotation.model.completed") return `judge answered · ${num(obj(p.usage).total_tokens) ?? "?"} tokens`;
  if (kind === "annotation.model.failed") return `judge failed · ${str(p.reason) ?? ""}`.trim();
  if (kind === "annotation.protocol.error") return `protocol error · ${str(p.stage) ?? ""}`.trim();
  if (kind === "annotation.closed") return `annotations closed · ${str(p.outcome) ?? ""}`.trim();
  if (kind === "annotation.control.received") return `control ${str(p.op) ?? ""} accepted · ${str(p.control_id) ?? ""}`.trim();
  if (kind === "annotation.control.refused") return `control refused · ${str(p.reason) ?? ""}`.trim();
  if (kind === "annotation.protocol.rebound") return `protocol rebound → ${str(p.protocol_revision_id) ?? ""}${p.state_carried ? " (state carried)" : ""}`;
  if (kind === "action") return `action · ${str(p.action) ?? ""}`.trim();
  if (kind === "reward_signal") return `reward ${num(p.value) ?? "unavailable"} @ step ${num(p.step) ?? "?"}`;
  if (kind === "observation") return `observation · step ${num(p.step) ?? "?"}`;
  if (kind === "span.policy.plan") return `plan · ${Array.isArray(p.actions) ? p.actions.length : num(p.length) ?? "?"} actions`;
  for (const key of ["message", "detail", "status", "phase"]) if (typeof p[key] === "string" && p[key]) return `${kind} · ${p[key]}`;
  return kind;
}

function newLane(name: string): Lane {
  return {
    name, status: "starting", done: 0, achievements: [], calls: 0, last: "opening rollout", rolloutEvents: 0,
    trace: [],
    findings: [], markers: [], metrics: {}, metricSeries: {}, model: { requested: 0, completed: 0, failed: 0 }, controls: [], rebinds: 0,
    protocolErrors: 0, annotationClosed: false, annotationEvents: 0, lastAnnotation: "waiting for the protocol", task: {},
  };
}

function isVerifierEvent(event: LiveEvalEvent): boolean {
  const kind = event.kind;
  const authority = str(event.payload.authority)?.toLowerCase();
  return kind === "rubric.grade"
    || kind.startsWith("span.verifier.")
    || kind.startsWith("span.evaluator.")
    || kind.startsWith("annotation.model.")
    || authority === "verifier"
    || authority === "grader"
    || authority === "evaluator";
}

function rememberTrace(lane: Lane, event: LiveEvalEvent): void {
  const annotation = isAnnotationEvent(event);
  lane.trace.push({
    kind: event.kind,
    stream: annotation ? "annotation" : "rollout",
    sequence: num(event.sequence),
    sourceSequence: annotation ? num(event.payload.source_sequence) : num(event.sequence),
    logicalTime: eventLogicalTime(event),
    occurredAt: timestamp(event),
    detail: eventDetail(event),
    payload: event.payload,
    verifier: isVerifierEvent(event),
  });
}

function rememberTaskFacts(lane: Lane, payload: Record<string, unknown>): void {
  const readout = obj(payload.readout);
  const observation = Object.keys(readout).length ? readout : obj(readout.observation);
  const sources = [payload, observation, obj(payload.result), obj(payload.score), obj(payload.rubric)];
  for (const source of sources) {
    for (const key of [
      "family", "task", "task_family", "query", "customer_query", "prompt", "response",
      "answer", "action", "label", "predicted_label", "canonical_label", "gold_label",
      "split", "seed", "criteria_met", "rubric_id", "rubric_text", "points", "score",
      "possible", "achieved", "finish_reason", "reward_kind",
      "skill", "level", "xp", "xp_delta", "xp_per_min", "peak_xp_per_min",
    ]) if (source[key] != null) lane.task[key] = source[key];
  }
  lane.family = str(lane.task.family) ?? str(lane.task.task_family) ?? str(lane.task.task) ?? lane.family;
}

function applyRollout(lane: Lane, event: LiveEvalEvent): void {
  const p = event.payload;
  const kind = event.kind;
  rememberTaskFacts(lane, p);
  lane.rolloutEvents += 1;
  if (kind === "env.episode.opened") { lane.status = "running"; lane.total = num(p.max_steps) ?? lane.total; }
  if (kind === "observation") {
    lane.status = "running";
    lane.done = num(p.step) ?? lane.done;
    const readout = obj(p.readout);
    const observation = Object.keys(readout).length ? readout : obj(obj(p.readout).observation);
    const ach = observation.achievements ?? p.achievements;
    if (Array.isArray(ach)) lane.achievements = ach.map(String);
    else if (ach && typeof ach === "object") lane.achievements = Object.entries(ach as Record<string, unknown>).filter(([, v]) => Boolean(v)).map(([k]) => k);
    const inventory = obj(observation.inventory);
    lane.health = num(inventory.health) ?? lane.health;
    lane.food = num(inventory.food) ?? lane.food;
    lane.drink = num(inventory.drink) ?? lane.drink;
    lane.energy = num(inventory.energy) ?? lane.energy;
  }
  if (kind === "snapshot") {
    lane.status = "running";
    const progress = obj(p.progress);
    lane.done = num(progress.done) ?? num(progress.env_steps) ?? num(p.step) ?? lane.done;
    lane.total = num(progress.total) ?? lane.total;
    lane.reward = num(p.total_reward) ?? num(p.reward) ?? lane.reward;
    if (typeof p.frame_url === "string") lane.frameUrl = p.frame_url;
  }
  if (kind === "frame" && typeof p.url === "string") lane.frameUrl = p.url;
  if (kind === "reward_signal") {
    const value = num(p.value);
    if (value != null) lane.reward = (lane.reward ?? 0) + value;
    lane.done = num(p.step) ?? lane.done;
    lane.task.reward_value = value;
    lane.task.reward_kind = p.kind ?? lane.task.reward_kind;
  }
  if (kind === "action") {
    lane.task.response = p.response ?? p.text ?? p.content ?? p.label ?? p.action ?? lane.task.response;
    lane.task.predicted_label = p.predicted_label ?? p.label ?? p.action ?? lane.task.predicted_label;
  }
  if (kind === "rubric.grade") {
    const grades = Array.isArray(lane.task.rubric_grades) ? lane.task.rubric_grades as unknown[] : [];
    lane.task.rubric_grades = [...grades, { ...p, logical_time: eventLogicalTime(event) }];
  }
  if (kind === "span.policy.plan") { lane.calls += 1; lane.planLength = Array.isArray(p.actions) ? p.actions.length : num(p.length); }
  if (kind === "achievement_unlocked") {
    const name = str(obj(p.payload).achievement) ?? str(p.achievement);
    if (name && !lane.achievements.includes(name)) lane.achievements = [...lane.achievements, name];
  }
  if (kind === "env.episode.closed" || kind === "status") {
    const status = str(p.status);
    if (status === "completed" || status === "truncated") lane.status = "finished";
    if (status === "failed" || status === "cancelled") lane.status = "failed";
    lane.done = num(p.steps) ?? lane.done;
  }
  if (kind === "eval.run.terminal" || kind === "run_finished") { lane.status = p.error ? "failed" : "finished"; lane.reward = num(p.reward) ?? lane.reward; }
  if (kind === "error" || kind === "eval.ops.warning") lane.status = "failed";
  lane.last = eventDetail(event);
}

function applyAnnotation(lane: Lane, event: LiveEvalEvent): void {
  const p = event.payload;
  const kind = event.kind;
  const sequence = num(event.sequence) ?? 0;
  lane.annotationEvents += 1;
  if (kind === "annotation.protocol.bound") {
    lane.protocol = { revisionId: str(p.protocol_revision_id), protocolId: str(p.protocol_id), model: (p.model as string | null | undefined) ?? null };
  } else if (kind === "annotation.finding") {
    const findingId = str(p.finding_id) ?? `finding:${sequence}`;
    const supersedes = str(p.supersedes);
    if (supersedes) {
      const previous = lane.findings.find((row) => row.findingId === supersedes);
      if (previous && previous.status === "provisional") { previous.status = "superseded"; previous.supersededBy = findingId; }
      for (const marker of lane.markers) if (marker.findingId === supersedes && marker.status === "provisional") marker.status = "superseded";
    }
    const evidence = obj(p.evidence);
    const finding: Finding = {
      findingId,
      kind: str(p.kind) ?? "note",
      label: str(p.label) ?? findingId,
      status: "provisional",
      step: num(p.step),
      confidence: num(p.confidence),
      sequences: Array.isArray(evidence.sequences) ? evidence.sequences.map((v) => num(v)).filter((v): v is number => v != null) : [],
      sourceSequence: num(p.source_sequence),
      supersedes,
      basis: str(obj(p.detail).basis),
      detail: obj(p.detail),
      ts: timestamp(event),
      logicalTime: eventLogicalTime(event),
    };
    lane.findings.push(finding);
    // Live protocols may expose structured task facts as finding detail. Keep
    // them available to the presentation adapter while preserving the finding.
    for (const [key, value] of Object.entries(finding.detail)) if (value != null) lane.task[key] = value;
    lane.markers.push({ sequence, logicalTime: finding.logicalTime, step: finding.step, kind: finding.kind, label: finding.label, status: "provisional", findingId });
  } else if (kind === "annotation.finding.retracted") {
    const findingId = str(p.finding_id);
    const finding = lane.findings.find((row) => row.findingId === findingId);
    if (finding) { finding.status = "retracted"; finding.retractedReason = str(p.reason); }
    for (const marker of lane.markers) if (marker.findingId === findingId) marker.status = "retracted";
  } else if (kind === "annotation.metric") {
    const name = str(p.name);
    const value = num(p.value);
    if (name && value != null) {
      lane.metrics[name] = value;
      (lane.metricSeries[name] ??= []).push({ step: num(p.step), value, sequence });
    }
  } else if (kind === "annotation.model.requested") lane.model.requested += 1;
  else if (kind === "annotation.model.completed") lane.model.completed += 1;
  else if (kind === "annotation.model.failed") lane.model.failed += 1;
  else if (kind === "annotation.protocol.error") lane.protocolErrors += 1;
  else if (kind === "annotation.control.received" || kind === "annotation.control.refused") {
    lane.controls.push({ sequence, op: str(p.op), controlId: str(p.control_id), accepted: kind === "annotation.control.received", reason: str(p.reason), sourceSequence: num(p.source_sequence) });
  } else if (kind === "annotation.protocol.rebound") {
    lane.rebinds += 1;
    lane.protocol = { ...(lane.protocol ?? {}), revisionId: str(p.protocol_revision_id), protocolId: str(p.protocol_id) ?? lane.protocol?.protocolId, model: (p.model as string | null | undefined) ?? null };
  }
  else if (kind === "annotation.closed") { lane.annotationOutcome = str(p.outcome); lane.annotationClosed = true; }
  else if (kind === "capture.closed") lane.annotationClosed = true;
  lane.lastAnnotation = eventDetail(event);
}

export function projectLanes(events: LiveEvalEvent[]): Lane[] {
  const lanes = new Map<string, Lane>();
  for (const raw of events) {
    const event = unwrapRelayed(raw);
    const name = laneName(event);
    const lane = lanes.get(name) ?? newLane(name);
    rememberTrace(lane, event);
    if (isAnnotationEvent(event)) {
      applyAnnotation(lane, event);
    } else {
      applyRollout(lane, event);
    }
    lanes.set(name, lane);
  }
  return [...lanes.values()];
}

export function activeFindings(lane: Lane): Finding[] {
  return lane.findings.filter((row) => row.status === "provisional");
}

function identityValues(payload: Record<string, unknown>): string[] {
  return ["request_id", "call_id", "model_call_id", "policy_call_id", "span_id", "invocation_id", "id", "call"]
    .map((key) => payload[key])
    .filter((value): value is string | number => typeof value === "string" || typeof value === "number")
    .map(String);
}

/**
 * Reconstruct inspectable LLM calls and attach only annotations with provenance.
 * Explicit call/request identities win; cited rollout/source sequences are the
 * fallback. A finding is never attached merely because it happened nearby.
 */
export function llmCalls(lane: Lane): LlmCall[] {
  const calls: LlmCall[] = [];
  let policy: LlmCall | undefined;
  let verifier: LlmCall | undefined;
  const annotators = new Map<string, LlmCall>();
  for (const row of lane.trace) {
    if (row.stream === "rollout" && (row.kind === "span.policy.opened" || (row.kind === "span.policy.plan" && !policy))) {
      policy = {
        callId: identityValues(row.payload)[0] ?? `policy:${row.sequence ?? calls.length + 1}`,
        role: "policy",
        model: str(row.payload.model),
        provider: str(row.payload.provider),
        status: "running",
        startedAt: row.logicalTime,
        sourceSequences: [],
        events: [],
        findings: [],
      };
      calls.push(policy);
    }
    if (policy && row.stream === "rollout") {
      policy.events.push(row);
      if (row.sequence != null && !policy.sourceSequences.includes(row.sequence)) policy.sourceSequences.push(row.sequence);
      policy.model ??= str(row.payload.model);
      policy.provider ??= str(row.payload.provider);
      if (row.kind === "span.policy.closed") {
        policy.status = str(row.payload.status) === "failed" ? "failed" : "completed";
        policy.endedAt = row.logicalTime;
        policy = undefined;
      }
    }
    if (row.stream === "rollout" && row.kind === "span.evaluator.opened") {
      verifier = {
        callId: identityValues(row.payload)[0] ?? `verifier:${row.sequence ?? calls.length + 1}`,
        role: "verifier",
        model: str(row.payload.model) ?? str(row.payload.wire_model),
        provider: str(row.payload.provider),
        status: "running",
        startedAt: row.logicalTime,
        sourceSequences: [],
        events: [],
        findings: [],
      };
      calls.push(verifier);
    }
    if (verifier && row.stream === "rollout") {
      verifier.events.push(row);
      if (row.sequence != null && !verifier.sourceSequences.includes(row.sequence)) verifier.sourceSequences.push(row.sequence);
      verifier.model ??= str(row.payload.model) ?? str(row.payload.wire_model);
      verifier.provider ??= str(row.payload.provider);
      if (row.kind === "span.evaluator.closed") {
        verifier.status = str(row.payload.status) === "failed" ? "failed" : "completed";
        verifier.endedAt = row.logicalTime;
        verifier = undefined;
      }
    }
    if (row.kind === "annotation.model.requested") {
      const id = identityValues(row.payload)[0] ?? `annotator:${row.sequence ?? calls.length + 1}`;
      const call: LlmCall = {
        callId: id,
        role: "annotator",
        model: str(row.payload.model),
        provider: str(row.payload.provider),
        status: "running",
        startedAt: row.logicalTime,
        sourceSequences: row.sourceSequence == null ? [] : [row.sourceSequence],
        events: [row],
        findings: [],
      };
      annotators.set(id, call);
      calls.push(call);
    } else if (row.kind === "annotation.model.completed" || row.kind === "annotation.model.failed") {
      const id = identityValues(row.payload)[0];
      const call = (id ? annotators.get(id) : undefined) ?? [...annotators.values()].reverse().find((candidate) => candidate.status === "running");
      if (call) {
        call.events.push(row);
        if (row.sourceSequence != null && !call.sourceSequences.includes(row.sourceSequence)) call.sourceSequences.push(row.sourceSequence);
        call.status = row.kind.endsWith("failed") ? "failed" : "completed";
        call.endedAt = row.logicalTime;
      }
    }
  }
  for (const call of calls) {
    const callIds = new Set([call.callId, ...call.events.flatMap((row) => identityValues(row.payload))]);
    const cited = new Set(call.sourceSequences);
    for (const finding of lane.findings) {
      const findingIds = identityValues(finding.detail);
      const explicit = findingIds.some((id) => callIds.has(id));
      const sequence = finding.sequences.some((value) => cited.has(value))
        || (finding.sourceSequence != null && cited.has(finding.sourceSequence));
      if (explicit || sequence) call.findings.push(finding);
    }
  }
  return calls;
}

export function countByKind(findings: Finding[]): Record<string, number> {
  const out: Record<string, number> = {};
  for (const row of findings) out[row.kind] = (out[row.kind] ?? 0) + 1;
  return out;
}

export function labelTally(lanes: Lane[], kind: string): Array<{ label: string; lanes: number; count: number }> {
  const byLabel = new Map<string, { lanes: Set<string>; count: number }>();
  for (const lane of lanes) {
    for (const row of activeFindings(lane)) {
      if (row.kind !== kind) continue;
      const entry = byLabel.get(row.label) ?? { lanes: new Set<string>(), count: 0 };
      entry.lanes.add(lane.name);
      entry.count += 1;
      byLabel.set(row.label, entry);
    }
  }
  return [...byLabel.entries()]
    .map(([label, entry]) => ({ label, lanes: entry.lanes.size, count: entry.count }))
    .sort((a, b) => b.lanes - a.lanes || b.count - a.count || a.label.localeCompare(b.label));
}

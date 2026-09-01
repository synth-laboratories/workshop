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
};

export type Marker = { sequence: number; step?: number; kind: string; label: string; status: FindingStatus; findingId: string };

export type Lane = {
  name: string;
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
  // --- annotation layer
  protocol?: { revisionId?: string; protocolId?: string; model?: string | null };
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
export function timestamp(event: LiveEvalEvent): string {
  return event.occurred_at ?? event.ts ?? "";
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
  const type = event.type ?? (typeof event.payload.type === "string" ? (event.payload.type as string) : undefined);
  if (type !== "eval.trial.event" && type !== "eval.trial.annotation") return event;
  const delta = obj(event.payload.delta);
  const inner = obj(delta.container_event ?? delta.annotation_event);
  if (!Object.keys(inner).length) return event;
  const kind = str(inner.kind) ?? event.kind;
  return {
    ...event,
    kind,
    sequence: num(inner.sequence) ?? event.sequence,
    occurred_at: str(inner.occurred_at) ?? event.occurred_at,
    payload: obj(inner.payload),
    lane: str(inner.rollout_id) ?? event.lane,
    ...(str(inner.rollout_id) ? { rollout_id: inner.rollout_id as string } : {}),
  } as LiveEvalEvent;
}

/**
 * Annotation rows are recognised by their declared stream identity first
 * (`stream:<rollout>:annotations`), so the stream's own `capture.*` closing
 * records are attributed to the annotation layer and never to the rollout.
 */
export function isAnnotationEvent(event: LiveEvalEvent): boolean {
  const streamId = (event as LiveEvalEvent & { stream_id?: string }).stream_id;
  return event.kind.startsWith("annotation.")
    || event.type === "eval.trial.annotation"
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
    findings: [], markers: [], metrics: {}, metricSeries: {}, model: { requested: 0, completed: 0, failed: 0 },
    protocolErrors: 0, annotationClosed: false, annotationEvents: 0, lastAnnotation: "waiting for the protocol",
  };
}

function applyRollout(lane: Lane, event: LiveEvalEvent): void {
  const p = event.payload;
  const kind = event.kind;
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
    };
    lane.findings.push(finding);
    lane.markers.push({ sequence, step: finding.step, kind: finding.kind, label: finding.label, status: "provisional", findingId });
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

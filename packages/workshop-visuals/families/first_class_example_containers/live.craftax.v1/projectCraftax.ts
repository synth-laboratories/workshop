import { projectLiveEval } from "../../../runtime/liveEvalReducer.ts";
import { mediaRefFrom, type MediaRef } from "../../../runtime/mediaClient.ts";
import type { LiveEvalEvent } from "../../../runtime/types.ts";

type Json = Record<string, unknown>;

function object(value: unknown): Json {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Json : {};
}

function finite(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

/** Rejoin the optimizer's frozen terminal lane with visual-only enrichment.
 *
 * Progress reducers intentionally keep these lanes separate so late evidence
 * cannot rewrite a terminal result. A trace viewer has the opposite need: it
 * must render both the frozen lifecycle and every retained trial envelope.
 */
export function mergeCraftaxOptimizerJournalEvents(
  terminalEvents: LiveEvalEvent[] | undefined,
  enrichmentEvents: LiveEvalEvent[] | undefined
): LiveEvalEvent[] | undefined {
  const combined = [...(terminalEvents ?? []), ...(enrichmentEvents ?? [])];
  if (combined.length === 0) return undefined;
  const seen = new Set<string>();
  return combined.filter((event) => {
    const raw = object(event);
    const eventId = raw.eventId ?? raw.event_id;
    const sequence = raw.sequenceNumber ?? raw.sequence_number;
    const optimizerRunId = raw.optimizerRunId ?? raw.optimizer_run_id;
    const identity = typeof eventId === "string" && eventId.length > 0
      ? `event:${eventId}`
      : (typeof sequence === "number" || typeof sequence === "string") && typeof optimizerRunId === "string"
        ? `sequence:${optimizerRunId}:${sequence}`
        : undefined;
    if (!identity) return true;
    if (seen.has(identity)) return false;
    seen.add(identity);
    return true;
  }).sort((left, right) => {
    const leftSequence = finite(object(left).sequenceNumber);
    const rightSequence = finite(object(right).sequenceNumber);
    return leftSequence != null && rightSequence != null ? leftSequence - rightSequence : 0;
  });
}

/** Normalize host envelopes before semantic reducers call string methods. */
export function craftaxEventKind(event: LiveEvalEvent): string {
  const record = object(event);
  for (const candidate of [record.kind, record.event_kind, record.eventKind, record.event_type, record.type]) {
    if (typeof candidate === "string" && candidate.length > 0) return candidate;
  }
  return "unknown";
}

function normalizedCraftaxEvent(event: LiveEvalEvent): LiveEvalEvent {
  const host = object(event);
  const delta = object(host.delta);
  const raw = object(host.raw);
  const containerEvent = object(delta.container_event ?? delta.containerEvent ?? raw.container_event ?? raw.containerEvent);
  const containerKind = craftaxEventKind(containerEvent as LiveEvalEvent);
  if (containerKind !== "unknown") {
    const rolloutId = containerEvent.rollout_id ?? containerEvent.rolloutId ?? delta.trial_id ?? delta.trialId;
    const occurredAt = containerEvent.occurred_at ?? containerEvent.occurredAt;
    const sequence = containerEvent.sequence ?? containerEvent.sequence_number ?? containerEvent.sequenceNumber;
    return {
      ...event,
      kind: containerKind,
      payload: object(containerEvent.payload),
      lane: typeof rolloutId === "string" ? rolloutId : event.lane,
      run_id: typeof rolloutId === "string" ? rolloutId : event.run_id,
      occurred_at: typeof occurredAt === "string" ? occurredAt : event.occurred_at,
      sequence: typeof sequence === "number" || typeof sequence === "string" ? sequence : event.sequence,
    };
  }
  const kind = craftaxEventKind(event);
  return event.kind === kind ? event : { ...event, kind };
}

/** Canonical streams use `value`; native GameBench history used `reward`. */
export function craftaxRewardValue(payload: unknown): number | undefined {
  const record = object(payload);
  return finite(record.value) ?? finite(record.reward);
}

export function craftaxEventLane(event: LiveEvalEvent): string {
  return event.lane || event.run_id || "eval";
}

export function craftaxEventSequence(event: LiveEvalEvent, fallback: number): number {
  const raw = (event as LiveEvalEvent & { sequence_number?: number | string | null }).sequence_number ?? event.sequence;
  const parsed = Number(raw);
  return raw !== null && raw !== undefined && raw !== "" && Number.isFinite(parsed) ? parsed : fallback;
}

export function scopeCraftaxEvents(events: LiveEvalEvent[], rolloutIds?: string[]): LiveEvalEvent[] {
  if (!rolloutIds?.length) return events;
  const allowed = new Set(rolloutIds);
  return events.filter((event) => allowed.has(craftaxEventLane(event)));
}

export type CraftaxTruthState = "pending" | "not_emitted" | "not_applicable" | "redacted" | "failed" | "present";

/** Keep absence states explicit; zero is a present value, never a missing fallback. */
export function craftaxTruthState(value: unknown, options: {
  terminal?: boolean;
  applicable?: boolean;
  failed?: boolean;
} = {}): CraftaxTruthState {
  if (options.failed) return "failed";
  if (options.applicable === false) return "not_applicable";
  if (value === "[REDACTED]" || object(value).redacted === true) return "redacted";
  if (value === undefined || value === null || value === "") return options.terminal ? "not_emitted" : "pending";
  return "present";
}

export function craftaxTruthLabel(state: CraftaxTruthState): string {
  return state.replace("_", " ");
}

function eventTime(event: LiveEvalEvent): number {
  const parsed = Date.parse(event.occurred_at ?? event.ts ?? "");
  return Number.isFinite(parsed) ? parsed : 0;
}

function achievementNames(value: unknown): string[] {
  if (Array.isArray(value)) return [...new Set(value.map(String))];
  return Object.entries(object(value))
    .filter(([, unlocked]) => Boolean(unlocked))
    .map(([name]) => name);
}

function observationAchievements(event: LiveEvalEvent): string[] {
  const payload = object(event.payload);
  const readout = object(payload.readout);
  const nested = object(readout.observation);
  return achievementNames(
    payload.achievements ?? readout.achievements ?? nested.achievements
  );
}

function observationGrid(event: LiveEvalEvent | undefined): string | null {
  if (!event) return null;
  const payload = object(event.payload);
  const readout = object(payload.readout);
  const nested = object(readout.observation);
  for (const value of [payload.grid, payload.text, payload.ascii, readout.grid, readout.ascii, nested.grid, nested.ascii]) {
    if (typeof value === "string" && value.length > 0) return value;
  }
  return null;
}

function payloadUsage(payload: Json): Json {
  const own = object(payload.usage);
  if (Object.keys(own).length) return own;
  const nested = object(object(payload.policy).usage);
  if (Object.keys(nested).length) return nested;
  return Object.fromEntries(
    ["prompt_tokens", "completion_tokens", "total_tokens", "cost_usd"]
      .filter((key) => finite(payload[key]) != null)
      .map((key) => [key, payload[key]])
  );
}

function aggregateTraceUsage(traceEvents: LiveEvalEvent[]): Json {
  const totals: Json = {};
  const add = (raw: unknown) => {
    const usage = object(raw);
    for (const key of ["prompt_tokens", "completion_tokens", "total_tokens", "cost_usd"] as const) {
      const value = finite(usage[key]);
      if (value != null) totals[key] = (finite(totals[key]) ?? 0) + value;
    }
  };
  for (const event of traceEvents) {
    if (event.kind !== "span.policy.data") continue;
    const payload = object(event.payload);
    if (payload.delta === true) continue;
    add(payloadUsage(payload));
    if (Array.isArray(payload.prior_attempts)) {
      for (const attempt of payload.prior_attempts) add(object(attempt).usage);
    }
  }
  return totals;
}

function lastPolicySnapshot(events: LiveEvalEvent[]): Json {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    if (events[index].kind !== "span.policy.data") continue;
    const payload = object(events[index].payload);
    if (payload.delta === true || payload.channel === "compact") continue;
    return payload;
  }
  return {};
}

function streamedChannel(events: LiveEvalEvent[], channel: string): string {
  return events
    .filter((event) => event.kind === "span.policy.data")
    .map((event) => object(event.payload))
    .filter((payload) => payload.delta === true && payload.channel === channel)
    .map((payload) => typeof payload.text === "string" ? payload.text : "")
    .join("");
}

function lastKind(events: LiveEvalEvent[], kind: string): LiveEvalEvent | undefined {
  return [...events].reverse().find((event) => event.kind === kind);
}

export type CraftaxPolicyProjection = {
  provider?: string;
  model?: string;
  actions: string[];
  usage: Json;
  assistant?: string;
  reasoning?: string;
  toolArguments?: string;
  actionAuthority?: string;
  fallback?: boolean;
  parseError?: string;
};

export type CraftaxViewerProjection = {
  ordered: LiveEvalEvent[];
  lanes: string[];
  selectedLane?: string;
  laneEvents: LiveEvalEvent[];
  visibleEvents: LiveEvalEvent[];
  visibleIndex: number;
  rewardSignals: LiveEvalEvent[];
  cumulativeReward?: number;
  reward?: number;
  ascii: string | null;
  frameUrl: string | null;
  frameMedia: MediaRef | null;
  frameUnavailable: boolean;
  frameEvents: LiveEvalEvent[];
  achievements: string[];
  traceEvents: LiveEvalEvent[];
  semanticTrace: CraftaxSemanticTraceItem[];
  policy: CraftaxPolicyProjection;
  terminal: boolean;
};

export type CraftaxTraceInteraction = {
  input?: unknown;
  thinking?: unknown;
  output?: unknown;
  tools?: unknown;
  responseType: "text" | "tool_call" | "mixed" | "pending" | "not_applicable";
};

export type CraftaxSemanticTraceItem = {
  id: string;
  category: "policy" | "environment" | "reward" | "achievement" | "lifecycle" | "evidence";
  kind: string;
  label: string;
  sequenceStart: number;
  sequenceEnd: number;
  step?: number;
  call?: number;
  rawEvents: LiveEvalEvent[];
  interaction?: CraftaxTraceInteraction;
};

function eventPayload(event: LiveEvalEvent | undefined): Json {
  return object(event?.payload);
}

function eventStep(event: LiveEvalEvent): number | undefined {
  const payload = eventPayload(event);
  const readout = object(payload.readout);
  return finite(payload.step) ?? finite(payload.step_index) ?? finite(payload.env_steps) ?? finite(readout.env_steps);
}

function interactionInput(events: LiveEvalEvent[], openedIndex: number): unknown {
  const observation = events.slice(0, openedIndex + 1).reverse().find((event) => event.kind === "observation");
  const payload = eventPayload(observation);
  const readout = object(payload.readout);
  return readout.observation_text ?? (Object.keys(readout).length ? readout : payload.grid);
}

function policyCallItem(
  events: LiveEvalEvent[],
  rawEvents: LiveEvalEvent[],
  openedIndex: number,
  ordinal: number
): CraftaxSemanticTraceItem {
  const opened = eventPayload(rawEvents[0]);
  const callConfig = object(opened.call);
  const snapshots = rawEvents
    .filter((event) => event.kind === "span.policy.data")
    .map(eventPayload)
    .filter((payload) => payload.delta !== true && payload.channel !== "compact");
  const snapshot = snapshots.at(-1) ?? {};
  const assistantMessage = object(snapshot.assistant);
  const channelText = (channel: string) => rawEvents
    .filter((event) => event.kind === "span.policy.data")
    .map(eventPayload)
    .filter((payload) => payload.delta === true && payload.channel === channel)
    .map((payload) => typeof payload.text === "string" ? payload.text : "")
    .join("");
  const reasoning = snapshot.reasoning ?? snapshot.thinking ?? assistantMessage.reasoning_content ?? assistantMessage.reasoning ?? (channelText("reasoning") || undefined);
  const textOutput = (Object.keys(assistantMessage).length ? assistantMessage.content : snapshot.assistant) ?? snapshot.output ?? snapshot.response ?? (channelText("content") || undefined);
  const tools = snapshot.tool_calls ?? snapshot.tool_arguments ?? assistantMessage.tool_calls ?? (channelText("tool") || undefined);
  const plan = rawEvents.map(eventPayload).find((payload, index) => rawEvents[index]?.kind === "span.policy.plan");
  const actions = Array.isArray(plan?.actions) ? plan.actions.map(String) : Array.isArray(snapshot.actions) ? snapshot.actions.map(String) : [];
  const call = finite(snapshot.call) ?? (finite(snapshot.call_index) != null ? finite(snapshot.call_index)! + 1 : ordinal);
  const model = String(snapshot.model ?? snapshot["gen_ai.request.model"] ?? object(snapshot.policy).model ?? callConfig.model ?? "model");
  const hasText = textOutput != null && textOutput !== "";
  const hasTools = tools != null && tools !== "";
  const responseType: CraftaxTraceInteraction["responseType"] = hasText && hasTools
    ? "mixed"
    : hasTools
      ? "tool_call"
      : hasText
        ? "text"
        : rawEvents.some((event) => event.kind === "span.policy.closed")
          ? "not_applicable"
          : "pending";
  const firstSequence = craftaxEventSequence(rawEvents[0], openedIndex);
  const lastSequence = craftaxEventSequence(rawEvents.at(-1) ?? rawEvents[0], firstSequence);
  return {
    id: `policy:${call}:${firstSequence}`,
    category: "policy",
    kind: "policy.call",
    label: `${model} · call ${call}${actions.length ? ` · ${actions.join(" → ")}` : ""}`,
    sequenceStart: firstSequence,
    sequenceEnd: lastSequence,
    call,
    rawEvents,
    interaction: {
      input: interactionInput(events, openedIndex),
      thinking: reasoning,
      output: textOutput,
      tools,
      responseType
    }
  };
}

/** Collapse transport partials into user-facing policy calls and environment steps. */
export function projectCraftaxSemanticTrace(events: LiveEvalEvent[]): CraftaxSemanticTraceItem[] {
  events = events.map(normalizedCraftaxEvent);
  const items: CraftaxSemanticTraceItem[] = [];
  let currentPolicy: { events: LiveEvalEvent[]; openedIndex: number; ordinal: number } | null = null;
  let policyOrdinal = 0;
  const flushPolicy = () => {
    if (!currentPolicy) return;
    items.push(policyCallItem(events, currentPolicy.events, currentPolicy.openedIndex, currentPolicy.ordinal));
    currentPolicy = null;
  };
  for (let index = 0; index < events.length; index += 1) {
    const event = events[index];
    if (event.kind === "span.policy.opened") {
      flushPolicy();
      currentPolicy = { events: [event], openedIndex: index, ordinal: ++policyOrdinal };
      continue;
    }
    if (event.kind.startsWith("span.policy.")) {
      const payload = eventPayload(event);
      if (
        event.kind === "span.policy.data"
        && payload.phase === "sample"
        && currentPolicy?.events.some((prior) => {
          const priorPayload = eventPayload(prior);
          return prior.kind === "span.policy.data" && priorPayload.phase === "sample" && priorPayload.delta !== true;
        })
      ) {
        flushPolicy();
      }
      if (!currentPolicy) currentPolicy = { events: [], openedIndex: index, ordinal: ++policyOrdinal };
      currentPolicy.events.push(event);
      if (event.kind === "span.policy.closed") flushPolicy();
      continue;
    }
    if (event.kind === "span.step.closed") {
      const payload = eventPayload(event);
      const step = eventStep(event);
      const sequence = craftaxEventSequence(event, index);
      items.push({
        id: `step:${step ?? sequence}`,
        category: "environment",
        kind: "environment.step",
        label: `Step ${step ?? "—"}${payload.action ? ` · ${String(payload.action)}` : ""}`,
        sequenceStart: sequence,
        sequenceEnd: sequence,
        step,
        rawEvents: [event]
      });
      continue;
    }
    if (event.kind === "snapshot") {
      const payload = eventPayload(event);
      const step = eventStep(event);
      const sequence = craftaxEventSequence(event, index);
      items.push({
        id: `snapshot:${step ?? sequence}:${sequence}`,
        category: "environment",
        kind: "environment.step",
        label: `Step ${step ?? "—"} · snapshot`,
        sequenceStart: sequence,
        sequenceEnd: sequence,
        step,
        rawEvents: [event]
      });
      continue;
    }
    if (event.kind === "achievement_unlocked") {
      const sequence = craftaxEventSequence(event, index);
      const payload = eventPayload(event);
      items.push({ id: `achievement:${sequence}`, category: "achievement", kind: event.kind, label: String(payload.achievement ?? "Achievement unlocked"), sequenceStart: sequence, sequenceEnd: sequence, step: eventStep(event), rawEvents: [event] });
      continue;
    }
    if (["trace.opened", "env.episode.opened", "env.episode.closed", "policy.session.opened", "policy.session.closed", "status", "capture.closed", "trace.reconciled", "terminal", "episode_truncated", "eval.run.terminal"].includes(event.kind)) {
      const sequence = craftaxEventSequence(event, index);
      const category = event.kind.startsWith("trace.") || event.kind === "capture.closed" ? "evidence" : "lifecycle";
      const payload = eventPayload(event);
      const terminalLabel = event.kind === "eval.run.terminal"
        ? `Run ${String(payload.stopped_on ?? (payload.terminated ? "terminated" : "finished"))}`
        : event.kind.replaceAll(".", " ");
      items.push({ id: `${event.kind}:${sequence}`, category, kind: event.kind, label: terminalLabel, sequenceStart: sequence, sequenceEnd: sequence, step: eventStep(event), rawEvents: [event] });
    }
  }
  flushPolicy();
  return items.sort((left, right) => left.sequenceStart - right.sequenceStart);
}

export type CraftaxTraceGroup = {
  key: string;
  /** Environment step this group belongs to; undefined for run-level groups. */
  step?: number;
  label: string;
  items: CraftaxSemanticTraceItem[];
};

/**
 * Fold the flat semantic trace into the environment hierarchy the run
 * actually has: run-level lifecycle, then one group per environment step
 * carrying the observation → policy call → action → reward chain that
 * produced it, then run-level evidence (seal/reconciliation) at the end.
 */
export function groupTraceByStep(items: CraftaxSemanticTraceItem[]): CraftaxTraceGroup[] {
  const groups: CraftaxTraceGroup[] = [];
  let pending: CraftaxSemanticTraceItem[] = [];
  const runLevel = (item: CraftaxSemanticTraceItem) =>
    item.category === "lifecycle" || item.category === "evidence";
  const flushRunLevel = (item: CraftaxSemanticTraceItem) => {
    const last = groups.at(-1);
    if (last && last.key === "run") last.items.push(item);
    else groups.push({ key: groups.length === 0 ? "run" : `run:${item.sequenceStart}`, label: "Run lifecycle", items: [item] });
  };
  for (const item of items) {
    if (runLevel(item) && pending.length === 0) {
      flushRunLevel(item);
      continue;
    }
    pending.push(item);
    if (item.kind === "environment.step") {
      const step = item.step;
      groups.push({
        key: `step:${step ?? item.sequenceStart}`,
        step,
        label: `Step ${step ?? "—"}`,
        items: pending
      });
      pending = [];
    }
  }
  if (pending.length > 0) {
    // Trailing items (an open policy call, terminal evidence) after the last
    // completed step.
    const trailingRunLevel = pending.every(runLevel);
    groups.push({
      key: trailingRunLevel ? `run:${pending[0].sequenceStart}` : "step:next",
      step: undefined,
      label: trailingRunLevel ? "Run lifecycle" : "In progress",
      items: pending
    });
  }
  return groups;
}

/**
 * Semantic replay checkpoints: the ordered-event indexes where scrubbing
 * should land. Transport partials (`span.policy.data` deltas) are not
 * checkpoints — a replay tick is an environment step, a policy-call boundary,
 * a reward, an achievement, or lifecycle evidence.
 */
/** Indexes of the events replay can stop on.
 *
 * These are *replay moments* — environment steps, policy-call boundaries,
 * rewards, achievements, and lifecycle evidence — not environment steps. The UI
 * called their count "checkpoints", so a 20-step rollout advertised "57 / 57
 * checkpoints" and read as a step count that was nearly three times the truth.
 * Environment steps are reported separately by `environmentStepCount`.
 */
export function replayMomentIndexes(ordered: LiveEvalEvent[]): number[] {
  ordered = ordered.map(normalizedCraftaxEvent);
  const indexes: number[] = [];
  for (let index = 0; index < ordered.length; index += 1) {
    const kind = ordered[index].kind;
    if (kind === "span.policy.data") continue;
    if (kind === "frame" || kind === "observation") continue;
    indexes.push(index);
  }
  if (ordered.length > 0 && indexes.at(-1) !== ordered.length - 1) {
    indexes.push(ordered.length - 1);
  }
  return indexes;
}

/** Honest replay availability for lifecycle-only or rejected traces. */
export function craftaxReplayAvailability(
  ordered: LiveEvalEvent[],
  evidenceState?: "pending" | "accepted" | "partial" | "missing" | "rejected"
): { markers: number; environmentSteps: number; replayable: boolean; reason?: string } {
  const markers = replayMomentIndexes(ordered).length;
  const steps = environmentStepCount(ordered);
  if (evidenceState === "rejected") {
    return { markers, environmentSteps: steps, replayable: false, reason: "evidence rejected" };
  }
  if (steps === 0) {
    return { markers, environmentSteps: 0, replayable: false, reason: "0 trustworthy environment steps" };
  }
  return { markers, environmentSteps: steps, replayable: true };
}

/** Project only persisted evidence visible for one lane at one replay cursor. */
export function projectCraftaxViewer(
  events: LiveEvalEvent[],
  chosenLane?: string | null,
  cutoffIndex?: number | null
): CraftaxViewerProjection {
  const ordered = events
    .map(normalizedCraftaxEvent)
    .map((event, arrival) => ({ event, arrival }))
    .sort((left, right) =>
      eventTime(left.event) - eventTime(right.event) ||
      craftaxEventLane(left.event).localeCompare(craftaxEventLane(right.event)) ||
      craftaxEventSequence(left.event, left.arrival) - craftaxEventSequence(right.event, right.arrival) ||
      left.arrival - right.arrival
    )
    .map(({ event }) => event);
  const observedLanes = [...new Set(ordered.map(craftaxEventLane))];
  // Optimizer journals include run-level lifecycle envelopes on a synthetic
  // `eval` lane. It is useful durable evidence, but it is not a rollout and
  // contains no gameplay or policy calls. Prefer actual rollout lanes whenever
  // at least one exists so a completed evaluation never opens on an empty
  // transcript merely because `eval` sorts first.
  const rolloutLanes = observedLanes.filter((lane) => lane !== "eval");
  const lanes = rolloutLanes.length > 0 ? rolloutLanes : observedLanes;
  const selectedLane = chosenLane && lanes.includes(chosenLane) ? chosenLane : lanes[0];
  const laneEvents = selectedLane
    ? ordered.filter((event) => craftaxEventLane(event) === selectedLane)
    : [];
  const lastIndex = laneEvents.length - 1;
  const visibleIndex = lastIndex < 0
    ? -1
    : cutoffIndex == null
      ? lastIndex
      : Math.max(0, Math.min(Math.trunc(cutoffIndex), lastIndex));
  const visibleEvents = visibleIndex < 0 ? [] : laneEvents.slice(0, visibleIndex + 1);
  const shared = projectLiveEval(visibleEvents);
  const rewardSignals = visibleEvents.filter((event) => event.kind === "reward_signal");
  const cumulativeReward = rewardSignals.reduce<number | undefined>((sum, event) => {
    const value = craftaxRewardValue(event.payload);
    return value == null ? sum : (sum ?? 0) + value;
  }, undefined);
  const frame = lastKind(visibleEvents, "frame");
  const frameEvents = laneEvents.filter((event) =>
    event.kind === "frame" && (
      (typeof event.payload.url === "string" && event.payload.url.length > 0)
      || mediaRefFrom(event.payload) !== null
    )
  );
  const observation = lastKind(visibleEvents, "observation") ?? lastKind(visibleEvents, "snapshot");
  const frameUrl = typeof frame?.payload.url === "string" && frame.payload.url.length > 0
    ? frame.payload.url
    : null;
  const frameMedia = mediaRefFrom(frame?.payload);
  const frameFormat = typeof frame?.payload.format === "string" ? frame.payload.format.toLowerCase() : "";
  const pngAdvertised = frameFormat === "png"
    || (typeof frameUrl === "string" && (frameUrl.includes(".png") || frameUrl.startsWith("data:image/png")));
  const ascii = pngAdvertised
    ? null
    : typeof frame?.payload.text === "string"
      ? frame.payload.text
      : observationGrid(observation);
  const frameUnavailable = Boolean(frame) && pngAdvertised && !frameUrl && !frameMedia;
  const achievements = [...new Set(visibleEvents.flatMap((event) => {
    if (event.kind === "achievement_unlocked") {
      const payload = object(event.payload);
      const name = payload.achievement ?? object(payload.payload).achievement;
      return typeof name === "string" && name.length > 0 ? [name] : [];
    }
    return observationAchievements(event);
  }))];
  const traceEvents = visibleEvents.filter((event) => event.kind.startsWith("span.policy."));
  const semanticTrace = projectCraftaxSemanticTrace(visibleEvents);
  const policyData = lastPolicySnapshot(traceEvents);
  const policyPlan = object(lastKind(traceEvents, "span.policy.plan")?.payload);
  const policyOpened = object(lastKind(traceEvents, "span.policy.opened")?.payload);
  const openedCall = object(policyOpened.call);
  const nestedPolicy = object(policyData.policy);
  const planActions = Array.isArray(policyPlan.actions) ? policyPlan.actions : policyData.actions;
  const actions = Array.isArray(planActions) ? planActions.map(String) : [];
  const usage = aggregateTraceUsage(traceEvents);
  const text = (value: unknown) => typeof value === "string" && value.length > 0 ? value : undefined;
  const provider = text(policyData.provider) ?? text(nestedPolicy.provider) ?? text(openedCall.provider);
  const model = text(policyData.model) ?? text(policyData["gen_ai.request.model"]) ?? text(nestedPolicy.model) ?? text(openedCall.model);
  const terminal = visibleEvents.some((event) => {
    if (event.kind === "trace.reconciled" || event.kind === "eval.run.terminal") return true;
    if (event.kind !== "status") return false;
    const status = String(event.payload.status ?? "").toLowerCase();
    return ["completed", "finished", "failed", "cancelled"].includes(status);
  });

  return {
    ordered,
    lanes,
    selectedLane,
    laneEvents,
    visibleEvents,
    visibleIndex,
    rewardSignals,
    cumulativeReward,
    reward: cumulativeReward ?? (shared.reward ?? undefined),
    ascii,
    frameUrl,
    frameMedia,
    frameUnavailable,
    frameEvents,
    achievements,
    traceEvents,
    semanticTrace,
    terminal,
    policy: {
      provider,
      model,
      actions,
      usage,
      assistant: text(policyData.assistant) ?? (streamedChannel(traceEvents, "content") || undefined),
      reasoning: text(policyData.reasoning) ?? (streamedChannel(traceEvents, "reasoning") || undefined),
      toolArguments: text(policyData.tool_arguments) ?? (streamedChannel(traceEvents, "tool") || undefined),
      actionAuthority: text(policyData.action_authority),
      fallback: typeof policyData.fallback === "boolean" ? policyData.fallback : undefined,
      parseError: text(policyData.parse_error)
    }
  };
}

export function policyPartialDetail(event: LiveEvalEvent): string {
  const payload = object(event.payload);
  if (event.kind === "span.policy.opened") {
    const call = object(payload.call);
    return [call.provider ?? payload.provider, call.model ?? payload.model, payload.harness]
      .filter((value) => typeof value === "string" && value.length > 0)
      .join(" · ") || "opened";
  }
  if (event.kind === "span.policy.plan") {
    return Array.isArray(payload.actions) ? payload.actions.map(String).join(" → ") : "plan unavailable";
  }
  if (event.kind === "span.policy.data") {
    if (payload.delta === true) {
      const channel = typeof payload.channel === "string" ? payload.channel : "token";
      const textValue = typeof payload.text === "string" ? payload.text : "";
      return textValue ? `${channel} Δ ${textValue}` : `${channel} Δ`;
    }
    if (payload.channel === "compact") {
      return `compact_every ${String(payload.compact_every ?? "—")} · dropped ${String(payload.dropped_turns ?? "—")}`;
    }
    return String(payload.action_authority ?? (payload.fallback === true ? "harness fallback" : "policy data"));
  }
  if (event.kind === "span.policy.closed") {
    return finite(payload.length) == null ? "closed" : `${payload.length} planned actions`;
  }
  return "—";
}

/** How many environment steps the retained evidence proves were completed.
 *
 * Canonical producer journals emit one `span.step.closed` per completed step;
 * count those across rollout lanes. Older native fixtures only expose indexed
 * snapshots, so they retain the previous highest-index fallback. */
export function environmentStepCount(ordered: LiveEvalEvent[]): number {
  const closedSteps = new Set<string>();
  for (const event of ordered) {
    if (event.kind !== "span.step.closed") continue;
    const step = eventStep(event);
    if (step != null) closedSteps.add(`${craftaxEventLane(event)}:${step}`);
  }
  if (closedSteps.size > 0) return closedSteps.size;

  let highest = -1;
  for (const event of ordered) {
    const step = eventStep(event);
    if (step != null && step > highest) highest = step;
  }
  return highest + 1;
}

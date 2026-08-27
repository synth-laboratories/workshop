/**
 * `eval_trace_view_v1` — one Craftax projection, folded from live relay events
 * or from a sealed Trace V5 document.
 *
 * There is deliberately **one fold**, not two. A live projector and a sealed
 * projector that are meant to agree will not stay in agreement: the moment they
 * disagree, the workbench shows one trajectory while the rollout was running
 * and a different one afterwards, and no one can tell which is the trace. So
 * both inputs are normalized to the producer's own event vocabulary — `frame`,
 * `observation`, `span.policy.data`, `action_applied`, `achievement_unlocked` —
 * and folded by the same code.
 *
 * The output model is the one the published Craftax viewer already reads
 * (`CraftaxTracePayload` in the frontend), extended with the things a *live*
 * view needs and a published archive does not: an incomplete call, a frame the
 * relay refused, and a coverage block saying how much of the episode this view
 * is actually based on.
 *
 * ## Projection rules
 *
 * These are the contract, and each exists because the alternative reading is
 * wrong rather than merely different:
 *
 * - `span.policy.opened` begins a call attempt. A call that never closes stays
 *   in the trajectory marked `running`. Dropping it would make the rail jump
 *   backwards when it finally lands.
 * - `span.policy.data.messages` supplies the policy-visible messages. The first
 *   `system` message is the system prompt.
 * - `assistant.reasoning_content` is thinking; `assistant.content` is the final
 *   response; `assistant.tool_calls` are the tool calls and their arguments.
 *   None of them is inferred from another.
 * - **`action_applied` is authoritative.** A proposed action is never counted
 *   as executed. `action_rejected` stays visible with its reason and is never
 *   merged into the applied list.
 * - `frame.payload.step` aligns media with environment steps. One call can
 *   commit several actions, so a call maps to a *range* of frames, never to one.
 * - `reward_delta` and `achievement_unlocked` attach to the call whose applied
 *   steps contain them.
 * - `resource_delta`, `state_transition` and `entity_transition` become state
 *   deltas on that same call.
 *
 * Nothing here invents a value. A field with no evidence stays absent, and the
 * viewer renders that as unknown rather than as zero.
 */

import { mediaRefFrom, type MediaRef } from "./mediaClient.ts";

export const EVAL_TRACE_VIEW_SCHEMA = "eval_trace_view_v1";
export const CRAFTAX_PROJECTION_KIND = "craftax-replay";

/** One producer event, in the shape both sources normalize to. */
export type ContainerEvent = {
  sequence: number;
  kind: string;
  occurredAt?: string | null;
  digest?: string | null;
  payload: Record<string, any>;
};

export type TraceMessage = {
  role: string;
  content: string;
  /** Present on tool results, so a viewer can pair them with their call. */
  toolCallId?: string | null;
  name?: string | null;
};

export type TraceToolCall = {
  id: string | null;
  name: string;
  /** Parsed when the producer sent JSON; the raw string otherwise. */
  arguments: unknown;
  argumentsText: string;
};

export type AppliedAction = { turn: number | null; name: string };
export type RejectedAction = { turn: number | null; name: string; reason: string | null };

export type StateDelta = {
  field: string;
  before: number | string | null;
  after: number | string | null;
  delta: number | null;
  turn: number | null;
  source: "resource_delta" | "state_transition" | "entity_transition";
};

export type TraceFrame = {
  /** Environment step this frame renders. */
  step: number;
  /** Media reference, when the relay retained bytes for it. */
  media: MediaRef | null;
  /** Why there are no bytes, when there are none. */
  unavailable: string | null;
  format: string;
  producerDigest: string | null;
  sequence: number;
};

export type TraceStep = {
  id: string;
  /** One-based, stable across appends. */
  index: number;
  title: string;
  /** `complete` once the call closed; `running` while it is still open. */
  status: "complete" | "running";
  turn_start: number | null;
  turn_end: number | null;
  tokens: { input: number | null; output: number | null };
  content: {
    /** Every policy-visible message for this call, in order. */
    input_messages: TraceMessage[];
    reasoning: string | null;
    message: string | null;
    /** The observation the policy saw before deciding. */
    observation: string | null;
    /** Structured readout, when the producer sent one. */
    readout: Record<string, any> | null;
  };
  tool_calls: TraceToolCall[];
  action: {
    /** What the model asked for. Secondary evidence, never executed-by-implication. */
    proposed: string[];
    /** What the environment actually did. Authoritative. */
    applied: AppliedAction[];
    /** Refused, with the recorded reason. Never merged into `applied`. */
    rejected: RejectedAction[];
    /** Applied but with no observable effect, when the producer says so. */
    noop: AppliedAction[];
  };
  reward: number | null;
  achievements: string[];
  state_delta: StateDelta[];
  /** Indices into `frames`, in order. A call can own several, or none. */
  frames: number[];
  /** Producer sequences behind this step, for raw disclosure. */
  raw: number[];
};

export type TraceCoverage = {
  /** Highest producer sequence folded. */
  highWater: number;
  /** The producer said its journal is closed. */
  closed: boolean;
  framesDeclared: number;
  framesRetained: number;
  /** Bounds the relay hit, verbatim. Never summarized away. */
  degradations: Array<{ reason: string; detail: string; dropped: number }>;
};

export type EvalTraceView = {
  schema: typeof EVAL_TRACE_VIEW_SCHEMA;
  source_schema: "optimizer_events" | "trace_v5";
  trace_id: string;
  task: { id: string; name: string; family: string };
  run: {
    model: string | null;
    provider: string | null;
    effort: string | null;
    seed: number | null;
    status: string;
    duration_ms: number | null;
    cost_usd: number | null;
    usage: { calls: number | null; input_tokens: number | null; output_tokens: number | null };
  };
  integrity: { status: "live" | "sealed"; content_digest: string | null; source: string };
  system_prompt: string | null;
  steps: TraceStep[];
  frames: TraceFrame[];
  total_reward: number | null;
  achievements: string[];
  coverage: TraceCoverage;
  /** Every folded producer event, for raw disclosure behind the viewer. */
  events: ContainerEvent[];
};

type Any = Record<string, any>;

const num = (value: unknown): number | null => {
  const parsed = typeof value === "string" ? Number(value) : value;
  return typeof parsed === "number" && Number.isFinite(parsed) ? parsed : null;
};

const text = (value: unknown): string | null => {
  if (typeof value !== "string") return null;
  return value.length ? value : null;
};

/** Identity for one folded view. Everything else is derived from the events. */
export type TraceIdentity = {
  traceId: string;
  scenario: string;
  seed: number | null;
  status: string;
  model?: string | null;
  provider?: string | null;
  effort?: string | null;
  durationMs?: number | null;
  costUsd?: number | null;
  totalReward?: number | null;
  contentDigest?: string | null;
  sealed?: boolean;
  /** Relay receipt from the trial record, when there is one. */
  relay?: Any | null;
};

/**
 * Normalize relayed optimizer events for one trial.
 *
 * Reads `eval.trial.event` envelopes and keeps the producer's `kind`. The
 * public `delta` copy wins over `raw` where both exist, because a producer may
 * legitimately omit a large field from the compact copy but never contradicts
 * it — the same merge rule the live eval panel already uses.
 */
export function containerEventsFromOptimizerEvents(
  optimizerEvents: readonly Any[],
  trialId?: string
): ContainerEvent[] {
  const rows = new Map<number, ContainerEvent>();
  for (const event of optimizerEvents) {
    const type = event?.type ?? event?.eventType;
    if (type !== "eval.trial.event") continue;
    const delta = (event.delta ?? {}) as Any;
    const raw = (event.raw ?? {}) as Any;
    if (trialId && (delta.trial_id ?? raw.trial_id) !== trialId) continue;
    const merged = {
      ...((raw.container_event ?? raw.containerEvent ?? {}) as Any),
      ...((delta.containerEvent ?? delta.container_event ?? {}) as Any)
    } as Any;
    const sequence = num(merged.sequence);
    const kind = text(merged.kind);
    if (sequence === null || !kind) continue;
    // Keyed by producer sequence: a replayed page and a resumed worker offer
    // the same fact twice, and the timeline must contain it once.
    rows.set(sequence, {
      sequence,
      kind,
      occurredAt: text(merged.occurred_at ?? merged.occurredAt),
      digest: text(merged.digest),
      payload: (merged.payload ?? {}) as Any
    });
  }
  return [...rows.values()].sort((a, b) => a.sequence - b.sequence);
}

/**
 * Normalize a sealed Trace V5 document's events.
 *
 * Harbor's promotion keeps every native event and records what it was called
 * under `payload.source_event_type`, so the sealed side needs no second
 * vocabulary — only this unwrapping.
 */
export function containerEventsFromSealedTrace(document: Any): ContainerEvent[] {
  const events = Array.isArray(document?.events) ? document.events : [];
  const rows: ContainerEvent[] = [];
  for (const [index, event] of events.entries()) {
    const payload = (event?.payload ?? {}) as Any;
    const kind =
      text(payload.source_event_type) ??
      text(event?.source_event_type) ??
      text(event?.event_type) ??
      text(event?.kind);
    if (!kind) continue;
    const { source_event_type, source_event_digest, ...native } = payload;
    rows.push({
      sequence: num(event?.order?.ordinal) ?? num(payload.sequence) ?? index + 1,
      kind,
      occurredAt: text(event?.occurred_at),
      digest: text(source_event_digest) ?? text(event?.content_digest),
      payload: native as Any
    });
  }
  return rows.sort((a, b) => a.sequence - b.sequence);
}

function messagesFrom(payload: Any): TraceMessage[] {
  const raw = payload.messages ?? payload?.request?.messages;
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((row): row is Any => Boolean(row) && typeof row === "object")
    .map((row) => ({
      role: String(row.role ?? "user"),
      content:
        typeof row.content === "string"
          ? row.content
          : // A structured content array is flattened for reading, but the raw
            // event stays available behind disclosure.
            Array.isArray(row.content)
            ? row.content
                .map((part: Any) => (typeof part === "string" ? part : (part?.text ?? "")))
                .join("")
            : "",
      toolCallId: text(row.tool_call_id ?? row.toolCallId),
      name: text(row.name)
    }));
}

function toolCallsFrom(assistant: Any): TraceToolCall[] {
  const raw = assistant?.tool_calls ?? assistant?.toolCalls;
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((row): row is Any => Boolean(row) && typeof row === "object")
    .map((row) => {
      const fn = (row.function ?? row) as Any;
      const argumentsText =
        typeof fn.arguments === "string" ? fn.arguments : JSON.stringify(fn.arguments ?? {});
      let parsed: unknown = argumentsText;
      try {
        parsed = JSON.parse(argumentsText);
      } catch {
        // Unparseable arguments are shown as the producer sent them. Inventing
        // a shape here would hide a real policy defect.
      }
      return {
        id: text(row.id),
        name: String(fn.name ?? "tool"),
        arguments: parsed,
        argumentsText
      };
    });
}

/** Actions a tool call asked for. Proposed only — never counted as applied. */
function proposedFrom(calls: TraceToolCall[], plan: string[] | null): string[] {
  if (plan?.length) return plan;
  const out: string[] = [];
  for (const call of calls) {
    const args = call.arguments as Any;
    const actions = args?.actions ?? args?.action;
    if (Array.isArray(actions)) out.push(...actions.map(String));
    else if (typeof actions === "string") out.push(actions);
  }
  return out;
}

function emptyStep(index: number, sequence: number): TraceStep {
  return {
    id: `call-${index}`,
    index,
    title: `Policy call ${index}`,
    status: "running",
    turn_start: null,
    turn_end: null,
    tokens: { input: null, output: null },
    content: { input_messages: [], reasoning: null, message: null, observation: null, readout: null },
    tool_calls: [],
    action: { proposed: [], applied: [], rejected: [], noop: [] },
    reward: null,
    achievements: [],
    state_delta: [],
    frames: [],
    raw: [sequence]
  };
}

/**
 * Fold producer events into the view model.
 *
 * Order is the only thing this trusts. Events arrive in producer sequence, and
 * everything after a `span.policy.opened` belongs to that call until the next
 * one opens — which is what makes a live fold and a sealed fold agree without
 * either needing to know which it is.
 */
export function foldCraftaxTrace(
  events: readonly ContainerEvent[],
  identity: TraceIdentity
): EvalTraceView {
  const steps: TraceStep[] = [];
  const frames: TraceFrame[] = [];
  const frameIndexByStep = new Map<number, number[]>();
  let current: TraceStep | null = null;
  let systemPrompt: string | null = null;
  // The observation standing before the next call opens. Craftax emits the
  // observation for step N before the policy is asked what to do next, so this
  // is genuinely "what the policy saw", not a look-ahead.
  let pendingObservation: { text: string | null; readout: Any | null } = {
    text: null,
    readout: null
  };
  let pendingFrames: number[] = [];
  const achievements: string[] = [];
  let rewardTotal: number | null = null;
  let calls = 0;
  let inputTokens: number | null = null;
  let outputTokens: number | null = null;

  const step = (): TraceStep => {
    if (current) return current;
    // An event that belongs to a call arrived before any call opened — an
    // environment prologue, or a producer that omits policy spans. It gets a
    // step of its own rather than being dropped.
    current = emptyStep(steps.length + 1, 0);
    current.title = steps.length === 0 ? "Environment prologue" : `Segment ${steps.length + 1}`;
    steps.push(current);
    return current;
  };

  for (const event of events) {
    const payload = event.payload ?? {};
    switch (event.kind) {
      case "span.policy.opened": {
        calls += 1;
        current = emptyStep(steps.length + 1, event.sequence);
        current.title = `Policy call ${calls}`;
        current.content.observation = pendingObservation.text;
        current.content.readout = pendingObservation.readout;
        // Frames rendered since the previous call belong to this call's "before"
        // picture; the ones its own actions produce are appended as they arrive.
        current.frames.push(...pendingFrames);
        pendingFrames = [];
        steps.push(current);
        break;
      }
      case "span.policy.data": {
        const target = step();
        target.raw.push(event.sequence);
        const messages = messagesFrom(payload);
        if (messages.length) {
          target.content.input_messages = messages;
          const system = messages.find((message) => message.role === "system");
          if (system && !systemPrompt) systemPrompt = system.content;
        }
        const assistant = (payload.assistant ?? payload.choices?.[0]?.message ?? {}) as Any;
        target.content.reasoning ??= text(
          assistant.reasoning_content ?? assistant.reasoning ?? payload.reasoning_content
        );
        target.content.message ??= text(assistant.content ?? payload.content);
        const tools = toolCallsFrom(assistant);
        if (tools.length) {
          target.tool_calls = tools;
          target.action.proposed = proposedFrom(tools, null);
        }
        const usage = (payload.usage ?? {}) as Any;
        const promptTokens = num(usage.prompt_tokens ?? usage.input_tokens);
        const completionTokens = num(usage.completion_tokens ?? usage.output_tokens);
        if (promptTokens !== null) {
          target.tokens.input = promptTokens;
          inputTokens = (inputTokens ?? 0) + promptTokens;
        }
        if (completionTokens !== null) {
          target.tokens.output = completionTokens;
          outputTokens = (outputTokens ?? 0) + completionTokens;
        }
        break;
      }
      case "span.policy.plan": {
        const target = step();
        target.raw.push(event.sequence);
        const plan = Array.isArray(payload.actions) ? payload.actions.map(String) : null;
        if (plan?.length) target.action.proposed = plan;
        break;
      }
      case "span.policy.closed": {
        if (current) {
          current.status = "complete";
          current.raw.push(event.sequence);
        }
        break;
      }
      case "action_applied":
      case "action": {
        const target = step();
        target.raw.push(event.sequence);
        const turn = num(payload.step ?? payload.turn);
        const name = String(payload.action ?? payload.name ?? "unknown");
        const applied: AppliedAction = { turn, name };
        // `noop` is only ever the producer's own word. A viewer that decided an
        // action "did nothing" by comparing states would be guessing.
        if (payload.effect === "noop" || payload.noop === true) {
          target.action.noop.push(applied);
        }
        target.action.applied.push(applied);
        if (turn !== null) {
          target.turn_start ??= turn;
          target.turn_end = turn;
        }
        break;
      }
      case "action_rejected": {
        const target = step();
        target.raw.push(event.sequence);
        target.action.rejected.push({
          turn: num(payload.step ?? payload.turn),
          name: String(payload.action ?? payload.name ?? "unknown"),
          reason: text(payload.reason)
        });
        break;
      }
      case "reward_signal": {
        const target = step();
        target.raw.push(event.sequence);
        const value = num(payload.value);
        if (value !== null) target.reward = (target.reward ?? 0) + value;
        break;
      }
      case "reward_delta": {
        const target = step();
        target.raw.push(event.sequence);
        const delta = num(payload.delta ?? payload.value);
        if (delta !== null) {
          target.reward = (target.reward ?? 0) + delta;
          rewardTotal = (rewardTotal ?? 0) + delta;
        }
        break;
      }
      case "achievement_unlocked": {
        const target = step();
        target.raw.push(event.sequence);
        const name = text(payload.achievement ?? payload.name);
        if (name) {
          target.achievements.push(name);
          if (!achievements.includes(name)) achievements.push(name);
        }
        break;
      }
      case "resource_delta":
      case "state_transition":
      case "entity_transition": {
        const target = step();
        target.raw.push(event.sequence);
        const before = payload.before ?? null;
        const after = payload.after ?? null;
        const beforeNumber = num(before);
        const afterNumber = num(after);
        target.state_delta.push({
          field: String(payload.resource ?? payload.field ?? payload.entity ?? event.kind),
          before,
          after,
          delta:
            beforeNumber !== null && afterNumber !== null ? afterNumber - beforeNumber : null,
          turn: num(payload.step ?? payload.turn),
          source: event.kind as StateDelta["source"]
        });
        break;
      }
      case "observation": {
        const readout = (payload.readout ?? {}) as Any;
        pendingObservation = {
          text: text(readout.observation_text ?? payload.grid),
          readout: Object.keys(readout).length ? readout : null
        };
        if (current) current.raw.push(event.sequence);
        break;
      }
      case "frame": {
        const stepNumber = num(payload.step);
        const media = mediaRefFrom(payload);
        const frame: TraceFrame = {
          step: stepNumber ?? frames.length,
          media,
          unavailable: media
            ? null
            : (text(payload.mediaError?.detail) ??
              (payload.format === "png"
                ? "the producer offered a PNG that Workshop did not retain"
                : `this step rendered ${payload.format ?? "no"} media, not a native frame`)),
          format: String(payload.format ?? "unknown"),
          producerDigest: text(payload.digest),
          sequence: event.sequence
        };
        const index = frames.length;
        frames.push(frame);
        const existing = frameIndexByStep.get(frame.step) ?? [];
        existing.push(index);
        frameIndexByStep.set(frame.step, existing);
        if (current) {
          current.frames.push(index);
          current.raw.push(event.sequence);
        } else {
          pendingFrames.push(index);
        }
        break;
      }
      default: {
        if (current) current.raw.push(event.sequence);
        break;
      }
    }
  }

  for (const target of steps) {
    if (target.turn_start === null && target.frames.length) {
      const first = frames[target.frames[0]];
      const last = frames[target.frames[target.frames.length - 1]];
      target.turn_start = first?.step ?? null;
      target.turn_end = last?.step ?? null;
    }
  }

  const relay = (identity.relay ?? {}) as Any;
  const declaredFrames = num(relay.framesDeclared);
  const retainedFrames = num(relay.framesRetained);
  return {
    schema: EVAL_TRACE_VIEW_SCHEMA,
    source_schema: identity.sealed ? "trace_v5" : "optimizer_events",
    trace_id: identity.traceId,
    task: { id: identity.scenario, name: identity.scenario, family: identity.scenario },
    run: {
      model: identity.model ?? null,
      provider: identity.provider ?? null,
      effort: identity.effort ?? null,
      seed: identity.seed ?? null,
      status: identity.status,
      duration_ms: identity.durationMs ?? null,
      cost_usd: identity.costUsd ?? null,
      usage: { calls: calls || null, input_tokens: inputTokens, output_tokens: outputTokens }
    },
    integrity: {
      status: identity.sealed ? "sealed" : "live",
      content_digest: identity.contentDigest ?? null,
      source: identity.sealed
        ? "container Trace V5 bundle"
        : "Workshop optimizer event relay"
    },
    system_prompt: systemPrompt,
    steps,
    frames,
    // The scored reward when there is one; otherwise what the deltas summed to.
    // A run with neither reports nothing rather than zero.
    total_reward: identity.totalReward ?? rewardTotal,
    achievements,
    coverage: {
      highWater: events.length ? events[events.length - 1].sequence : 0,
      closed: relay.journalClosed === true,
      framesDeclared: declaredFrames ?? frames.length,
      framesRetained: retainedFrames ?? frames.filter((frame) => frame.media).length,
      degradations: Array.isArray(relay.degradations) ? relay.degradations : []
    },
    events: [...events]
  };
}

/** Fold one trial's relayed optimizer events. */
export function craftaxTraceFromOptimizerEvents(
  optimizerEvents: readonly Any[],
  identity: TraceIdentity & { trialId?: string }
): EvalTraceView {
  return foldCraftaxTrace(
    containerEventsFromOptimizerEvents(optimizerEvents, identity.trialId),
    identity
  );
}

/** Fold a sealed Trace V5 document. */
export function craftaxTraceFromSealedTrace(
  document: Any,
  identity: TraceIdentity
): EvalTraceView {
  return foldCraftaxTrace(containerEventsFromSealedTrace(document), {
    ...identity,
    sealed: true,
    contentDigest: identity.contentDigest ?? text(document?.content_digest)
  });
}

/** One trial of a run, folded and labelled for the trajectory picker. */
export type TrialView = {
  trialId: string;
  rolloutId: string | null;
  seed: number | null;
  pool: string | null;
  /** `queued` before its first event; `running` until it settles. */
  state: "queued" | "running" | "done" | "failed";
  reward: number | null;
  view: EvalTraceView;
  /** The terminal record, for the fields the event stream does not carry. */
  record: Any | null;
};

/**
 * Split one optimizer run's events into per-trial Craftax views.
 *
 * Trials come from `eval.trial.started`, not from the container events, so a
 * seed that has been dispatched but has emitted nothing yet is a `queued` row
 * rather than a gap the picker has to explain. A run of five seeds opens at
 * 0/5 and fills in, instead of appearing one seed at a time.
 */
export function craftaxTrialsFromRun(
  run: Any,
  optimizerEvents: readonly Any[],
  scenarioFallback = "craftax"
): TrialView[] {
  type Pending = {
    trialId: string;
    rolloutId: string | null;
    seed: number | null;
    pool: string | null;
    started: boolean;
    record: Any | null;
    failed: boolean;
  };
  const order: string[] = [];
  const pending = new Map<string, Pending>();
  const claim = (trialId: string): Pending => {
    let row = pending.get(trialId);
    if (!row) {
      row = {
        trialId,
        rolloutId: null,
        seed: null,
        pool: null,
        started: false,
        record: null,
        failed: false
      };
      pending.set(trialId, row);
      order.push(trialId);
    }
    return row;
  };

  for (const event of optimizerEvents) {
    const type = event?.type ?? event?.eventType;
    const delta = (event?.delta ?? {}) as Any;
    const item = (event?.item ?? {}) as Any;
    const trialId = text(delta.trial_id ?? event?.raw?.trial_id ?? item.id);
    if (!trialId) continue;
    const row = claim(trialId);
    row.seed ??= num(delta.seed ?? item.seed ?? item.raw?.seed);
    row.pool ??= text(delta.pool ?? item.raw?.pool);
    if (type === "eval.trial.started") {
      row.started = true;
      row.rolloutId ??= text(delta.rollout_id);
    }
    if (type === "eval.trial.event") row.started = true;
    if (type === "eval.trial.terminal") {
      row.record = (item.raw ?? item) as Any;
      row.rolloutId ??= text(row.record?.rolloutId);
      row.failed = item.valid === false || text(row.record?.error) !== null;
    }
  }

  const scenario = text(run?.summary?.task) ?? scenarioFallback;
  return order.map((trialId) => {
    const row = pending.get(trialId)!;
    const record = row.record;
    const state: TrialView["state"] = record
      ? row.failed
        ? "failed"
        : "done"
      : row.started
        ? "running"
        : "queued";
    const view = craftaxTraceFromOptimizerEvents(optimizerEvents, {
      trialId,
      traceId: row.rolloutId ?? trialId,
      scenario,
      seed: row.seed,
      status: state,
      model: text(run?.summary?.policyRef?.model) ?? text(run?.summary?.model),
      provider: text(run?.summary?.policyRef?.provider),
      effort: text(run?.summary?.policy?.effort),
      totalReward: num(record?.reward),
      costUsd: num(record?.usage?.cost_usd),
      relay: (record?.relay ?? null) as Any
    });
    return {
      trialId,
      rolloutId: row.rolloutId,
      seed: row.seed,
      pool: row.pool,
      state,
      reward: num(record?.reward),
      view,
      record
    };
  });
}

/**
 * Replace a live view with its sealed counterpart, once the bundle arrives.
 *
 * Reconciliation is a *check*, not a merge. The sealed trace is authoritative,
 * so the answer is the sealed view — but a sealed view that covers less than
 * the live one already showed would silently delete evidence the reviewer was
 * looking at, so that case keeps the live view and says why.
 *
 * Selection is not part of either view. The caller holds it, which is what lets
 * the workbench swap the projection under a reviewer without moving them.
 */
export function reconcileCraftaxTrace(
  live: EvalTraceView | null,
  sealed: EvalTraceView | null
): { view: EvalTraceView | null; source: "live" | "sealed"; note: string | null } {
  if (!sealed) return { view: live, source: "live", note: null };
  if (!live) return { view: sealed, source: "sealed", note: null };
  const liveFrames = live.frames.length;
  const sealedFrames = sealed.frames.length;
  const liveCalls = live.steps.length;
  const sealedCalls = sealed.steps.length;
  if (sealedFrames < liveFrames || sealedCalls < liveCalls) {
    return {
      view: live,
      source: "live",
      note:
        `The sealed trace covers ${sealedCalls} call(s) and ${sealedFrames} frame(s), ` +
        `fewer than the ${liveCalls} call(s) and ${liveFrames} frame(s) relayed live. ` +
        "Showing the live projection so nothing observed is hidden."
    };
  }
  return { view: sealed, source: "sealed", note: null };
}

/**
 * The map rows a tile renderer should be given.
 *
 * The ASCII fallback used to be handed the entire textual observation —
 * inventory lines, status readout and all — and asked to paint it as a tile
 * grid, which produced a wall of noise wherever a native frame was missing.
 * The structured `local_map` is the map. When a producer sends only text, the
 * `local_map:` block is extracted and nothing else is passed on; when there is
 * no map at all, this returns `null` and the caller shows the observation as
 * text, which is honest.
 */
export function localMapRows(step: TraceStep | null | undefined): string[] | null {
  const readout = step?.content.readout;
  const structured = readout?.local_map;
  if (Array.isArray(structured) && structured.every((row) => typeof row === "string")) {
    return structured as string[];
  }
  if (typeof structured === "string" && structured.trim()) {
    return structured.replace(/\n+$/, "").split("\n");
  }
  const observation = step?.content.observation;
  if (typeof observation !== "string") return null;
  const block = observation.match(/local_map:\n([\s\S]*?)(?:\n\s*\n|\n[a-z_]+:|$)/);
  if (!block) return null;
  const rows = block[1].replace(/\n+$/, "").split("\n");
  return rows.length ? rows : null;
}

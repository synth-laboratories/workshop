import type { LiveEvalEvent } from "./types.ts";
import {
  parentTerminalEventKind,
  parentTerminalPolicyCallClosure,
  producerPolicyCallClosure,
  type PolicyCallClosure,
  type PolicyCallOutcome
} from "./policyCallOutcome.ts";

type Json = Record<string, unknown>;
export type EvidenceState = "visible" | "redacted" | "not_emitted" | "not_applicable" | "contract_defect" | "pending";
export type EvidenceField = { state: EvidenceState; value?: unknown; detail?: string };
export type ModelCall = {
  id: string; callNumber: number; sourceSequenceStart: number; sourceSequenceEnd: number;
  environmentStepStart?: number; environmentStepEnd?: number; provider?: string; model?: string; authority?: string;
  input: EvidenceField; reasoning: EvidenceField; output: EvidenceField; toolCalls: EvidenceField; toolResults: EvidenceField;
  usage: Json; latencyMs?: number; costUsd?: number; rawEvents: LiveEvalEvent[];
  /** Null only while the call is genuinely live. */
  outcome: PolicyCallOutcome | null;
  closure: PolicyCallClosure | null;
};
export type AgentTurnProjection = { calls: ModelCall[]; callIdByEnvironmentStep: Map<number, string>; missingPolicyEnvelopeCount: number };

function object(value: unknown): Json { return value && typeof value === "object" && !Array.isArray(value) ? value as Json : {}; }
function finite(value: unknown): number | undefined { const number = Number(value); return value !== null && value !== "" && Number.isFinite(number) ? number : undefined; }
function sequence(event: LiveEvalEvent, fallback: number): number { return finite((event as LiveEvalEvent & { sequence_number?: unknown; sequenceNumber?: unknown }).sequence_number ?? (event as LiveEvalEvent & { sequenceNumber?: unknown }).sequenceNumber ?? event.sequence) ?? fallback; }
function step(event: LiveEvalEvent | undefined): number | undefined { const payload = object(event?.payload); const readout = object(payload.readout); return finite(payload.step) ?? finite(payload.step_index) ?? finite(payload.env_steps) ?? finite(readout.env_steps); }
function isRedacted(value: unknown): boolean { return value === "[REDACTED]" || object(value).redacted === true; }
function field(value: unknown, options: { complete: boolean; applicable?: boolean; defect?: string }): EvidenceField {
  if (options.applicable === false) return { state: "not_applicable" };
  if (options.defect) return { state: "contract_defect", detail: options.defect };
  if (isRedacted(value)) return { state: "redacted" };
  if (value === undefined || value === null || value === "") return { state: options.complete ? "not_emitted" : "pending" };
  return { state: "visible", value };
}
function channelText(events: LiveEvalEvent[], name: string): string | undefined {
  const text = events.filter((event) => event.kind === "span.policy.data").map((event) => object(event.payload))
    .filter((payload) => payload.delta === true && payload.channel === name).map((payload) => typeof payload.text === "string" ? payload.text : "").join("");
  return text || undefined;
}
function nearestObservation(events: LiveEvalEvent[], before: number): unknown {
  const observation = events.slice(0, before + 1).reverse().find((event) => event.kind === "observation" || event.kind === "snapshot");
  const payload = object(observation?.payload); const readout = object(payload.readout);
  return readout.observation_text ?? readout.observation ?? (Object.keys(readout).length ? readout : payload.ascii ?? payload.grid);
}

/** Transport-agnostic Trace V5 → model-call projection over host-supplied durable envelopes. */
export function projectAgentTurns(events: LiveEvalEvent[]): AgentTurnProjection {
  const calls: ModelCall[] = []; const callIdByEnvironmentStep = new Map<number, string>();
  const terminalEvent = events.find((event) => parentTerminalEventKind(event.kind) !== null);
  const terminalSource = terminalEvent ? parentTerminalEventKind(terminalEvent.kind) : null;
  const terminalSequence = terminalEvent ? sequence(terminalEvent, events.indexOf(terminalEvent)) : null;
  let open: { events: LiveEvalEvent[]; index: number; ordinal: number } | undefined; let ordinal = 0; let missingPolicyEnvelopeCount = 0;
  const flush = () => {
    if (!open) return;
    const raw = open.events; const opened = object(raw[0]?.payload); const config = object(opened.call);
    const snapshots = raw.filter((event) => event.kind === "span.policy.data").map((event) => object(event.payload)).filter((payload) => payload.delta !== true && payload.channel !== "compact");
    const snapshot = snapshots.at(-1) ?? {};
    const assistant = object(snapshot.assistant);
    const closedEvent = raw.find((event) => event.kind === "span.policy.closed");
    const closure = closedEvent
      ? producerPolicyCallClosure(object(closedEvent.payload), sequence(closedEvent, open.index))
      : terminalSource && terminalSequence !== null && terminalSequence >= sequence(raw[0], open.index)
        ? parentTerminalPolicyCallClosure(terminalSource, terminalSequence)
        : null;
    // `open` is reassigned to undefined at the end of this flush, so the guard
    // above does not narrow it inside a callback. Read the index once.
    const openIndex = open.index;
    const nextPolicyOpenedIndex = events.findIndex((event, index) => index > openIndex && event.kind === "span.policy.opened");
    const causalWindow = events.slice(open.index + 1, nextPolicyOpenedIndex < 0 ? events.length : nextPolicyOpenedIndex);
    const producedSteps = causalWindow
      .filter((event) => event.kind === "frame" || event.kind === "span.step.closed")
      .map((event) => step(event))
      .filter((value): value is number => value !== undefined);
    const precedingObservation = [...events.slice(0, open.index + 1)].reverse().find((event) => event.kind === "observation" || event.kind === "snapshot");
    const precedingStep = step(precedingObservation);
    // A policy call can emit a batch of actions. Associate the entire batch up
    // to the next policy opening, not merely the first closed environment step.
    // The initial observation belongs to the first decision; subsequent calls
    // begin at the first frame/step they actually produce so ranges never
    // overlap at the preceding observation.
    const startStep = producedSteps[0] ?? precedingStep;
    const endStep = producedSteps.at(-1) ?? startStep;
    const callNumber = finite(snapshot.call) ?? finite(opened.call_number) ?? open.ordinal;
    const provider = String(snapshot.provider ?? object(snapshot.policy).provider ?? config.provider ?? "") || undefined;
    const model = String(snapshot.model ?? snapshot["gen_ai.request.model"] ?? object(snapshot.policy).model ?? config.model ?? "") || undefined;
    const authority = String(snapshot.authority ?? snapshot.action_authority ?? object(snapshot.policy).authority ?? config.authority ?? "") || undefined;
    const reasoning = snapshot.reasoning ?? snapshot.thinking ?? assistant.reasoning_content ?? channelText(raw, "reasoning");
    const output = Object.keys(assistant).length
      ? assistant.content
      : snapshot.assistant ?? snapshot.output ?? snapshot.response ?? channelText(raw, "content");
    const tools = snapshot.tool_calls ?? assistant.tool_calls ?? snapshot.tool_arguments ?? channelText(raw, "tool"); const results = snapshot.tool_results ?? snapshot.tool_outputs;
    const usage = object(snapshot.usage ?? object(snapshot.policy).usage); const first = sequence(raw[0], open.index); const last = sequence(raw.at(-1) ?? raw[0], first);
    const malformed = !raw.some((event) => event.kind === "span.policy.opened"); const id = `model-call:${callNumber}:${first}`;
    calls.push({ id, callNumber, sourceSequenceStart: first, sourceSequenceEnd: last, environmentStepStart: startStep, environmentStepEnd: endStep,
      provider, model, authority, input: field(nearestObservation(events, open.index), { complete: closure !== null, defect: malformed ? "policy data was emitted without span.policy.opened" : undefined }),
      reasoning: field(reasoning, { complete: closure !== null }), output: field(output, { complete: closure !== null }), toolCalls: field(tools, { complete: closure !== null }),
      toolResults: field(results, { complete: closure !== null, applicable: tools != null && tools !== "" }), usage,
      latencyMs: finite(snapshot.latency_ms ?? snapshot.duration_ms), costUsd: finite(usage.cost_usd), rawEvents: raw,
      outcome: closure?.outcome ?? null, closure });
    if (open.ordinal === 1 && precedingStep != null) callIdByEnvironmentStep.set(precedingStep, id);
    if (startStep != null && endStep != null) for (let value = startStep; value <= endStep; value += 1) callIdByEnvironmentStep.set(value, id);
    if (malformed) missingPolicyEnvelopeCount += 1; open = undefined;
  };
  for (let index = 0; index < events.length; index += 1) {
    const event = events[index];
    if (event.kind === "span.policy.opened") { flush(); open = { events: [event], index, ordinal: ++ordinal }; }
    else if (event.kind.startsWith("span.policy.")) { if (!open) open = { events: [], index, ordinal: ++ordinal }; open.events.push(event); if (event.kind === "span.policy.closed") flush(); }
  }
  flush(); return { calls, callIdByEnvironmentStep, missingPolicyEnvelopeCount };
}
export function reconcileCallSelection(calls: ModelCall[], selectedId: string | null, focus: boolean): string | null {
  if (!calls.length) return null; if (selectedId && calls.some((call) => call.id === selectedId)) return selectedId; return focus ? calls[0].id : calls.at(-1)?.id ?? null;
}
export function callForSequence(calls: ModelCall[], sourceSequence: number): ModelCall | undefined {
  return calls.find((call) => sourceSequence >= call.sourceSequenceStart && sourceSequence <= call.sourceSequenceEnd) ?? [...calls].reverse().find((call) => call.sourceSequenceStart <= sourceSequence);
}

import type { TraceV5Item } from "../../../components/TraceV5EventList.tsx";
import { projectAgentTurns, type EvidenceField, type ModelCall } from "../../../runtime/agentTranscript.ts";
import type { LiveEvalEvent } from "../../../runtime/types.ts";
import type { Lane } from "./project.ts";

type Json = Record<string, unknown>;
function object(value: unknown): Json { return value && typeof value === "object" && !Array.isArray(value) ? value as Json : {}; }
function json(value: unknown): string { return typeof value === "string" ? value : JSON.stringify(value, null, 2); }
function at(value: unknown, path: string[]): unknown { let current = value; for (const key of path) current = object(current)[key]; return current; }
function first(root: unknown, paths: string[][]): unknown { for (const path of paths) { const value = at(root, path); if (value !== undefined && value !== null && value !== "") return value; } }

function laneEvents(lane: Lane): LiveEvalEvent[] {
  return lane.trace.filter((row) => row.stream === "rollout").map((row) => ({
    run_id: lane.name,
    lane: lane.name,
    kind: row.kind,
    sequence: row.sequence,
    occurred_at: row.occurredAt,
    logical_time: row.logicalTime,
    payload: row.payload,
  }));
}

function snapshot(call: ModelCall): Json {
  const snapshots = call.rawEvents.filter((event) => event.kind === "span.policy.data").map((event) => object(event.payload)).filter((payload) => payload.delta !== true);
  return snapshots.at(-1) ?? {};
}

function evidenceBody(field: EvidenceField): string | undefined { return field.state === "visible" ? json(field.value) : undefined; }
function evidenceStatus(field: EvidenceField): string { return field.state.replaceAll("_", " "); }
function label(call: ModelCall): string {
  const start = call.environmentStepStart;
  const end = call.environmentStepEnd;
  const step = start == null ? "step not reported" : end != null && end !== start ? `steps ${start}–${end}` : `step ${start}`;
  return `Call ${call.callNumber} · ${step}`;
}

function toolItems(call: ModelCall, base: number): TraceV5Item[] {
  if (call.toolCalls.state !== "visible") return [{ id: `${call.id}:tools`, sequence: base, family: "tool", kind: "tool_calls", title: `${label(call)} · Tool calls`, status: evidenceStatus(call.toolCalls), body: call.toolCalls.detail }];
  const value = call.toolCalls.value;
  const rows = Array.isArray(value) ? value : [value];
  return rows.map((tool, index) => {
    const row = object(tool); const fn = object(row.function);
    const name = String(row.name ?? fn.name ?? `Tool call ${index + 1}`);
    const args = row.arguments ?? fn.arguments ?? tool;
    return { id: `${call.id}:tool:${index}`, sequence: base + index, family: "tool", kind: name, title: `${label(call)} · ${name}`, status: "visible", detail: json(args), collapsible: true, openLabel: "Open arguments" };
  });
}

function callItems(call: ModelCall, ordinal: number): TraceV5Item[] {
  const snap = snapshot(call); const assistant = object(snap.assistant);
  const messages = first(snap, [["input_messages"], ["messages"], ["request", "messages"], ["input", "messages"], ["call", "messages"]]);
  const cotSummary = first(snap, [["reasoning_summary"], ["cot_summary"], ["thinking_summary"], ["assistant", "reasoning_summary"]]);
  const input = messages ?? call.input.value;
  const inputState = input === undefined ? call.input.state : "visible";
  const base = call.sourceSequenceStart * 100 + ordinal * 10;
  const items: TraceV5Item[] = [{
    id: `${call.id}:input`, sequence: base, family: "input", kind: messages ? "input_messages" : "policy_input",
    title: `${label(call)} · Input messages`, status: inputState.replaceAll("_", " "),
    body: messages ? `${Array.isArray(messages) ? messages.length : 1} message${Array.isArray(messages) && messages.length === 1 ? "" : "s"} retained` : input === undefined ? call.input.detail : "Policy-visible observation retained",
    detail: input === undefined ? undefined : json(input), collapsible: input !== undefined, openLabel: "Open input",
  }, {
    id: `${call.id}:cot-summary`, sequence: base + 1, family: "thinking", kind: "cot_summary",
    title: `${label(call)} · CoT summary`, status: cotSummary === undefined ? "not emitted" : "visible",
    body: cotSummary === undefined ? "The provider did not emit a distinct reasoning summary for this call." : json(cotSummary),
    collapsible: cotSummary !== undefined, openLabel: "Open summary",
  }, {
    id: `${call.id}:cot`, sequence: base + 2, family: "thinking", kind: "reasoning",
    title: `${label(call)} · CoT / reasoning`, status: evidenceStatus(call.reasoning),
    body: call.reasoning.state === "visible" ? "Retained reasoning evidence" : call.reasoning.detail ?? `Reasoning was ${evidenceStatus(call.reasoning)} by the trace producer.`,
    detail: evidenceBody(call.reasoning), collapsible: call.reasoning.state === "visible", openLabel: "Open reasoning",
  }];
  items.push(...toolItems(call, base + 3));
  if (call.toolResults.state !== "not_applicable") items.push({ id: `${call.id}:tool-results`, sequence: base + 7, family: "tool", kind: "tool_results", title: `${label(call)} · Tool results`, status: evidenceStatus(call.toolResults), body: call.toolResults.state === "visible" ? "Structured tool results retained" : call.toolResults.detail, detail: evidenceBody(call.toolResults), collapsible: call.toolResults.state === "visible", openLabel: "Open results" });
  items.push({ id: `${call.id}:output`, sequence: base + 8, family: "output", kind: "assistant_output", title: `${label(call)} · Assistant output`, status: evidenceStatus(call.output), body: evidenceBody(call.output) ?? call.output.detail ?? `Output was ${evidenceStatus(call.output)}.`, detail: Object.keys(assistant).length ? json(assistant) : undefined, collapsible: Object.keys(assistant).length > 0, openLabel: "Open response envelope" });
  return items;
}

export function laneTraceV5Items(lane: Lane): { items: TraceV5Item[]; callCount: number; missingPolicyEnvelopeCount: number } {
  const projection = projectAgentTurns(laneEvents(lane));
  return { items: projection.calls.flatMap(callItems), callCount: projection.calls.length, missingPolicyEnvelopeCount: projection.missingPolicyEnvelopeCount };
}

/**
 * Compose `optimizer_run` dialect: map optimizer_event.v1 into the same
 * EventStream envelopes as eval `stream`, without flattening child eval traces.
 *
 * Product optimizer.* chrome stays. This path does not project GEPA/SFT/CISPO
 * inspector slices — it only labels envelopes by `type` so includeKinds can
 * match. Hosted RLVR is CISPO (`cispo.*`), not a generic `rlvr.*` firehose.
 */

import type { LiveEvalEvent } from "./types.ts";
import { isControlEnvelope } from "./liveStream.ts";

export const OPTIMIZER_EVENT_SCHEMA = "optimizer_event.v1";

export type OptimizerComposeResult =
  | { ok: true; events: LiveEvalEvent[] }
  | { ok: false; error: string };

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function schemaOf(event: Record<string, unknown>): string {
  return String(event.schema_version ?? event.schemaVersion ?? "");
}

function typeOf(event: Record<string, unknown>): string {
  return String(event.type ?? event.event_type ?? "");
}

function kindOf(event: Record<string, unknown>): string {
  return String(event.kind ?? "");
}

function isEvalTraceKind(kind: string): boolean {
  if (!kind) return false;
  return (
    kind.startsWith("rollout.")
    || kind.startsWith("env.")
    || kind === "run_started"
    || kind === "run_finished"
    || kind === "stream.subscribed"
  );
}

/** Harbor/Craftax eval envelopes must not appear on the optimizer_run input. */
export function looksLikeEvalTrace(event: unknown): boolean {
  const row = asRecord(event);
  if (!row) return false;
  if (schemaOf(row) === OPTIMIZER_EVENT_SCHEMA) return false;
  const type = typeOf(row);
  const kind = kindOf(row);
  const algorithm = row.algorithm_id ?? row.algorithmId;
  const runId = row.optimizer_run_id ?? row.optimizerRunId;
  if (type && (algorithm != null || runId != null)) return false;
  if (isEvalTraceKind(kind)) return true;
  return Boolean(kind) && !type;
}

function isOptimizerControl(event: LiveEvalEvent): boolean {
  if (isControlEnvelope(event)) return true;
  const label = String(event.kind ?? event.type ?? "");
  return label === "optimizer.visual.ready";
}

function payloadOf(event: Record<string, unknown>): Record<string, unknown> {
  return asRecord(event.delta) ?? asRecord(event.snapshot) ?? asRecord(event.item) ?? {};
}

function sequenceOf(event: Record<string, unknown>, index: number): number {
  const raw = event.sequenceNumber ?? event.sequence_number ?? event.sequence;
  if (
    (typeof raw !== "number" && typeof raw !== "string")
    || raw === ""
    || !Number.isSafeInteger(Number(raw))
    || Number(raw) < 1
  ) {
    throw new Error(`Optimizer event ${index + 1} is missing a valid sequence number`);
  }
  return Number(raw);
}

/**
 * Fail closed on eval traces. Map remaining optimizer_event.v1 envelopes so
 * EventStream can filter `includeKinds` against `kind` or `type`.
 */
export function optimizerEventsToLiveEval(raw: unknown): OptimizerComposeResult {
  if (raw == null) return { ok: true, events: [] };
  if (!Array.isArray(raw)) {
    return { ok: false, error: "optimizer_run events must be an array" };
  }
  const evalIndex = raw.findIndex((event) => looksLikeEvalTrace(event));
  if (evalIndex >= 0) {
    return {
      ok: false,
      error: "optimizer_run input does not flatten eval traces"
    };
  }
  const events: LiveEvalEvent[] = [];
  for (const [index, entry] of raw.entries()) {
    const row = asRecord(entry);
    if (!row) {
      return { ok: false, error: `Optimizer event ${index + 1} is not an object` };
    }
    let sequence: number;
    try {
      sequence = sequenceOf(row, index);
    } catch (reason) {
      return { ok: false, error: reason instanceof Error ? reason.message : String(reason) };
    }
    const type = typeOf(row) || "unknown";
    const occurredAt = String(row.occurredAt ?? row.occurred_at ?? row.created_at ?? row.ts ?? "");
    const mapped: LiveEvalEvent & { type: string } = {
      ts: occurredAt,
      occurred_at: occurredAt,
      run_id: String(row.optimizerRunId ?? row.optimizer_run_id ?? row.run_id ?? ""),
      kind: type,
      type,
      sequence,
      schema_version: schemaOf(row) || OPTIMIZER_EVENT_SCHEMA,
      payload: payloadOf(row)
    };
    if (isOptimizerControl(mapped)) continue;
    events.push(mapped);
  }
  return { ok: true, events };
}

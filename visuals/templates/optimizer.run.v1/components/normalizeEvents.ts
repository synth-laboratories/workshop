import type { OptimizerEvent } from "./projectEvents.ts";

function requiredSequence(event: Record<string, unknown>, index: number): number {
  const raw = event.sequenceNumber ?? event.sequence_number;
  if (
    (typeof raw !== "number" && typeof raw !== "string") ||
    raw === "" ||
    !Number.isSafeInteger(Number(raw)) ||
    Number(raw) < 1
  ) {
    throw new Error(`Optimizer event ${index + 1} is missing a valid sequence number`);
  }
  return Number(raw);
}

/** Normalize wire aliases without manufacturing ordering evidence. */
export function normalizeOptimizerEvents(events: unknown[]): OptimizerEvent[] {
  return events.map((event, index) => {
    if (!event || typeof event !== "object" || Array.isArray(event)) {
      throw new Error(`Optimizer event ${index + 1} is not an object`);
    }
    const e = event as Record<string, unknown>;
    return {
      schemaVersion: e.schemaVersion ? String(e.schemaVersion) : undefined,
      eventId: e.eventId ? String(e.eventId) : undefined,
      type: String(e.type ?? e.event_type ?? "unknown"),
      sequenceNumber: requiredSequence(e, index),
      occurredAt: String(e.occurredAt ?? e.occurred_at ?? e.created_at ?? ""),
      optimizerRunId: String(e.optimizerRunId ?? e.optimizer_run_id ?? e.run_id ?? ""),
      algorithmId: String(e.algorithmId ?? e.algorithm_id ?? "unknown"),
      level: e.level ? String(e.level) : undefined,
      item: e.item as OptimizerEvent["item"],
      delta: (e.delta as Record<string, unknown>) ?? {},
      snapshot: e.snapshot as Record<string, unknown> | undefined,
      usageDelta: e.usageDelta as Record<string, number> | undefined ??
        (e.usage_delta as Record<string, number> | undefined),
      artifactRefs: (e.artifactRefs as unknown[]) ?? (e.artifact_refs as unknown[]) ?? [],
      error: e.error,
      raw: e.raw
    };
  });
}

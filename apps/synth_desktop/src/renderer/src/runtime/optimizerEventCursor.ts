export type OptimizerEventCursorState = {
  events: unknown[];
  cursor: number;
  gap: boolean;
};

export function optimizerEventSequence(value: unknown): number {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Optimizer event is not an object");
  }
  const event = value as Record<string, unknown>;
  const raw = event.sequenceNumber ?? event.sequence_number;
  if (
    (typeof raw !== "number" && typeof raw !== "string") ||
    raw === "" ||
    !Number.isSafeInteger(Number(raw)) ||
    Number(raw) < 1
  ) {
    throw new Error("Optimizer event is missing a valid sequence number");
  }
  return Number(raw);
}

/**
 * Append a persisted event page. Sequence is the durable identity: the Rust
 * store enforces one event per (run, sequence), so replayed pages are harmless.
 */
export function mergeOptimizerEventPage(
  current: OptimizerEventCursorState,
  incoming: unknown[]
): OptimizerEventCursorState {
  if (incoming.length === 0) return current;
  const ordered = incoming
    .map((event) => ({ event, sequence: optimizerEventSequence(event) }))
    .sort((left, right) => left.sequence - right.sequence);
  const next = new Map<number, unknown>();
  for (const event of current.events) next.set(optimizerEventSequence(event), event);

  let cursor = current.cursor;
  let gap = current.gap;
  for (const row of ordered) {
    if (row.sequence > cursor + 1) gap = true;
    if (row.sequence > cursor) cursor = row.sequence;
    if (!next.has(row.sequence)) next.set(row.sequence, row.event);
  }
  return {
    events: [...next.entries()].sort(([left], [right]) => left - right).map(([, event]) => event),
    cursor,
    gap
  };
}

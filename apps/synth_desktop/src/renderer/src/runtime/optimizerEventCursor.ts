export type OptimizerEventCursorState = {
  events: unknown[];
  cursor: number;
  gap: boolean;
  /**
   * Retained sequence index, carried across merges so appending a page costs
   * the page rather than the history. Optional because callers construct an
   * empty state as a literal; it is built on first merge when absent.
   */
  index?: Map<number, unknown>;
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

function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    const record = value as Record<string, unknown>;
    return `{${Object.keys(record).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(record[key])}`).join(",")}}`;
  }
  return JSON.stringify(value) ?? "undefined";
}

/**
 * A fresh, empty cursor state.
 *
 * Callers must use this rather than an object literal: the retained index
 * below is what keeps a paged walk linear, and a hand-built literal silently
 * opts out of it.
 */
export function emptyOptimizerEventCursor(): OptimizerEventCursorState {
  return { events: [], cursor: 0, gap: false, index: new Map() };
}

/**
 * A defensive copy of the accumulated events, for handing to a consumer.
 *
 * The cursor state is single-owner and mutated in place while a page walk is
 * in flight — that is what makes the walk linear. The moment those events are
 * published to a React tree they stop being ours, so the boundary takes one
 * copy. One copy per *publish* is the correct cost; one per *page* is what the
 * quadratic version was paying.
 */
export function publishedOptimizerEvents(state: OptimizerEventCursorState): unknown[] {
  return [...state.events];
}

/**
 * Append a persisted event page. Sequence is the durable identity: the Rust
 * store enforces one event per (run, sequence), so replayed pages are harmless.
 *
 * Linear in the size of the *page*, not the size of the history, and mutates
 * `current` in place.
 *
 * The previous shape rebuilt a `Map` from every event accumulated so far and
 * re-sorted the whole set on every page, so paging a run was quadratic in its
 * event count: 3ms at 2,259 events, 19ms at 10,000, 268ms at 50,000 — and the
 * journal walk is precisely the thing that has to stay flat as histories grow.
 *
 * Mutation is deliberate and is why this is linear. The state has exactly one
 * owner (the subscription entry), a walk merges N/500 times but publishes
 * once, and the publish boundary takes the single defensive copy above. Pages
 * read forward by cursor arrive strictly ascending, so the common path is a
 * push; a page that genuinely lands out of order — reachable only after a
 * snapshot reload across a hole — pays for one re-sort.
 */
export function mergeOptimizerEventPage(
  current: OptimizerEventCursorState,
  incoming: unknown[]
): OptimizerEventCursorState {
  if (incoming.length === 0) return current;

  // Adopt the retained index, or build one once from a state that arrived
  // without one — an older object literal, or a test fixture.
  let index = current.index;
  if (!index) {
    index = new Map<number, unknown>();
    for (const event of current.events) index.set(optimizerEventSequence(event), event);
    current.index = index;
  }

  const ordered = incoming
    .map((event) => ({ event, sequence: optimizerEventSequence(event) }))
    .sort((left, right) => left.sequence - right.sequence);

  let highest = current.events.length > 0
    ? optimizerEventSequence(current.events[current.events.length - 1])
    : 0;
  let outOfOrder = false;

  for (const row of ordered) {
    if (row.sequence > current.cursor + 1) current.gap = true;
    if (row.sequence > current.cursor) current.cursor = row.sequence;
    const existing = index.get(row.sequence);
    if (existing !== undefined) {
      // A replayed page is harmless; a replayed page carrying *different*
      // content means the durable log was rewritten underneath us, which no
      // cursor can reconcile. Only reachable on a real duplicate, so the
      // canonical comparison stays off the hot path.
      if (canonicalJson(existing) !== canonicalJson(row.event)) {
        throw new Error(`Optimizer event sequence ${row.sequence} was replayed with different content`);
      }
      continue;
    }
    index.set(row.sequence, row.event);
    if (row.sequence > highest) {
      highest = row.sequence;
      current.events.push(row.event);
    } else {
      outOfOrder = true;
    }
  }

  if (outOfOrder) {
    current.events = [...index.entries()]
      .sort(([left], [right]) => left - right)
      .map(([, event]) => event);
  }
  return current;
}

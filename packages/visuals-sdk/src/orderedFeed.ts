import { stableSerialize } from "./query.ts";

export type OrderedFeedEnvelope<T> = {
  scope: string;
  sequence: number;
  id?: string;
  value: T;
  digest?: string;
};

export type OrderedFeedGap = { scope: string; after: number; before: number };
export type OrderedFeedConflict = { identity: string; previousDigest: string; incomingDigest: string };
export type OrderedFeedState<T> = {
  events: OrderedFeedEnvelope<T>[];
  gaps: OrderedFeedGap[];
  conflicts: OrderedFeedConflict[];
  highWaterByScope: Map<string, number>;
  closedScopes: Set<string>;
};

function digest(value: unknown): string { return stableSerialize(value); }
function identity<T>(event: OrderedFeedEnvelope<T>): string { return `${event.scope}:${event.id ?? event.sequence}`; }

export class OrderedFeed<T> {
  readonly #events = new Map<string, OrderedFeedEnvelope<T>>();
  readonly #digests = new Map<string, string>();
  readonly #received = new Map<string, Set<number>>();
  readonly #highWater = new Map<string, number>();
  readonly #closed = new Set<string>();
  readonly #conflicts: OrderedFeedConflict[] = [];

  ingest(incoming: Iterable<OrderedFeedEnvelope<T>>): void {
    for (const event of incoming) {
      if (!event.scope || !Number.isSafeInteger(event.sequence) || event.sequence < 0) throw new Error("Ordered feed events require scope and a non-negative integer sequence");
      const key = identity(event);
      const incomingDigest = event.digest ?? digest(event.value);
      const previousDigest = this.#digests.get(key);
      if (previousDigest) {
        if (previousDigest !== incomingDigest) this.#conflicts.push({ identity: key, previousDigest, incomingDigest });
        continue;
      }
      this.#events.set(key, Object.freeze({ ...event }));
      this.#digests.set(key, incomingDigest);
      const received = this.#received.get(event.scope) ?? new Set<number>();
      received.add(event.sequence);
      this.#received.set(event.scope, received);
      this.#highWater.set(event.scope, Math.max(this.#highWater.get(event.scope) ?? event.sequence, event.sequence));
    }
  }

  close(scope: string): void { this.#closed.add(scope); }

  snapshot(): OrderedFeedState<T> {
    const events = [...this.#events.values()].sort((a, b) => a.scope.localeCompare(b.scope) || a.sequence - b.sequence || identity(a).localeCompare(identity(b)));
    const gaps: OrderedFeedGap[] = [];
    for (const [scope, received] of this.#received) {
      const sequences = [...received].sort((a, b) => a - b);
      for (let index = 1; index < sequences.length; index += 1) {
        const previous = sequences[index - 1]!;
        const current = sequences[index]!;
        if (current > previous + 1) gaps.push({ scope, after: previous, before: current });
      }
    }
    return {
      events,
      gaps,
      conflicts: this.#conflicts.map((row) => ({ ...row })),
      highWaterByScope: new Map(this.#highWater),
      closedScopes: new Set(this.#closed),
    };
  }
}


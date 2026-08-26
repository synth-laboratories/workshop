/**
 * The transport contract between Workshop and a live template.
 *
 * A template used to work out its own transport: read the bindings prop, find a
 * URL, and call an optional `pollStream` callback if the host happened to pass
 * one. Every step of that could fail into silence — an unreadable bindings
 * shape produced zero streams, zero polls, zero errors, and a pane that sat at
 * `connecting` while ten terminal rollouts waited behind it.
 *
 * So the host builds the client and the template consumes it:
 *
 * - `streams` is required and explicit. Zero declared streams is a state the
 *   template can render, not an absence it has to infer.
 * - `poll` is required. A template cannot silently lose it.
 * - Response-shape tolerance lives here, in one place, as declared
 *   compatibility rather than an inline ternary repeated per hook.
 *
 * See: docs/contracts/visual_replay_transport.md.
 */

import type { LiveEnvelope } from "./liveStream.ts";

export type ReplayStream = {
  /** Stable identity for diagnostics and cursor bookkeeping. */
  streamId: string;
  /** Durable poll authority. Replay works from this alone. */
  pollUrl: string;
  /** Optional incremental transport. Its loss is never data loss. */
  sseUrl?: string;
};

export type ReplayCursor = {
  next: number;
  highWater?: number;
  hasMore: boolean;
  closed: boolean;
};

export type ReplayPage = {
  events: LiveEnvelope[];
  cursor: ReplayCursor;
};

export type ReplayClient = {
  /** Declared streams, in binding order. Never inferred from a prop bag. */
  streams: ReplayStream[];
  poll(stream: ReplayStream, after: number, limit: number): Promise<ReplayPage>;
};

/** Transport lifecycle. `connecting` is not in it: see `TransportState`. */
export type TransportState =
  /** No stream is declared. Nothing is pending and nothing is wrong. */
  | "idle"
  /** Streams are declared; the first response has not arrived yet. */
  | "declared"
  /** Reading durable history from cursor zero. */
  | "replaying"
  /** Caught up, at least one stream still open. */
  | "live"
  /** Every declared stream reported closed. */
  | "terminal"
  /** Bounded and named. Never a resting state without a reason. */
  | "error";

/**
 * How long a declared stream may go without a first response before the pane
 * says so. The failure this replaces had no deadline at all, so "never asked"
 * and "asked and waiting" looked identical for as long as anyone watched.
 */
export const REPLAY_FIRST_RESPONSE_TIMEOUT_MS = 15_000;

/** Rows per poll. Bounded so one page cannot become an unbounded response. */
export const REPLAY_PAGE_LIMIT = 500;

/** Hard ceiling on pages per stream, so a stuck cursor cannot loop forever. */
export const REPLAY_PAGE_LIMIT_MAX = 1_000;

type RawPage =
  | LiveEnvelope[]
  | {
      events?: LiveEnvelope[];
      page?: { events?: LiveEnvelope[] };
      cursor?: { next?: number; high_water?: number; has_more?: boolean; closed?: boolean };
    };

/**
 * Normalize the three page shapes producers emit today.
 *
 * COMPAT: a bare array has no cursor, so it is treated as one closed page —
 * that is the only reading which cannot silently drop rows. Remove the array
 * and top-level `events` arms once every producer emits `page`+`cursor`.
 */
export function parseReplayPage(body: unknown, after: number): ReplayPage {
  if (Array.isArray(body)) {
    return { events: body as LiveEnvelope[], cursor: { next: after, hasMore: false, closed: true } };
  }
  if (!body || typeof body !== "object") {
    throw new Error(`replay page is ${body === null ? "null" : typeof body}, not an object`);
  }
  const page = body as Exclude<RawPage, LiveEnvelope[]>;
  const events = page.page?.events ?? page.events;
  if (!Array.isArray(events)) {
    throw new Error("replay page has neither page.events nor events");
  }
  const sequences = events
    .map((row) => Number(row.sequence_number ?? row.sequence))
    .filter(Number.isFinite);
  const next = page.cursor?.next ?? (sequences.length ? Math.max(...sequences) : after);
  const highWater = page.cursor?.high_water;
  return {
    events,
    cursor: {
      next,
      highWater,
      hasMore: page.cursor?.has_more ?? (highWater != null && next < highWater),
      closed: page.cursor?.closed ?? false
    }
  };
}

/**
 * Build a client over a host-supplied transport.
 *
 * `transport` is the native allowlisted poll in Workshop. In a browser preview
 * there is none, and `fetch` stands in — the caller chooses, and the template
 * never learns which it got.
 */
export function createReplayClient(
  streams: ReplayStream[],
  transport?: (pollUrl: string, after: number, limit: number) => Promise<unknown>
): ReplayClient {
  return {
    streams,
    async poll(stream, after, limit) {
      const body = transport
        ? await transport(stream.pollUrl, after, limit)
        : await (async () => {
            const url = new URL(stream.pollUrl, stream.sseUrl);
            url.searchParams.set("after", String(after));
            url.searchParams.set("limit", String(limit));
            const response = await fetch(url, { headers: { Accept: "application/json" } });
            if (!response.ok) {
              throw new Error(`replay poll HTTP ${response.status} for ${stream.streamId}`);
            }
            return response.json();
          })();
      return parseReplayPage(body, after);
    }
  };
}

/**
 * What the host gives every live template. One declaration, so a template
 * cannot quietly disagree with the host about how transport arrives.
 */
export type LiveTemplateProps = {
  /** Declared transport. Absent only outside Workshop (browser preview). */
  replay?: ReplayClient;
  /** Declared live streams the host could not give a durable poll authority. */
  replayMissingTransport?: string[];
  visualId?: string | null;
  revision?: number | null;
};

/** Declared live streams, in binding order, from resolved binding slots. */
export function replayStreamsFromBindings(
  slots: Array<{ input?: string; slot?: string; kind: string; source?: string; poll_url?: string }>
): { streams: ReplayStream[]; missingTransport: string[] } {
  const live = slots.filter((slot) => slot.kind === "live_sse");
  const streams: ReplayStream[] = [];
  const missingTransport: string[] = [];
  for (const [index, binding] of live.entries()) {
    if (!binding.poll_url) {
      // A stream with no durable poll authority cannot be replayed after it
      // closes, so it is reported rather than quietly dropped.
      missingTransport.push(binding.source ?? `${binding.input ?? binding.slot}[${index}]`);
      continue;
    }
    streams.push({
      streamId: binding.source ?? binding.poll_url,
      pollUrl: binding.poll_url,
      sseUrl: binding.source
    });
  }
  return { streams, missingTransport };
}

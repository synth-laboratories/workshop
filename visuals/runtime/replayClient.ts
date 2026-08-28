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

/**
 * The host's fold of everything it has observed for this visual, in the shape
 * a seal freezes (`synth.live-eval-projection.v1`).
 *
 * Derived values only: `event_count` stands in for the envelopes, which travel
 * beside it as `ReplayPage.events` rather than being sent twice. A host
 * without Rust supplies none of this and the template folds locally, which is
 * what browser preview and fixture replay do.
 */
export type HostLiveEvalProjection = {
  schema_version: string;
  kinds: string[];
  has_live_frames: boolean;
  has_reward_txt: boolean;
  reward: number | null;
  usage: {
    prompt_tokens: number | null;
    completion_tokens: number | null;
    total_tokens: number | null;
    cost_usd: number | null;
  } | null;
  event_count: number;
};

export type ReplayPage = {
  events: LiveEnvelope[];
  cursor: ReplayCursor;
  /**
   * What the host folded, when the host is Workshop. Absent in browser
   * preview and fixture replay, where the template folds for itself — so a
   * reader treats this as the authoritative answer when it is there and as
   * nothing at all when it is not.
   */
  projection?: HostLiveEvalProjection;
  /** The host's own account of the transport, when the host keeps one. */
  receipt?: unknown;
  /**
   * The host's retained evidence stopped short of the run, so `projection` is
   * a lower bound rather than the whole eval.
   */
  evidenceTruncated?: boolean;
};

/**
 * Envelope version of a Workshop poll answer. A body carrying this string
 * brings the host's fold with it; anything else is a producer page and is
 * folded by the reader.
 */
export const HOST_POLL_SCHEMA = "synth.visual-stream-poll.v1";

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
      schemaVersion?: string;
      events?: LiveEnvelope[];
      page?: { events?: LiveEnvelope[] };
      cursor?: { next?: number; high_water?: number; has_more?: boolean; closed?: boolean };
      projection?: HostLiveEvalProjection | null;
      receipt?: unknown;
      evidenceTruncated?: boolean;
    };

/**
 * Normalize the page shapes this client can be handed.
 *
 * Four, and only one of them is new: Workshop's own answer, which wraps the
 * producer's envelopes and cursor beside the fold the host already performed.
 * It is read by the same two fields as a producer page on purpose — the host
 * passes the producer's cursor through rather than recomputing it — so the
 * only thing the wrapper adds here is the projection and the receipt.
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
    },
    // Carried only when the host actually folded. `projection: null` is the
    // host saying it observed nothing for this visual, which is not the same
    // claim as a host that folds nothing at all, and neither is an empty fold.
    ...(page.schemaVersion === HOST_POLL_SCHEMA
      ? {
          ...(page.projection ? { projection: page.projection } : {}),
          ...(page.receipt !== undefined ? { receipt: page.receipt } : {}),
          evidenceTruncated: page.evidenceTruncated === true
        }
      : {})
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
  /**
   * Host-mediated binary media (`synth.visual.media.v1`). Absent outside
   * Workshop, where a template renders its frame references as unavailable
   * rather than reaching for the store itself.
   */
  media?: import("./mediaClient.ts").MediaClient;
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

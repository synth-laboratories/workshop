/**
 * `synth.visual.media.v1` — the binary side of a visual's evidence.
 *
 * The `local_cas` binding decodes a stored object as JSON. That is right for a
 * chart spec and useless for a 768×768 PNG, which is why native frames used to
 * reach panes as base64 smuggled through optimizer event payloads — one copy of
 * every frame in every progress update, growing without bound.
 *
 * So the timeline carries *references*: `{ casDigest, mediaType, width, height }`.
 * A template asks the host for one digest at a time and the host decides, from
 * the run this visual is bound to, whether it may have it. Nothing about that
 * decision is visible here on purpose: a client that could describe the rule
 * could be argued with.
 *
 * Two behaviours are the whole point of this file being a module rather than an
 * inline `fetch`:
 *
 * - **Bounded loading.** The selected frame plus a small preload window, never
 *   the timeline. A 500-step episode is 500 PNGs; asking for all of them to
 *   show one is how a pane becomes unresponsive.
 * - **Immutable caching.** A digest names bytes, so a decoded object is correct
 *   forever. Re-fetching it across a scrub is pure waste.
 *
 * See: docs/contracts/visual_media_bridge.md.
 */

export const VISUAL_MEDIA_PROTOCOL = "synth.visual.media.v1";

/** What the timeline carries in place of the bytes. */
export type MediaRef = {
  /** Workshop's own SHA-256 of the stored object. 64 hex characters. */
  casDigest: string;
  mediaType?: string;
  width?: number | null;
  height?: number | null;
  /**
   * The producer's own digest. Provenance only — the one observed in the field
   * is 16 characters, so it is not a content address and must never be used as
   * one. Kept so a viewer can show what the container called this frame.
   */
  producerDigest?: string | null;
};

export type LoadedMedia = {
  casDigest: string;
  mediaType: string;
  /** A `data:` URL, ready for `<img src>`. */
  dataUrl: string;
  width: number | null;
  height: number | null;
  byteSize: number;
};

/** How the host answers. Supplied by Workshop; absent in a browser preview. */
export type MediaTransport = (casDigest: string) => Promise<{
  casDigest: string;
  mediaType: string;
  byteSize: number;
  width: number | null;
  height: number | null;
  dataUrl: string;
}>;

export type MediaClient = {
  /** A decoded object, if it is already in hand. Never triggers a fetch. */
  peek(casDigest: string): LoadedMedia | undefined;
  /** Load one object. Concurrent calls for the same digest share one request. */
  load(casDigest: string): Promise<LoadedMedia>;
  /**
   * Load `selected` and a small window around it, in that order.
   *
   * Returns once the selected object is in hand; the window keeps loading in
   * the background. A scrubber must not wait on its own lookahead.
   */
  warm(digests: readonly string[], selected: number): Promise<LoadedMedia | undefined>;
  /** Digests whose last load failed, with the reason. */
  failures(): ReadonlyMap<string, string>;
};

/** Objects kept decoded. Bounded so a long scrub cannot grow without limit. */
export const MEDIA_CACHE_LIMIT = 64;

/** Objects loaded ahead of and behind the selection. */
export const MEDIA_PRELOAD_AHEAD = 4;
export const MEDIA_PRELOAD_BEHIND = 2;

/** A digest is 64 lowercase hex characters. Anything else is not asked for. */
export function isCasDigest(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}

/**
 * Read a media reference off a relayed container event payload.
 *
 * Tolerant about where it sits and strict about what it is: a payload with a
 * `media` block that has no usable digest returns `null` rather than a
 * half-built reference that fails later at load time.
 */
export function mediaRefFrom(payload: unknown): MediaRef | null {
  if (!payload || typeof payload !== "object") return null;
  const media = (payload as Record<string, unknown>).media;
  if (!media || typeof media !== "object") return null;
  const row = media as Record<string, unknown>;
  const digest = row.casDigest ?? row.cas_digest;
  if (!isCasDigest(digest)) return null;
  const num = (value: unknown): number | null =>
    typeof value === "number" && Number.isFinite(value) ? value : null;
  return {
    casDigest: digest,
    mediaType: typeof row.mediaType === "string" ? row.mediaType : "image/png",
    width: num(row.width),
    height: num(row.height),
    producerDigest:
      typeof row.producerDigest === "string"
        ? row.producerDigest
        : typeof row.producer_digest === "string"
          ? row.producer_digest
          : null
  };
}

/**
 * A client with no transport.
 *
 * Every method resolves or rejects explicitly. A template holding this renders
 * "media is not available here", which is a state — unlike a client that
 * silently never answers, which is indistinguishable from a slow one.
 */
export const NO_MEDIA: MediaClient = {
  peek: () => undefined,
  load: async () => {
    throw new Error("no media transport is available for this visual");
  },
  warm: async () => undefined,
  failures: () => new Map()
};

export function createMediaClient(transport?: MediaTransport): MediaClient {
  if (!transport) return NO_MEDIA;
  // Insertion-ordered, so the oldest entry is the first key. Re-inserting on
  // hit makes this least-recently-used without a second structure.
  const cache = new Map<string, LoadedMedia>();
  const inflight = new Map<string, Promise<LoadedMedia>>();
  const failed = new Map<string, string>();

  const remember = (media: LoadedMedia) => {
    cache.delete(media.casDigest);
    cache.set(media.casDigest, media);
    while (cache.size > MEDIA_CACHE_LIMIT) {
      const oldest = cache.keys().next();
      if (oldest.done) break;
      cache.delete(oldest.value);
    }
  };

  const load = (casDigest: string): Promise<LoadedMedia> => {
    if (!isCasDigest(casDigest)) {
      return Promise.reject(new Error(`${casDigest} is not a content digest`));
    }
    const cached = cache.get(casDigest);
    if (cached) {
      cache.delete(casDigest);
      cache.set(casDigest, cached);
      return Promise.resolve(cached);
    }
    const pending = inflight.get(casDigest);
    if (pending) return pending;
    const request = transport(casDigest)
      .then((response) => {
        const media: LoadedMedia = {
          casDigest,
          mediaType: response.mediaType,
          dataUrl: response.dataUrl,
          width: response.width ?? null,
          height: response.height ?? null,
          byteSize: response.byteSize
        };
        remember(media);
        failed.delete(casDigest);
        return media;
      })
      .catch((error: unknown) => {
        // Recorded rather than swallowed: a frame that will not load is a
        // thing the pane should say, not a tile that stays blank.
        failed.set(casDigest, error instanceof Error ? error.message : String(error));
        throw error;
      })
      .finally(() => {
        inflight.delete(casDigest);
      });
    inflight.set(casDigest, request);
    return request;
  };

  return {
    peek: (casDigest) => cache.get(casDigest),
    load,
    failures: () => failed,
    async warm(digests, selected) {
      const target = digests[selected];
      if (!target) return undefined;
      // The selection first and awaited; the window after and not awaited. A
      // scrub that blocked on its own lookahead would get slower the more it
      // tried to look ahead.
      const media = await load(target).catch(() => undefined);
      const from = Math.max(0, selected - MEDIA_PRELOAD_BEHIND);
      const to = Math.min(digests.length, selected + MEDIA_PRELOAD_AHEAD + 1);
      for (let index = from; index < to; index += 1) {
        const digest = digests[index];
        if (index === selected || !digest || cache.has(digest)) continue;
        void load(digest).catch(() => undefined);
      }
      return media;
    }
  };
}

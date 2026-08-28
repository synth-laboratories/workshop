/**
 * Shared live-eval projector for Craftax and Harbor — the TypeScript mirror.
 * Missing reward / usage / score stay null. Control envelopes are not evidence.
 *
 * The authoritative projector is `stream_fold::project_live_eval`; this is the
 * copy hosts with no Rust underneath them need in order to draw. Both are
 * pinned to `visuals/fixtures/live_fold_golden.json`. See `liveStream.ts` for
 * what the mirror does and does not keep.
 */

import { envelopeStream, formatMissingNumber, isControlEnvelope, type LiveEnvelope } from "./liveStream.ts";

/**
 * A logical cutoff into a folded stream set: how many evidence envelopes of
 * each stream to include.
 *
 * Not a sequence. The real multiplexed capture
 * (`live.craftax.v1/examples/cua-luna-low-10.json`, one stream and ten lanes)
 * sequences with non-numeric strings, so a scalar numeric cutoff is a no-op on
 * it and a per-scope numeric vector cannot address its events either. Arrival
 * order within a stream is the one total order that always exists — the spool
 * persists it verbatim and the fold preserves it — so a cutoff is a prefix
 * length per stream. A stream the vector does not name contributes nothing:
 * a cutoff says what is included.
 *
 * This is also how the shipped shells already scrub, by array index rather
 * than by sequence.
 */
export type CursorVector = Record<string, number>;

export type LiveEvalProjection = {
  events: LiveEnvelope[];
  kinds: string[];
  has_live_frames: boolean;
  has_reward_txt: boolean;
  reward: number | null;
  usage: Record<string, number | null> | null;
  /** The cutoff this projection was folded at, or null for the whole prefix. */
  cutoff: CursorVector | null;
};

const FORBIDDEN_BLOBS = ["collector", "capability_blob", "capabilities_blob"] as const;

function jsonKeys(payload: unknown, acc: Set<string> = new Set()): Set<string> {
  if (!payload || typeof payload !== "object") return acc;
  for (const [key, value] of Object.entries(payload as Record<string, unknown>)) {
    acc.add(key);
    jsonKeys(value, acc);
  }
  return acc;
}

function payloadNumber(payload: Record<string, unknown> | undefined, keys: string[]): number | null {
  if (!payload) return null;
  for (const key of keys) {
    const value = payload[key];
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  return null;
}

export function projectLiveEval(
  events: LiveEnvelope[],
  cutoff?: CursorVector
): LiveEvalProjection {
  const rows: LiveEnvelope[] = [];
  const taken = new Map<string, number>();
  for (const event of events) {
    // `isControlEnvelope` is the one control predicate; it honours the explicit
    // `control: true` flag, so the projector and the ingest fold agree.
    if (isControlEnvelope(event)) continue;
    if (cutoff) {
      const stream = envelopeStream(event);
      const already = taken.get(stream) ?? 0;
      if (already >= (cutoff[stream] ?? 0)) continue;
      taken.set(stream, already + 1);
    }
    rows.push(event);
  }
  const kinds = rows.map((event) => String(event.kind ?? event.type ?? ""));
  const has_live_frames = kinds.includes("frame");
  const has_reward_txt = rows.some((event) => jsonKeys(event.payload).has("reward.txt"));
  const lastVerifier = [...rows].reverse().find((event) => String(event.kind ?? event.type) === "verifier");
  const lastReward = [...rows].reverse().find((event) => {
    const kind = String(event.kind ?? event.type);
    return kind === "reward_signal" || kind === "eval.run.terminal";
  });
  let reward: number | null = null;
  if (lastVerifier) {
    const payload = (lastVerifier.payload ?? {}) as Record<string, unknown>;
    const nested = payload["reward.txt"];
    if (typeof nested === "number" && Number.isFinite(nested)) reward = nested;
  }
  if (reward == null && lastReward) {
    reward = payloadNumber(lastReward.payload as Record<string, unknown>, ["value", "reward", "total"]);
  }
  const usageEvent = [...rows].reverse().find((event) => {
    const payload = event.payload as Record<string, unknown> | undefined;
    return payload && typeof payload.usage === "object" && payload.usage != null;
  });
  const usageRaw = usageEvent ? ((usageEvent.payload as Record<string, unknown>).usage as Record<string, unknown>) : null;
  const usage = usageRaw
    ? {
        prompt_tokens: typeof usageRaw.prompt_tokens === "number" ? usageRaw.prompt_tokens : null,
        completion_tokens: typeof usageRaw.completion_tokens === "number" ? usageRaw.completion_tokens : null,
        total_tokens: typeof usageRaw.total_tokens === "number" ? usageRaw.total_tokens : null,
        cost_usd: typeof usageRaw.cost_usd === "number" ? usageRaw.cost_usd : null
      }
    : null;
  const projection: LiveEvalProjection = {
    events: rows,
    kinds,
    has_live_frames,
    has_reward_txt,
    reward,
    usage,
    cutoff: cutoff ?? null
  };
  const blob = JSON.stringify(projection);
  for (const name of FORBIDDEN_BLOBS) {
    if (blob.includes(name)) {
      throw new Error(`live eval projection leaked forbidden blob "${name}"`);
    }
  }
  return projection;
}

export function displayReward(projection: LiveEvalProjection): string {
  return formatMissingNumber(projection.reward);
}

/** `/reward` mapping for env status. Incomplete stays null. */
export function rewardFromEnvStatus(status: string | null | undefined): number | null {
  if (status === "completed") return 1;
  if (status === "game_over") return 0;
  return null;
}

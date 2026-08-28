/**
 * Shared live-eval projector for Craftax and Harbor.
 * Missing reward / usage / score stay null. Control envelopes are not evidence.
 */

import { formatMissingNumber, isControlEnvelope, type LiveEnvelope } from "./liveStream.ts";

export type LiveEvalProjection = {
  events: LiveEnvelope[];
  kinds: string[];
  has_live_frames: boolean;
  has_reward_txt: boolean;
  reward: number | null;
  usage: Record<string, number | null> | null;
  cutoff_sequence: number | null;
};

const FORBIDDEN_BLOBS = ["collector", "capability_blob", "capabilities_blob"] as const;

function envelopeSequence(event: LiveEnvelope): number | null {
  const raw = event.sequence_number ?? event.sequence;
  if (typeof raw === "number" && Number.isFinite(raw)) return raw;
  if (typeof raw === "string" && raw.length > 0 && Number.isFinite(Number(raw))) return Number(raw);
  return null;
}

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
  cutoffSequence?: number
): LiveEvalProjection {
  const rows: LiveEnvelope[] = [];
  for (const event of events) {
    // `isControlEnvelope` is the one control predicate; it honours the explicit
    // `control: true` flag, so the projector and the ingest fold agree.
    if (isControlEnvelope(event)) continue;
    const seq = envelopeSequence(event);
    if (cutoffSequence != null && seq != null && seq > cutoffSequence) continue;
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
    cutoff_sequence: cutoffSequence ?? null
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

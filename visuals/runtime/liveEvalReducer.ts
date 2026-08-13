/**
 * Shared live-eval projector for Craftax, Harbor, and dig.bench.
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

export type DigbenchLaneProjection = {
  harness: string | null;
  config: string | null;
  label: string;
  evidence_class: "stub" | "live_basic" | "live_codex_exec" | "live_codex_mcp" | "unknown";
  actions: number;
  invalid_actions: number;
  mcp_calls: number;
  unique_observations: number;
  levels_beaten: number | null;
  applied_moves: number;
  command_authority_passed: boolean | null;
  malformed_commands: number;
};

const FORBIDDEN_BLOBS = ["collector", "capability_blob", "capabilities_blob", "DIGBENCH_API_TOKEN"] as const;

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
    if (isControlEnvelope(event) || event.control === true) continue;
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

/** `/reward` mapping for dig.bench env status. Incomplete stays null. */
export function rewardFromEnvStatus(status: string | null | undefined): number | null {
  if (status === "completed") return 1;
  if (status === "game_over") return 0;
  return null;
}

/** Observable harness identity and diagnostics for one dig.bench lane. */
export function projectDigbenchLane(events: LiveEnvelope[]): DigbenchLaneProjection {
  const semantic = events.filter((event) => !isControlEnvelope(event) && event.control !== true);
  const opened = semantic.find((event) => String(event.kind ?? event.type) === "trace.opened");
  const openedPayload = (opened?.payload ?? {}) as Record<string, unknown>;
  const policyRef =
    openedPayload.policy_ref && typeof openedPayload.policy_ref === "object"
      ? (openedPayload.policy_ref as Record<string, unknown>)
      : {};
  const actionRows = semantic.filter((event) => String(event.kind ?? event.type) === "action");
  const actionPayload = (actionRows[0]?.payload ?? {}) as Record<string, unknown>;
  const harness =
    typeof policyRef.harness === "string"
      ? policyRef.harness
      : typeof actionPayload.harness === "string"
        ? actionPayload.harness
        : null;
  const config = typeof policyRef.config === "string" ? policyRef.config : null;
  const mcpOpened = semantic.filter((event) => String(event.kind ?? event.type) === "span.mcp.opened");
  const simulated = semantic.some((event) => {
    const payload = (event.payload ?? {}) as Record<string, unknown>;
    return payload.evidence_class === "simulated" || String(payload.action_authority ?? "").includes("stub");
  });
  let evidence_class: DigbenchLaneProjection["evidence_class"] = "unknown";
  if (simulated) evidence_class = "stub";
  else if (harness === "codex" && mcpOpened.length > 0) evidence_class = "live_codex_mcp";
  else if (
    harness === "codex" &&
    actionRows.some((event) => String(event.payload.action_authority ?? "") === "codex_exec_live")
  ) evidence_class = "live_codex_exec";
  else if (harness && actionRows.length > 0) evidence_class = "live_basic";
  const observations = semantic
    .filter((event) => String(event.kind ?? event.type) === "observation")
    .map((event) => {
      const payload = (event.payload ?? {}) as Record<string, unknown>;
      return typeof payload.text === "string" ? payload.text.trim() : JSON.stringify(payload.raw ?? payload);
    })
    .filter(Boolean);
  const summaryRow = [...semantic]
    .reverse()
    .find((event) => String(event.kind ?? event.type) === "trace.summary");
  const summary = (summaryRow?.payload ?? {}) as Record<string, unknown>;
  const summaryNumber = (key: string): number | null => {
    const value = summary[key];
    return typeof value === "number" && Number.isFinite(value) ? value : null;
  };
  const authority = summary.command_authority_passed;
  const displayHarness = harness === "codex" ? "Codex" : harness === "react_legal_actions" || harness === "react" ? "Basic" : harness;
  return {
    harness,
    config,
    label: [displayHarness, config].filter(Boolean).join(" · ") || "unidentified harness",
    evidence_class,
    actions: summaryNumber("applied_moves") ?? actionRows.length,
    invalid_actions:
      summaryNumber("locally_rejected_illegal_attempts") ??
      semantic.filter((event) => String(event.kind ?? event.type) === "invalid_action").length,
    mcp_calls: mcpOpened.length,
    unique_observations: new Set(observations).size,
    levels_beaten: summaryNumber("levels_beaten"),
    applied_moves: summaryNumber("applied_moves") ?? actionRows.length,
    command_authority_passed: typeof authority === "boolean" ? authority : null,
    malformed_commands: summaryNumber("malformed_local_commands") ?? 0,
  };
}

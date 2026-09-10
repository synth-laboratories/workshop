import type { LiveEvalEvent } from "./types.ts";

export type HarborTrialView = {
  key: string;
  instruction?: string;
  sandbox?: string;
  trialId?: string;
  status: "planned" | "launched" | "completed" | "verified" | "failed";
  reward?: number | null;
  verifierScript?: string;
};

const text = (value: unknown): string | undefined =>
  typeof value === "string" && value.trim() ? value : undefined;
const rewardValue = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

/** Fold only identity-bearing trial evidence; ambiguous events remain in the raw stream. */
export function foldHarborTrials(events: LiveEvalEvent[]): HarborTrialView[] {
  const trials = new Map<string, HarborTrialView>();
  const lanes = new Map<string, Set<string>>();
  for (const event of events) {
    if (!["trial.planned", "trial.prepared", "trial.launched", "env.episode.opened", "verifier", "trial.completed", "trial.failed"].includes(event.kind)) continue;
    const payload = event.payload ?? {};
    const lane = text(event.lane) ?? text(payload.rollout_id);
    const trialId = text(payload.trial_id) ?? text(payload.trialId) ?? text(payload.attempt_id);
    let key = trialId ? JSON.stringify([lane ?? "", trialId]) : undefined;
    if (!key && lane) {
      const candidates = lanes.get(lane);
      if (candidates && candidates.size > 1) continue;
      key = candidates?.values().next().value ?? JSON.stringify([lane, ""]);
    }
    if (!key) continue;
    if (lane) {
      const keys = lanes.get(lane) ?? new Set<string>();
      keys.add(key);
      lanes.set(lane, keys);
    }
    const previous = trials.get(key) ?? { key, trialId, status: "planned" as const };
    const next: HarborTrialView = {
      ...previous,
      instruction: text(payload.instruction) ?? text(payload.task) ?? previous.instruction,
      sandbox: text(payload.sandbox) ?? previous.sandbox,
    };
    if (event.kind === "trial.launched" || event.kind === "env.episode.opened") {
      if (next.status === "planned") next.status = "launched";
    } else if (event.kind === "verifier") {
      next.status = next.status === "failed" ? "failed" : "verified";
      next.verifierScript = text(payload.script) ?? next.verifierScript;
      next.reward = rewardValue(payload["reward.txt"]);
    } else if (event.kind === "trial.completed" || event.kind === "trial.failed") {
      next.status = event.kind === "trial.failed" ? "failed" : next.status === "verified" ? "verified" : "completed";
      if (next.reward === undefined) next.reward = rewardValue(payload.reward);
    }
    trials.set(key, next);
  }
  return [...trials.values()];
}

/** Latest measured skill values, separated by rollout and skill. */
export function harborSkillProgress(events: LiveEvalEvent[]) {
  const rows = new Map<string, { lane: string; skill: string; xp: number; xpPerMin: number | null; samples: number }>();
  for (const event of events) {
    if (event.kind !== "game.skill_sample") continue;
    const lane = text(event.lane) ?? text(event.payload.rollout_id);
    const skill = text(event.payload.skill);
    const xp = rewardValue(event.payload.xp);
    if (!lane || !skill || xp === null) continue;
    const key = JSON.stringify([lane, skill]);
    rows.set(key, { lane, skill, xp, xpPerMin: rewardValue(event.payload.xp_per_min), samples: (rows.get(key)?.samples ?? 0) + 1 });
  }
  return [...rows.values()];
}

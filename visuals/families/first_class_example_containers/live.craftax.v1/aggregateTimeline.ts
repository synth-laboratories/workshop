import type { LiveEvalEvent } from "../../../runtime/types.ts";
import { craftaxEventLane, craftaxEventSequence, craftaxRewardValue } from "./projectCraftax.ts";

type Json = Record<string, unknown>;

export type CraftaxTimelinePoint = {
  step: number;
  reward: number;
};

export type CraftaxAchievementMarker = {
  step: number;
  reward: number;
  name: string;
  icon: string;
};

export type CraftaxRolloutTimeline = {
  lane: string;
  points: CraftaxTimelinePoint[];
  achievements: CraftaxAchievementMarker[];
  terminalReward: number;
  terminalStep: number;
};

export type CraftaxTerminalTimelineFact = {
  lane: string;
  reward?: number;
  steps?: number;
};

function object(value: unknown): Json {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Json : {};
}

function finite(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function eventStep(event: LiveEvalEvent): number | undefined {
  const payload = object(event.payload);
  const readout = object(payload.readout);
  return finite(payload.step) ?? finite(payload.step_index) ?? finite(payload.env_steps) ?? finite(readout.env_steps);
}

function achievementNames(value: unknown): string[] {
  if (Array.isArray(value)) return value.map(String);
  return Object.entries(object(value)).filter(([, unlocked]) => Boolean(unlocked)).map(([name]) => name);
}

function eventAchievementNames(event: LiveEvalEvent): string[] {
  const payload = object(event.payload);
  const readout = object(payload.readout);
  const observation = object(readout.observation);
  if (event.kind === "achievement_unlocked") {
    const explicit = payload.achievement ?? payload.name ?? payload.achievement_name;
    return explicit == null ? [] : [String(explicit)];
  }
  if (event.kind !== "snapshot" && event.kind !== "observation") return [];
  return achievementNames(payload.achievements ?? readout.achievements ?? observation.achievements);
}

/** Small, semantically recognizable markers; the source trace remains the label authority. */
export function craftaxAchievementIcon(name: string): string {
  const normalized = name.toLowerCase();
  if (normalized.includes("pickaxe")) return "⛏";
  if (normalized.includes("sword")) return "⚔";
  if (normalized.includes("diamond")) return "💎";
  if (normalized.includes("iron")) return "⚙";
  if (normalized.includes("wood")) return "🪵";
  if (normalized.includes("stone")) return "◆";
  if (normalized.includes("sapling") || normalized.includes("plant")) return "🌱";
  if (normalized.includes("table") || normalized.includes("craft")) return "🛠";
  if (normalized.includes("furnace") || normalized.includes("fire")) return "🔥";
  if (normalized.includes("water") || normalized.includes("drink")) return "💧";
  if (normalized.includes("cow")) return "🐄";
  if (normalized.includes("zombie")) return "Z";
  if (normalized.includes("skeleton")) return "☠";
  return "✦";
}

function rewardAtStep(points: CraftaxTimelinePoint[], step: number): number {
  let reward = 0;
  for (const point of points) {
    if (point.step > step) break;
    reward = point.reward;
  }
  return reward;
}

function pushPoint(points: CraftaxTimelinePoint[], step: number, reward: number): void {
  const previous = points.at(-1);
  if (previous?.step === step) {
    previous.reward = reward;
    return;
  }
  points.push({ step, reward });
}

/**
 * Project every rollout onto one shared environment-step axis. Achievement
 * markers use only explicit unlocks or first-seen observation evidence.
 */
export function projectCraftaxAggregateTimeline(
  events: LiveEvalEvent[],
  lanes: string[],
  terminalFacts: CraftaxTerminalTimelineFact[] = []
): CraftaxRolloutTimeline[] {
  const facts = new Map(terminalFacts.map((fact) => [fact.lane, fact]));
  return lanes.map((lane) => {
    const laneEvents = events
      .filter((event) => craftaxEventLane(event) === lane)
      .map((event, index) => ({ event, index }))
      .sort((left, right) => craftaxEventSequence(left.event, left.index) - craftaxEventSequence(right.event, right.index))
      .map(({ event }) => event);
    const points: CraftaxTimelinePoint[] = [{ step: 0, reward: 0 }];
    const pendingAchievements: Array<{ step: number; name: string }> = [];
    const seenAchievements = new Set<string>();
    let step = 0;
    let reward = 0;

    for (const event of laneEvents) {
      step = Math.max(step, eventStep(event) ?? step);
      if (event.kind === "reward_signal") {
        const delta = craftaxRewardValue(event.payload);
        if (delta != null) {
          reward += delta;
          pushPoint(points, step, reward);
        }
      } else if (event.kind === "snapshot") {
        const total = finite(object(event.payload).total_reward);
        if (total != null) {
          reward = total;
          pushPoint(points, step, reward);
        }
      }
      for (const name of eventAchievementNames(event)) {
        if (seenAchievements.has(name)) continue;
        seenAchievements.add(name);
        pendingAchievements.push({ step, name });
      }
    }

    const terminal = facts.get(lane);
    const terminalStep = Math.max(step, terminal?.steps ?? 0);
    const terminalReward = terminal?.reward ?? reward;
    if (terminalStep > points.at(-1)!.step || terminalReward !== points.at(-1)!.reward) {
      pushPoint(points, terminalStep, terminalReward);
    }
    const achievements = pendingAchievements.map(({ step: achievementStep, name }) => ({
      step: achievementStep,
      reward: rewardAtStep(points, achievementStep),
      name,
      icon: craftaxAchievementIcon(name),
    }));
    return { lane, points, achievements, terminalReward, terminalStep };
  });
}

/** SVG path with horizontal-then-vertical segments for cumulative outcomes. */
export function craftaxStepPath(
  points: CraftaxTimelinePoint[],
  maxStep: number,
  minReward: number,
  maxReward: number,
  width = 760,
  height = 250
): string {
  if (!points.length) return "";
  const left = 42;
  const right = width - 18;
  const top = 22;
  const bottom = height - 34;
  const stepRange = Math.max(1, maxStep);
  const rewardRange = Math.max(1, maxReward - minReward);
  const x = (step: number) => left + (Math.max(0, step) / stepRange) * (right - left);
  const y = (reward: number) => bottom - ((reward - minReward) / rewardRange) * (bottom - top);
  let path = `M ${x(points[0].step).toFixed(1)} ${y(points[0].reward).toFixed(1)}`;
  for (const point of points.slice(1)) {
    path += ` H ${x(point.step).toFixed(1)} V ${y(point.reward).toFixed(1)}`;
  }
  return path;
}

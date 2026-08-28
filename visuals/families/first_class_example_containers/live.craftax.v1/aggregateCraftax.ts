import { projectAgentTurns } from "../../../runtime/agentTranscript.ts";
import type { LiveEvalEvent } from "../../../runtime/types.ts";
import {
  craftaxEventLane,
  environmentStepCount,
  projectCraftaxViewer
} from "./projectCraftax.ts";

export type CraftaxRolloutAggregate = {
  lane: string;
  reward?: number;
  steps: number;
  calls: number;
  tokens?: number;
  achievements: string[];
};

export type CraftaxRunAggregate = {
  rollouts: CraftaxRolloutAggregate[];
  rewardMean?: number;
  rewardMin?: number;
  rewardMax?: number;
  reportedRewards: number;
  totalSteps: number;
  minSteps: number;
  maxSteps: number;
  totalCalls: number;
  minCalls: number;
  maxCalls: number;
  totalTokens?: number;
  achievementNames: string[];
  achievementRollouts: number;
};

function finite(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function sum(values: number[]): number {
  return values.reduce((total, value) => total + value, 0);
}

/** Run-wide facts derived exclusively from the retained rollout journals. */
export function summarizeCraftaxRun(events: LiveEvalEvent[]): CraftaxRunAggregate {
  const overall = projectCraftaxViewer(events);
  const rollouts = overall.lanes.map((lane) => {
    const laneEvents = overall.ordered.filter((event) => craftaxEventLane(event) === lane);
    const projection = projectCraftaxViewer(laneEvents, lane);
    const calls = projectAgentTurns(projection.visibleEvents).calls;
    const tokenValues = calls.map((call) => finite(call.usage.total_tokens));
    const tokens = calls.length > 0 && tokenValues.every((value) => value !== undefined)
      ? sum(tokenValues as number[])
      : undefined;
    return {
      lane,
      reward: projection.reward,
      steps: environmentStepCount(projection.visibleEvents),
      calls: calls.length,
      tokens,
      achievements: projection.achievements
    };
  });
  const rewards = rollouts.flatMap((rollout) => rollout.reward == null ? [] : [rollout.reward]);
  const stepValues = rollouts.map((rollout) => rollout.steps);
  const callValues = rollouts.map((rollout) => rollout.calls);
  const tokenValues = rollouts.map((rollout) => rollout.tokens);
  const totalTokens = rollouts.length > 0 && tokenValues.every((value) => value !== undefined)
    ? sum(tokenValues as number[])
    : undefined;
  return {
    rollouts,
    rewardMean: rewards.length ? sum(rewards) / rewards.length : undefined,
    rewardMin: rewards.length ? Math.min(...rewards) : undefined,
    rewardMax: rewards.length ? Math.max(...rewards) : undefined,
    reportedRewards: rewards.length,
    totalSteps: sum(stepValues),
    minSteps: stepValues.length ? Math.min(...stepValues) : 0,
    maxSteps: stepValues.length ? Math.max(...stepValues) : 0,
    totalCalls: sum(callValues),
    minCalls: callValues.length ? Math.min(...callValues) : 0,
    maxCalls: callValues.length ? Math.max(...callValues) : 0,
    totalTokens,
    achievementNames: [...new Set(rollouts.flatMap((rollout) => rollout.achievements))].sort(),
    achievementRollouts: rollouts.filter((rollout) => rollout.achievements.length > 0).length
  };
}

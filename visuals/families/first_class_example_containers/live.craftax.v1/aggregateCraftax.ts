import { projectAgentTurns } from "../../../runtime/agentTranscript.ts";
import type { LiveEvalEvent } from "../../../runtime/types.ts";
import {
  craftaxEventLane,
  environmentStepCount,
  projectCraftaxViewer
} from "./projectCraftax.ts";

export type CraftaxRolloutAggregate = {
  lane: string;
  seed?: number;
  status?: string;
  reward?: number;
  steps?: number;
  calls?: number;
  tokens?: number;
  costUsd?: number;
  achievements: string[];
  achievementsReported: boolean;
};

export type CraftaxTerminalRollout = {
  lane: string;
  seed?: number;
  status: string;
  reward?: number;
  steps?: number;
  calls?: number;
  tokens?: number;
  costUsd?: number;
  achievements?: string[];
};

export type CraftaxRunAggregate = {
  rollouts: CraftaxRolloutAggregate[];
  rewardMean?: number;
  rewardMin?: number;
  rewardMax?: number;
  reportedRewards: number;
  totalSteps?: number;
  minSteps?: number;
  maxSteps?: number;
  reportedSteps: number;
  totalCalls?: number;
  minCalls?: number;
  maxCalls?: number;
  reportedCalls: number;
  totalTokens?: number;
  minTokens?: number;
  maxTokens?: number;
  reportedTokens: number;
  totalCostUsd?: number;
  knownCostUsd?: number;
  minCostUsd?: number;
  maxCostUsd?: number;
  reportedCosts: number;
  achievementNames: string[];
  totalAchievements?: number;
  minAchievements?: number;
  maxAchievements?: number;
  achievementRollouts: number;
  reportedAchievements: number;
};

function finite(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function sum(values: number[]): number {
  return values.reduce((total, value) => total + value, 0);
}

/**
 * Run-wide facts. During a live run the retained journal is the only source;
 * after terminal settlement, scored outcomes and terminal step/token facts
 * come from the terminal records. Journal calls remain useful replay evidence,
 * but are not silently promoted into provider-billed call totals.
 */
export function summarizeCraftaxRun(
  events: LiveEvalEvent[],
  terminalRollouts?: CraftaxTerminalRollout[]
): CraftaxRunAggregate {
  const overall = projectCraftaxViewer(events);
  const journalRollouts = new Map(overall.lanes.map((lane) => {
    const laneEvents = overall.ordered.filter((event) => craftaxEventLane(event) === lane);
    const projection = projectCraftaxViewer(laneEvents, lane);
    const calls = projectAgentTurns(projection.visibleEvents).calls;
    const tokenValues = calls.map((call) => finite(call.usage.total_tokens));
    const tokens = calls.length > 0 && tokenValues.every((value) => value !== undefined)
      ? sum(tokenValues as number[])
      : undefined;
    const costValues = calls.map((call) => finite(call.costUsd));
    const costUsd = calls.length > 0 && costValues.every((value) => value !== undefined)
      ? sum(costValues as number[])
      : undefined;
    return [lane, {
      lane,
      reward: projection.reward,
      steps: environmentStepCount(projection.visibleEvents),
      calls: calls.length,
      tokens,
      costUsd,
      achievements: projection.achievements,
      achievementsReported: true
    }] as const;
  }));
  const rollouts: CraftaxRolloutAggregate[] = terminalRollouts
    ? terminalRollouts.map((terminal) => {
        const journal = journalRollouts.get(terminal.lane);
        return {
          lane: terminal.lane,
          ...(terminal.seed != null ? { seed: terminal.seed } : {}),
          status: terminal.status,
          // Terminal omissions are meaningful. Never resurrect a provisional
          // reward from an earlier journal event after scoring has settled.
          ...(terminal.reward != null ? { reward: terminal.reward } : {}),
          ...(terminal.steps != null ? { steps: terminal.steps } : {}),
          ...(journal?.calls != null ? { calls: journal.calls } : terminal.calls != null ? { calls: terminal.calls } : {}),
          ...(terminal.tokens != null ? { tokens: terminal.tokens } : {}),
          ...(terminal.costUsd != null ? { costUsd: terminal.costUsd } : {}),
          achievements: terminal.achievements ?? [],
          achievementsReported: terminal.achievements !== undefined
        };
      })
    : [...journalRollouts.values()];
  const rewards = rollouts.flatMap((rollout) => rollout.reward == null ? [] : [rollout.reward]);
  const stepValues = rollouts.flatMap((rollout) => rollout.steps == null ? [] : [rollout.steps]);
  const callValues = rollouts.flatMap((rollout) => rollout.calls == null ? [] : [rollout.calls]);
  const tokenValues = rollouts.flatMap((rollout) => rollout.tokens == null ? [] : [rollout.tokens]);
  const costValues = rollouts.flatMap((rollout) => rollout.costUsd == null ? [] : [rollout.costUsd]);
  const achievementValues = rollouts.flatMap((rollout) => rollout.achievementsReported ? [rollout.achievements.length] : []);
  const totalTokens = rollouts.length > 0 && tokenValues.length === rollouts.length
    ? sum(tokenValues)
    : undefined;
  const totalCostUsd = rollouts.length > 0 && costValues.length === rollouts.length
    ? sum(costValues)
    : undefined;
  const achievementsReported = terminalRollouts
    ? terminalRollouts.filter((rollout) => rollout.achievements !== undefined).length
    : rollouts.length;
  return {
    rollouts,
    rewardMean: rewards.length ? sum(rewards) / rewards.length : undefined,
    rewardMin: rewards.length ? Math.min(...rewards) : undefined,
    rewardMax: rewards.length ? Math.max(...rewards) : undefined,
    reportedRewards: rewards.length,
    ...(stepValues.length ? { totalSteps: sum(stepValues), minSteps: Math.min(...stepValues), maxSteps: Math.max(...stepValues) } : {}),
    reportedSteps: stepValues.length,
    ...(callValues.length ? { totalCalls: sum(callValues), minCalls: Math.min(...callValues), maxCalls: Math.max(...callValues) } : {}),
    reportedCalls: callValues.length,
    totalTokens,
    ...(tokenValues.length ? { minTokens: Math.min(...tokenValues), maxTokens: Math.max(...tokenValues) } : {}),
    reportedTokens: tokenValues.length,
    totalCostUsd,
    ...(costValues.length ? { knownCostUsd: sum(costValues), minCostUsd: Math.min(...costValues), maxCostUsd: Math.max(...costValues) } : {}),
    reportedCosts: costValues.length,
    achievementNames: [...new Set(rollouts.flatMap((rollout) => rollout.achievements))].sort(),
    ...(achievementValues.length ? { totalAchievements: sum(achievementValues), minAchievements: Math.min(...achievementValues), maxAchievements: Math.max(...achievementValues) } : {}),
    achievementRollouts: rollouts.filter((rollout) => rollout.achievements.length > 0).length,
    reportedAchievements: achievementsReported
  };
}

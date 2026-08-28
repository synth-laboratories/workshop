import { projectAgentTurns } from "../../../runtime/agentTranscript.ts";
import type { LiveEvalEvent } from "../../../runtime/types.ts";
import {
  craftaxEventLane,
  environmentStepCount,
  projectCraftaxViewer
} from "./projectCraftax.ts";

export type CraftaxRolloutAggregate = {
  lane: string;
  status?: string;
  reward?: number;
  steps?: number;
  calls?: number;
  tokens?: number;
  achievements: string[];
};

export type CraftaxTerminalRollout = {
  lane: string;
  status: string;
  reward?: number;
  steps?: number;
  calls?: number;
  tokens?: number;
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
  reportedTokens: number;
  achievementNames: string[];
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
    return [lane, {
      lane,
      reward: projection.reward,
      steps: environmentStepCount(projection.visibleEvents),
      calls: calls.length,
      tokens,
      achievements: projection.achievements
    }] as const;
  }));
  const rollouts: CraftaxRolloutAggregate[] = terminalRollouts
    ? terminalRollouts.map((terminal) => {
        const journal = journalRollouts.get(terminal.lane);
        return {
          lane: terminal.lane,
          status: terminal.status,
          // Terminal omissions are meaningful. Never resurrect a provisional
          // reward from an earlier journal event after scoring has settled.
          ...(terminal.reward != null ? { reward: terminal.reward } : {}),
          ...(terminal.steps != null ? { steps: terminal.steps } : {}),
          ...(journal?.calls != null ? { calls: journal.calls } : terminal.calls != null ? { calls: terminal.calls } : {}),
          ...(terminal.tokens != null ? { tokens: terminal.tokens } : {}),
          achievements: terminal.achievements ?? []
        };
      })
    : [...journalRollouts.values()];
  const rewards = rollouts.flatMap((rollout) => rollout.reward == null ? [] : [rollout.reward]);
  const stepValues = rollouts.flatMap((rollout) => rollout.steps == null ? [] : [rollout.steps]);
  const callValues = rollouts.flatMap((rollout) => rollout.calls == null ? [] : [rollout.calls]);
  const tokenValues = rollouts.flatMap((rollout) => rollout.tokens == null ? [] : [rollout.tokens]);
  const totalTokens = rollouts.length > 0 && tokenValues.length === rollouts.length
    ? sum(tokenValues)
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
    reportedTokens: tokenValues.length,
    achievementNames: [...new Set(rollouts.flatMap((rollout) => rollout.achievements))].sort(),
    achievementRollouts: rollouts.filter((rollout) => rollout.achievements.length > 0).length,
    reportedAchievements: achievementsReported
  };
}

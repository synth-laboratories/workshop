type Json = Record<string, unknown>;

export type EvalAggregateV1 = {
  schemaVersion: "eval.aggregate.v1";
  runId: string;
  asOfSequence: number;
  projectionRevision: number;
  lifecycle: string;
  work: Json;
  evidence: Json;
  selection: string;
  meanReward: number | null;
  scoredTrials: number;
  evaluatorEvidence: number;
  traceCount: number;
  evidenceRefCount: number;
};

export type EvalAggregateWorkFacts = {
  rolloutCount: number | null;
  terminalCount: number;
  running: number;
  queued: number;
  failed: number;
  started: number;
};

export type EvalTerminalRolloutFact = {
  seed?: number;
  reward?: number;
  tokens?: number;
  achievements?: string[];
};

export type EvalTerminalFacts = {
  rewardMean: number | null;
  rewardMedian: number | null;
  rewardMin: number | null;
  rewardMax: number | null;
  scoredRollouts: number;
  runtimeTokens: number | null;
  reportedTokenRollouts: number;
  achievementOccurrences: Record<string, number>;
  reportedAchievementRollouts: number;
};

function object(value: unknown): Json | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Json : null;
}

function count(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : 0;
}

/** Accept only the revision-addressed backend aggregate for the bound run. */
export function evalAggregateV1(value: unknown, runId?: string | null): EvalAggregateV1 | null {
  const aggregate = object(value);
  if (
    aggregate?.schemaVersion !== "eval.aggregate.v1"
    || typeof aggregate.runId !== "string"
    || (runId && aggregate.runId !== runId)
    || typeof aggregate.projectionRevision !== "number"
    || typeof aggregate.asOfSequence !== "number"
    || !object(aggregate.work)
  ) {
    return null;
  }
  return aggregate as EvalAggregateV1;
}

/** Work counts are formatted from the canonical aggregate, never raw rows. */
export function evalAggregateWorkFacts(aggregate: EvalAggregateV1): EvalAggregateWorkFacts {
  const planned = typeof aggregate.work.planned === "number" && Number.isFinite(aggregate.work.planned)
    ? aggregate.work.planned
    : null;
  const running = count(aggregate.work.running);
  const queued = count(aggregate.work.queued);
  const failed = count(aggregate.work.failed) + count(aggregate.work.cancelled);
  const terminalCount = count(aggregate.work.succeeded) + failed;
  return {
    rolloutCount: planned,
    terminalCount,
    running,
    queued,
    failed,
    started: terminalCount + running
  };
}

/**
 * Aggregate only terminal record facts. Provider receipts deliberately do not
 * enter this projection: their token and cost totals describe billing, while
 * these values describe what the container runtime retained per rollout.
 */
export function evalTerminalFacts(rollouts: readonly EvalTerminalRolloutFact[]): EvalTerminalFacts {
  const rewards = rollouts
    .flatMap((rollout) => typeof rollout.reward === "number" && Number.isFinite(rollout.reward) ? [rollout.reward] : [])
    .sort((left, right) => left - right);
  const tokenValues = rollouts.flatMap((rollout) => (
    typeof rollout.tokens === "number" && Number.isFinite(rollout.tokens) ? [rollout.tokens] : []
  ));
  const achievementRows = rollouts.filter((rollout) => Array.isArray(rollout.achievements));
  const achievementOccurrences = achievementRows.reduce<Record<string, number>>((counts, rollout) => {
    for (const name of rollout.achievements ?? []) {
      counts[name] = (counts[name] ?? 0) + 1;
    }
    return counts;
  }, {});
  const midpoint = Math.floor(rewards.length / 2);
  const rewardMedian = rewards.length === 0
    ? null
    : rewards.length % 2
      ? rewards[midpoint]
      : (rewards[midpoint - 1] + rewards[midpoint]) / 2;
  return {
    rewardMean: rewards.length ? rewards.reduce((sum, value) => sum + value, 0) / rewards.length : null,
    rewardMedian,
    rewardMin: rewards[0] ?? null,
    rewardMax: rewards.at(-1) ?? null,
    scoredRollouts: rewards.length,
    runtimeTokens: rollouts.length > 0 && tokenValues.length === rollouts.length
      ? tokenValues.reduce((sum, value) => sum + value, 0)
      : null,
    reportedTokenRollouts: tokenValues.length,
    achievementOccurrences,
    reportedAchievementRollouts: achievementRows.length
  };
}

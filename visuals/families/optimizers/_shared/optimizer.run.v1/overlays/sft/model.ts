/** Pure SFT workspace derivations, importable from node tests. */

import type {
  ProjectedState,
  SftComparisonPair,
  SftCurationCandidate
} from "../../components/projectEvents.ts";
import type { WorkspaceStage } from "../../components/workspace/WorkspaceChrome.tsx";

export type SftState = NonNullable<ProjectedState["sft"]>;

export const SFT_TERMINAL_STATUSES = ["completed", "failed", "canceled", "cancelled", "succeeded"];

/** Derive the semantic SFT stages from what the event stream actually shows. */
export function sftStages(sft: SftState, status: string, promotedCheckpointId?: string): WorkspaceStage[] {
  const terminal = SFT_TERMINAL_STATUSES.includes(status);
  const failed = status === "failed";
  const baselineScored = (sft.baseline?.seeds.length ?? 0) > 0;
  const collected = sft.curation.collected ?? 0;
  const curationSettled = (sft.curation.accepted ?? 0) > 0;
  const datasetReady = Object.keys((sft.dataset.splits as Record<string, unknown> | undefined) ?? {}).length > 0;
  const trainingStarted = sft.points.length > 0;
  const checkpointCount = sft.checkpoints.length;
  const readyCount = sft.checkpoints.filter((ckpt) => ckpt.ready === true || ckpt.promoted === true).length;
  const campaignCount = sft.campaigns.length;
  const evaluationCount = campaignCount + sft.evaluations.length;
  const campaignsSettled = campaignCount > 0 && sft.campaigns.every((campaign) =>
    ["completed", "failed"].includes(String(campaign.status ?? ""))
  );
  const selected = promotedCheckpointId != null ||
    sft.checkpoints.some((ckpt) => ckpt.selected === true || ckpt.promoted === true);
  const upliftClaimed = sft.checkpoints.some((ckpt) => ckpt.promoted === true);
  const comparison = sftComparison(sft);
  const settle = (started: boolean, done: boolean): WorkspaceStage["status"] => {
    if (done) return "completed";
    if (started) return terminal ? (failed ? "failed" : "completed") : "active";
    return terminal ? "skipped" : "pending";
  };
  return [
    {
      id: "baseline",
      label: "Baseline",
      status: settle(baselineScored, baselineScored),
      detail: baselineScored ? `${sft.baseline?.seeds.length} seeds scored` : "unchanged student on frozen seeds"
    },
    {
      id: "collection",
      label: "Collection",
      status: settle(collected > 0, collected > 0 && curationSettled),
      detail: collected > 0 ? `${collected} teacher rollouts` : undefined
    },
    {
      id: "curation",
      label: "Curation",
      status: settle((sft.curation.considered ?? 0) > 0, curationSettled),
      detail: sft.curation.accepted != null && sft.curation.considered != null
        ? `${sft.curation.accepted}/${sft.curation.considered} retained`
        : undefined
    },
    { id: "dataset", label: "Dataset", status: datasetReady ? "completed" : terminal ? "skipped" : "pending" },
    {
      id: "training",
      label: "Training",
      status: settle(trainingStarted, trainingStarted && terminal && !failed),
      detail: trainingStarted ? `${sft.points.length} metric records` : undefined
    },
    {
      id: "checkpoints",
      label: "Checkpoints",
      status: settle(checkpointCount > 0, checkpointCount > 0 && readyCount === checkpointCount && terminal),
      detail: checkpointCount > 0 ? `${readyCount}/${checkpointCount} ready` : undefined
    },
    {
      id: "evaluation",
      label: "Evaluations",
      status: settle(evaluationCount > 0, terminal && evaluationCount > 0 && (campaignCount === 0 || campaignsSettled)),
      detail: evaluationCount > 0
        ? `${sft.evaluations.length} result${sft.evaluations.length === 1 ? "" : "s"}${campaignCount > 0 ? ` · ${campaignCount} rollout campaign${campaignCount === 1 ? "" : "s"}` : ""}`
        : undefined
    },
    {
      id: "promotion",
      label: "Selection",
      status: selected ? "completed" : terminal ? "skipped" : "pending",
      detail: selected
        ? (upliftClaimed ? "uplift claimed" : "selected · no measured improvement")
        : "requires an explicit select/promote event — checkpoint 'ready' is not promotion"
    },
    {
      id: "heldout",
      label: "Heldout comparison",
      status: settle(comparison != null, comparison != null && comparison.paired > 0),
      detail: comparison
        ? `${comparison.paired} paired seeds`
        : "base vs promoted on untouched seeds — the only evidence for an uplift claim"
    }
  ];
}

/* ── Paired heldout comparison ───────────────────────────────────────────
   Every statistic below is computed over seeds where BOTH arms returned an
   authoritative reward. Seeds missing either side are counted and surfaced,
   never imputed as zero — an unmeasured rollout is not a failed rollout. */

export type SftComparison = {
  /** Seeds where both arms reported a reward. All statistics use these. */
  paired: number;
  /** Seeds present in the split but missing a reward on one or both arms. */
  unpaired: number;
  baseLabel: string;
  trainedLabel: string;
  splitDigest?: string;
  baseMean: number | null;
  trainedMean: number | null;
  baseMedian: number | null;
  trainedMedian: number | null;
  baseSd: number | null;
  trainedSd: number | null;
  baseSuccessRate: number | null;
  trainedSuccessRate: number | null;
  /** trained − base over paired seeds. */
  absoluteUplift: number | null;
  /** Relative to the base mean; null when the base mean is zero or missing. */
  relativeUplift: number | null;
  /** 95% CI of the paired mean difference (Student t). */
  upliftCi: [number, number] | null;
  wins: number;
  losses: number;
  ties: number;
  baseAchievements: string[];
  trainedAchievements: string[];
  achievementsGained: string[];
  achievementsLost: string[];
  baseMeanSteps: number | null;
  trainedMeanSteps: number | null;
  rows: SftComparisonRow[];
};

export type SftComparisonRow = {
  seed: string;
  baseReward: number | null;
  trainedReward: number | null;
  delta: number | null;
  outcome: "win" | "loss" | "tie" | "unpaired";
};

function mean(values: number[]): number | null {
  return values.length === 0 ? null : values.reduce((sum, value) => sum + value, 0) / values.length;
}

function median(values: number[]): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0 ? (sorted[mid - 1] + sorted[mid]) / 2 : sorted[mid];
}

/** Sample standard deviation. Undefined for n < 2. */
function stdev(values: number[]): number | null {
  if (values.length < 2) return null;
  const avg = mean(values) as number;
  const variance = values.reduce((sum, value) => sum + (value - avg) ** 2, 0) / (values.length - 1);
  return Math.sqrt(variance);
}

/** Two-sided t critical values at 95%, indexed by degrees of freedom. */
const T95: Record<number, number> = {
  1: 12.706, 2: 4.303, 3: 3.182, 4: 2.776, 5: 2.571, 6: 2.447, 7: 2.365, 8: 2.306,
  9: 2.262, 10: 2.228, 11: 2.201, 12: 2.179, 13: 2.160, 14: 2.145, 15: 2.131,
  16: 2.120, 17: 2.110, 18: 2.101, 19: 2.093, 20: 2.086, 21: 2.080, 22: 2.074,
  23: 2.069, 24: 2.064, 25: 2.060, 26: 2.056, 27: 2.052, 28: 2.048, 29: 2.045
};

function tCritical(df: number): number {
  if (df <= 0) return Number.NaN;
  return T95[df] ?? 1.96;
}

/**
 * Reduce the projected pairs into the report §10 requires. Returns null when
 * no heldout comparison has been emitted — the caller must then say so rather
 * than render an empty table that looks like a measured null result.
 */
export function sftComparison(sft: SftState): SftComparison | null {
  const comparison = sft.comparison;
  if (!comparison || comparison.pairs.length === 0) return null;

  const rows: SftComparisonRow[] = comparison.pairs.map((pair) => {
    const baseReward = pair.base?.reward ?? null;
    const trainedReward = pair.trained?.reward ?? null;
    if (baseReward == null || trainedReward == null) {
      return { seed: pair.seed, baseReward, trainedReward, delta: null, outcome: "unpaired" as const };
    }
    const delta = trainedReward - baseReward;
    return {
      seed: pair.seed,
      baseReward,
      trainedReward,
      delta,
      outcome: delta > 0 ? ("win" as const) : delta < 0 ? ("loss" as const) : ("tie" as const)
    };
  });

  const pairedRows = rows.filter((row) => row.outcome !== "unpaired");
  const baseRewards = pairedRows.map((row) => row.baseReward as number);
  const trainedRewards = pairedRows.map((row) => row.trainedReward as number);
  const deltas = pairedRows.map((row) => row.delta as number);

  const baseMean = mean(baseRewards);
  const trainedMean = mean(trainedRewards);
  const absoluteUplift = baseMean != null && trainedMean != null ? trainedMean - baseMean : null;

  const deltaSd = stdev(deltas);
  const upliftCi: [number, number] | null =
    absoluteUplift != null && deltaSd != null && deltas.length > 1
      ? (() => {
          const margin = tCritical(deltas.length - 1) * (deltaSd / Math.sqrt(deltas.length));
          return [absoluteUplift - margin, absoluteUplift + margin] as [number, number];
        })()
      : null;

  const pairedSeeds = new Set(pairedRows.map((row) => row.seed));
  const achievementsOf = (pick: (pair: SftComparisonPair) => string[] | undefined) =>
    [...new Set(
      comparison.pairs
        .filter((pair) => pairedSeeds.has(pair.seed))
        .flatMap((pair) => pick(pair) ?? [])
    )].sort();
  const baseAchievements = achievementsOf((pair) => pair.base?.achievements);
  const trainedAchievements = achievementsOf((pair) => pair.trained?.achievements);

  const stepsOf = (pick: (pair: SftComparisonPair) => number | undefined) =>
    mean(
      comparison.pairs
        .filter((pair) => pairedSeeds.has(pair.seed))
        .map(pick)
        .filter((value): value is number => typeof value === "number")
    );

  // "Success" is a strictly positive authoritative reward. Craftax pays nothing
  // for an episode that achieved nothing, so zero is a real failure here.
  const successRate = (values: number[]) =>
    values.length === 0 ? null : values.filter((value) => value > 0).length / values.length;

  return {
    paired: pairedRows.length,
    unpaired: rows.length - pairedRows.length,
    baseLabel: comparison.baseLabel,
    trainedLabel: comparison.trainedLabel,
    splitDigest: comparison.splitDigest,
    baseMean,
    trainedMean,
    baseMedian: median(baseRewards),
    trainedMedian: median(trainedRewards),
    baseSd: stdev(baseRewards),
    trainedSd: stdev(trainedRewards),
    baseSuccessRate: successRate(baseRewards),
    trainedSuccessRate: successRate(trainedRewards),
    absoluteUplift,
    relativeUplift:
      absoluteUplift != null && baseMean != null && baseMean !== 0 ? absoluteUplift / Math.abs(baseMean) : null,
    upliftCi,
    wins: pairedRows.filter((row) => row.outcome === "win").length,
    losses: pairedRows.filter((row) => row.outcome === "loss").length,
    ties: pairedRows.filter((row) => row.outcome === "tie").length,
    baseAchievements,
    trainedAchievements,
    achievementsGained: trainedAchievements.filter((name) => !baseAchievements.includes(name)),
    achievementsLost: baseAchievements.filter((name) => !trainedAchievements.includes(name)),
    baseMeanSteps: stepsOf((pair) => pair.base?.steps),
    trainedMeanSteps: stepsOf((pair) => pair.trained?.steps),
    rows
  };
}

/* ── Baseline ─────────────────────────────────────────────────────────── */

export type SftDistribution = {
  n: number;
  scored: number;
  missing: number;
  mean: number | null;
  median: number | null;
  sd: number | null;
  min: number | null;
  max: number | null;
};

export function sftDistribution(rewards: Array<number | null>): SftDistribution {
  const scored = rewards.filter((value): value is number => value != null);
  return {
    n: rewards.length,
    scored: scored.length,
    missing: rewards.length - scored.length,
    mean: mean(scored),
    median: median(scored),
    sd: stdev(scored),
    min: scored.length ? Math.min(...scored) : null,
    max: scored.length ? Math.max(...scored) : null
  };
}

/* ── Curation ─────────────────────────────────────────────────────────── */

export type SftCurationFunnel = {
  steps: Array<{ id: string; label: string; count: number | null }>;
  acceptanceRate: number | null;
  topRejections: Array<{ reason: string; count: number }>;
  seedsCovered: number | null;
  achievementsCovered: string[];
  accepted: SftCurationCandidate[];
  rejected: SftCurationCandidate[];
};

export function sftCurationFunnel(sft: SftState): SftCurationFunnel {
  const { collected, considered, accepted } = sft.curation;
  return {
    steps: [
      { id: "collected", label: "Teacher rollouts sealed", count: collected },
      { id: "considered", label: "Candidates ranked", count: considered },
      { id: "accepted", label: "Trajectories retained", count: accepted }
    ],
    acceptanceRate:
      accepted != null && considered != null && considered > 0 ? accepted / considered : null,
    topRejections: Object.entries(sft.curation.rejectionsByReason)
      .map(([reason, count]) => ({ reason, count }))
      .sort((a, b) => b.count - a.count),
    seedsCovered: sft.curation.seedsCovered,
    achievementsCovered: sft.curation.achievementsCovered,
    accepted: sft.curation.candidates.filter((candidate) => candidate.accepted),
    rejected: sft.curation.candidates.filter((candidate) => !candidate.accepted)
  };
}

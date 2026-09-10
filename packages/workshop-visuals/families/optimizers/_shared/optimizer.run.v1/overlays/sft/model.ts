/** Pure SFT workspace derivations, importable from node tests. */

import type {
  ProjectedState,
  SftComparisonPair,
  SftCurationCandidate
} from "../../components/projectEvents.ts";
import type { WorkspaceStage } from "../../components/workspace/WorkspaceChrome.tsx";

export type SftState = NonNullable<ProjectedState["sft"]>;

export const SFT_TERMINAL_STATUSES = ["completed", "failed", "canceled", "cancelled", "succeeded"];

export type SftAggregateBaseline = {
  checkpointId?: string;
  metric: string;
  score: number;
  n: number | null;
};

/** Collapse duplicate summary aliases while retaining the newest payload. */
export function sftDistinctEvaluations(sft: SftState): Array<Record<string, unknown>> {
  const byIdentity = new Map<string, Record<string, unknown>>();
  for (const evaluation of sft.evaluations) {
    const key = [
      evaluation.role ?? evaluation.split ?? evaluation.phase,
      evaluation.checkpoint_id ?? evaluation.checkpointId,
      evaluation.step,
      evaluation.metric,
      evaluation.score ?? evaluation.accuracy ?? evaluation.calibration_accuracy,
      evaluation.n ?? evaluation.sampleCount ?? evaluation.sample_count
    ].map((value) => String(value ?? "")).join("|");
    byIdentity.set(key, evaluation);
  }
  return [...byIdentity.values()];
}

/**
 * Public SFT services may report the unchanged student as one aggregate
 * selection evaluation instead of hundreds of `baseline_rollout` events.
 * Preserve that evidence as an aggregate; never manufacture per-example rows.
 *
 * The matched record is by construction a `selection`-role evaluation, so this
 * score belongs to the selection split. It is not comparable with the base arm
 * of `sftHeldoutSummary`, which is measured on the locked heldout split: the
 * two are expected to differ, and rendering either in place of the other would
 * misreport what a run's uplift was measured against.
 */
export function sftAggregateBaseline(sft: SftState): SftAggregateBaseline | null {
  const evaluation = [...sft.evaluations].reverse().find((candidate) => {
    const role = String(candidate.role ?? candidate.split ?? candidate.phase ?? "");
    const candidateName = String(candidate.candidate ?? "");
    const checkpointId = String(candidate.checkpoint_id ?? candidate.checkpointId ?? "");
    const step = Number(candidate.step);
    const score = Number(candidate.score ?? candidate.accuracy ?? candidate.calibration_accuracy);
    return role === "selection"
      && step === 0
      && (candidateName === "base" || checkpointId.startsWith("inference-0-"))
      && Number.isFinite(score);
  });
  if (!evaluation) return null;
  const n = Number(evaluation.n ?? evaluation.sampleCount ?? evaluation.sample_count);
  const checkpointId = evaluation.checkpoint_id ?? evaluation.checkpointId;
  return {
    checkpointId: typeof checkpointId === "string" ? checkpointId : undefined,
    metric: String(evaluation.metric ?? (evaluation.accuracy != null ? "accuracy" : "score")),
    score: Number(evaluation.score ?? evaluation.accuracy ?? evaluation.calibration_accuracy),
    n: Number.isFinite(n) ? n : null
  };
}

/** A stale admitted-run status must not override newer streamed work evidence. */
export function sftEffectiveStatus(sft: SftState, reportedStatus: string): string {
  if (SFT_TERMINAL_STATUSES.includes(reportedStatus)) return reportedStatus;
  const workObserved = sft.points.length > 0
    || sft.evaluations.length > 0
    || sft.checkpoints.length > 0
    || sft.campaigns.length > 0;
  return reportedStatus === "queued" && workObserved ? "running" : reportedStatus;
}

/** Derive the semantic SFT stages from what the event stream actually shows. */
export function sftStages(sft: SftState, status: string, promotedCheckpointId?: string): WorkspaceStage[] {
  const terminal = SFT_TERMINAL_STATUSES.includes(status);
  const failed = status === "failed";
  const aggregateBaseline = sftAggregateBaseline(sft);
  const baselineScored = (sft.baseline?.seeds.length ?? 0) > 0 || aggregateBaseline != null;
  const collected = sft.curation.collected ?? 0;
  const curationSettled = (sft.curation.accepted ?? 0) > 0;
  const datasetReady = Object.keys((sft.dataset.splits as Record<string, unknown> | undefined) ?? {}).length > 0;
  const trainingStarted = sft.points.length > 0;
  const checkpointCount = sft.checkpoints.length;
  const readyCount = sft.checkpoints.filter((ckpt) => ckpt.ready === true || ckpt.promoted === true).length;
  const campaignCount = sft.campaigns.length;
  const distinctEvaluationCount = sftDistinctEvaluations(sft).length;
  const evaluationCount = campaignCount + distinctEvaluationCount;
  const campaignsSettled = campaignCount > 0 && sft.campaigns.every((campaign) =>
    ["completed", "failed"].includes(String(campaign.status ?? ""))
  );
  const selected = promotedCheckpointId != null ||
    sft.checkpoints.some((ckpt) => ckpt.selected === true || ckpt.promoted === true);
  const comparison = sftComparison(sft);
  const heldoutSummary = sftHeldoutSummary(sft);
  const upliftClaimed = sft.checkpoints.some((ckpt) => ckpt.promoted === true)
    || heldoutSummary?.claimReady === true;
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
      detail: (sft.baseline?.seeds.length ?? 0) > 0
        ? `${sft.baseline?.seeds.length} seeds scored`
        : aggregateBaseline
          ? `${aggregateBaseline.n ?? "—"} selection examples · ${aggregateBaseline.metric} ${aggregateBaseline.score}`
          : "unchanged student on frozen seeds"
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
        ? `${distinctEvaluationCount} result${distinctEvaluationCount === 1 ? "" : "s"}${campaignCount > 0 ? ` · ${campaignCount} rollout campaign${campaignCount === 1 ? "" : "s"}` : ""}`
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
      status: settle(
        comparison != null || heldoutSummary != null,
        (comparison?.paired ?? heldoutSummary?.paired ?? 0) > 0
      ),
      detail: comparison || heldoutSummary
        ? `${comparison?.paired ?? heldoutSummary?.paired} paired examples`
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

export type SftHeldoutSummary = {
  paired: number;
  baseScore: number | null;
  trainedScore: number | null;
  absoluteUplift: number;
  upliftCi: [number, number] | null;
  verdict?: string;
  claimReady: boolean;
  checkpointId?: string;
};

/**
 * Aggregate paired result emitted by classification trainers, measured on the
 * locked heldout split — a different split from `sftAggregateBaseline`.
 */
export function sftHeldoutSummary(sft: SftState): SftHeldoutSummary | null {
  const evaluation = [...sft.evaluations].reverse().find((candidate) => {
    const phase = String(candidate.phase ?? candidate.role ?? candidate.split ?? "");
    return phase === "heldout" && Number(candidate.pairedN ?? candidate.paired_n ?? 0) > 0;
  });
  if (!evaluation) return null;
  const uplift = Number(evaluation.delta);
  const trainedScore = Number(evaluation.score);
  const paired = Number(evaluation.pairedN ?? evaluation.paired_n);
  if (!Number.isFinite(uplift) || !Number.isFinite(paired) || paired <= 0) return null;
  const ciLow = Number(evaluation.ciLow ?? evaluation.ci_low);
  const ciHigh = Number(evaluation.ciHigh ?? evaluation.ci_high);
  return {
    paired,
    // Reconstructed, because producers report the selected checkpoint's score
    // and the paired uplift but not the base arm's own score. Recomputing it
    // keeps the arm from disappearing from the comparison; it does not make it
    // an independently reported measurement, and any surface showing it has to
    // say which split it came from.
    baseScore: Number.isFinite(trainedScore) ? trainedScore - uplift : null,
    trainedScore: Number.isFinite(trainedScore) ? trainedScore : null,
    absoluteUplift: uplift,
    upliftCi: Number.isFinite(ciLow) && Number.isFinite(ciHigh) ? [ciLow, ciHigh] : null,
    verdict: typeof evaluation.verdict === "string" ? evaluation.verdict : undefined,
    claimReady: evaluation.claimReady === true || evaluation.claim_ready === true,
    checkpointId: typeof evaluation.checkpointId === "string"
      ? evaluation.checkpointId
      : typeof evaluation.checkpoint_id === "string"
        ? evaluation.checkpoint_id
        : undefined
  };
}

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

/**
 * One place for "this has not happened yet".
 *
 * Every phase panel used to carry its own paragraph explaining what was missing
 * and why it mattered. Four such paragraphs pushed the evidence that did exist
 * below the fold, and the repetition read as failure rather than as sequence.
 * The workspace now states each prerequisite once, in order, and each panel
 * keeps a single short line.
 */
export type SftPrerequisite = {
  id: "baseline" | "collection" | "training" | "evaluation" | "heldout";
  label: string;
  /** Why the run cannot claim uplift without it. Shown once, in the checklist. */
  why: string;
};

export function sftMissingPrerequisites(sft: SftState): SftPrerequisite[] {
  const missing: SftPrerequisite[] = [];
  const funnel = sftCurationFunnel(sft);
  const hasBaseline = (sft.baseline?.seeds.length ?? 0) > 0 || sftAggregateBaseline(sft) != null;
  // Direct supervised recipes may begin from an already versioned corpus.
  // In that case teacher collection is not missing evidence: the dataset
  // digest is the provenance boundary and collection/curation are genuinely
  // not applicable to this run.
  const hasVersionedDataset = typeof sft.dataset.digest === "string" && sft.dataset.digest.length > 0;
  const hasCollection = hasVersionedDataset
    || funnel.steps.some((step) => step.count != null)
    || funnel.accepted.length > 0;
  const hasTraining = sft.points.length > 0;
  const hasEvaluations = sftDistinctEvaluations(sft).length > 0;
  const hasHeldout = sftComparison(sft) != null || sftHeldoutSummary(sft) != null;
  if (!hasBaseline) {
    missing.push({
      id: "baseline",
      label: "Baseline evaluation of the unchanged student",
      why: "There is nothing to measure uplift against until the untrained student is scored on the frozen baseline seeds."
    });
  }
  if (!hasCollection) {
    missing.push({
      id: "collection",
      label: "Teacher collection and curation decisions",
      why: "Trajectories must be sealed and then accepted or rejected with an explicit reason before a dataset can claim provenance."
    });
  }
  if (!hasTraining) {
    missing.push({
      id: "training",
      label: "Training metrics",
      why: "Loss and step records begin once the training job reports its first step."
    });
  }
  if (!hasEvaluations) {
    missing.push({
      id: "evaluation",
      label: "Checkpoint evaluations",
      why: "Selection needs at least one scored checkpoint. Selection retains a checkpoint; it is not an uplift claim."
    });
  }
  if (!hasHeldout) {
    missing.push({
      id: "heldout",
      label: "Paired base-vs-selected heldout run",
      why: "Training completing, a checkpoint reaching ready, and even a promotion decision are not uplift. Only the paired heldout comparison can license the claim."
    });
  }
  return missing;
}

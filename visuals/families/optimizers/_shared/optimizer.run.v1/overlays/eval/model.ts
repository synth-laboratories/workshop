/**
 * Derived view model for an eval run.
 *
 * The whole point of eval is that a number is only worth reading when the
 * evidence behind it held, so this module never invents one. A candidate with
 * no valid trial has a `null` mean, not a zero; a stage with no evidence is
 * pending, not passed; and orchestration progress is kept strictly separate
 * from whether anything was promoted.
 */

import type { EvalScorecard, EvalState, EvalTrial } from "../../components/projectEvents.ts";

export type EvalStageId = "plan" | "screen" | "prune" | "confirm" | "select";
export type EvalStageStatus = "pending" | "active" | "completed" | "skipped" | "failed";

export type EvalStage = {
  id: EvalStageId;
  label: string;
  status: EvalStageStatus;
  detail?: string;
};

export const EVAL_TERMINAL_STATUSES = ["completed", "failed", "cancelled", "canceled"];

/** Selection outcomes, and whether each one licenses a promotion claim. */
export const SELECTION_TONE: Record<string, { tone: "ok" | "warn" | "bad"; label: string }> = {
  promoted: { tone: "ok", label: "Promoted" },
  no_champion: { tone: "warn", label: "No champion" },
  inconclusive: { tone: "warn", label: "Inconclusive" },
  invalid_evidence: { tone: "bad", label: "Invalid evidence" }
};

export function metricMean(card: EvalScorecard | undefined, metric: string): number | null {
  if (!card) return null;
  return card.metrics.find((entry) => entry.metric === metric)?.mean ?? null;
}

/** Trials for one stage, oldest id first. */
export function trialsForStage(state: EvalState, stage: string): EvalTrial[] {
  return state.trials.filter((trial) => trial.stage === stage);
}

export function scorecardsForStage(state: EvalState, stage: string): EvalScorecard[] {
  return state.scorecards.filter((card) => card.stage === stage);
}

/**
 * The matrix as counts. `valid` is the only population a decision may use;
 * `failed` is retained separately so a rig problem never reads as a bad policy.
 */
export function trialCounts(state: EvalState): {
  planned: number;
  terminal: number;
  valid: number;
  failed: number;
  running: number;
  queued: number;
} {
  let terminal = 0;
  let valid = 0;
  let failed = 0;
  let running = 0;
  let queued = 0;
  for (const trial of state.trials) {
    if (trial.status === "running") running += 1;
    else if (trial.status === "queued") queued += 1;
    else {
      terminal += 1;
      if (trial.valid) valid += 1;
      else failed += 1;
    }
  }
  return { planned: state.plannedTrials, terminal, valid, failed, running, queued };
}

export function evalStages(
  state: EvalState | undefined,
  runStatus: string
): EvalStage[] {
  const terminal = EVAL_TERMINAL_STATUSES.includes(runStatus);
  const failed = runStatus === "failed";
  const blank: EvalStage[] = [
    { id: "plan", label: "Plan", status: "pending" },
    { id: "screen", label: "Screen", status: "pending" },
    { id: "prune", label: "Prune", status: "pending" },
    { id: "confirm", label: "Confirm", status: "pending" },
    { id: "select", label: "Select", status: "pending" }
  ];
  if (!state) return blank;

  const screenTrials = trialsForStage(state, "screen");
  const confirmTrials = trialsForStage(state, "confirm");
  const screenCards = scorecardsForStage(state, "screen");
  const confirmCards = scorecardsForStage(state, "confirm");
  const eliminated = screenCards.filter((card) => card.eliminationReason);
  const hasConfirmSeeds = (state.seedLedger?.confirmation.length ?? 0) > 0;

  const settle = (
    id: EvalStageId,
    label: string,
    started: boolean,
    done: boolean,
    detail?: string
  ): EvalStage => {
    if (done) return { id, label, status: "completed", detail };
    if (started) {
      return { id, label, status: terminal ? (failed ? "failed" : "completed") : "active", detail };
    }
    return { id, label, status: terminal ? "skipped" : "pending", detail };
  };

  const screenDone = screenTrials.length > 0 && screenCards.length > 0
    && screenTrials.every((trial) => trial.status !== "queued" && trial.status !== "running");
  const confirmDone = confirmTrials.length > 0 && confirmCards.length > 0
    && confirmTrials.every((trial) => trial.status !== "queued" && trial.status !== "running");

  return [
    settle(
      "plan",
      "Plan",
      state.plannedTrials > 0,
      state.seedLedger !== null,
      state.plannedTrials > 0 ? `${state.plannedTrials} trials` : undefined
    ),
    settle(
      "screen",
      "Screen",
      screenTrials.length > 0,
      screenDone,
      screenTrials.length > 0 ? `${screenTrials.length} trials` : undefined
    ),
    // Pruning is only a stage when the recipe declared a rule that fired.
    screenDone && eliminated.length === 0
      ? { id: "prune", label: "Prune", status: "skipped", detail: "no rule fired" }
      : settle(
          "prune",
          "Prune",
          eliminated.length > 0,
          eliminated.length > 0,
          eliminated.length > 0 ? `${eliminated.length} eliminated` : undefined
        ),
    hasConfirmSeeds
      ? settle(
          "confirm",
          "Confirm",
          confirmTrials.length > 0,
          confirmDone,
          confirmTrials.length > 0 ? `${confirmTrials.length} trials` : undefined
        )
      : { id: "confirm", label: "Confirm", status: "skipped", detail: "report-only recipe" },
    settle(
      "select",
      "Select",
      state.selection !== null,
      state.selection !== null,
      state.selection ? SELECTION_TONE[state.selection.status]?.label : undefined
    )
  ];
}

/**
 * Rows for the comparison table: every candidate in every stage it was scored
 * in, with the primary metric surfaced and the baseline marked.
 */
export type EvalComparisonRow = {
  key: string;
  stage: string;
  label: string;
  candidateId: string;
  isBaseline: boolean;
  primary: number | null;
  lift: number | null;
  pairedTrials: number;
  valid: number;
  failed: number;
  costUsd: number | null;
  eliminationReason: string | null;
  isWinner: boolean;
};

export function evalComparison(state: EvalState | undefined): EvalComparisonRow[] {
  if (!state) return [];
  const primary = state.selection?.primaryMetric
    ?? state.scorecards[0]?.metrics[0]?.metric
    ?? "";
  const winner = state.selection?.winnerId ?? null;
  return state.scorecards.map((card) => ({
    key: `${card.stage}:${card.candidateId}`,
    stage: card.stage,
    label: card.label,
    candidateId: card.candidateId,
    isBaseline: card.isBaseline,
    primary: metricMean(card, primary),
    lift: card.pairedLift,
    pairedTrials: card.pairedTrials,
    valid: card.trials.valid,
    failed: card.trials.failed,
    costUsd: card.costUsd,
    eliminationReason: card.eliminationReason,
    isWinner: winner !== null && card.candidateId === winner
  }));
}

/** Derivations over the projected GEPA slice shared by the workspace views. */

import type { GepaEvaluation, GepaState } from "../../components/projectEvents.ts";

export type CandidateRecord = Record<string, unknown>;

export function shortId(id: string): string {
  return id.replace(/^gepa_/, "").slice(0, 8) || id;
}

export function candidateName(candidate: CandidateRecord): string {
  if (String(candidate.source ?? "") === "seed" || candidate.parentId == null) return "Seed";
  const generation = typeof candidate.generation === "number" ? candidate.generation : undefined;
  const index = typeof candidate.proposal_index === "number" ? candidate.proposal_index : undefined;
  if (generation != null) {
    return `Gen ${generation} proposal${index != null ? ` ${index + 1}` : ""}`;
  }
  return `Proposal ${shortId(String(candidate.id ?? ""))}`;
}

export function candidateValues(candidate: CandidateRecord): Record<string, string> {
  for (const key of ["values", "payload"]) {
    const value = candidate[key];
    if (value && typeof value === "object" && !Array.isArray(value)) {
      return Object.fromEntries(
        Object.entries(value as Record<string, unknown>)
          .map(([lever, text]) => [lever, typeof text === "string" ? text : JSON.stringify(text, null, 2)])
      );
    }
  }
  const common = ["prompt", "instruction", "systemPrompt", "system_prompt", "content", "text"];
  return Object.fromEntries(
    common.filter((key) => typeof candidate[key] === "string").map((key) => [key, String(candidate[key])])
  );
}

export function statusLabel(status: unknown): string {
  const value = String(status ?? "");
  const labels: Record<string, string> = {
    registered: "Registered",
    evaluating: "Evaluating",
    minibatch_evaluated: "Minibatch scored",
    full_train_evaluated: "Train scored",
    accepted: "Accepted",
    rejected_minibatch: "Rejected",
    rejected: "Rejected",
    deferred_budget: "Deferred"
  };
  return labels[value] ?? (value ? value.replaceAll("_", " ") : "—");
}

export function statusTone(status: unknown): "ok" | "bad" | "warn" | "live" | undefined {
  const value = String(status ?? "");
  if (value === "accepted") return "ok";
  if (value.startsWith("rejected")) return "bad";
  if (value.startsWith("deferred")) return "warn";
  if (value === "evaluating") return "live";
  return undefined;
}

const REASON_TEXT: Record<string, string> = {
  primary_not_improved: "requires strict improvement over the parent",
  budget_exhausted: "evaluation budget exhausted"
};

export function decisionText(candidate: CandidateRecord): string | undefined {
  const decision = candidate.decision as
    | { outcome: string; gate: string; reason?: string; candidateScore?: number; parentScore?: number }
    | undefined;
  if (!decision) return undefined;
  const gateLabel = decision.gate === "minibatch" ? "minibatch gate" : decision.gate === "full_train" ? "full-train gate" : "budget";
  if (decision.outcome === "accepted") {
    return `Accepted at the ${gateLabel}${decision.candidateScore != null ? ` · scored ${decision.candidateScore.toFixed(2)}` : ""}`;
  }
  if (decision.outcome === "deferred") {
    return `Deferred · ${REASON_TEXT[decision.reason ?? ""] ?? decision.reason ?? "budget"}`;
  }
  const delta = decision.candidateScore != null && decision.parentScore != null
    ? decision.candidateScore - decision.parentScore
    : undefined;
  const parts = [`Rejected at the ${gateLabel}`];
  if (delta != null) {
    parts.push(`Δ ${delta >= 0 ? "+" : ""}${delta.toFixed(2)} vs parent (${decision.parentScore!.toFixed(2)} → ${decision.candidateScore!.toFixed(2)})`);
  }
  parts.push(REASON_TEXT[decision.reason ?? ""] ?? (decision.reason ? decision.reason.replaceAll("_", " ") : "did not beat the parent"));
  return parts.join(" · ");
}

export type StageMetrics = {
  stage: string;
  total: number;
  completed: number;
  mean?: number;
  solved: number;
  failures: number;
};

/** Aggregate evaluation rollouts per candidate per stage. */
export function metricsByCandidate(evaluations: GepaEvaluation[]): Map<string, Map<string, StageMetrics>> {
  const result = new Map<string, Map<string, StageMetrics>>();
  for (const evaluation of evaluations) {
    const candidateId = evaluation.candidateId ?? "unknown";
    const stage = evaluation.stage ?? "unknown";
    const perStage = result.get(candidateId) ?? new Map<string, StageMetrics>();
    const metrics = perStage.get(stage) ?? { stage, total: 0, completed: 0, solved: 0, failures: 0 };
    metrics.total += 1;
    if (evaluation.reward != null) {
      metrics.completed += 1;
      metrics.mean = ((metrics.mean ?? 0) * (metrics.completed - 1) + evaluation.reward) / metrics.completed;
      if (evaluation.reward > 0) metrics.solved += 1;
      else metrics.failures += 1;
    }
    perStage.set(stage, metrics);
    result.set(candidateId, perStage);
  }
  return result;
}

const QUALITY_STAGE_ORDER = [
  "seed_full_train",
  "candidate_full_train",
  "candidate_minibatch",
  "parent_minibatch_reference",
  "heldout"
];

export type CandidatePoint = {
  id: string;
  quality?: number;
  coverage?: number;
  basisStage?: string;
};

/**
 * Place a candidate on the quality (mean reward) × coverage (share of examples
 * solved) plane using its most informative evaluated stage. Candidates without
 * completed rollouts return no coordinates — they are listed, never plotted at 0.
 */
export function candidatePoint(
  candidate: CandidateRecord,
  metrics: Map<string, StageMetrics> | undefined
): CandidatePoint {
  const id = String(candidate.id ?? "");
  for (const stage of QUALITY_STAGE_ORDER) {
    const stageMetrics = metrics?.get(stage);
    if (stageMetrics && stageMetrics.completed > 0) {
      return {
        id,
        quality: stageMetrics.mean,
        coverage: stageMetrics.solved / stageMetrics.completed,
        basisStage: stage
      };
    }
  }
  const score = candidate.score ?? candidate.train_reward ?? candidate.minibatchReward;
  if (typeof score === "number" && Number.isFinite(score)) {
    return { id, quality: score, coverage: undefined };
  }
  return { id };
}

export const STAGE_TITLES: Record<string, string> = {
  seed_full_train: "Seed · full train",
  parent_minibatch_reference: "Parent · minibatch reference",
  candidate_minibatch: "Proposal · minibatch",
  candidate_full_train: "Proposal · full train",
  heldout: "Heldout",
  unknown: "Other rollouts"
};

export function stageTitle(stage?: string): string {
  return STAGE_TITLES[stage ?? "unknown"] ?? (stage ?? "Other rollouts").replaceAll("_", " ");
}

export type MinibatchComparisonRow = {
  exampleId: string;
  parent?: number | null;
  candidate?: number | null;
};

/** Join parent-reference and proposal minibatch rollouts on example id. */
export function minibatchComparison(
  evaluations: GepaEvaluation[],
  parentId: string,
  candidateId: string
): MinibatchComparisonRow[] {
  const rows = new Map<string, MinibatchComparisonRow>();
  for (const evaluation of evaluations) {
    if (!evaluation.exampleId) continue;
    const isParent = evaluation.candidateId === parentId && evaluation.stage === "parent_minibatch_reference";
    const isCandidate = evaluation.candidateId === candidateId && evaluation.stage === "candidate_minibatch";
    if (!isParent && !isCandidate) continue;
    const row = rows.get(evaluation.exampleId) ?? { exampleId: evaluation.exampleId };
    if (isParent) row.parent = evaluation.reward;
    else row.candidate = evaluation.reward;
    rows.set(evaluation.exampleId, row);
  }
  return [...rows.values()].sort((a, b) =>
    a.exampleId.localeCompare(b.exampleId, undefined, { numeric: true })
  );
}

export function limitOf(gepa: GepaState | undefined, kind: string) {
  return gepa?.limits.find((limit) => limit.kind === kind);
}

export function formatProgress(spent?: number, max?: number): string {
  if (spent == null && max == null) return "—";
  const left = spent == null ? "—" : String(Math.round(spent));
  const right = max == null ? "—" : String(Math.round(max));
  return `${left} / ${right}`;
}

export function formatClock(iso?: string): string {
  if (!iso) return "—";
  const time = iso.slice(11, 19);
  return time || iso;
}

export function formatDurationMs(ms?: number): string {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return "—";
  const seconds = Math.floor(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

export function elapsedLabel(timing: GepaState["timing"] | undefined, terminal: boolean): string {
  if (!timing?.startedAt) return "—";
  const end = terminal ? timing.endedAt ?? timing.lastEventAt : timing.lastEventAt;
  if (!end) return "—";
  const ms = Date.parse(end) - Date.parse(timing.startedAt);
  return formatDurationMs(ms);
}

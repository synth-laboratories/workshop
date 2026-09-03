/** Derivations over the projected GEPA slice shared by the workspace views. */

import type { GepaEvaluation, GepaState } from "../../components/projectEvents.ts";

export type CandidateRecord = Record<string, unknown>;

const GENERATION_PALETTE = [
  { color: "#2563eb", tint: "#eff6ff" },
  { color: "#7c3aed", tint: "#f5f3ff" },
  { color: "#0f766e", tint: "#f0fdfa" },
  { color: "#b45309", tint: "#fffbeb" },
  { color: "#be123c", tint: "#fff1f2" },
  { color: "#0369a1", tint: "#f0f9ff" }
] as const;
const SEED_COLOR = { color: "#667085", tint: "#f5f6f7" } as const;

export function candidateGeneration(candidate: CandidateRecord): number | undefined {
  return typeof candidate.generation === "number" && Number.isFinite(candidate.generation)
    ? Math.max(0, Math.floor(candidate.generation))
    : undefined;
}

export function generationPalette(generation?: number): { color: string; tint: string } {
  if (generation == null) return SEED_COLOR;
  return GENERATION_PALETTE[generation % GENERATION_PALETTE.length];
}

export function candidatePalette(candidate: CandidateRecord): { color: string; tint: string } {
  return generationPalette(candidateGeneration(candidate));
}

export function shortId(id: string): string {
  return id.replace(/^gepa_/, "").slice(0, 8) || id;
}

export function candidateName(candidate: CandidateRecord): string {
  if (String(candidate.source ?? "") === "seed" || candidate.parentId == null) return "Seed";
  const generation = candidateGeneration(candidate);
  const index = typeof candidate.proposal_index === "number" ? candidate.proposal_index : undefined;
  if (generation != null) {
    return `Gen ${generation} proposal${index != null ? ` ${index + 1}` : ""}`;
  }
  return `Proposal ${shortId(String(candidate.id ?? ""))}`;
}

/**
 * Display labels are not identities. `candidateName` returns "Seed" for every
 * parentless candidate, so a run that registered two of them rendered two
 * indistinguishable frontier rows. Disambiguate collisions with the stable
 * short id rather than silently presenting two rows as the same candidate.
 */
export function candidateLabels(candidates: CandidateRecord[]): Map<string, string> {
  const counts = new Map<string, number>();
  for (const candidate of candidates) {
    const name = candidateName(candidate);
    counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  const labels = new Map<string, string>();
  for (const candidate of candidates) {
    const id = String(candidate.id ?? "");
    const name = candidateName(candidate);
    labels.set(id, (counts.get(name) ?? 0) > 1 ? `${name} ${shortId(id)}` : name);
  }
  return labels;
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
    rejected_full_train: "Rejected · train scored",
    rejected: "Rejected",
    deferred_budget: "Deferred",
    aborted: "Not evaluated"
  };
  return labels[value] ?? (value ? value.replaceAll("_", " ") : "—");
}

export function statusTone(status: unknown): "ok" | "bad" | "warn" | "live" | undefined {
  const value = String(status ?? "");
  if (value === "accepted") return "ok";
  if (value.startsWith("rejected")) return "bad";
  if (value.startsWith("deferred")) return "warn";
  if (value === "aborted") return "warn";
  if (value === "evaluating") return "live";
  return undefined;
}

const REASON_TEXT: Record<string, string> = {
  primary_not_improved: "requires strict improvement over the parent",
  budget_exhausted: "evaluation budget exhausted"
};

export function decisionText(candidate: CandidateRecord): string | undefined {
  const decision = candidate.decision as
    | { outcome: string; gate: string; reason?: string; comparison?: string; candidateScore?: number; parentScore?: number; incumbentId?: string; selectionObjective?: string; selectionDelta?: number; rationale?: string }
    | undefined;
  if (!decision) return undefined;
  const gateLabel = decision.gate === "minibatch" ? "minibatch gate" : decision.gate === "full_train" ? "full-train gate" : "budget";
  if (decision.outcome === "accepted") {
    const incumbent = decision.incumbentId ? ` over incumbent ${shortId(decision.incumbentId)}` : "";
    const delta = decision.selectionDelta;
    return `Accepted at the ${gateLabel}${incumbent}${decision.candidateScore != null ? ` · scored ${decision.candidateScore.toFixed(3)}` : ""}${delta != null ? ` · Δ ${delta >= 0 ? "+" : ""}${delta.toFixed(3)}` : ""}`;
  }
  if (decision.outcome === "deferred") {
    return `Deferred · ${REASON_TEXT[decision.reason ?? ""] ?? decision.reason ?? "budget"}`;
  }
  const delta = decision.selectionDelta ?? (decision.candidateScore != null && decision.parentScore != null
    ? decision.candidateScore - decision.parentScore
    : undefined);
  const parts = [`Rejected at the ${gateLabel}`];
  if (delta != null) {
    const target = decision.incumbentId ? `incumbent ${shortId(decision.incumbentId)}` : "incumbent";
    const scores = decision.parentScore != null && decision.candidateScore != null
      ? ` (${decision.parentScore.toFixed(3)} → ${decision.candidateScore.toFixed(3)})`
      : "";
    parts.push(`Δ ${delta >= 0 ? "+" : ""}${delta.toFixed(3)} vs ${target}${scores}`);
  }
  parts.push(decision.rationale ?? REASON_TEXT[decision.reason ?? ""] ?? (decision.reason ? decision.reason.replaceAll("_", " ") : "did not beat the incumbent"));
  return parts.join(" · ");
}

export function fullTrainCandidateScore(candidate: CandidateRecord): number | undefined {
  const status = String(candidate.status ?? "");
  if (!(["accepted", "full_train_evaluated", "rejected_full_train"].includes(status) || String(candidate.source ?? "") === "seed")) return undefined;
  for (const value of [candidate.train_reward, candidate.candidate_train_reward, candidate.score]) {
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  return undefined;
}

export function orderedScoredCandidates(gepa: GepaState) {
  return gepa.candidates.map((candidate) => ({
    candidate,
    id: String(candidate.id ?? ""),
    sequence: typeof candidate.sequence === "number" ? candidate.sequence : 0,
    score: fullTrainCandidateScore(candidate)
  })).filter((point): point is { candidate: CandidateRecord; id: string; sequence: number; score: number } => point.score != null)
    .sort((a, b) => a.sequence - b.sequence);
}

export function incumbentCandidateIds(gepa: GepaState): string[] {
  const ids: string[] = [];
  for (const snapshot of gepa.frontierHistory) {
    if (snapshot.bestCandidateId && ids.at(-1) !== snapshot.bestCandidateId) ids.push(snapshot.bestCandidateId);
  }
  if (ids.length === 0) {
    for (const candidate of gepa.candidates) {
      const id = String(candidate.id ?? "");
      if ((String(candidate.source ?? "") === "seed" || candidate.status === "accepted") && id && ids.at(-1) !== id) ids.push(id);
    }
  }
  return ids;
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
  const endMs = terminal
    ? Date.parse(timing.endedAt ?? timing.lastEventAt ?? "")
    : Date.now();
  if (!Number.isFinite(endMs)) return "—";
  const ms = endMs - Date.parse(timing.startedAt);
  return formatDurationMs(ms);
}

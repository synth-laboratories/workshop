import type { GepaState } from "../../components/projectEvents.ts";
import { candidateName, candidateValues, fullTrainCandidateScore, type CandidateRecord } from "./model.ts";

export type GepaSelectionKind = "run" | "candidate" | "proposal" | "trial" | "trace_item" | "artifact" | "evaluation" | "visual";

export type GepaLinkedSelection = {
  runId: string;
  kind: GepaSelectionKind;
  id: string;
  eventId?: string;
  sequenceNumber?: number;
  candidateId?: string;
  visualId?: string;
  visualRevision?: number;
  sourceDigest?: string;
};

export type CandidateDecisionFilter = "all" | "accepted" | "rejected" | "pending";
export type CandidateSort = "sequence" | "score" | "generation" | "status" | "frontier_credit";
export type SortDirection = "asc" | "desc";

export type GepaPresentationState = {
  version: 1;
  query: string;
  decision: CandidateDecisionFilter;
  frontierOnly: boolean;
  sort: CandidateSort;
  direction: SortDirection;
  stageFilter: string | null;
  selection: GepaLinkedSelection | null;
};

export const DEFAULT_GEPA_PRESENTATION_STATE: GepaPresentationState = {
  version: 1,
  query: "",
  decision: "all",
  frontierOnly: false,
  sort: "sequence",
  direction: "asc",
  stageFilter: null,
  selection: null
};

const DECISIONS = new Set<CandidateDecisionFilter>(["all", "accepted", "rejected", "pending"]);
const SORTS = new Set<CandidateSort>(["sequence", "score", "generation", "status", "frontier_credit"]);
const DIRECTIONS = new Set<SortDirection>(["asc", "desc"]);
const SELECTION_KINDS = new Set<GepaSelectionKind>(["run", "candidate", "proposal", "trial", "trace_item", "artifact", "evaluation", "visual"]);

export function presentationStorageKey(runId: string): string {
  return `synth.optimizer.gepa.presentation.v1:${runId}`;
}

export function normalizeGepaPresentationState(raw: unknown, runId: string): GepaPresentationState {
  if (!raw || typeof raw !== "object") return { ...DEFAULT_GEPA_PRESENTATION_STATE };
  const value = raw as Record<string, unknown>;
  const selectionValue = value.selection && typeof value.selection === "object"
    ? value.selection as Record<string, unknown>
    : null;
  const kind = selectionValue && SELECTION_KINDS.has(selectionValue.kind as GepaSelectionKind)
    ? selectionValue.kind as GepaSelectionKind
    : null;
  const id = selectionValue && typeof selectionValue.id === "string" ? selectionValue.id : null;
  const selectionFields = selectionValue ?? {};
  const selection: GepaLinkedSelection | null = kind && id
    ? {
        runId,
        kind,
        id,
        ...(typeof selectionFields.eventId === "string" ? { eventId: selectionFields.eventId } : {}),
        ...(typeof selectionFields.sequenceNumber === "number" ? { sequenceNumber: selectionFields.sequenceNumber } : {}),
        ...(typeof selectionFields.candidateId === "string" ? { candidateId: selectionFields.candidateId } : {}),
        ...(typeof selectionFields.visualId === "string" ? { visualId: selectionFields.visualId } : {}),
        ...(typeof selectionFields.visualRevision === "number" ? { visualRevision: selectionFields.visualRevision } : {}),
        ...(typeof selectionFields.sourceDigest === "string" ? { sourceDigest: selectionFields.sourceDigest } : {})
      }
    : null;
  return {
    version: 1,
    query: typeof value.query === "string" ? value.query.slice(0, 240) : "",
    decision: DECISIONS.has(value.decision as CandidateDecisionFilter) ? value.decision as CandidateDecisionFilter : "all",
    frontierOnly: value.frontierOnly === true,
    sort: SORTS.has(value.sort as CandidateSort) ? value.sort as CandidateSort : "sequence",
    direction: DIRECTIONS.has(value.direction as SortDirection) ? value.direction as SortDirection : "asc",
    stageFilter: typeof value.stageFilter === "string" ? value.stageFilter : null,
    selection
  };
}

export function loadGepaPresentationState(runId: string, storage?: Pick<Storage, "getItem">): GepaPresentationState {
  if (!storage) return { ...DEFAULT_GEPA_PRESENTATION_STATE };
  try {
    const raw = storage.getItem(presentationStorageKey(runId));
    return normalizeGepaPresentationState(raw ? JSON.parse(raw) : null, runId);
  } catch {
    return { ...DEFAULT_GEPA_PRESENTATION_STATE };
  }
}

export function saveGepaPresentationState(runId: string, state: GepaPresentationState, storage?: Pick<Storage, "setItem">): void {
  if (!storage) return;
  try {
    storage.setItem(presentationStorageKey(runId), JSON.stringify(normalizeGepaPresentationState(state, runId)));
  } catch {
    // Presentation state is best-effort and must never break the evidence view.
  }
}

function decisionGroup(candidate: CandidateRecord): Exclude<CandidateDecisionFilter, "all"> {
  const outcome = String((candidate.decision as { outcome?: unknown } | undefined)?.outcome ?? "");
  const status = String(candidate.status ?? "");
  if (outcome === "accepted" || status === "accepted") return "accepted";
  if (outcome === "rejected" || status.startsWith("rejected")) return "rejected";
  return "pending";
}

export function frontierCreditByCandidate(gepa: GepaState): Map<string, number> {
  const eligibleStages = new Set(["seed_full_train", "candidate_full_train"]);
  const scoresByExample = new Map<string, Map<string, number>>();
  for (const evaluation of gepa.evaluations) {
    if (!evaluation.candidateId || !evaluation.exampleId || evaluation.reward == null || !eligibleStages.has(evaluation.stage ?? "")) continue;
    const scores = scoresByExample.get(evaluation.exampleId) ?? new Map<string, number>();
    scores.set(evaluation.candidateId, evaluation.reward);
    scoresByExample.set(evaluation.exampleId, scores);
  }
  const credit = new Map<string, number>();
  for (const scores of scoresByExample.values()) {
    const best = Math.max(...scores.values());
    for (const [candidateId, reward] of scores) {
      if (Math.abs(reward - best) <= Number.EPSILON) credit.set(candidateId, (credit.get(candidateId) ?? 0) + 1);
    }
  }
  return credit;
}

export function visibleCandidates(gepa: GepaState, state: GepaPresentationState): CandidateRecord[] {
  const query = state.query.trim().toLocaleLowerCase();
  const frontier = new Set(gepa.frontier.map((member) => String(member.candidateId)));
  const credit = frontierCreditByCandidate(gepa);
  const rows = gepa.candidates.filter((candidate) => {
    const id = String(candidate.id ?? "");
    if (state.frontierOnly && !frontier.has(id)) return false;
    if (state.decision !== "all" && decisionGroup(candidate) !== state.decision) return false;
    if (!query) return true;
    const haystack = [id, candidateName(candidate), candidate.status, candidate.source, ...Object.values(candidateValues(candidate))]
      .map((value) => String(value ?? "").toLocaleLowerCase())
      .join("\n");
    return haystack.includes(query);
  });
  rows.sort((a, b) => {
    const aId = String(a.id ?? "");
    const bId = String(b.id ?? "");
    let order = 0;
    const numeric = state.sort === "score"
      ? [fullTrainCandidateScore(a), fullTrainCandidateScore(b)]
      : state.sort === "generation"
        ? [typeof a.generation === "number" ? a.generation : -1, typeof b.generation === "number" ? b.generation : -1]
        : state.sort === "frontier_credit"
          ? [credit.get(aId), credit.get(bId)]
          : state.sort === "sequence"
            ? [typeof a.sequence === "number" ? a.sequence : undefined, typeof b.sequence === "number" ? b.sequence : undefined]
            : null;
    if (numeric) {
      const [aValue, bValue] = numeric;
      if (aValue == null || bValue == null) {
        if (aValue == null && bValue != null) return 1;
        if (aValue != null && bValue == null) return -1;
      } else {
        order = aValue - bValue;
      }
    } else {
      order = String(a.status ?? "").localeCompare(String(b.status ?? ""));
    }
    if (state.direction === "desc") order *= -1;
    return order || aId.localeCompare(bId, undefined, { numeric: true });
  });
  return rows;
}

export function resolvedSelection(selection: GepaLinkedSelection | null, gepa: GepaState): GepaLinkedSelection | null {
  if (!selection) return null;
  if (selection.kind === "candidate" || selection.kind === "proposal") {
    return gepa.candidates.some((candidate) => String(candidate.id) === selection.id) ? selection : null;
  }
  if (selection.kind === "evaluation" || selection.kind === "trial") {
    return gepa.evaluations.some((evaluation) => evaluation.ref.id === selection.id) ? selection : null;
  }
  return selection;
}

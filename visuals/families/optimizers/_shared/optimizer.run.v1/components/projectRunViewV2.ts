/**
 * Presentation adapter for the backend-owned optimizer projection.
 *
 * This file deliberately does not accept raw events. Live product state must
 * be a formatting of OptimizerRunViewV2; the event reducer is reserved for an
 * explicit historical cursor.
 */

import type {
  OptimizerRun,
  ProjectedState,
  GepaStage,
  EvalSelection
} from "./projectEvents.ts";

type ResourceRef = {
  kind: string;
  id: string;
  digest?: string | null;
  role?: string | null;
  title?: string | null;
  metadata?: unknown;
};

type WorkItem = {
  workItemId: string;
  kind: string;
  lifecycle: string;
  terminal?: string | null;
  externalRef?: string | null;
};

type RunViewHeader = {
  runId: string;
  algorithm: string;
  lifecycle: string;
  phase?: string | null;
  condition: string;
  placement: string;
  specId: string;
  specDigest: string;
  executionBindings: Array<Record<string, unknown>>;
  inputRefs: ResourceRef[];
  outputRefs: ResourceRef[];
  visualRefs: ResourceRef[];
  usage: {
    costUsd?: number | null;
    promptTokens?: number | null;
    completionTokens?: number | null;
    steps?: number | null;
  };
  evidence: {
    completeness: string;
    reason?: string | null;
    refs: ResourceRef[];
  };
  failureRef?: string | null;
  terminal?: {
    kind: string;
    finalSequence: number;
    sealedAt: string;
  } | null;
  projectionSchemaVersion: string;
  asOfSequence: number;
  projectionRevision: number;
};

export type OptimizerRunViewV2Like = {
  algorithm: "eval" | "gepa" | "go-ex" | "sft" | "cispo";
  header: RunViewHeader;
  projection: Record<string, unknown>;
  result?: Record<string, unknown> | null;
};

function records(value: unknown): Array<Record<string, unknown>> {
  if (!Array.isArray(value)) return [];
  return value.filter((entry): entry is Record<string, unknown> =>
    Boolean(entry && typeof entry === "object" && !Array.isArray(entry))
  );
}

function strings(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((entry): entry is string => typeof entry === "string")
    : [];
}

function numberOrNull(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function statusOf(header: RunViewHeader): string {
  if (header.lifecycle === "terminal") return header.terminal?.kind ?? "unknown";
  return header.lifecycle;
}

function artifact(ref: ResourceRef): Record<string, unknown> {
  return {
    kind: ref.kind,
    id: ref.id,
    digest: ref.digest ?? undefined,
    role: ref.role ?? undefined,
    title: ref.title ?? undefined,
    metadata: ref.metadata
  };
}

function workStatus(item: WorkItem): string {
  return item.lifecycle === "terminal" ? item.terminal ?? "terminal" : item.lifecycle;
}

function baseProjection(run: OptimizerRun, view: OptimizerRunViewV2Like): ProjectedState {
  const { header } = view;
  if (
    header.runId !== run.id ||
    header.algorithm !== view.algorithm ||
    run.algorithmId !== view.algorithm
  ) {
    throw new Error("optimizer V2 view identity does not match the bound run");
  }
  return {
    cursorSeq: header.asOfSequence,
    summary: {
      id: header.runId,
      algorithmId: header.algorithm,
      status: statusOf(header),
      objective: run.objective,
      source: run.source,
      phase: header.phase,
      condition: header.condition,
      placement: header.placement,
      specId: header.specId,
      specDigest: header.specDigest,
      evidence: header.evidence,
      failureRef: header.failureRef,
      terminal: header.terminal,
      projectionSchemaVersion: header.projectionSchemaVersion,
      projectionRevision: header.projectionRevision,
      cursorSeq: header.asOfSequence,
      summary: view.result ?? {}
    },
    // Live timelines/logs are diagnostic and intentionally not reconstructed
    // into product claims. Historical scrubbing supplies them separately.
    timeline: [],
    logs: [],
    usage: {
      costUsd: header.usage.costUsd ?? null,
      promptTokens: header.usage.promptTokens ?? null,
      completionTokens: header.usage.completionTokens ?? null,
      steps: header.usage.steps ?? null
    },
    artifacts: [
      ...header.outputRefs.map(artifact),
      ...header.visualRefs.map(artifact),
      ...header.evidence.refs.map(artifact)
    ],
    execution: { bindings: header.executionBindings }
  };
}

function evalProjection(base: ProjectedState, view: OptimizerRunViewV2Like): void {
  const projection = view.projection;
  const candidates = strings(projection.candidates);
  const workItems = records(projection.workItems) as WorkItem[];
  const result = view.result ?? {};
  const selection = typeof result.selection === "string"
    ? {
        status: result.selection,
        winnerId: typeof result.selectedCandidateId === "string" ? result.selectedCandidateId : null,
        baselineId: candidates[0] ?? null,
        primaryMetric: "reward",
        lift: null,
        minLift: 0,
        reason: result.selection
      } satisfies EvalSelection
    : null;
  base.eval = {
    candidates: candidates.map((id, index) => ({ id, label: id, isBaseline: index === 0 })),
    scorecards: [],
    trials: workItems.map((item) => ({
      id: item.workItemId,
      status: workStatus(item),
      valid: item.terminal === "completed" ? true : item.terminal ? false : undefined,
      metrics: {},
      missingGates: [],
      missingArtifacts: []
    })),
    rollouts: [],
    selection,
    seedLedger: {
      screening: (Array.isArray(projection.seeds) ? projection.seeds : [])
        .filter((value): value is number => typeof value === "number"),
      confirmation: [],
      scenarios: strings(projection.scenarios)
    },
    manifestDigest: null,
    candidateSetId: null,
    evidenceDir: null,
    plannedTrials: workItems.length,
    parallelism: null,
    globalCapacity: null,
    paused: view.header.lifecycle === "paused"
  };
}

const GEPA_STAGE_LABELS: Record<string, string> = {
  seed: "Seed evaluation",
  proposal: "Reflection + proposal",
  minibatch: "Minibatch gate",
  full_train: "Full train evaluation",
  heldout: "Heldout",
  complete: "Complete"
};

function gepaProjection(base: ProjectedState, view: OptimizerRunViewV2Like): void {
  const projection = view.projection;
  const candidateMap = projection.candidates && typeof projection.candidates === "object"
    ? projection.candidates as Record<string, Record<string, unknown>>
    : {};
  const order = strings(projection.candidateOrder);
  const candidates = (order.length ? order : Object.keys(candidateMap)).map((id) => ({
    candidateId: id,
    id,
    ...(candidateMap[id] ?? {})
  }));
  const phase = typeof projection.phase === "string" ? projection.phase : "selection";
  const terminal = view.header.lifecycle === "terminal";
  const stages = Object.entries(GEPA_STAGE_LABELS).map(([id, label]) => ({
    id,
    label,
    status: id === "complete"
      ? terminal ? "completed" : "pending"
      : id === phase ? "active" : "pending"
  })) as GepaStage[];
  const incumbentId = typeof projection.incumbentId === "string"
    ? projection.incumbentId
    : typeof projection.selectedCandidateId === "string" ? projection.selectedCandidateId : undefined;
  base.gepa = {
    candidates,
    frontier: strings(projection.frontierHistory).map((candidateId) => ({ candidateId })),
    reflections: [],
    budget: projection.rolloutBudget == null ? undefined : { maxTotalRollouts: projection.rolloutBudget },
    limits: [],
    contract: { program: { mutableFields: [] }, objectiveSet: { objectives: [] } },
    frontierHistory: [],
    stages,
    evaluations: [],
    failedAttempts: [],
    coverage: [],
    proposerTraces: [],
    activity: {
      phase,
      label: terminal ? "Search complete" : GEPA_STAGE_LABELS[phase] ?? phase,
      proposalActive: phase === "proposal" && !terminal,
      evaluationActive: ["minibatch", "full_train", "heldout"].includes(phase) && !terminal,
      activeCandidateIds: [],
      terminal
    },
    incumbentId,
    best: incumbentId ? { candidateId: incumbentId } : undefined,
    models: {},
    timing: {},
    rolloutsCompleted: typeof projection.rolloutsScored === "number" ? projection.rolloutsScored : 0,
    runtime: {
      activeWorkers: numberOrNull(projection.maxActiveWorkers) ?? undefined,
      reportedCostUsd: view.header.usage.costUsd ?? undefined,
      costTelemetryComplete: view.header.usage.costUsd != null
    }
  };
}

function goExProjection(base: ProjectedState, view: OptimizerRunViewV2Like): void {
  const projection = view.projection;
  const selected = typeof projection.selectedCandidateId === "string"
    ? projection.selectedCandidateId
    : undefined;
  base.goex = {
    board: {
      phase: projection.phase ?? view.header.phase ?? "unknown",
      remoteStatus: projection.remoteStatus ?? null
    },
    themes: strings(projection.themes).map((id) => ({ id })),
    candidates: strings(projection.candidateIds).map((id) => ({
      id,
      candidate_id: id,
      selected: id === selected
    })),
    frontier: selected ? { bestCandidateId: selected } : {},
    dataEngine: { childEvalRunIds: strings(projection.childEvalRunIds) },
    agents: {},
    rollouts: []
  };
}

function sftProjection(base: ProjectedState, view: OptimizerRunViewV2Like): void {
  const projection = view.projection;
  const step = numberOrNull((projection.usage as Record<string, unknown> | undefined)?.steps);
  const loss = numberOrNull(projection.trainLoss);
  const checkpoints = strings(projection.checkpoints);
  base.sft = {
    curves: {
      steps: step == null ? [] : [step],
      epochs: [],
      trainLoss: loss == null ? [] : [loss],
      validationLoss: [],
      learningRate: []
    },
    points: step == null ? [] : [{ step, ...(loss == null ? {} : { trainLoss: loss }) }],
    checkpoints: checkpoints.map((id) => ({
      id,
      status: id === projection.selectedCheckpointId ? "selected" : "ready"
    })),
    evaluations: strings(projection.childEvalRunIds).map((optimizerRunId) => ({ optimizerRunId })),
    // Eval campaigns are not a runtime concept in V2.
    campaigns: [],
    dataset: {
      digest: projection.datasetDigest ?? null,
      configDigest: projection.configDigest ?? null,
      splits: {}
    },
    compute: { producedAdapter: projection.producedAdapter ?? null },
    examples: [],
    lineage: {
      selectedCheckpointId: projection.selectedCheckpointId ?? null,
      producedAdapter: projection.producedAdapter ?? null
    },
    curation: {
      collected: null,
      considered: null,
      accepted: null,
      rejected: null,
      rejectionsByReason: {},
      seedsCovered: null,
      achievementsCovered: [],
      candidates: []
    }
  };
}

function cispoProjection(base: ProjectedState, view: OptimizerRunViewV2Like, run: OptimizerRun): void {
  const projection = view.projection;
  sftProjection(base, view);
  base.cispo = {
    objective: run.objective ?? "CISPO clipped-importance policy optimization",
    clipLow: null,
    clipHigh: null,
    groupSize: null,
    rewardVariance: null,
    advantageMean: numberOrNull(projection.meanAdvantage),
    advantageStd: null,
    optimizerSteps: typeof view.header.usage.steps === "number" ? view.header.usage.steps : 0,
    warmStartArtifactId: typeof projection.warmStartId === "string" ? projection.warmStartId : null,
    checkpointIds: strings(projection.checkpoints),
    noLearningSignal: projection.noLearningSignal === true
  };
}

export function projectRunViewV2(
  run: OptimizerRun,
  view: OptimizerRunViewV2Like
): ProjectedState {
  const base = baseProjection(run, view);
  switch (view.algorithm) {
    case "eval":
      evalProjection(base, view);
      break;
    case "gepa":
      gepaProjection(base, view);
      break;
    case "go-ex":
      goExProjection(base, view);
      break;
    case "sft":
      sftProjection(base, view);
      break;
    case "cispo":
      cispoProjection(base, view, run);
      break;
  }
  return base;
}

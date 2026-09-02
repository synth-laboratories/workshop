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

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function optionalString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
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
  const projectedSelection = record(projection.selection);
  const selection = typeof projectedSelection.status === "string"
    ? {
        status: projectedSelection.status,
        winnerId: optionalString(projectedSelection.winner_id ?? projectedSelection.winnerId) ?? null,
        baselineId: optionalString(projectedSelection.baseline_id ?? projectedSelection.baselineId) ?? candidates[0] ?? null,
        primaryMetric: optionalString(projectedSelection.primary_metric ?? projectedSelection.primaryMetric) ?? "reward",
        lift: numberOrNull(projectedSelection.lift),
        minLift: numberOrNull(projectedSelection.min_lift ?? projectedSelection.minLift) ?? 0,
        reason: optionalString(projectedSelection.reason) ?? projectedSelection.status
      } satisfies EvalSelection
    : typeof result.selection === "string"
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
  const setup = record(projection.setup);
  const seedLedger = record(projection.seedLedger);
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
      screening: (Array.isArray(seedLedger.screening) ? seedLedger.screening : Array.isArray(projection.seeds) ? projection.seeds : [])
        .filter((value): value is number => typeof value === "number"),
      confirmation: (Array.isArray(seedLedger.confirmation) ? seedLedger.confirmation : [])
        .filter((value): value is number => typeof value === "number"),
      scenarios: strings(seedLedger.scenarios ?? projection.scenarios)
    },
    manifestDigest: null,
    candidateSetId: null,
    evidenceDir: null,
    plannedTrials: numberOrNull(setup.plannedTrials ?? setup.planned_trials) ?? workItems.length,
    parallelism: numberOrNull(setup.parallelism),
    globalCapacity: numberOrNull(setup.globalCapacity ?? setup.global_capacity),
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
  if (typeof projection.rolloutsScored === "number") {
    base.usage.rollouts = projection.rolloutsScored;
  }
  const candidateMap = projection.candidates && typeof projection.candidates === "object"
    ? projection.candidates as Record<string, Record<string, unknown>>
    : {};
  const order = strings(projection.candidateOrder);
  const candidates = (order.length ? order : Object.keys(candidateMap)).map((id) => {
    const candidate = candidateMap[id] ?? {};
    const trainReward = numberOrNull(candidate.trainReward);
    const heldoutReward = numberOrNull(candidate.heldoutReward);
    const gateAccepted = typeof candidate.gateAccepted === "boolean" ? candidate.gateAccepted : undefined;
    const minibatchReward = numberOrNull(candidate.minibatchReward);
    return {
      candidateId: id,
      id,
      ...candidate,
      // The workspace predates the V2 wire shape. Normalize the canonical
      // camel-case fields at this one boundary instead of teaching every view
      // to read both the event-reducer and run-view spellings.
      train_reward: trainReward ?? undefined,
      heldout_reward: heldoutReward ?? undefined,
      minibatchReward: minibatchReward ?? undefined,
      status: gateAccepted === true
        ? "accepted"
        : gateAccepted === false
          ? trainReward != null ? "rejected_full_train" : "rejected_minibatch"
          : trainReward != null
            ? "full_train_evaluated"
            : "registered"
    };
  });
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
  const incumbent = incumbentId ? candidateMap[incumbentId] : undefined;
  const incumbentTrain = numberOrNull(incumbent?.trainReward);
  const incumbentHeldout = numberOrNull(incumbent?.heldoutReward);
  const evaluations = records(projection.evaluations).map((entry, index) => ({
    candidateId: typeof entry.candidateId === "string" ? entry.candidateId : undefined,
    sequence: index + 1,
    ref: {
      kind: "container_rollout" as const,
      id: typeof entry.rolloutId === "string" ? entry.rolloutId : String(entry.id ?? `evaluation-${index + 1}`),
      role: "candidate_evaluation"
    },
    stage: typeof entry.stage === "string" ? entry.stage : undefined,
    exampleId: typeof entry.exampleId === "string" ? entry.exampleId : undefined,
    reward: numberOrNull(entry.reward),
    costUsd: numberOrNull(entry.costUsd) ?? undefined
  }));
  const proposerCalls = records(projection.proposerCalls);
  const projectedContract = record(projection.contract);
  const task = record(projectedContract.task);
  const program = record(projectedContract.program);
  const objectiveSet = record(projectedContract.objectiveSet);
  const objectiveRows = records(objectiveSet.objectives);
  const splitRows = record(projectedContract.splits);
  const dataset = record(projectedContract.dataset);
  const datasetSplits = record(dataset.splits);
  const container = record(projectedContract.container);
  const projectedRuntime = record(projection.runtime);
  base.gepa = {
    candidates,
    frontier: strings(projection.frontierHistory).map((candidateId) => ({ candidateId })),
    reflections: [],
    budget: projection.rolloutBudget == null ? undefined : { maxTotalRollouts: projection.rolloutBudget },
    limits: [],
    contract: {
      task: {
        id: optionalString(task.id),
        name: optionalString(task.name),
        objective: optionalString(task.objective),
        description: optionalString(task.description),
        family: optionalString(task.family),
        version: optionalString(task.version),
        outputKind: optionalString(task.outputKind)
      },
      program: {
        id: optionalString(program.id),
        mutableFields: strings(program.mutableFields)
      },
      objectiveSet: {
        id: optionalString(objectiveSet.id),
        hash: optionalString(objectiveSet.hash),
        frontierType: optionalString(objectiveSet.frontierType),
        selectionObjective: optionalString(objectiveSet.selectionObjective),
        objectives: objectiveRows.map((objective) => ({
          name: optionalString(objective.name) ?? optionalString(objective.id) ?? "objective",
          direction: optionalString(objective.direction),
          aggregation: optionalString(objective.aggregation),
          splitPolicy: optionalString(objective.splitPolicy ?? objective.split_policy)
        }))
      },
      splits: {
        train: numberOrNull(splitRows.train) ?? undefined,
        minibatch: numberOrNull(splitRows.minibatch) ?? undefined,
        reflection: numberOrNull(splitRows.reflection) ?? undefined,
        pareto: numberOrNull(splitRows.pareto) ?? undefined,
        heldout: numberOrNull(splitRows.heldout) ?? undefined
      },
      dataset: {
        source: optionalString(dataset.source),
        config: optionalString(dataset.config),
        revision: optionalString(dataset.revision),
        digest: optionalString(dataset.digest),
        rowCount: numberOrNull(dataset.rowCount) ?? undefined,
        labelCount: numberOrNull(dataset.labelCount) ?? undefined,
        splits: {
          train: numberOrNull(datasetSplits.train) ?? undefined,
          selection: numberOrNull(datasetSplits.selection) ?? undefined,
          heldout: numberOrNull(datasetSplits.heldout) ?? undefined
        }
      },
      container: {
        verified: container.verified === true,
        specId: optionalString(container.specId),
        url: optionalString(container.url),
        workshopInstance: optionalString(container.workshopInstance),
        credentialMode: optionalString(container.credentialMode),
        evaluatorId: optionalString(container.evaluatorId),
        runtimeFamily: optionalString(container.runtimeFamily),
        targetId: optionalString(container.targetId),
        rewardAuthority: optionalString(container.rewardAuthority),
        policyHarness: optionalString(container.policyHarness),
        policyConfig: optionalString(container.policyConfig),
        scaleLeases: numberOrNull(container.scaleLeases) ?? undefined,
        retention: optionalString(container.retention)
      }
    },
    frontierHistory: [],
    stages,
    evaluations,
    failedAttempts: [],
    coverage: [],
    proposerTraces: proposerCalls.map((call, index) => ({
      generation: numberOrNull(call.generation) ?? index,
      sequence: index + 1,
      status: "completed",
      model: typeof call.model === "string" ? call.model : undefined,
      provider: typeof call.provider === "string" ? call.provider : undefined,
      proposalCount: numberOrNull(call.proposalCount) ?? undefined,
      costUsd: numberOrNull(call.costUsd) ?? undefined,
      candidateIds: [],
      steps: []
    })),
    activity: {
      phase,
      label: terminal ? "Search complete" : GEPA_STAGE_LABELS[phase] ?? phase,
      proposalActive: phase === "proposal" && !terminal,
      evaluationActive: ["minibatch", "full_train", "heldout"].includes(phase) && !terminal,
      activeCandidateIds: [],
      terminal
    },
    incumbentId,
    best: incumbentId ? {
      candidateId: incumbentId,
      trainReward: incumbentTrain ?? undefined,
      heldoutReward: incumbentHeldout ?? undefined
    } : undefined,
    heldout: incumbentId && incumbentHeldout != null
      ? { candidateId: incumbentId, reward: incumbentHeldout }
      : undefined,
    models: {
      policy: optionalString(container.policyModel),
      proposer: proposerCalls.map((call) => optionalString(call.model)).find(Boolean)
    },
    timing: {},
    rolloutsCompleted: evaluations.length || (typeof projection.rolloutsScored === "number" ? projection.rolloutsScored : 0),
    runtime: {
      activeWorkers: numberOrNull(projection.maxActiveWorkers) ?? undefined,
      configuredRolloutWorkers: numberOrNull(projectedRuntime.configuredRolloutWorkers) ?? undefined,
      staticRolloutWorkers: numberOrNull(projectedRuntime.staticRolloutWorkers) ?? undefined,
      estimatedEffectiveConcurrency: numberOrNull(projectedRuntime.estimatedEffectiveConcurrency) ?? undefined,
      rolloutsPerMinute: numberOrNull(projectedRuntime.rolloutsPerMinute) ?? undefined,
      rolloutSubmissionMode: optionalString(projectedRuntime.rolloutSubmissionMode),
      maxDispatchChunkSize: numberOrNull(projectedRuntime.maxDispatchChunkSize) ?? undefined,
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
  const dataset = record(projection.datasetSummary);
  const compute = record(projection.computeSummary);
  const curation = record(projection.curationSummary);
  const comparison = record(projection.comparisonSummary);
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
      ...dataset,
      splits: record(dataset.splits)
    },
    compute: { ...compute, producedAdapter: projection.producedAdapter ?? null },
    examples: [],
    lineage: {
      selectedCheckpointId: projection.selectedCheckpointId ?? null,
      producedAdapter: projection.producedAdapter ?? null
    },
    curation: {
      collected: numberOrNull(curation.collected),
      considered: numberOrNull(curation.considered),
      accepted: numberOrNull(curation.accepted),
      rejected: numberOrNull(curation.rejected),
      rejectionsByReason: record(curation.rejectionsByReason ?? curation.rejections_by_reason) as Record<string, number>,
      seedsCovered: numberOrNull(curation.seedsCovered ?? curation.seeds_covered),
      achievementsCovered: strings(curation.achievementsCovered ?? curation.achievements_covered),
      candidates: []
    },
    ...(Object.keys(comparison).length > 0
      ? {
          comparison: {
            splitDigest: optionalString(comparison.splitDigest ?? comparison.split_digest),
            baseLabel: optionalString(comparison.baseLabel ?? comparison.base_label) ?? "base",
            trainedLabel: optionalString(comparison.trainedLabel ?? comparison.trained_label) ?? "trained",
            pairs: []
          }
        }
      : {})
  };
}

function cispoProjection(base: ProjectedState, view: OptimizerRunViewV2Like, run: OptimizerRun): void {
  const projection = view.projection;
  sftProjection(base, view);
  const clip = record(projection.clipConfig);
  base.cispo = {
    objective: run.objective ?? "CISPO clipped-importance policy optimization",
    clipLow: numberOrNull(clip.clipLow ?? clip.clip_low ?? clip.low),
    clipHigh: numberOrNull(clip.clipHigh ?? clip.clip_high ?? clip.high),
    groupSize: numberOrNull(projection.groupSize),
    rewardVariance: numberOrNull(projection.rewardVariance),
    advantageMean: numberOrNull(projection.meanAdvantage),
    advantageStd: numberOrNull(projection.advantageStd),
    optimizerSteps: numberOrNull(projection.optimizerSteps) ?? (typeof view.header.usage.steps === "number" ? view.header.usage.steps : 0),
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

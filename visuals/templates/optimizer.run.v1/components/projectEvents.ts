/** Project optimizer_event.v1 fixtures into shared + algorithm slices at a cursor. */

import { formatMissingNumber, formatMissingUsd, missingNumber } from "../../../runtime/liveStream.ts";

export type ContainerRolloutRef = {
  kind: "container_rollout";
  id: string;
  role?: string;
  attributes?: {
    stream_id?: string;
    reward_url?: string;
    reward?: number | null;
    [key: string]: unknown;
  };
};

export type OptimizerEvent = {
  schemaVersion?: string;
  eventId?: string;
  type: string;
  sequenceNumber: number;
  occurredAt: string;
  optimizerRunId: string;
  algorithmId: string;
  level?: string;
  item?: {
    kind?: string;
    type?: string;
    id?: string;
    status?: string;
    raw?: Record<string, unknown>;
  };
  delta?: Record<string, unknown>;
  snapshot?: Record<string, unknown>;
  usageDelta?: Record<string, number | null>;
  artifactRefs?: unknown[];
  error?: unknown;
  raw?: unknown;
};

export type OptimizerRun = {
  id: string;
  algorithmId: string;
  status: string;
  source?: string;
  objective?: string;
  cursorSeq?: number;
  capabilities?: Record<string, boolean>;
  summary?: Record<string, unknown>;
  usage?: {
    costUsd?: number;
    promptTokens?: number;
    completionTokens?: number;
    rollouts?: number;
    wallTimeMs?: number;
  };
  executionBindings?: Array<Record<string, unknown>>;
  error?: unknown;
};

export type GepaLimit = {
  kind: string;
  max?: number;
  spent?: number;
  reserved?: number;
  remaining?: number;
  utilization?: number;
  hard?: boolean;
  source?: string;
  forecast?: {
    confidence?: string;
    model?: string;
    predictedCrossingAt?: string;
    predictedCrossingAtLow?: string;
    predictedCrossingAtHigh?: string;
    secondsToLimit?: number;
    secondsToLimitLow?: number;
    secondsToLimitHigh?: number;
    sampleCount?: number;
  };
};

export type GepaStageStatus = "pending" | "active" | "completed" | "skipped" | "failed";

export type GepaStage = {
  id: "seed" | "proposal" | "minibatch" | "full_train" | "heldout" | "complete";
  label: string;
  status: GepaStageStatus;
  startedAt?: string;
  endedAt?: string;
  detail?: string;
};

export type GepaDecision = {
  outcome: "accepted" | "rejected" | "deferred";
  gate: "minibatch" | "full_train" | "budget";
  reason?: string;
  comparison?: string;
  candidateScore?: number;
  parentScore?: number;
  incumbentId?: string;
  selectionObjective?: string;
  selectionDelta?: number;
  rationale?: string;
};

export type GepaFrontierSnapshot = {
  sequence: number;
  occurredAt?: string;
  generation?: number;
  reason?: string;
  bestCandidateId?: string;
  bestTrainReward?: number;
  bestCandidateSolved?: number;
  optimisticSolved?: number;
  totalExamples?: number;
  coverageSemantics?: string;
  frontierSize?: number;
  addedCandidateIds: string[];
  removedCandidateIds: string[];
};

export type GepaContract = {
  task?: { id?: string; name?: string; objective?: string; outputKind?: string };
  program?: { id?: string; mutableFields: string[] };
  objectiveSet?: {
    id?: string;
    hash?: string;
    frontierType?: string;
    selectionObjective?: string;
    objectives: Array<{ name: string; direction?: string; aggregation?: string; splitPolicy?: string }>;
  };
  splits?: { minibatch?: number; reflection?: number; pareto?: number; heldout?: number };
  container?: {
    runtimeFamily?: string;
    targetId?: string;
    rewardAuthority?: string;
    policyHarness?: string;
    policyConfig?: string;
    scaleLeases?: number;
    retention?: string;
  };
};

export type GepaEvaluation = {
  candidateId?: string;
  sequence: number;
  ref: ContainerRolloutRef;
  stage?: string;
  exampleId?: string;
  reward?: number | null;
  costUsd?: number;
  usage?: Record<string, unknown>;
  occurredAt?: string;
};

export type GepaFailedAttempt = {
  candidateId?: string;
  sequence: number;
  stage?: string;
  exampleId?: string;
  jobId?: string;
  attempt?: number;
  maxAttempts?: number;
  failureClass?: string;
  message?: string;
  occurredAt?: string;
};

export type GepaCoverage = {
  candidateId?: string;
  stage?: string;
  required: number;
  scored: number;
  failed: number;
  pending: number;
  complete: boolean;
  promotionEligible: boolean;
  sequence: number;
};

export type GepaTraceStep = {
  sequence: number;
  at?: string;
  kind: "context" | "generation" | "status" | "output" | "candidate";
  label: string;
  detail?: string;
  candidateId?: string;
};

export type GepaTruncatedText = {
  text?: string | null;
  truncated?: boolean;
  totalChars?: number;
};

export type GepaProposerReflection = {
  critique?: GepaTruncatedText;
  rationale?: GepaTruncatedText;
  failurePatterns: GepaTruncatedText[];
  winningPatterns: GepaTruncatedText[];
  candidateComparison?: GepaTruncatedText;
  proposals: Array<{
    proposalType?: string;
    parentCandidateIds?: string[];
    rationale?: GepaTruncatedText;
    proposedPayload?: GepaTruncatedText;
  }>;
};

export type GepaProposerTrace = {
  generation: number;
  sequence: number;
  status: string;
  runtimeEffectId?: string;
  jobId?: string;
  model?: string;
  provider?: string;
  backend?: string;
  runtimeSubstrate?: string;
  workspace?: string;
  wallSeconds?: number;
  costUsd?: number;
  usage?: Record<string, unknown>;
  warnings?: unknown[];
  startedAt?: string;
  endedAt?: string;
  parentCandidateId?: string;
  proposalCount?: number;
  lossCount?: number;
  candidateIds?: string[];
  steps?: GepaTraceStep[];
  /** Live text accumulated from `proposer.delta` chunks, keyed by channel. */
  streaming?: Record<string, string>;
  /** Reflection narrative from `proposer.transcript.loaded` (durable reopen). */
  reflection?: GepaProposerReflection;
  /** Canonical, user-visible Trace V5 items projected from the sealed app-server event log. */
  traceV5Items?: Array<{
    id: string;
    sequence: number;
    family: "input" | "thinking" | "tool" | "output" | "artifact" | "system";
    kind: string;
    title: string;
    occurredAt?: string;
    body?: string;
    detail?: string;
    status?: string;
  }>;
};

export type GepaState = {
  candidates: Array<Record<string, unknown>>;
  frontier: Array<Record<string, unknown>>;
  reflections: Array<Record<string, unknown>>;
  budget?: Record<string, unknown>;
  limits: GepaLimit[];
  nearestLimit?: GepaLimit;
  contract: GepaContract;
  frontierHistory: GepaFrontierSnapshot[];
  stages: GepaStage[];
  evaluations: GepaEvaluation[];
  failedAttempts: GepaFailedAttempt[];
  coverage: GepaCoverage[];
  proposerTraces: GepaProposerTrace[];
  activity: {
    phase: string;
    label: string;
    detail?: string;
    proposalActive: boolean;
    evaluationActive: boolean;
    evaluationStage?: string;
    activeCandidateIds: string[];
    generation?: number;
    requestedProposalCount?: number;
    sequence?: number;
    terminal: boolean;
  };
  incumbentId?: string;
  best?: { candidateId?: string; trainReward?: number; heldoutReward?: number };
  heldout?: { candidateId?: string; reward?: number; skipped?: boolean; blocked?: boolean; reason?: string };
  models: { proposer?: string; policy?: string };
  timing: { startedAt?: string; endedAt?: string; lastEventAt?: string };
  rolloutsCompleted: number;
  runtime: {
    activeWorkers?: number;
    semaphoreSize?: number;
    queuedRollouts?: number;
    rolloutsPerMinute?: number;
    reportedCostUsd?: number;
    costTelemetryComplete?: boolean;
    job?: {
      state: "running" | "completed" | "failed" | "cancelled" | "terminated";
      eventType?: string;
      reason?: string;
      message?: string;
      occurredAt?: string;
      rollingFailureRate?: number;
      tolerance?: number;
    };
  };
};

export type ProjectedState = {
  cursorSeq: number;
  summary: Record<string, unknown>;
  timeline: Array<Record<string, unknown>>;
  usage: Record<string, number | null>;
  logs: Array<Record<string, unknown>>;
  artifacts: unknown[];
  execution: { bindings: Array<Record<string, unknown>> };
  gepa?: GepaState;
  goex?: {
    board: Record<string, unknown>;
    themes: Array<Record<string, unknown>>;
    candidates: Array<Record<string, unknown>>;
    frontier: Record<string, unknown>;
    dataEngine: Record<string, unknown>;
    agents: Record<string, unknown>;
    rollouts: Array<{
      candidateId?: string;
      seed?: number;
      split?: string;
      lane?: string;
      status?: string;
      reward?: number | null;
      ref: ContainerRolloutRef;
    }>;
  };
  sft?: {
    curves: {
      steps: number[];
      epochs: number[];
      trainLoss: number[];
      validationLoss: number[];
      learningRate: number[];
    };
    /** Aligned per-step records. Family templates must not plot parallel arrays. */
    points: Array<{
      step: number;
      epoch?: number;
      trainLoss?: number;
      validationLoss?: number;
      learningRate?: number;
    }>;
    checkpoints: Array<Record<string, unknown>>;
    evaluations: Array<Record<string, unknown>>;
    campaigns: Array<{
      id: string;
      checkpointId?: string;
      status?: string;
      splitRole?: string;
      children: ContainerRolloutRef[];
    }>;
    dataset: Record<string, unknown>;
    compute: Record<string, unknown>;
    examples: Array<Record<string, unknown>>;
    lineage?: Record<string, unknown>;
  };
};

function optimizerFailureDetail(error: unknown): string | undefined {
  if (!error) return undefined;
  const value = typeof error === "object" && !Array.isArray(error)
    ? error as Record<string, unknown>
    : {};
  const source = [value.stderrTail, value.message, error]
    .find((candidate) => typeof candidate === "string") as string | undefined;
  if (!source) return undefined;
  const container = source.match(/container error:\s*([^\n]+)/i)?.[1]?.trim();
  if (container) return container;
  return source
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line && !line.includes("resource_tracker") && !line.startsWith("warnings.warn"));
}

export function isContainerRolloutRef(value: unknown): value is ContainerRolloutRef {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const rec = value as Record<string, unknown>;
  return rec.kind === "container_rollout" && typeof rec.id === "string";
}

export function refsFromUnknown(value: unknown): ContainerRolloutRef[] {
  if (isContainerRolloutRef(value)) return [value];
  if (Array.isArray(value)) return value.filter(isContainerRolloutRef);
  return [];
}

export function extractContainerRolloutRefs(events: OptimizerEvent[]): ContainerRolloutRef[] {
  const refs: ContainerRolloutRef[] = [];
  for (const event of events) {
    refs.push(...refsFromUnknown(event.delta?.resource_ref));
    refs.push(...refsFromUnknown(event.delta?.child_resource_ref));
    refs.push(...refsFromUnknown(event.delta?.children));
    refs.push(...refsFromUnknown(event.artifactRefs));
  }
  return refs;
}

export function formatChildEvalReward(ref: ContainerRolloutRef): string {
  return formatMissingNumber(ref.attributes?.reward);
}

export function formatChildEvalCost(ref: ContainerRolloutRef): string {
  return formatMissingUsd(ref.attributes?.cost_usd ?? ref.attributes?.costUsd);
}

export function fixtureHasEnvFrames(events: OptimizerEvent[]): boolean {
  return events.some((event) => {
    const kind = String(event.type ?? event.item?.kind ?? "").toLowerCase();
    return kind.includes("frame") || kind.includes("nev") || kind === "observation";
  });
}

export function projectAtCursor(
  run: OptimizerRun,
  events: OptimizerEvent[],
  atSeq?: number
): ProjectedState {
  const maxSeq = atSeq ?? Math.max(0, ...events.map((e) => e.sequenceNumber), run.cursorSeq ?? 0);
  const visible = events
    .filter((e) => e.sequenceNumber <= maxSeq)
    .sort((a, b) => a.sequenceNumber - b.sequenceNumber);

  const usage: Record<string, number | null> = {};
  let costReceiptSeen = false;
  let costTelemetryComplete = true;
  const timeline: Array<Record<string, unknown>> = [];
  const logs: Array<Record<string, unknown>> = [];
  const artifacts: unknown[] = [];
  const candidates = new Map<string, Record<string, unknown>>();
  let frontier: Array<Record<string, unknown>> = [];
  const reflections: Array<Record<string, unknown>> = [];
  const gepaEvaluations: GepaEvaluation[] = [];
  const gepaFailedAttempts: GepaFailedAttempt[] = [];
  const gepaCoverage = new Map<string, GepaCoverage>();
  const proposerTraces = new Map<number, GepaProposerTrace>();
  let budget: Record<string, unknown> | undefined;
  let limits: GepaLimit[] = [];
  let nearestLimit: GepaLimit | undefined;
  const contract: GepaContract = {
    program: { mutableFields: [] },
    objectiveSet: { objectives: [] }
  };
  const frontierHistory: GepaFrontierSnapshot[] = [];
  const stageOrder: Array<GepaStage["id"]> = ["seed", "proposal", "minibatch", "full_train", "heldout", "complete"];
  const stageLabels: Record<GepaStage["id"], string> = {
    seed: "Seed evaluation",
    proposal: "Reflection + proposal",
    minibatch: "Minibatch gate",
    full_train: "Full train evaluation",
    heldout: "Heldout",
    complete: "Complete"
  };
  const stageState = new Map<GepaStage["id"], GepaStage>();
  const markStage = (id: GepaStage["id"], next: GepaStageStatus, at?: string, detail?: string) => {
    const previous = stageState.get(id);
    const settled = previous && ["completed", "skipped", "failed"].includes(previous.status);
    if (settled && next === "active") return;
    stageState.set(id, {
      id,
      label: stageLabels[id],
      status: next,
      startedAt: previous?.startedAt ?? (next === "active" ? at || undefined : undefined),
      endedAt: ["completed", "skipped", "failed"].includes(next) ? at || undefined : previous?.endedAt,
      detail: detail ?? previous?.detail
    });
  };
  let best: GepaState["best"];
  let heldout: GepaState["heldout"];
  const models: GepaState["models"] = {};
  let rolloutsCompleted = 0;
  const rolloutCompletionTimes: number[] = [];
  const runtime: GepaState["runtime"] = {};
  let runStartedAt: string | undefined;
  let runEndedAt: string | undefined;
  let lastEventAt: string | undefined;
  let incumbentId: string | undefined;
  let activityPhase = "queued";
  let activityDetail: string | undefined;
  let activitySequence: number | undefined;
  let activityGeneration: number | undefined;
  let requestedProposalCount: number | undefined;
  let jobTermination: NonNullable<GepaState["runtime"]["job"]> | undefined;
  let proposalActive = false;
  let evaluationActive = false;
  let evaluationStage: string | undefined;
  const activeCandidateIds = new Set<string>();
  let board: Record<string, unknown> = { phase: "idle", tick: 0 };
  const themes: Array<Record<string, unknown>> = [];
  let goexCandidates: Array<Record<string, unknown>> = [];
  let goexFrontier: Record<string, unknown> = {};
  let goexDataEngine: Record<string, unknown> = {};
  let goexAgents: Record<string, unknown> = {};
  const goexEventCandidates = new Map<string, Record<string, unknown>>();
  const goexEventRollouts = new Map<string, Record<string, unknown>>();
  const checkpoints: Array<Record<string, unknown>> = [];
  const evaluations: Array<Record<string, unknown>> = [];
  const campaigns: Array<{
    id: string;
    checkpointId?: string;
    status?: string;
    splitRole?: string;
    children: ContainerRolloutRef[];
  }> = [];
  const curves = {
    steps: [] as number[],
    epochs: [] as number[],
    trainLoss: [] as number[],
    validationLoss: [] as number[],
    learningRate: [] as number[]
  };
  const points: Array<{
    step: number;
    epoch?: number;
    trainLoss?: number;
    validationLoss?: number;
    learningRate?: number;
  }> = [];
  let dataset: Record<string, unknown> = { splits: {} };
  let compute: Record<string, unknown> = {};
  let examples: Array<Record<string, unknown>> = [];
  let lineage: Record<string, unknown> = {};
  let status = run.status;
  let summary = { ...(run.summary ?? {}) };

  const candidateIdFrom = (event: OptimizerEvent): string | undefined => {
    const value = event.item?.id ?? event.delta?.candidate_id ?? event.delta?.candidateId;
    return typeof value === "string" && value ? value : undefined;
  };
  const candidateStatusFor = (event: OptimizerEvent): string | undefined => {
    if (event.item?.status) return event.item.status;
    if (event.type === "candidate.registered") return "registered";
    if (event.type === "candidate.evaluated") return "full_train_evaluated";
    if (event.type === "candidate.full_train_evaluated") return "full_train_evaluated";
    if (event.type === "candidate.minibatch_evaluated") return "minibatch_evaluated";
    if (event.type === "candidate.accepted") return "accepted";
    if (event.type === "candidate.rejected") {
      const score = event.delta?.score && typeof event.delta.score === "object" && !Array.isArray(event.delta.score)
        ? event.delta.score as Record<string, unknown>
        : {};
      return score.evaluation_stage === "candidate_full_train" || event.delta?.candidate_train_reward != null
        ? "rejected_full_train"
        : "rejected_minibatch";
    }
    if (event.type === "candidate.deferred") return "deferred_budget";
    if (event.type === "optimizer.candidate_evaluation.allocated" || event.type === "optimizer.child_rollout.attached") {
      return "evaluating";
    }
    return undefined;
  };
  const evaluationLabel = (stage?: string): string => {
    if (stage === "seed_full_train") return "Evaluating seed candidate";
    if (stage === "parent_minibatch_reference") return "Evaluating parent reference";
    if (stage === "candidate_minibatch") return "Evaluating proposed candidates";
    if (stage === "candidate_full_train") return "Evaluating full training set";
    if (stage === "heldout") return "Evaluating heldout frontier";
    return "Evaluating candidates";
  };
  const clearActiveCandidates = () => {
    for (const id of activeCandidateIds) {
      const candidate = candidates.get(id);
      if (!candidate || candidate.status !== "evaluating") continue;
      const { preEvaluationStatus, ...rest } = candidate;
      candidates.set(id, { ...rest, status: preEvaluationStatus ?? "registered" });
    }
    activeCandidateIds.clear();
  };

  const applyUsage = (source: Record<string, unknown>, replace = false) => {
    const nested = source.usage && typeof source.usage === "object" && !Array.isArray(source.usage)
      ? source.usage as Record<string, unknown>
      : {};
    const wallSeconds = missingNumber(source.wall_seconds ?? source.wallSeconds);
    const costSource = Object.prototype.hasOwnProperty.call(source, "cost_usd")
      ? source.cost_usd
      : Object.prototype.hasOwnProperty.call(source, "costUsd")
        ? source.costUsd
        : Object.prototype.hasOwnProperty.call(nested, "cost_usd")
          ? nested.cost_usd
          : Object.prototype.hasOwnProperty.call(nested, "costUsd")
            ? nested.costUsd
            : undefined;
    const costPresent = costSource !== undefined;
    const tokenUsagePresent = ["prompt_tokens", "promptTokens", "completion_tokens", "completionTokens"]
      .some((key) => Object.prototype.hasOwnProperty.call(source, key) || Object.prototype.hasOwnProperty.call(nested, key));
    const reportedCost = missingNumber(costSource);
    if (costPresent) {
      if (replace) {
        costReceiptSeen = true;
        costTelemetryComplete = costTelemetryComplete && reportedCost != null;
        usage.costUsd = costTelemetryComplete && reportedCost != null ? reportedCost : null;
      } else {
        costReceiptSeen = true;
        if (reportedCost == null) costTelemetryComplete = false;
        usage.costUsd = costTelemetryComplete
          ? (usage.costUsd ?? 0) + (reportedCost ?? 0)
          : null;
      }
    } else if (tokenUsagePresent) {
      costReceiptSeen = true;
      costTelemetryComplete = false;
      usage.costUsd = null;
    }
    const values = {
      promptTokens: missingNumber(
        source.prompt_tokens ?? source.promptTokens ?? nested.prompt_tokens ?? nested.promptTokens
      ),
      completionTokens: missingNumber(
        source.completion_tokens ?? source.completionTokens ??
          nested.completion_tokens ?? nested.completionTokens
      ),
      rollouts: missingNumber(source.rollouts ?? source.rollout_count ?? source.rolloutCount),
      wallTimeMs: missingNumber(source.wall_time_ms ?? source.wallTimeMs) ??
        (wallSeconds == null ? null : wallSeconds * 1000)
    };
    for (const [key, value] of Object.entries(values)) {
      if (value == null) continue;
      const current = usage[key];
      usage[key] = replace && key !== "wallTimeMs"
        ? value
        : (typeof current === "number" ? current : 0) + value;
    }
  };

  for (const event of visible) {
    timeline.push({
      sequence: event.sequenceNumber,
      type: event.type,
      occurredAt: event.occurredAt,
      itemId: event.item?.id
    });
    logs.push({
      sequence: event.sequenceNumber,
      type: event.type,
      occurredAt: event.occurredAt,
      message: event.delta?.message ?? null
    });
    if (event.artifactRefs) artifacts.push(...event.artifactRefs);
    if (!runStartedAt && event.occurredAt) runStartedAt = event.occurredAt;
    if (event.occurredAt) lastEventAt = event.occurredAt;
    if (event.usageDelta) {
      applyUsage(event.usageDelta);
    } else if (event.type === "runtime.job.completed") {
      applyUsage(event.delta ?? {});
    }
    if (event.type === "gepa.run.finished") {
      // GEPA's terminal event is the authoritative total. Replacing accumulated
      // counters avoids double counting while preserving summed job wall time.
      applyUsage(event.delta ?? {}, true);
    }
    const nextStatus = (
      event.snapshot?.status ??
      event.delta?.status ??
      (event.type === "optimizer.state.transitioned" ? event.delta?.to : undefined) ??
      (event.type === "gepa.run.finished" ? event.delta?.state : undefined)
    ) as string | undefined;
    if (nextStatus) status = nextStatus;
    if (event.type === "rollout.circuit_breaker.tripped") {
      const rollingFailureRate = missingNumber(event.delta?.rolling_failure_rate);
      const tolerance = missingNumber(event.delta?.tolerance);
      const reason = typeof event.delta?.reason === "string" ? event.delta.reason : "circuit_breaker_tripped";
      const message = typeof event.delta?.message === "string" ? event.delta.message : "Rollout circuit breaker tripped";
      status = "terminated";
      runEndedAt = event.occurredAt || runEndedAt;
      activityPhase = "terminated";
      activityDetail = rollingFailureRate != null && tolerance != null
        ? `${message}: ${(rollingFailureRate * 100).toFixed(2)}% failure rate exceeded ${(tolerance * 100).toFixed(2)}% tolerance`
        : `${message}: ${reason.replaceAll("_", " ")}`;
      activitySequence = event.sequenceNumber;
      proposalActive = false;
      evaluationActive = false;
      clearActiveCandidates();
      jobTermination = {
        state: "terminated",
        eventType: event.type,
        reason,
        message,
        occurredAt: event.occurredAt || undefined,
        rollingFailureRate: rollingFailureRate ?? undefined,
        tolerance: tolerance ?? undefined
      };
    }
    if (event.snapshot?.summary && typeof event.snapshot.summary === "object") {
      summary = { ...summary, ...(event.snapshot.summary as Record<string, unknown>) };
    }
    if (typeof event.snapshot?.bestScore === "number") summary.bestScore = event.snapshot.bestScore;

    const eventDelta = event.delta ?? {};
    if (event.type === "container.task_info.loaded") {
      contract.task = {
        id: typeof eventDelta.task_id === "string" ? eventDelta.task_id : undefined,
        name: typeof eventDelta.task_name === "string" ? eventDelta.task_name : undefined,
        objective: typeof eventDelta.objective === "string" ? eventDelta.objective : undefined,
        outputKind: typeof eventDelta.output_kind === "string" ? eventDelta.output_kind : undefined
      };
    }
    if (event.type === "container.program.loaded") {
      contract.program = {
        id: typeof eventDelta.program_id === "string" ? eventDelta.program_id : undefined,
        mutableFields: Array.isArray(eventDelta.mutable_fields) ? eventDelta.mutable_fields.map(String) : []
      };
    }
    if (event.type === "objective_set.declared") {
      const objectives = Array.isArray(eventDelta.objectives) ? eventDelta.objectives : [];
      contract.objectiveSet = {
        id: typeof eventDelta.objective_set_id === "string" ? eventDelta.objective_set_id : undefined,
        hash: typeof eventDelta.objective_set_hash === "string" ? eventDelta.objective_set_hash : undefined,
        frontierType: typeof eventDelta.frontier_type === "string" ? eventDelta.frontier_type : undefined,
        selectionObjective: typeof eventDelta.selection_objective === "string" ? eventDelta.selection_objective : undefined,
        objectives: objectives.filter((value): value is Record<string, unknown> => Boolean(value && typeof value === "object" && !Array.isArray(value))).map((value) => ({
          name: String(value.name ?? value.id ?? "objective"),
          direction: typeof value.direction === "string" ? value.direction : undefined,
          aggregation: typeof value.aggregation === "string" ? value.aggregation : undefined,
          splitPolicy: typeof value.split_policy === "string" ? value.split_policy : undefined
        }))
      };
    }
    if (event.type === "taskset.tasks.loaded") {
      contract.splits = {
        minibatch: missingNumber(eventDelta.minibatch_rows),
        reflection: missingNumber(eventDelta.reflection_rows),
        pareto: missingNumber(eventDelta.pareto_rows),
        heldout: missingNumber(eventDelta.heldout_rows)
      };
    }
    if (event.type === "container.contract.verified") {
      const refs = Array.isArray(eventDelta.policy_refs) ? eventDelta.policy_refs : [];
      const policy = refs.find((value) => value && typeof value === "object" && !Array.isArray(value)) as Record<string, unknown> | undefined;
      contract.container = {
        runtimeFamily: typeof eventDelta.runtime_family === "string" ? eventDelta.runtime_family : undefined,
        targetId: typeof eventDelta.target_id === "string" ? eventDelta.target_id : undefined,
        rewardAuthority: typeof eventDelta.reward_authority === "string" ? eventDelta.reward_authority : undefined,
        policyHarness: typeof policy?.harness === "string" ? policy.harness : undefined,
        policyConfig: typeof policy?.config === "string" ? policy.config : undefined,
        scaleLeases: missingNumber(eventDelta.scale_leases),
        retention: typeof eventDelta.retention === "string" ? eventDelta.retention : undefined
      };
    }

    if (event.type === "optimizer.state.transitioned") {
      const details = event.delta?.details && typeof event.delta.details === "object" && !Array.isArray(event.delta.details)
        ? event.delta.details as Record<string, unknown>
        : {};
      const nextPhase = typeof event.delta?.to === "string" ? event.delta.to : activityPhase;
      const message = typeof event.delta?.message === "string" ? event.delta.message : undefined;
      const trigger = typeof event.delta?.trigger === "string" ? event.delta.trigger : undefined;
      const generation = missingNumber(details.generation ?? event.delta?.generation);
      const proposalCount = missingNumber(details.proposal_count ?? event.delta?.proposal_count);
      activityPhase = nextPhase;
      activityDetail = message;
      activitySequence = event.sequenceNumber;
      if (generation != null) activityGeneration = generation;
      if (trigger === "proposer_started" && proposalCount != null) {
        requestedProposalCount = proposalCount;
      }
      if (trigger === "proposer_started" || nextPhase === "proposing") proposalActive = true;
      if (trigger === "proposer_finished") proposalActive = false;
      if (trigger === "rollouts_started" || trigger === "rollouts_queued") evaluationActive = true;
      if (trigger === "evaluation_finished") {
        evaluationActive = false;
        clearActiveCandidates();
      }
      if (trigger === "run_completed" || ["completed", "failed", "canceled"].includes(nextPhase)) {
        proposalActive = false;
        evaluationActive = false;
        clearActiveCandidates();
      }
      const lowerMessage = message?.toLowerCase() ?? "";
      if (lowerMessage.includes("seed candidate")) evaluationStage = "seed_full_train";
      else if (lowerMessage.includes("parent minibatch")) evaluationStage = "parent_minibatch_reference";
      else if (lowerMessage.includes("candidate minibatch")) evaluationStage = "candidate_minibatch";
      else if (lowerMessage.includes("full train")) evaluationStage = "candidate_full_train";
      else if (lowerMessage.includes("heldout")) evaluationStage = "heldout";
      const stageName = typeof details.stage === "string" ? details.stage : undefined;
      if (stageName) evaluationStage = stageName;
      if ((trigger === "rollouts_queued" || trigger === "rollouts_started") && evaluationStage) {
        if (evaluationStage === "seed_full_train") markStage("seed", "active", event.occurredAt);
        else if (evaluationStage === "parent_minibatch_reference" || evaluationStage === "candidate_minibatch") {
          markStage("minibatch", "active", event.occurredAt);
        } else if (evaluationStage === "candidate_full_train") markStage("full_train", "active", event.occurredAt);
        else if (evaluationStage === "heldout") markStage("heldout", "active", event.occurredAt);
      }
      if (trigger === "proposer_started") markStage("proposal", "active", event.occurredAt);
      if (trigger === "proposer_finished") markStage("proposal", "completed", event.occurredAt);
      if (trigger === "run_completed") runEndedAt = event.occurredAt || undefined;
      if (typeof details.policy_model === "string") models.policy = details.policy_model;
      if (typeof details.proposer_model === "string") models.proposer = details.proposer_model;
    }

    if (
      event.type.includes("candidate.") ||
      event.type === "gepa.candidate.updated"
    ) {
      const id = candidateIdFrom(event);
      if (id) {
        const previous = candidates.get(id) ?? {};
        const nextStatus = candidateStatusFor(event);
        const scoreRecord = event.delta?.score && typeof event.delta.score === "object" && !Array.isArray(event.delta.score)
          ? event.delta.score as Record<string, unknown>
          : {};
        const comparisonRecord = scoreRecord.comparison && typeof scoreRecord.comparison === "object" && !Array.isArray(scoreRecord.comparison)
          ? scoreRecord.comparison as Record<string, unknown>
          : {};
        const decisionGate: GepaDecision["gate"] = scoreRecord.evaluation_stage === "candidate_full_train" || event.delta?.candidate_train_reward != null
          ? "full_train"
          : "minibatch";
        const authoritativeCandidateScore = missingNumber(scoreRecord.challenger_selection_score);
        const authoritativeIncumbentScore = missingNumber(scoreRecord.incumbent_selection_score);
        const commonDecision = {
          gate: decisionGate,
          candidateScore: authoritativeCandidateScore ?? missingNumber(
            decisionGate === "full_train"
              ? event.delta?.candidate_train_reward ?? event.delta?.train_reward
              : event.delta?.candidate_minibatch_reward ?? event.delta?.minibatch_reward
          ),
          parentScore: authoritativeIncumbentScore ?? missingNumber(event.delta?.parent_minibatch_reward),
          incumbentId: typeof comparisonRecord.incumbent_candidate_id === "string" ? comparisonRecord.incumbent_candidate_id : incumbentId,
          selectionObjective: typeof scoreRecord.selection_objective === "string" ? scoreRecord.selection_objective : undefined,
          selectionDelta: missingNumber(scoreRecord.selection_delta),
          rationale: typeof comparisonRecord.rationale === "string" ? comparisonRecord.rationale : undefined,
          comparison: typeof comparisonRecord.result === "string"
            ? comparisonRecord.result
            : typeof scoreRecord.comparison_result === "string"
              ? scoreRecord.comparison_result
              : typeof event.delta?.comparison_result === "string" ? event.delta.comparison_result : undefined
        };
        const decision: GepaDecision | undefined =
          event.type === "candidate.accepted"
            ? {
                outcome: "accepted",
                ...commonDecision
              }
            : event.type === "candidate.rejected"
              ? {
                  outcome: "rejected",
                  ...commonDecision,
                  reason: typeof event.delta?.reason === "string" ? event.delta.reason : undefined,
                }
              : event.type === "candidate.deferred"
                ? {
                    outcome: "deferred",
                    gate: "budget",
                    reason: typeof event.delta?.reason === "string" ? event.delta.reason : "budget"
                  }
                : undefined;
        candidates.set(id, {
          ...previous,
          id,
          status: nextStatus ?? previous.status,
          ...(event.item?.raw ?? {}),
          ...(event.delta ?? {}),
          score: event.delta?.train_reward ?? event.delta?.candidate_train_reward ??
            event.delta?.minibatch_reward ?? event.delta?.candidate_minibatch_reward ?? previous.score,
          parentId: event.delta?.parent_id ?? event.delta?.parentId ?? previous.parentId,
          generation: missingNumber(event.delta?.generation) ?? previous.generation,
          minibatchReward: missingNumber(
            event.delta?.minibatch_reward ?? event.delta?.candidate_minibatch_reward
          ) ?? previous.minibatchReward,
          parentMinibatchReward: missingNumber(event.delta?.parent_minibatch_reward) ?? previous.parentMinibatchReward,
          minibatchDelta: missingNumber(event.delta?.minibatch_delta) ?? previous.minibatchDelta,
          registeredAt: event.type === "candidate.registered" ? event.occurredAt : previous.registeredAt,
          decision: decision ?? previous.decision,
          sequence: event.sequenceNumber
        });
        if (["candidate.evaluated", "candidate.minibatch_evaluated", "candidate.accepted", "candidate.rejected", "candidate.deferred"].includes(event.type)) {
          activeCandidateIds.delete(id);
        }
        if (event.type === "candidate.accepted") incumbentId = id;
        if (event.type === "candidate.evaluated") {
          const source = String(previous.source ?? event.delta?.source ?? "");
          const seedish = source === "seed" || String(event.delta?.message ?? "").toLowerCase().includes("seed");
          markStage(seedish ? "seed" : "full_train", "completed", event.occurredAt);
        }
        if (event.type === "candidate.minibatch_evaluated" || ((event.type === "candidate.accepted" || event.type === "candidate.rejected") && decision?.gate === "minibatch")) {
          markStage(
            "minibatch",
            "completed",
            event.occurredAt,
            decision ? `${id} ${decision.outcome}${decision.reason ? ` · ${decision.reason.replaceAll("_", " ")}` : ""}` : undefined
          );
        }
        if (event.type === "candidate.full_train_evaluated" || ((event.type === "candidate.accepted" || event.type === "candidate.rejected") && decision?.gate === "full_train")) {
          markStage("full_train", "completed", event.occurredAt, decision ? `${id} ${decision.outcome}` : undefined);
        }
        if (event.type === "candidate.registered" && event.delta?.source !== "seed") {
          const generation = missingNumber(event.delta?.generation);
          const trace = generation == null ? undefined : proposerTraces.get(generation);
          if (trace) {
            trace.candidateIds = [...new Set([...(trace.candidateIds ?? []), id])];
            trace.steps = [...(trace.steps ?? []), {
              sequence: event.sequenceNumber,
              at: event.occurredAt || undefined,
              kind: "candidate",
              label: `Registered candidate ${id}`,
              candidateId: id
            }];
          }
        }
      }
    }
    if (event.type === "optimizer.candidate_evaluation.allocated" || event.type === "optimizer.child_rollout.attached") {
      const id = candidateIdFrom(event);
      if (id) {
        const previous = candidates.get(id) ?? {};
        const stage = typeof event.delta?.stage === "string" ? event.delta.stage : evaluationStage;
        candidates.set(id, {
          ...previous,
          id,
          status: "evaluating",
          preEvaluationStatus: previous.status === "evaluating" ? previous.preEvaluationStatus : previous.status,
          stage,
          sequence: event.sequenceNumber
        });
        activeCandidateIds.add(id);
        evaluationActive = true;
        if (stage) evaluationStage = stage;
      }
    }
    if (event.type.startsWith("frontier.") || event.type === "gepa.frontier.updated") {
      const cells = event.snapshot?.cells ?? event.delta?.cells;
      if (Array.isArray(cells)) {
        frontier = cells as Array<Record<string, unknown>>;
      } else {
        const canonical = event.delta?.members ?? event.delta?.frontier;
        if (Array.isArray(canonical)) {
          const bestId = event.delta?.best_candidate_id;
          frontier = canonical.map((value) => {
            const member = value as Record<string, unknown>;
            const candidateId = member.candidate_id ?? member.candidateId;
            return {
              candidateId,
              quality: member.train_reward ?? member.trainReward,
              heldoutQuality: member.heldout_reward ?? member.heldoutReward,
              costUsd: member.cost_usd ?? member.costUsd,
              coveredExampleCount: member.covered_example_count ?? member.coveredExampleCount,
              evaluatedExampleCount: member.evaluated_example_count ?? member.evaluatedExampleCount,
              coverage: typeof member.covered_example_count === "number" && typeof member.evaluated_example_count === "number" && member.evaluated_example_count > 0
                ? member.covered_example_count / member.evaluated_example_count
                : member.coverage,
              accent: member.is_best === true || candidateId === bestId,
              status: member.status,
              parentId: member.parent_id ?? member.parentId
            };
          });
        }
      }
      const bestId = typeof event.delta?.best_candidate_id === "string" ? event.delta.best_candidate_id : undefined;
      const bestTrain = missingNumber(event.delta?.best_train_reward);
      if (bestId || bestTrain != null) {
        best = {
          candidateId: bestId ?? best?.candidateId,
          trainReward: bestTrain ?? best?.trainReward,
          heldoutReward: best?.heldoutReward
        };
      }
      if (bestTrain != null) summary = { ...summary, bestScore: bestTrain };
      const added = Array.isArray(event.delta?.added_candidate_ids) ? event.delta.added_candidate_ids.map(String) : [];
      const removed = Array.isArray(event.delta?.removed_candidate_ids) ? event.delta.removed_candidate_ids.map(String) : [];
      if (!frontierHistory.some((entry) => entry.sequence === event.sequenceNumber)) {
        frontierHistory.push({
          sequence: event.sequenceNumber,
          occurredAt: event.occurredAt || undefined,
          generation: missingNumber(event.delta?.generation),
          reason: typeof event.delta?.reason === "string" ? event.delta.reason : undefined,
          bestCandidateId: bestId,
          bestTrainReward: bestTrain ?? undefined,
          bestCandidateSolved: missingNumber(event.delta?.best_candidate_example_count),
          optimisticSolved: missingNumber(event.delta?.covered_train_example_count),
          totalExamples: missingNumber(event.delta?.train_example_count ?? event.delta?.train_row_count),
          coverageSemantics: typeof event.delta?.coverage_semantics === "string" ? event.delta.coverage_semantics : undefined,
          frontierSize: missingNumber(event.delta?.frontier_size) ?? frontier.length,
          addedCandidateIds: added,
          removedCandidateIds: removed
        });
      }
    }
    if (event.type === "gepa.reflection" || event.type === "proposer.completed") {
      reflections.push({
        sequence: event.sequenceNumber,
        occurredAt: event.occurredAt,
        message: event.delta?.message,
        ...(event.delta ?? {})
      });
    }
    if (event.type === "optimizer.state.transitioned" && event.delta?.trigger === "proposer_started") {
      const details = event.delta.details && typeof event.delta.details === "object" && !Array.isArray(event.delta.details)
        ? event.delta.details as Record<string, unknown>
        : {};
      const generation = missingNumber(details.generation) ?? 0;
      const parentCandidateId = typeof details.parent_candidate_id === "string" ? details.parent_candidate_id : undefined;
      const lossCount = missingNumber(details.loss_count);
      const rolloutRowCount = missingNumber(details.rollout_row_count);
      const model = typeof details.model === "string" ? details.model : undefined;
      if (model) models.proposer = model;
      const contextDetail = [
        parentCandidateId ? `parent ${parentCandidateId}` : null,
        lossCount != null ? `${lossCount} failing rollouts to study` : null,
        rolloutRowCount != null ? `${rolloutRowCount} rollout rows of evidence` : null
      ].filter(Boolean).join(" · ");
      proposerTraces.set(generation, {
        generation,
        sequence: event.sequenceNumber,
        status: "running",
        model,
        provider: typeof details.provider === "string" ? details.provider : undefined,
        backend: typeof details.backend === "string" ? details.backend : undefined,
        workspace: typeof details.workspace === "string" ? details.workspace : undefined,
        startedAt: event.occurredAt || undefined,
        parentCandidateId,
        lossCount,
        candidateIds: [],
        steps: [
          {
            sequence: event.sequenceNumber,
            at: event.occurredAt || undefined,
            kind: "context",
            label: "Reflection context assembled",
            detail: contextDetail || undefined
          },
          {
            sequence: event.sequenceNumber,
            at: event.occurredAt || undefined,
            kind: "generation",
            label: model ? `${model} is drafting a proposal` : "Proposer model is drafting a proposal"
          }
        ]
      });
    }
    if (event.type === "proposer.started") {
      const generation = missingNumber(event.delta?.generation) ?? 0;
      const existing = proposerTraces.get(generation);
      const model = typeof event.delta?.model === "string" ? event.delta.model : existing?.model;
      if (model) models.proposer = model;
      proposerTraces.set(generation, {
        ...(existing ?? {}),
        generation,
        sequence: event.sequenceNumber,
        status: "running",
        model,
        provider: typeof event.delta?.provider === "string" ? event.delta.provider : existing?.provider,
        backend: typeof event.delta?.backend === "string" ? event.delta.backend : existing?.backend,
        workspace: typeof event.delta?.workspace === "string" ? event.delta.workspace : existing?.workspace,
        startedAt: existing?.startedAt ?? (event.occurredAt || undefined),
        candidateIds: existing?.candidateIds ?? [],
        steps: existing?.steps ?? [{
          sequence: event.sequenceNumber,
          at: event.occurredAt || undefined,
          kind: "generation",
          label: model ? `${model} is drafting a proposal` : "Proposer model is drafting a proposal"
        }]
      });
      proposalActive = true;
    }
    if (event.type === "runtime.job.completed" && event.delta?.lane === "proposer") {
      const generation = missingNumber(event.delta.generation) ?? 0;
      const existing = proposerTraces.get(generation);
      const wallSeconds = missingNumber(event.delta.wall_seconds) ?? undefined;
      const jobUsage = event.delta.usage && typeof event.delta.usage === "object" && !Array.isArray(event.delta.usage)
        ? event.delta.usage as Record<string, unknown>
        : undefined;
      const totalTokens = missingNumber(jobUsage?.total_tokens ?? event.delta.total_tokens);
      const statusDetail = [
        wallSeconds != null ? `${wallSeconds.toFixed(1)} s` : null,
        totalTokens != null ? `${totalTokens.toLocaleString()} tokens` : null
      ].filter(Boolean).join(" · ");
      proposerTraces.set(generation, {
        ...(existing ?? {}),
        generation,
        sequence: event.sequenceNumber,
        status: "completed",
        runtimeEffectId: typeof event.delta.runtime_effect_id === "string" ? event.delta.runtime_effect_id : existing?.runtimeEffectId,
        jobId: typeof event.delta.job_id === "string" ? event.delta.job_id : existing?.jobId,
        model: typeof event.delta.model === "string" ? event.delta.model : existing?.model,
        backend: typeof event.delta.backend === "string" ? event.delta.backend : existing?.backend,
        wallSeconds: wallSeconds ?? existing?.wallSeconds,
        costUsd: missingNumber(event.delta.cost_usd) ?? existing?.costUsd,
        usage: jobUsage ?? existing?.usage,
        steps: [...(existing?.steps ?? []), {
          sequence: event.sequenceNumber,
          at: event.occurredAt || undefined,
          kind: "status",
          label: "Proposer call finished",
          detail: statusDetail || undefined
        }]
      });
    }
    if (event.type === "proposer.completed") {
      const generation = missingNumber(event.delta?.generation) ?? 0;
      const existing = proposerTraces.get(generation);
      const proposalCount = missingNumber(event.delta?.proposal_count);
      const model = typeof event.delta?.model === "string" ? event.delta.model : existing?.model;
      if (model) models.proposer = model;
      proposerTraces.set(generation, {
        ...(existing ?? {}),
        generation,
        status: "completed",
        provider: typeof event.delta?.provider === "string" ? event.delta.provider : existing?.provider,
        backend: typeof event.delta?.backend === "string" ? event.delta.backend : existing?.backend,
        model,
        runtimeSubstrate: typeof event.delta?.runtime_substrate === "string" ? event.delta.runtime_substrate : existing?.runtimeSubstrate,
        workspace: typeof event.delta?.workspace === "string" ? event.delta.workspace : existing?.workspace,
        warnings: Array.isArray(event.delta?.warnings) ? event.delta.warnings : existing?.warnings,
        endedAt: event.occurredAt || undefined,
        proposalCount: proposalCount ?? existing?.proposalCount,
        sequence: event.sequenceNumber,
        steps: [...(existing?.steps ?? []), {
          sequence: event.sequenceNumber,
          at: event.occurredAt || undefined,
          kind: "output",
          label: proposalCount != null
            ? `Returned ${proposalCount} proposal${proposalCount === 1 ? "" : "s"}`
            : "Returned proposals"
        }]
      });
      markStage("proposal", "completed", event.occurredAt);
      proposalActive = false;
      activityPhase = "proposal_ready";
      activityDetail = typeof event.delta?.message === "string" ? event.delta.message : "Proposer returned candidates";
      activitySequence = event.sequenceNumber;
    }
    if (event.type === "proposer.delta") {
      // Live proposer content chunks, mirroring span.policy.data: each chunk
      // extends one open trace in place rather than creating a new row.
      const generation = missingNumber(event.delta?.generation) ?? 0;
      const channel = typeof event.delta?.channel === "string" ? event.delta.channel : "content";
      const text = typeof event.delta?.text === "string" ? event.delta.text : "";
      const existing = proposerTraces.get(generation);
      if (text) {
        proposerTraces.set(generation, {
          ...(existing ?? {
            generation,
            sequence: event.sequenceNumber,
            status: "running",
            candidateIds: [],
            steps: []
          }),
          generation,
          status: existing?.status === "completed" ? "completed" : "running",
          startedAt: existing?.startedAt ?? (event.occurredAt || undefined),
          streaming: {
            ...(existing?.streaming ?? {}),
            [channel]: `${existing?.streaming?.[channel] ?? ""}${text}`
          }
        });
        proposalActive = proposalActive || existing?.status !== "completed";
      }
    }
    if (event.type === "proposer.transcript.loaded") {
      const generation = missingNumber(event.delta?.generation) ?? 0;
      const existing = proposerTraces.get(generation);
      const asTruncated = (value: unknown): GepaTruncatedText | undefined => {
        if (!value || typeof value !== "object" || Array.isArray(value)) {
          return typeof value === "string" ? { text: value, truncated: false } : undefined;
        }
        const rec = value as Record<string, unknown>;
        const text = typeof rec.text === "string" ? rec.text : rec.text === null ? null : undefined;
        if (text === undefined) return undefined;
        return {
          text,
          truncated: rec.truncated === true,
          totalChars: missingNumber(rec.total_chars ?? rec.totalChars)
        };
      };
      const asTruncatedList = (value: unknown): GepaTruncatedText[] =>
        Array.isArray(value)
          ? value.map(asTruncated).filter((entry): entry is GepaTruncatedText => entry != null && entry.text != null)
          : [];
      const proposals = Array.isArray(event.delta?.proposals)
        ? (event.delta.proposals as Array<Record<string, unknown>>).map((proposal) => ({
            proposalType: typeof proposal.proposal_type === "string" ? proposal.proposal_type : undefined,
            parentCandidateIds: Array.isArray(proposal.parent_candidate_ids)
              ? proposal.parent_candidate_ids.map(String)
              : undefined,
            rationale: asTruncated(proposal.rationale),
            proposedPayload: asTruncated(proposal.proposed_payload)
          }))
        : [];
      proposerTraces.set(generation, {
        ...(existing ?? {
          generation,
          sequence: event.sequenceNumber,
          status: "completed",
          candidateIds: [],
          steps: []
        }),
        generation,
        reflection: {
          critique: asTruncated(event.delta?.critique),
          rationale: asTruncated(event.delta?.rationale),
          failurePatterns: asTruncatedList(event.delta?.failure_patterns),
          winningPatterns: asTruncatedList(event.delta?.winning_patterns),
          candidateComparison: asTruncated(event.delta?.candidate_comparison),
          proposals
        }
      });
    }
    if (event.type === "proposer.trace_v5.loaded") {
      const generation = missingNumber(event.delta?.generation);
      const items = event.delta?.items;
      if (generation != null && Array.isArray(items)) {
        const existing = proposerTraces.get(generation);
        proposerTraces.set(generation, {
          ...(existing ?? { generation, sequence: event.sequenceNumber, status: "completed", candidateIds: [], steps: [] }),
          traceV5Items: items.filter((item): item is NonNullable<GepaProposerTrace["traceV5Items"]>[number] =>
            Boolean(item && typeof item === "object" && !Array.isArray(item) && typeof (item as Record<string, unknown>).id === "string")
          ) as NonNullable<GepaProposerTrace["traceV5Items"]>
        });
      }
    }
    if (event.type === "heldout.completed") {
      const id = candidateIdFrom(event);
      if (id) {
        activeCandidateIds.delete(id);
        const previous = candidates.get(id) ?? {};
        const { preEvaluationStatus, ...rest } = previous;
        candidates.set(id, {
          ...rest,
          id,
          status: preEvaluationStatus ?? "full_train_evaluated",
          heldout_reward: event.delta?.heldout_reward ?? event.delta?.reward ?? previous.heldout_reward,
          sequence: event.sequenceNumber
        });
      }
      const reward = missingNumber(event.delta?.heldout_reward ?? event.delta?.reward);
      heldout = { candidateId: id ?? heldout?.candidateId, reward: reward ?? heldout?.reward };
      if (reward != null) best = { ...(best ?? {}), heldoutReward: reward };
      markStage("heldout", "completed", event.occurredAt);
    }
    if (event.type === "optimizer.limit.estimate_updated" && Array.isArray(event.delta?.limits)) {
      const parseLimit = (limit: Record<string, unknown>): GepaLimit => {
        const forecast = limit.forecast && typeof limit.forecast === "object" && !Array.isArray(limit.forecast)
          ? limit.forecast as Record<string, unknown>
          : {};
        return {
        kind: String(limit.kind ?? "unknown"),
        max: missingNumber(limit.max_value ?? limit.max),
        spent: missingNumber(limit.spent),
        reserved: missingNumber(limit.reserved),
        remaining: missingNumber(limit.remaining),
        utilization: missingNumber(limit.utilization),
        hard: typeof limit.hard === "boolean" ? limit.hard : undefined,
        source: typeof limit.source === "string" ? limit.source : undefined,
        forecast: Object.keys(forecast).length ? {
          confidence: typeof forecast.confidence === "string" ? forecast.confidence : undefined,
          model: typeof forecast.model === "string" ? forecast.model : undefined,
          predictedCrossingAt: typeof forecast.predicted_crossing_at === "string" ? forecast.predicted_crossing_at : undefined,
          predictedCrossingAtLow: typeof forecast.predicted_crossing_at_low === "string" ? forecast.predicted_crossing_at_low : undefined,
          predictedCrossingAtHigh: typeof forecast.predicted_crossing_at_high === "string" ? forecast.predicted_crossing_at_high : undefined,
          secondsToLimit: missingNumber(forecast.seconds_to_limit),
          secondsToLimitLow: missingNumber(forecast.seconds_to_limit_low),
          secondsToLimitHigh: missingNumber(forecast.seconds_to_limit_high),
          sampleCount: missingNumber(forecast.sample_count)
        } : undefined
      };
      };
      limits = (event.delta.limits as Array<Record<string, unknown>>).map(parseLimit);
      if (event.delta.nearest && typeof event.delta.nearest === "object" && !Array.isArray(event.delta.nearest)) {
        nearestLimit = parseLimit(event.delta.nearest as Record<string, unknown>);
      }
    }
    if (event.type === "gepa.run.finished") {
      runEndedAt = event.occurredAt || undefined;
      markStage("complete", "completed", event.occurredAt);
      const bestId = typeof event.delta?.best_candidate_id === "string" ? event.delta.best_candidate_id : undefined;
      const finalHeldout = missingNumber(event.delta?.heldout_reward);
      if (bestId || finalHeldout != null) {
        best = {
          candidateId: bestId ?? best?.candidateId,
          trainReward: best?.trainReward,
          heldoutReward: finalHeldout ?? best?.heldoutReward
        };
      }
      if (event.delta?.heldout_skipped === true) {
        heldout = { ...(heldout ?? {}), skipped: true };
        markStage("heldout", "skipped", event.occurredAt);
      }
      const finishedUsage = event.delta?.usage && typeof event.delta.usage === "object" && !Array.isArray(event.delta.usage)
        ? event.delta.usage as Record<string, unknown>
        : {};
      const reconcile = (kind: string, spent?: number) => {
        if (spent == null) return;
        const existing = limits.find((limit) => limit.kind === kind);
        if (existing) {
          existing.spent = spent;
          if (existing.max != null) {
            existing.remaining = Math.max(0, existing.max - spent);
            existing.utilization = existing.max > 0 ? spent / existing.max : existing.utilization;
          }
        } else {
          limits.push({ kind, spent });
        }
      };
      reconcile("total_rollouts", missingNumber(finishedUsage.rollout_calls ?? event.delta?.rollout_count));
      reconcile("cost_usd", missingNumber(event.delta?.cost_usd));
      reconcile("proposer_calls", missingNumber(finishedUsage.proposer_calls));
    }
    if (
      event.type === "gepa.evaluation.linked" ||
      event.type === "candidate.evaluation.linked" ||
      event.type === "optimizer.child_rollout.attached"
    ) {
      const refs = [
        ...refsFromUnknown(event.delta?.resource_ref),
        ...refsFromUnknown(event.delta?.child_resource_ref),
        ...refsFromUnknown(event.artifactRefs)
      ];
      for (const ref of refs) {
        const candidateId = event.item?.id ??
          (typeof event.delta?.candidate_id === "string" ? event.delta.candidate_id : undefined);
        const stage = typeof event.delta?.stage === "string" ? event.delta.stage : undefined;
        const exampleId = typeof event.delta?.example_id === "string" ? event.delta.example_id : undefined;
        const existing = gepaEvaluations.find((entry) =>
          entry.ref.id === ref.id ||
          (entry.candidateId === candidateId && entry.stage === stage && entry.exampleId === exampleId)
        );
        if (existing) {
          existing.ref = ref;
          continue;
        }
        gepaEvaluations.push({
          candidateId,
          sequence: event.sequenceNumber,
          ref,
          stage,
          exampleId,
          occurredAt: event.occurredAt || undefined
        });
      }
    }
    if (event.type === "optimizer.rollout_queue.updated") {
      const activeWorkers = missingNumber(event.delta?.active_workers);
      const semaphoreSize = missingNumber(event.delta?.semaphore_size);
      const queuedRollouts = missingNumber(event.delta?.queued_rollouts);
      if (activeWorkers != null) runtime.activeWorkers = activeWorkers;
      if (semaphoreSize != null) runtime.semaphoreSize = semaphoreSize;
      if (queuedRollouts != null) runtime.queuedRollouts = queuedRollouts;
    }
    if (event.type === "optimizer.evaluation_result.received") {
      const rolloutId = typeof event.delta?.rollout_id === "string"
        ? event.delta.rollout_id
        : refsFromUnknown(event.delta?.child_resource_ref)[0]?.id;
      const candidateId = candidateIdFrom(event);
      const stage = typeof event.delta?.stage === "string" ? event.delta.stage : undefined;
      const exampleId = typeof event.delta?.example_id === "string" ? event.delta.example_id : undefined;
      let evaluation = gepaEvaluations.find((entry) =>
        (rolloutId != null && entry.ref.id === rolloutId) ||
        (entry.candidateId === candidateId && entry.stage === stage && entry.exampleId === exampleId)
      );
      if (!evaluation) {
        const childRef = refsFromUnknown(event.delta?.child_resource_ref)[0];
        if (childRef) {
          evaluation = {
            candidateId,
            sequence: event.sequenceNumber,
            ref: childRef,
            stage,
            exampleId,
            occurredAt: event.occurredAt || undefined
          };
          gepaEvaluations.push(evaluation);
        }
      }
      const firstCompletion = evaluation?.reward == null;
      if (firstCompletion) {
        rolloutsCompleted += 1;
        const completedAt = Date.parse(event.occurredAt);
        if (Number.isFinite(completedAt)) rolloutCompletionTimes.push(completedAt);
        const reportedCost = missingNumber(event.delta?.cost_usd);
        if (reportedCost == null) {
          runtime.costTelemetryComplete = false;
          delete runtime.reportedCostUsd;
        } else if (runtime.costTelemetryComplete !== false) {
          runtime.costTelemetryComplete = true;
          runtime.reportedCostUsd = (runtime.reportedCostUsd ?? 0) + reportedCost;
        }
      }
      const activeWorkers = missingNumber(event.delta?.active_workers);
      const semaphoreSize = missingNumber(event.delta?.semaphore_size);
      const queuedRollouts = missingNumber(event.delta?.queued_rollouts);
      if (activeWorkers != null) runtime.activeWorkers = activeWorkers;
      if (semaphoreSize != null) runtime.semaphoreSize = semaphoreSize;
      if (queuedRollouts != null) runtime.queuedRollouts = queuedRollouts;
      if (evaluation) {
        evaluation.reward = missingNumber(event.delta?.reward);
        evaluation.costUsd = missingNumber(event.delta?.cost_usd) ?? undefined;
        evaluation.usage = event.delta?.usage && typeof event.delta.usage === "object" && !Array.isArray(event.delta.usage)
          ? event.delta.usage as Record<string, unknown>
          : undefined;
        evaluation.ref = {
          ...evaluation.ref,
          attributes: { ...(evaluation.ref.attributes ?? {}), reward: evaluation.reward }
        };
      }
    }
    if (event.type === "optimizer.candidate_evaluation.attempt.failed") {
      const failure = event.delta?.failure && typeof event.delta.failure === "object" && !Array.isArray(event.delta.failure)
        ? event.delta.failure as Record<string, unknown>
        : {};
      gepaFailedAttempts.push({
        candidateId: candidateIdFrom(event),
        sequence: event.sequenceNumber,
        stage: typeof event.delta?.stage === "string" ? event.delta.stage : undefined,
        exampleId: typeof event.delta?.example_id === "string" ? event.delta.example_id : undefined,
        jobId: typeof event.delta?.job_id === "string" ? event.delta.job_id : undefined,
        attempt: missingNumber(event.delta?.attempt),
        maxAttempts: missingNumber(event.delta?.max_attempts),
        failureClass: typeof failure.reason_code === "string"
          ? failure.reason_code
          : typeof failure.failure_type === "string" ? failure.failure_type : undefined,
        message: typeof failure.message === "string" ? failure.message : undefined,
        occurredAt: event.occurredAt || undefined
      });
    }
    if (event.type === "optimizer.evaluation.coverage.updated") {
      const candidateId = candidateIdFrom(event);
      const stage = typeof event.delta?.stage === "string" ? event.delta.stage : undefined;
      const required = missingNumber(event.delta?.required ?? event.delta?.required_rollout_count) ?? 0;
      const scored = missingNumber(event.delta?.scored ?? event.delta?.scored_rollout_count) ?? 0;
      const failed = missingNumber(event.delta?.failed ?? event.delta?.failed_rollout_count) ?? 0;
      const pending = missingNumber(event.delta?.pending ?? event.delta?.pending_rollout_count) ?? Math.max(0, required - scored - failed);
      gepaCoverage.set(`${candidateId ?? "run"}::${stage ?? "unknown"}`, {
        candidateId,
        stage,
        required,
        scored,
        failed,
        pending,
        complete: required > 0 && scored === required && failed === 0 && pending === 0,
        promotionEligible: event.delta?.promotion_eligible === true || event.delta?.complete === true,
        sequence: event.sequenceNumber
      });
    }
    if (event.type === "heldout.blocked") {
      const candidateId = candidateIdFrom(event);
      const reason = typeof event.delta?.reason === "string" ? event.delta.reason : "incomplete_evidence";
      heldout = { candidateId, blocked: true, reason };
      markStage("heldout", "failed", event.occurredAt, "Promotion blocked: heldout evidence is incomplete");
    }
    if (event.type === "gepa.budget.updated" || event.type === "budget.updated") {
      budget = { ...(event.snapshot ?? event.delta ?? {}) };
    }
    if (typeof event.snapshot?.incumbentId === "string") {
      incumbentId = event.snapshot.incumbentId;
    }
    if (typeof event.delta?.incumbentId === "string") {
      incumbentId = String(event.delta.incumbentId);
    }
    if (event.type.includes("board.updated")) {
      board = { ...(event.snapshot ?? event.delta ?? {}) };
    }
    if (event.type.includes("theme.updated")) {
      themes.push({ sequence: event.sequenceNumber, ...(event.delta ?? {}) });
    }
    if (event.type === "goex.tick_transition") {
      board = {
        ...board,
        phase: event.delta?.to_phase ?? board.phase,
        previousPhase: event.delta?.from_phase,
        tick: event.delta?.tick_index ?? board.tick,
        reason: event.delta?.reason,
        occurredAt: event.occurredAt
      };
    }
    if (event.type === "goex.core_proposer_started" || event.type === "goex.core_proposer_finished") {
      const existing = goexAgents.coreProposer && typeof goexAgents.coreProposer === "object" && !Array.isArray(goexAgents.coreProposer)
        ? goexAgents.coreProposer as Record<string, unknown>
        : {};
      goexAgents = {
        ...goexAgents,
        coreProposer: {
          ...existing,
          status: event.type.endsWith("started") ? "running" : "completed",
          sequence: event.sequenceNumber,
          ...(event.delta ?? {})
        }
      };
    }
    if (event.type === "goex.seed_candidate_registered") {
      const id = typeof event.delta?.candidate_id === "string" ? event.delta.candidate_id : undefined;
      if (id) goexEventCandidates.set(id, { ...(goexEventCandidates.get(id) ?? {}), id, candidate_id: id, status: "registered", ...(event.delta ?? {}) });
    }
    if (run.algorithmId === "go-ex" && event.type === "candidate.registered") {
      const id = typeof event.delta?.candidate_id === "string" ? event.delta.candidate_id : undefined;
      if (id) goexEventCandidates.set(id, { ...(goexEventCandidates.get(id) ?? {}), id, candidate_id: id, ...(event.delta ?? {}) });
    }
    if (run.algorithmId === "go-ex" && event.type === "candidate.evaluated") {
      const id = typeof event.delta?.candidate_id === "string" ? event.delta.candidate_id : undefined;
      if (id) goexEventCandidates.set(id, { ...(goexEventCandidates.get(id) ?? {}), id, candidate_id: id, ...(event.delta ?? {}) });
    }
    if (run.algorithmId === "go-ex" && event.type === "goex.acceptance_completed") {
      const championId = typeof event.delta?.champion_candidate_id === "string" ? event.delta.champion_candidate_id : undefined;
      const baselineId = typeof event.delta?.baseline_candidate_id === "string" ? event.delta.baseline_candidate_id : undefined;
      if (championId) {
        goexEventCandidates.set(championId, {
          ...(goexEventCandidates.get(championId) ?? {}),
          id: championId,
          candidate_id: championId,
          status: "accepted",
          decision: "accepted",
          on_frontier: true
        });
        const currentFrontier = goexFrontier.candidate_frontier && typeof goexFrontier.candidate_frontier === "object" && !Array.isArray(goexFrontier.candidate_frontier)
          ? goexFrontier.candidate_frontier as Record<string, unknown>
          : {};
        goexFrontier = { ...goexFrontier, candidate_frontier: { ...currentFrontier, global: [championId] } };
      }
      if (baselineId && baselineId !== championId) {
        goexEventCandidates.set(baselineId, {
          ...(goexEventCandidates.get(baselineId) ?? {}),
          id: baselineId,
          candidate_id: baselineId,
          status: "rejected",
          decision: "rejected"
        });
      }
    }
    if (run.algorithmId === "go-ex" && event.type === "proposer.delta") {
      const text = typeof event.delta?.text === "string" ? event.delta.text : "";
      const existing = goexAgents.coreProposer && typeof goexAgents.coreProposer === "object" && !Array.isArray(goexAgents.coreProposer)
        ? goexAgents.coreProposer as Record<string, unknown>
        : {};
      const streaming = existing.streaming && typeof existing.streaming === "object" && !Array.isArray(existing.streaming)
        ? existing.streaming as Record<string, unknown>
        : {};
      const channel = typeof event.delta?.channel === "string" ? event.delta.channel : "content";
      goexAgents = {
        ...goexAgents,
        coreProposer: {
          ...existing,
          status: existing.status === "completed" ? "completed" : "running",
          streaming: { ...streaming, [channel]: `${String(streaming[channel] ?? "")}${text}` },
          sequence: event.sequenceNumber
        }
      };
    }
    if (event.type === "goex.best_base_decided") {
      const id = typeof event.delta?.candidate_id === "string" ? event.delta.candidate_id : undefined;
      if (id) goexEventCandidates.set(id, { ...(goexEventCandidates.get(id) ?? {}), id, candidate_id: id, status: "best_base", on_frontier: true, ...(event.delta ?? {}) });
    }
    if (event.type === "goex.theme_state_changed") {
      const id = typeof event.delta?.theme_id === "string" ? event.delta.theme_id : undefined;
      if (id) {
        const index = themes.findIndex((theme) => theme.theme_id === id);
        const next = { ...(index >= 0 ? themes[index] : {}), ...(event.delta ?? {}), status: event.delta?.to };
        if (index >= 0) themes[index] = next; else themes.push(next);
      }
    }
    if (event.type.startsWith("child.rollout.")) {
      const resource = event.delta?.resource_ref && typeof event.delta.resource_ref === "object" && !Array.isArray(event.delta.resource_ref)
        ? event.delta.resource_ref as Record<string, unknown>
        : {};
      const id = typeof resource.rollout_id === "string" ? resource.rollout_id : undefined;
      if (id) {
        const stream = resource.stream && typeof resource.stream === "object" && !Array.isArray(resource.stream)
          ? resource.stream as Record<string, unknown>
          : {};
        goexEventRollouts.set(id, {
          ...(goexEventRollouts.get(id) ?? {}),
          rollout_id: id,
          candidate_id: event.delta?.candidate_id,
          split: event.delta?.split,
          lane: event.delta?.evaluation_stage,
          status: event.delta?.status,
          reward: event.delta?.reward,
          metadata: { stream }
        });
      }
    }
    if (event.type === "goex.state.batch.updated") {
      const slices = event.snapshot?.slices;
      const sliceMap = slices && typeof slices === "object" && !Array.isArray(slices)
        ? slices as Record<string, unknown>
        : {};
      const dataOf = (name: string): unknown => {
        const envelope = sliceMap[name];
        return envelope && typeof envelope === "object" && !Array.isArray(envelope)
          ? (envelope as Record<string, unknown>).data
          : undefined;
      };
      const boardData = dataOf("board");
      if (boardData && typeof boardData === "object" && !Array.isArray(boardData)) {
        const boardRecord = boardData as Record<string, unknown>;
        const boardSummary = boardRecord.summary && typeof boardRecord.summary === "object" && !Array.isArray(boardRecord.summary)
          ? boardRecord.summary as Record<string, unknown>
          : {};
        board = {
          ...board,
          ...boardRecord,
          ...boardSummary,
          phase: boardSummary.phase ?? boardRecord.phase ?? board.phase,
          tick: boardSummary.tick_index ?? boardRecord.tick_index ?? board.tick,
        };
      }
      const themeData = dataOf("themes");
      const themeRows = themeData && typeof themeData === "object" && !Array.isArray(themeData)
        ? (themeData as Record<string, unknown>).themes
        : themeData;
      if (Array.isArray(themeRows)) {
        themes.splice(0, themes.length, ...(themeRows as Array<Record<string, unknown>>));
      }
      const candidateData = dataOf("candidates");
      const candidateRows = candidateData && typeof candidateData === "object" && !Array.isArray(candidateData)
        ? (candidateData as Record<string, unknown>).candidates
        : candidateData;
      if (Array.isArray(candidateRows)) goexCandidates = candidateRows as Array<Record<string, unknown>>;
      const frontierData = dataOf("frontier");
      if (frontierData && typeof frontierData === "object" && !Array.isArray(frontierData)) {
        goexFrontier = frontierData as Record<string, unknown>;
      }
      const dataEngine = dataOf("data-engine");
      if (dataEngine && typeof dataEngine === "object" && !Array.isArray(dataEngine)) {
        goexDataEngine = dataEngine as Record<string, unknown>;
      }
      const agentData = dataOf("agents");
      if (agentData && typeof agentData === "object" && !Array.isArray(agentData)) {
        goexAgents = { ...goexAgents, ...(agentData as Record<string, unknown>) };
      }
    }
    if (event.type === "sft.checkpoint.created" && event.item) {
      checkpoints.push({
        id: event.item.id,
        status: event.item.status ?? "created",
        ready: false,
        promoted: false,
        ...(event.item.raw ?? {})
      });
    }
    if (event.type === "sft.checkpoint.ready" && event.item) {
      const id = event.item.id;
      const existing = checkpoints.find((ckpt) => ckpt.id === id);
      if (existing) {
        existing.status = event.item.status ?? "ready";
        existing.ready = true;
      } else {
        checkpoints.push({ id, status: "ready", ready: true, promoted: false, ...(event.item.raw ?? {}) });
      }
    }
    if (event.type === "sft.checkpoint.promoted" && event.item) {
      const id = event.item.id;
      const existing = checkpoints.find((ckpt) => ckpt.id === id);
      if (existing) {
        existing.status = "promoted";
        existing.promoted = true;
      } else {
        checkpoints.push({ id, status: "promoted", promoted: true, ...(event.item.raw ?? {}) });
      }
      summary = { ...summary, promotedCheckpointId: id };
    }
    if (event.type === "sft.step.metrics" || event.type === "sft.training.metrics") {
      const step = missingNumber(event.delta?.step ?? event.delta?.global_step);
      const epoch = missingNumber(event.delta?.epoch);
      const trainLoss = missingNumber(event.delta?.train_loss ?? event.delta?.trainLoss);
      const validationLoss = missingNumber(event.delta?.validation_loss ?? event.delta?.validationLoss);
      const learningRate = missingNumber(event.delta?.learning_rate ?? event.delta?.learningRate);
      if (step != null) {
        curves.steps.push(step);
        points.push({
          step,
          ...(epoch != null ? { epoch } : {}),
          ...(trainLoss != null ? { trainLoss } : {}),
          ...(validationLoss != null ? { validationLoss } : {}),
          ...(learningRate != null ? { learningRate } : {})
        });
      }
      if (epoch != null) curves.epochs.push(epoch);
      if (trainLoss != null) curves.trainLoss.push(trainLoss);
      if (validationLoss != null) curves.validationLoss.push(validationLoss);
      if (learningRate != null) curves.learningRate.push(learningRate);
    }
    if (
      event.type === "sft.campaign.updated" ||
      event.type === "sft.checkpoint_evaluation.allocated"
    ) {
      const id = String(
        event.item?.id ?? event.delta?.evaluation_id ?? event.delta?.id ?? `campaign_${event.sequenceNumber}`
      );
      const checkpointId = event.delta?.checkpointId ?? event.delta?.checkpoint_id;
      campaigns.push({
        id,
        checkpointId: checkpointId ? String(checkpointId) : undefined,
        status: event.item?.status ?? (event.delta?.status ? String(event.delta.status) : undefined),
        splitRole: event.delta?.split_role
          ? String(event.delta.split_role)
          : event.delta?.splitRole
            ? String(event.delta.splitRole)
            : undefined,
        children: [
          ...refsFromUnknown(event.delta?.children),
          ...refsFromUnknown(event.delta?.resource_ref),
          ...refsFromUnknown(event.artifactRefs)
        ]
      });
    }
    if (event.type === "sft.checkpoint_evaluation.completed") {
      const id = String(event.item?.id ?? event.delta?.evaluation_id ?? "");
      const existing = campaigns.find((campaign) => campaign.id === id);
      if (existing) existing.status = event.item?.status ?? "completed";
    }
    if (event.type === "sft.checkpoint_rollout.allocated") {
      const rolloutId = String(event.item?.id ?? event.delta?.rollout_id ?? "");
      const evaluationId = String(event.delta?.evaluation_id ?? "");
      if (rolloutId && evaluationId) {
        let campaign = campaigns.find((entry) => entry.id === evaluationId);
        if (!campaign) {
          campaign = {
            id: evaluationId,
            checkpointId: typeof event.delta?.checkpoint_id === "string" ? event.delta.checkpoint_id : undefined,
            status: "running",
            splitRole: typeof event.delta?.split_role === "string" ? event.delta.split_role : undefined,
            children: []
          };
          campaigns.push(campaign);
        }
        if (!campaign.children.some((child) => child.id === rolloutId)) {
          const streamId = typeof event.delta?.stream_id === "string" ? event.delta.stream_id : undefined;
          campaign.children.push({
            kind: "container_rollout",
            id: rolloutId,
            role: "checkpoint_evaluation",
            attributes: {
              ...(streamId ? { stream_id: streamId } : {}),
              reward: null,
              checkpoint_id: event.delta?.checkpoint_id,
              split_role: event.delta?.split_role,
              seed: event.delta?.seed
            }
          });
        }
      }
    }
    if (
      event.type === "sft.checkpoint_rollout.completed" ||
      event.type === "sft.checkpoint_rollout.failed"
    ) {
      const rolloutId = String(event.item?.id ?? event.delta?.rollout_id ?? "");
      if (rolloutId) {
        const rewardValue = event.delta?.reward ?? event.delta?.score;
        const costUsd = missingNumber(
          event.usageDelta?.cost_usd ??
            event.usageDelta?.costUsd ??
            event.delta?.cost_usd ??
            event.delta?.costUsd
        );
        for (const campaign of campaigns) {
          const child = campaign.children.find((ref) => ref.id === rolloutId);
          if (!child) continue;
          child.attributes = {
            ...(child.attributes ?? {}),
            reward:
              typeof rewardValue === "number" && Number.isFinite(rewardValue) ? rewardValue : null,
            ...(costUsd != null ? { cost_usd: costUsd } : {})
          };
        }
      }
    }
    if (event.type === "sft.dataset.validated") {
      dataset = { ...(event.snapshot ?? event.delta ?? {}) };
    }
    if (event.type === "sft.compute.updated") {
      compute = { ...(event.snapshot ?? event.delta ?? {}) };
    }
    if (
      event.type === "sft.checkpoint_eval.completed" ||
      event.type === "sft.heldout_eval.completed" ||
      event.type === "sft.checkpoint_evaluation.completed"
    ) {
      evaluations.push({
        sequence: event.sequenceNumber,
        role: event.delta?.role ?? (event.type.includes("heldout") ? "heldout" : "selection"),
        measurementOnly: Boolean(event.delta?.measurementOnly),
        ...(event.delta ?? {}),
        item: event.item
      });
    }
    if (event.type === "sft.examples.updated" && Array.isArray(event.snapshot?.examples)) {
      examples = event.snapshot.examples as Array<Record<string, unknown>>;
    }
    if (event.type === "sft.model.materialized" && event.item) {
      lineage = {
        baseModel: event.item.raw?.baseModel,
        adapter: event.item.raw?.adapter,
        checkpointId: event.item.raw?.checkpointId ?? event.item.id,
        deployable: event.item.raw?.deployable,
        digest: event.item.raw?.digest,
        status: event.item.status
      };
    }
  }

  const projected: ProjectedState = {
    cursorSeq: maxSeq,
    summary: {
      id: run.id,
      algorithmId: run.algorithmId,
      status,
      objective: run.objective,
      source: run.source,
      capabilities: run.capabilities,
      summary,
      cursorSeq: maxSeq
    },
    timeline,
    usage,
    logs,
    artifacts,
    execution: { bindings: run.executionBindings ?? [] }
  };
  if (costReceiptSeen && !costTelemetryComplete) projected.usage.costUsd = null;

  if (!incumbentId) {
    const accent = frontier.find((cell) => cell.accent);
    if (accent?.candidateId) incumbentId = String(accent.candidateId);
  }

  if (run.algorithmId === "gepa") {
    const statusText = String(status);
    const terminal = ["completed", "failed", "canceled", "cancelled", "succeeded", "terminated"].includes(statusText);
    if (terminal) {
      proposalActive = false;
      evaluationActive = false;
    }
    const failed = statusText === "failed" || statusText === "terminated";
    const stages: GepaStage[] = stageOrder.map((id) => {
      const entry = stageState.get(id);
      if (entry) {
        if (terminal && entry.status === "active") {
          return { ...entry, status: failed ? "failed" : "completed", endedAt: entry.endedAt ?? runEndedAt };
        }
        return entry;
      }
      if (id === "complete") {
        return {
          id,
          label: stageLabels[id],
          status: terminal ? (failed ? "failed" : "completed") : "pending",
          endedAt: terminal ? runEndedAt : undefined
        };
      }
      return { id, label: stageLabels[id], status: terminal ? "skipped" : "pending" };
    });
    const failedStage = stages.find((stage) => stage.status === "failed");
    const activityLabel = statusText === "terminated"
      ? "Run terminated"
      : terminal
      ? failed
        ? `${failedStage?.label ?? "Search"} failed`
        : "Search complete"
      : proposalActive && evaluationActive
        ? "Creating + evaluating candidates"
        : proposalActive
          ? "Creating candidates"
          : evaluationActive
            ? evaluationLabel(evaluationStage)
            : activityPhase === "completed"
              ? "Search complete"
              : activityPhase === "ready"
                ? "Updating Pareto frontier"
                : activityDetail ?? "Preparing search";
    const recentCompletionTimes = rolloutCompletionTimes.length > 0
      ? rolloutCompletionTimes.filter((time) => time >= rolloutCompletionTimes.at(-1)! - 60_000)
      : [];
    if (recentCompletionTimes.length >= 2) {
      const elapsedMs = recentCompletionTimes.at(-1)! - recentCompletionTimes[0];
      if (elapsedMs > 0) {
        runtime.rolloutsPerMinute = (recentCompletionTimes.length - 1) * 60_000 / elapsedMs;
      }
    }
    runtime.job = jobTermination ?? {
      state: statusText === "failed"
        ? "failed"
        : ["canceled", "cancelled"].includes(statusText)
          ? "cancelled"
          : ["completed", "succeeded"].includes(statusText)
            ? "completed"
            : "running",
      occurredAt: terminal ? runEndedAt : lastEventAt
    };
    projected.gepa = {
      candidates: [...candidates.values()].map((candidate) => terminal && ["registered", "evaluating"].includes(String(candidate.status ?? ""))
        ? { ...candidate, status: "aborted", abortedReason: jobTermination?.reason ?? "run_ended_before_evaluation" }
        : candidate),
      frontier,
      reflections,
      budget,
      limits,
      nearestLimit,
      contract,
      frontierHistory,
      stages,
      evaluations: gepaEvaluations,
      failedAttempts: gepaFailedAttempts,
      coverage: [...gepaCoverage.values()],
      proposerTraces: [...proposerTraces.values()],
      activity: {
        phase: activityPhase,
        label: activityLabel,
        detail: failed ? optimizerFailureDetail(run.error) ?? activityDetail : activityDetail,
        proposalActive,
        evaluationActive,
        evaluationStage,
        activeCandidateIds: [...activeCandidateIds],
        generation: activityGeneration,
        requestedProposalCount,
        sequence: activitySequence,
        terminal
      },
      incumbentId,
      best,
      heldout,
      models,
      timing: { startedAt: runStartedAt, endedAt: runEndedAt, lastEventAt },
      rolloutsCompleted,
      runtime
    };
  } else if (run.algorithmId === "go-ex") {
    const evidence = goexDataEngine.rollout_evidence;
    const evidenceMap = evidence && typeof evidence === "object" && !Array.isArray(evidence)
      ? evidence as Record<string, unknown>
      : {};
    const evidenceRows = [
      ...(Array.isArray(evidenceMap.search) ? evidenceMap.search : []),
      ...(Array.isArray(evidenceMap.heldout_measurement) ? evidenceMap.heldout_measurement : [])
    ] as Array<Record<string, unknown>>;
    const nativeRows = (Array.isArray(goexDataEngine.child_streams) ? goexDataEngine.child_streams : [])
      .filter((row): row is Record<string, unknown> => Boolean(row && typeof row === "object" && !Array.isArray(row)))
      .map((row) => ({
        ...row,
        rollout_id: row.rollout_id,
        status: row.state,
        metadata: { stream: row.stream }
      }));
    const rowsByRollout = new Map<string, Record<string, unknown>>();
    for (const row of [...nativeRows, ...evidenceRows]) {
      const id = typeof row.rollout_id === "string" ? row.rollout_id : undefined;
      if (id) rowsByRollout.set(id, row);
    }
    for (const [id, row] of goexEventRollouts) {
      rowsByRollout.set(id, { ...(rowsByRollout.get(id) ?? {}), ...row });
    }
    const rows = [...rowsByRollout.values()];
    const rollouts = rows.flatMap((row) => {
      const metadata = row.metadata && typeof row.metadata === "object" && !Array.isArray(row.metadata)
        ? row.metadata as Record<string, unknown>
        : {};
      const stream = metadata.stream && typeof metadata.stream === "object" && !Array.isArray(metadata.stream)
        ? metadata.stream as Record<string, unknown>
        : {};
      const id = typeof row.rollout_id === "string" ? row.rollout_id : undefined;
      const streamId = typeof (stream.id ?? stream["stream.id"] ?? metadata.stream_id) === "string"
        ? String(stream.id ?? stream["stream.id"] ?? metadata.stream_id)
        : undefined;
      const transports = stream.transports && typeof stream.transports === "object" && !Array.isArray(stream.transports)
        ? stream.transports as Record<string, unknown>
        : {};
      const poll = transports.poll && typeof transports.poll === "object" && !Array.isArray(transports.poll)
        ? transports.poll as Record<string, unknown>
        : {};
      const sse = transports.sse && typeof transports.sse === "object" && !Array.isArray(transports.sse)
        ? transports.sse as Record<string, unknown>
        : {};
      const rewardDescriptor = stream.reward && typeof stream.reward === "object" && !Array.isArray(stream.reward)
        ? stream.reward as Record<string, unknown>
        : {};
      if (!id) return [];
      const asString = (value: unknown): string | undefined =>
        typeof value === "string" && value.length > 0 ? value : undefined;
      const rewardUrl = asString(stream.reward_url) ?? asString(rewardDescriptor.url);
      const pollUrl = asString(stream.poll_url) ?? asString(poll.url);
      const sseUrl = asString(stream.sse_url) ?? asString(sse.url);
      return [{
        candidateId: typeof row.candidate_id === "string" ? row.candidate_id : undefined,
        seed: typeof row.seed === "number" ? row.seed : undefined,
        split: typeof row.split === "string" ? row.split : undefined,
        lane: typeof row.lane === "string" ? row.lane : undefined,
        status: typeof row.status === "string" ? row.status : undefined,
        reward: typeof row.reward === "number" ? row.reward : row.reward === null ? null : undefined,
        ref: {
          kind: "container_rollout" as const,
          id,
          role: typeof row.lane === "string" ? row.lane : undefined,
          attributes: {
            ...(streamId ? { stream_id: streamId } : {}),
            ...(rewardUrl ? { reward_url: rewardUrl } : {}),
            ...(pollUrl ? { poll_url: pollUrl } : {}),
            ...(sseUrl ? { sse_url: sseUrl } : {}),
            reward: typeof row.reward === "number" ? row.reward : row.reward === null ? null : undefined,
            stream
          }
        }
      }];
    });
    projected.goex = {
      board,
      themes,
      candidates: (() => {
        const merged = new Map<string, Record<string, unknown>>();
        for (const candidate of goexCandidates) {
          const id = String(candidate.candidate_id ?? candidate.id ?? "");
          if (id) merged.set(id, candidate);
        }
        for (const [id, candidate] of goexEventCandidates) merged.set(id, { ...(merged.get(id) ?? {}), ...candidate });
        return [...merged.values()];
      })(),
      frontier: goexFrontier,
      dataEngine: goexDataEngine,
      agents: goexAgents,
      rollouts
    };
  } else if (run.algorithmId === "sft") {
    projected.sft = {
      curves,
      points,
      checkpoints,
      evaluations,
      campaigns,
      dataset,
      compute,
      examples,
      lineage
    };
  }

  return projected;
}

/** Project optimizer_event.v1 fixtures into shared + algorithm slices at a cursor. */

import { formatMissingNumber, formatMissingUsd, missingNumber } from "../../../../../runtime/liveStream.ts";

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
  task?: {
    id?: string;
    name?: string;
    objective?: string;
    description?: string;
    family?: string;
    version?: string;
    outputKind?: string;
  };
  program?: { id?: string; mutableFields: string[] };
  objectiveSet?: {
    id?: string;
    hash?: string;
    frontierType?: string;
    selectionObjective?: string;
    objectives: Array<{ name: string; direction?: string; aggregation?: string; splitPolicy?: string }>;
  };
  splits?: { train?: number; minibatch?: number; reflection?: number; pareto?: number; heldout?: number };
  dataset?: {
    source?: string;
    config?: string;
    revision?: string;
    digest?: string;
    rowCount?: number;
    labelCount?: number;
    splits?: {
      train?: number;
      selection?: number;
      heldout?: number;
    };
  };
  container?: {
    verified?: boolean;
    specId?: string;
    url?: string;
    workshopInstance?: string;
    credentialMode?: string;
    evaluatorId?: string;
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
    configuredRolloutWorkers?: number;
    staticRolloutWorkers?: number;
    estimatedEffectiveConcurrency?: number;
    rolloutSubmissionMode?: string;
    maxDispatchChunkSize?: number;
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

export type EvalTrial = {
  id: string;
  candidateId?: string;
  stage?: string;
  seed?: number;
  scenario?: string;
  status: string;
  benchmarkStatus?: string | null;
  /** Gate- and artifact-complete evidence. Only valid trials may be scored. */
  valid?: boolean;
  metrics: Record<string, number>;
  missingGates: string[];
  missingArtifacts: string[];
  evidenceDir?: string;
};

export type EvalRollout = {
  trialId: string;
  seed?: number;
  world?: string;
  status: "starting" | "running" | "finished";
  ply: number;
  actions: string[];
  policyReason?: string;
  rewardTotal: number | null;
  rewardDelta: number | null;
  achievements: string[];
  resources: Record<string, number>;
  playerPos?: unknown;
  frame?: {
    dataUrl: string;
    sha256?: string;
    width?: number;
    height?: number;
  };
  costUsd: number | null;
  sequence: number;
};

export type EvalScorecard = {
  candidateId: string;
  label: string;
  stage: string;
  isBaseline: boolean;
  trials: { total: number; valid: number; failed: number };
  metrics: Array<{ metric: string; mean: number | null; min: number | null; max: number | null; count: number }>;
  gateFailures: Record<string, number>;
  /** Mean signed difference against the baseline over shared seeds. */
  pairedLift: number | null;
  pairedTrials: number;
  eliminatedAt: string | null;
  eliminationReason: string | null;
  costUsd: number | null;
  /**
   * Share of the scored episodes this candidate's own policy chose. A policy
   * that spends its budget is replaced by a fallback for the rest of the
   * episode, so a mean read without this can be mostly the fallback's score
   * under the candidate's name. `null` when nothing reported coverage — a code
   * policy has no budget to exhaust, which is absence, not zero.
   */
  policyStepFraction: number | null;
  budgetExhaustedTrials: number;
};

export type EvalSelection = {
  status: string;
  winnerId: string | null;
  baselineId: string | null;
  primaryMetric: string;
  lift: number | null;
  minLift: number;
  reason: string;
};

export type EvalState = {
  candidates: Array<{ id: string; label: string; isBaseline: boolean }>;
  scorecards: EvalScorecard[];
  trials: EvalTrial[];
  rollouts: EvalRollout[];
  selection: EvalSelection | null;
  seedLedger: { screening: number[]; confirmation: number[]; scenarios: string[] } | null;
  manifestDigest: string | null;
  candidateSetId: string | null;
  evidenceDir: string | null;
  plannedTrials: number;
  parallelism: number | null;
  globalCapacity: number | null;
  paused: boolean;
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
  eval?: EvalState;
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
  cispo?: CispoState;
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
    /**
     * Phase A — the unchanged student scored on frozen baseline seeds. Absent
     * until the producer emits it; never synthesized from training metrics.
     */
    baseline?: {
      splitDigest?: string;
      seeds: SftSeedResult[];
    };
    /**
     * Phases B/C — teacher collection and the curation funnel. Counts stay null
     * when the producer has not reported them; a null is not a zero.
     */
    curation: {
      collected: number | null;
      considered: number | null;
      accepted: number | null;
      rejected: number | null;
      rejectionsByReason: Record<string, number>;
      seedsCovered: number | null;
      achievementsCovered: string[];
      candidates: SftCurationCandidate[];
    };
    /**
     * Phase G — the paired base-vs-promoted comparison on untouched heldout
     * seeds. This is the only evidence that licenses an uplift claim.
     */
    comparison?: {
      splitDigest?: string;
      baseLabel: string;
      trainedLabel: string;
      pairs: SftComparisonPair[];
    };
  };
  dag?: DagState;
};

export function optimizerFailureDetail(error: unknown): string | undefined {
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

/** One evaluated seed for a single policy arm. */
export type SftSeedResult = {
  seed: string;
  reward: number | null;
  steps?: number;
  achievements?: string[];
  rolloutId?: string;
  traceDigest?: string;
  status?: string;
};

/** One trajectory considered by the curator, with its accept/reject reason. */
export type SftCurationCandidate = {
  id: string;
  seed?: string;
  score: number | null;
  reward: number | null;
  steps?: number;
  achievements?: string[];
  accepted: boolean;
  reason?: string;
  traceDigest?: string;
};

/** The same heldout seed scored by both arms. Either side may be missing. */
export type SftComparisonPair = {
  seed: string;
  base: SftSeedResult | null;
  trained: SftSeedResult | null;
};

export type CispoState = {
  objective: string;
  clipLow: number | null;
  clipHigh: number | null;
  groupSize: number | null;
  rewardVariance: number | null;
  advantageMean: number | null;
  advantageStd: number | null;
  optimizerSteps: number;
  warmStartArtifactId: string | null;
  checkpointIds: string[];
  rolloutGroups: Array<{
    id: string;
    iteration: number | null;
    label: string | null;
    rewardMean: number | null;
    rewardVariance: number | null;
    size: number;
    sequence: number;
  }>;
  zeroAdvantageGroups: number;
  learningSignalGroups: number;
  noLearningSignal: boolean;
};

export type DagNodeState = {
  id: string;
  kind?: string;
  status: string; // planned | running | paused | sealed | failed | cancelled
  partitionsSealed?: number;
  partitionsTotal?: number;
  /** null = missing/unmetered; never fabricate 0 */
  costUsd?: number | null;
  wallSeconds?: number | null;
  accountedSeconds?: number | null;
  unmetered?: boolean;
  /** Step id: source, annotate.behavior, … */
  algorithmId?: string;
};

export type DagState = {
  dag?: string;
  nodes: DagNodeState[];
  /** Sum of non-null node costs; null if any sealed metered node is missing cost. */
  knownCostUsd: number | null;
  unmeteredCount: number;
  missingMeterCount: number;
  sequence?: number;
};

export function isDagAlgorithm(algorithmId: string): boolean {
  return algorithmId === "dag" || algorithmId.startsWith("dag.");
}

const DAG_NODE_STATUS_FROM_TYPE: Record<string, string> = {
  "node.planned": "planned",
  "node.started": "running",
  "node.sealed": "sealed",
  "node.failed": "failed",
  "node.paused": "paused",
  "node.resumed": "running"
};

function canonicalDagEventType(type: string): string {
  const dotted = type.replaceAll("_", ".");
  if (dotted.startsWith("dag.node.") || dotted.startsWith("dag.partition.")) {
    return dotted.slice("dag.".length);
  }
  return dotted;
}

function dagUsageSource(event: OptimizerEvent): Record<string, unknown> | undefined {
  if (event.usageDelta) return event.usageDelta;
  const delta = event.delta ?? {};
  const nested = delta.usage_delta ?? delta.usageDelta;
  if (nested && typeof nested === "object" && !Array.isArray(nested)) {
    return nested as Record<string, unknown>;
  }
  return undefined;
}

function finalizeDagState(
  nodes: DagNodeState[],
  dag?: string,
  sequence?: number
): DagState {
  let unmeteredCount = 0;
  let missingMeterCount = 0;
  let knownSum = 0;
  let hasKnown = false;
  for (const node of nodes) {
    if (node.unmetered) {
      unmeteredCount += 1;
      continue;
    }
    if (node.costUsd != null) {
      knownSum += node.costUsd;
      hasKnown = true;
      continue;
    }
    if (node.status === "sealed") missingMeterCount += 1;
  }
  return {
    dag,
    nodes,
    knownCostUsd: missingMeterCount > 0 ? null : hasKnown ? knownSum : unmeteredCount > 0 ? knownSum : null,
    unmeteredCount,
    missingMeterCount,
    sequence
  };
}

function numberOrNull(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function optionalNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function stringList(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  return value.map((entry) => String(entry));
}

function optionalString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function objectRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function numberList(value: unknown): number[] {
  if (!Array.isArray(value)) return [];
  return value.map((entry) => Number(entry)).filter((entry) => Number.isFinite(entry));
}

/**
 * Numeric map that drops non-numbers rather than coercing them. A metric the
 * producer did not report stays absent, so the UI can render "—" instead of a
 * zero nobody measured.
 */
function numberRecord(value: unknown): Record<string, number> {
  if (!value || typeof value !== "object") return {};
  const out: Record<string, number> = {};
  for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
    if (typeof entry === "number" && Number.isFinite(entry)) out[key] = entry;
  }
  return out;
}

/**
 * Normalize one evaluated seed. Reward is deliberately `null` — not `0` — when
 * the producer did not report an authoritative score, so a missing measurement
 * can never be averaged in as a real zero.
 */
function seedResult(raw: unknown): SftSeedResult | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const rec = raw as Record<string, unknown>;
  const seed = rec.seed ?? rec.world ?? rec.seed_id ?? rec.id;
  if (seed == null) return null;
  return {
    seed: String(seed),
    reward: numberOrNull(rec.reward ?? rec.total_reward ?? rec.score),
    steps: optionalNumber(rec.steps ?? rec.episode_length ?? rec.step_count),
    achievements: stringList(rec.achievements),
    rolloutId: typeof rec.rollout_id === "string" ? rec.rollout_id : undefined,
    traceDigest: typeof rec.trace_v5_digest === "string"
      ? rec.trace_v5_digest
      : typeof rec.trace_digest === "string" ? rec.trace_digest : undefined,
    status: typeof rec.status === "string" ? rec.status : undefined
  };
}

/** Pull the per-seed detail list off either arm of a paired payload. */
function armDetails(raw: unknown): { label?: string; seeds: SftSeedResult[] } {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return { seeds: [] };
  const rec = raw as Record<string, unknown>;
  const list = rec.details ?? rec.seeds ?? rec.rollouts;
  const seeds = Array.isArray(list)
    ? list.map(seedResult).filter((entry): entry is SftSeedResult => entry != null)
    : [];
  return { label: typeof rec.label === "string" ? rec.label : undefined, seeds };
}

function curationCandidate(raw: unknown): SftCurationCandidate | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const rec = raw as Record<string, unknown>;
  const id = rec.id ?? rec.candidate_id ?? rec.rollout_id ?? rec.trace_id;
  if (id == null) return null;
  const decision = rec.decision ?? rec.status;
  const accepted = rec.accepted === true
    || decision === "accepted"
    || decision === "accept"
    || decision === "retained";
  return {
    id: String(id),
    seed: rec.seed == null ? undefined : String(rec.seed),
    score: numberOrNull(rec.score ?? rec.rank_score),
    reward: numberOrNull(rec.reward ?? rec.total_reward),
    steps: optionalNumber(rec.steps ?? rec.episode_length),
    achievements: stringList(rec.achievements),
    accepted,
    reason: typeof rec.reason === "string"
      ? rec.reason
      : typeof rec.curation_reason === "string" ? rec.curation_reason : undefined,
    traceDigest: typeof rec.trace_v5_digest === "string" ? rec.trace_v5_digest : undefined
  };
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
  const evalTrials = new Map<string, EvalTrial>();
  const evalRollouts = new Map<string, EvalRollout>();
  const evalScorecards = new Map<string, EvalScorecard>();
  let evalSelection: EvalSelection | null = null;
  let evalLedger: EvalState["seedLedger"] = null;
  let evalPlan: Record<string, unknown> = {};
  let evalEvidenceDir: string | null = null;
  let evalPaused = false;
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
  const baselineSeeds = new Map<string, SftSeedResult>();
  let baselineSplitDigest: string | undefined;
  const curationCandidates = new Map<string, SftCurationCandidate>();
  let curationFunnel: Record<string, unknown> = {};
  let teacherRolloutCount = 0;
  const comparisonBase = new Map<string, SftSeedResult>();
  const comparisonTrained = new Map<string, SftSeedResult>();
  let comparisonSplitDigest: string | undefined;
  let comparisonBaseLabel: string | undefined;
  let comparisonTrainedLabel: string | undefined;
  let cispoClipLow: number | null = null;
  let cispoClipHigh: number | null = null;
  let cispoGroupSize: number | null = null;
  let cispoRewardVariance: number | null = null;
  let cispoAdvantageMean: number | null = null;
  let cispoAdvantageStd: number | null = null;
  let cispoOptimizerSteps = 0;
  const cispoRolloutGroups: CispoState["rolloutGroups"] = [];
  let cispoZeroAdvantageGroups = 0;
  let cispoLearningSignalGroups = 0;
  let cispoWarmStartArtifactId: string | null =
    typeof run.summary?.warmStartArtifactId === "string" ? run.summary.warmStartArtifactId
      : typeof run.summary?.trainingArtifactId === "string" ? run.summary.trainingArtifactId
        : null;
  let cispoNoLearningSignal = false;
  const dagNodes = new Map<string, DagNodeState>();
  const dagFailedPartitions = new Map<string, number>();
  let dagName: string | undefined;
  let dagSequence: number | undefined;
  const projectDag = isDagAlgorithm(run.algorithmId);
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
    if (event.type === "gepa.run.finished" || event.type === "goex.run_finished") {
      // GEPA's terminal event is the authoritative total. Replacing accumulated
      // counters avoids double counting while preserving summed job wall time.
      applyUsage(event.delta ?? {}, true);
    }
    if (projectDag) {
      const kind = canonicalDagEventType(event.type);
      const delta = event.delta ?? {};
      const snapshot = event.snapshot ?? {};
      const dagLabel = [delta.dag, snapshot.dag, delta.name, snapshot.name]
        .find((value) => typeof value === "string" && value) as string | undefined;
      if (dagLabel) dagName = dagLabel;
      if (kind === "dag.checkpoint.written") {
        dagSequence = event.sequenceNumber;
      }
      const nodeIdRaw = event.item?.id ?? delta.node ?? delta.node_id ?? delta.nodeId;
      const nodeId = typeof nodeIdRaw === "string" && nodeIdRaw ? nodeIdRaw : "";
      const isNodeEvent = kind.startsWith("node.");
      const isPartitionEvent = kind.startsWith("partition.");
      if (nodeId && (isNodeEvent || isPartitionEvent)) {
        const previous = dagNodes.get(nodeId) ?? { id: nodeId, status: "planned" };
        const statusFromType = DAG_NODE_STATUS_FROM_TYPE[kind];
        const nextStatus = (typeof event.item?.status === "string" && event.item.status
          ? event.item.status
          : statusFromType)
          ?? (isPartitionEvent && previous.status === "planned" ? "running" : previous.status);
        const unmetered = delta.unmetered === true || previous.unmetered === true;
        const usageSource = dagUsageSource(event);
        const costRaw = usageSource
          ? (Object.prototype.hasOwnProperty.call(usageSource, "cost_usd")
            ? usageSource.cost_usd
            : Object.prototype.hasOwnProperty.call(usageSource, "costUsd")
              ? usageSource.costUsd
              : undefined)
          : (Object.prototype.hasOwnProperty.call(delta, "cost_usd")
            ? delta.cost_usd
            : Object.prototype.hasOwnProperty.call(delta, "costUsd")
              ? delta.costUsd
              : undefined);
        const reportedCost = missingNumber(costRaw);
        let costUsd = previous.costUsd ?? null;
        if (unmetered) {
          costUsd = null;
        } else if (reportedCost != null) {
          costUsd = kind === "node.sealed" ? reportedCost : (costUsd ?? 0) + reportedCost;
        }

        let partitionsSealed = previous.partitionsSealed;
        let partitionsTotal = previous.partitionsTotal;
        let partitionsFailed = dagFailedPartitions.get(nodeId) ?? 0;
        const sealedCount = missingNumber(delta.partitions_sealed ?? delta.partitionsSealed);
        const totalCount = missingNumber(delta.partitions_total ?? delta.partitionsTotal);
        const failedCount = missingNumber(delta.partitions_failed ?? delta.partitionsFailed);
        if (kind === "partition.sealed") {
          partitionsSealed = sealedCount ?? (partitionsSealed ?? 0) + 1;
        } else if (sealedCount != null) {
          partitionsSealed = sealedCount;
        }
        if (kind === "partition.failed") {
          partitionsFailed = failedCount ?? partitionsFailed + 1;
        } else if (failedCount != null) {
          partitionsFailed = failedCount;
        }
        dagFailedPartitions.set(nodeId, partitionsFailed);
        if (totalCount != null) {
          partitionsTotal = totalCount;
        } else if (isPartitionEvent) {
          partitionsTotal = Math.max(partitionsTotal ?? 0, (partitionsSealed ?? 0) + partitionsFailed);
        }

        const wallSeconds = missingNumber(delta.wall_seconds ?? delta.wallSeconds);
        const accountedSeconds = missingNumber(delta.accounted_seconds ?? delta.accountedSeconds);
        const kindLabel = event.item?.kind ?? event.item?.type ?? (typeof delta.kind === "string" ? delta.kind : undefined);
        const stepId = typeof delta.algorithm_id === "string"
          ? delta.algorithm_id
          : typeof delta.algorithmId === "string"
            ? delta.algorithmId
            : typeof delta.step === "string"
              ? delta.step
              : undefined;

        dagNodes.set(nodeId, {
          ...previous,
          id: nodeId,
          status: nextStatus,
          kind: typeof kindLabel === "string" ? kindLabel : previous.kind,
          algorithmId: stepId ?? previous.algorithmId ?? nodeId,
          unmetered: unmetered || undefined,
          costUsd: unmetered ? null : costUsd,
          partitionsSealed,
          partitionsTotal,
          wallSeconds: wallSeconds ?? previous.wallSeconds ?? null,
          accountedSeconds: accountedSeconds ?? previous.accountedSeconds ?? null
        });
        dagSequence = event.sequenceNumber;
      }
    }
    if (event.type.startsWith("eval.")) {
      if (event.type === "eval.run.planned") {
        evalPlan = event.snapshot ?? {};
      } else if (event.type === "eval.seed_ledger.sealed") {
        const ledger = (event.snapshot?.seedLedger ?? {}) as Record<string, unknown>;
        evalLedger = {
          screening: numberList(ledger.screening),
          confirmation: numberList(ledger.confirmation),
          scenarios: stringList(ledger.scenarios) ?? []
        };
      } else if (event.type === "eval.trial.queued" || event.type === "eval.trial.started") {
        const id = String(event.delta?.trial_id ?? "");
        if (id) {
          const existing = evalTrials.get(id);
          evalTrials.set(id, {
            ...(existing ?? { id, status: "queued", metrics: {}, missingGates: [], missingArtifacts: [] }),
            id,
            candidateId: optionalString(event.delta?.candidate_id) ?? existing?.candidateId,
            stage: optionalString(event.delta?.stage) ?? existing?.stage,
            seed: optionalNumber(event.delta?.seed) ?? existing?.seed,
            scenario: optionalString(event.delta?.scenario) ?? existing?.scenario,
            status: event.type.endsWith("started") ? "running" : "queued"
          });
        }
      } else if (event.type === "eval.trial.event") {
        const trialId = String(event.delta?.trial_id ?? "");
        const container = (event.delta?.containerEvent ?? {}) as Record<string, unknown>;
        const containerType = String(container.event ?? "");
        const existing = evalRollouts.get(trialId);
        if (trialId && containerType === "rollout.started") {
          evalRollouts.set(trialId, {
            trialId,
            seed: optionalNumber(container.seed),
            world: optionalString(container.world),
            status: "starting",
            ply: 0,
            actions: [],
            rewardTotal: null,
            rewardDelta: null,
            achievements: [],
            resources: {},
            costUsd: null,
            sequence: event.sequenceNumber
          });
        } else if (trialId && containerType === "rollout.step") {
          const frame = (container.frame ?? {}) as Record<string, unknown>;
          const dataUrl = optionalString(frame.data_url ?? frame.dataUrl);
          evalRollouts.set(trialId, {
            ...(existing ?? {
              trialId,
              status: "running" as const,
              ply: 0,
              actions: [],
              rewardTotal: null,
              rewardDelta: null,
              achievements: [],
              resources: {},
              costUsd: null,
              sequence: event.sequenceNumber
            }),
            seed: optionalNumber(container.seed) ?? existing?.seed,
            status: "running",
            ply: optionalNumber(container.ply) ?? existing?.ply ?? 0,
            actions: stringList(container.actions) ?? existing?.actions ?? [],
            policyReason: optionalString(container.policy_reason) ?? existing?.policyReason,
            rewardTotal: numberOrNull(container.reward_total),
            rewardDelta: numberOrNull(container.reward_delta),
            achievements: stringList(container.achievements) ?? existing?.achievements ?? [],
            resources: numberRecord(container.resources),
            playerPos: container.player_pos ?? existing?.playerPos,
            frame: dataUrl
              ? {
                  dataUrl,
                  sha256: optionalString(frame.sha256),
                  width: optionalNumber(frame.width),
                  height: optionalNumber(frame.height)
                }
              : existing?.frame,
            sequence: event.sequenceNumber
          });
        } else if (trialId && containerType === "rollout.finished" && existing) {
          evalRollouts.set(trialId, {
            ...existing,
            status: "finished",
            rewardTotal: numberOrNull(container.reward) ?? existing.rewardTotal,
            achievements: stringList(container.unique_achievements) ?? existing.achievements,
            costUsd: numberOrNull(container.cost_usd),
            sequence: event.sequenceNumber
          });
        }
      } else if (event.type === "eval.trial.terminal" && event.item) {
        const item = event.item as Record<string, unknown>;
        const id = String(item.id ?? "");
        if (id) {
          evalTrials.set(id, {
            id,
            candidateId: optionalString(item.candidateId),
            stage: optionalString(item.stage),
            seed: optionalNumber(item.seed),
            scenario: optionalString(item.scenario),
            status: String(item.status ?? "unknown"),
            benchmarkStatus: optionalString(item.benchmarkStatus) ?? null,
            valid: item.valid === true,
            metrics: numberRecord(item.metrics),
            missingGates: stringList(item.missingGates) ?? [],
            missingArtifacts: stringList(item.missingArtifacts) ?? [],
            evidenceDir: optionalString(item.evidenceDir)
          });
        }
      } else if (event.type === "eval.candidate.scored" && event.item) {
        const item = event.item as Record<string, unknown>;
        const trials = (item.trials ?? {}) as Record<string, unknown>;
        const card: EvalScorecard = {
          candidateId: String(item.id ?? ""),
          label: String(item.label ?? item.id ?? ""),
          stage: String(item.stage ?? ""),
          isBaseline: item.isBaseline === true,
          trials: {
            total: optionalNumber(trials.total) ?? 0,
            valid: optionalNumber(trials.valid) ?? 0,
            failed: optionalNumber(trials.failed) ?? 0
          },
          metrics: Array.isArray(item.metrics)
            ? (item.metrics as Array<Record<string, unknown>>).map((entry) => ({
                metric: String(entry.metric ?? ""),
                mean: numberOrNull(entry.mean),
                min: numberOrNull(entry.min),
                max: numberOrNull(entry.max),
                count: optionalNumber(entry.count) ?? 0
              }))
            : [],
          gateFailures: numberRecord(item.gateFailures),
          pairedLift: numberOrNull(item.pairedLift),
          pairedTrials: optionalNumber(item.pairedTrials) ?? 0,
          policyStepFraction: numberOrNull(item.policyStepFraction),
          budgetExhaustedTrials:
            optionalNumber(trials.budget_exhausted)
            ?? optionalNumber(item.budgetExhaustedTrials)
            ?? 0,
          eliminatedAt: optionalString(item.eliminatedAt) ?? null,
          eliminationReason: optionalString(item.eliminationReason) ?? null,
          costUsd: numberOrNull(item.costUsd)
        };
        // One row per candidate per stage; a later scoring of the same pair
        // supersedes the earlier one (elimination is applied as a rescore).
        evalScorecards.set(`${card.stage}:${card.candidateId}`, card);
      } else if (event.type === "eval.selection.completed") {
        const sel = (event.snapshot?.selection ?? {}) as Record<string, unknown>;
        evalSelection = {
          status: String(sel.status ?? "inconclusive"),
          winnerId: optionalString(sel.winner_id) ?? null,
          baselineId: optionalString(sel.baseline_id) ?? null,
          primaryMetric: String(sel.primary_metric ?? ""),
          lift: numberOrNull(sel.lift),
          minLift: optionalNumber(sel.min_lift) ?? 0,
          reason: String(sel.reason ?? "")
        };
      } else if (event.type === "eval.run.paused") {
        evalPaused = true;
      } else if (event.type === "eval.run.resumed") {
        evalPaused = false;
      }
    }
    if (event.type === "optimizer.run.completed" || event.type === "optimizer.run.failed"
      || event.type === "optimizer.run.cancelled") {
      evalPaused = false;
      const dir = optionalString(event.delta?.evidenceDir);
      if (dir) evalEvidenceDir = dir;
    }

    // Item lifecycle events also carry `status`; those describe candidates,
    // checkpoints, or child rollouts and must never overwrite the run status.
    const runLifecycleEvent = event.type.startsWith("optimizer.run.")
      || event.type.startsWith("gepa.run.")
      || event.type === "goex.run_started"
      || event.type === "goex.run_finished"
      || event.type === "goex.run_failed"
      || event.type.startsWith("sft.run.")
      || event.type === "run.queued"
      || event.type === "run.started"
      || event.type === "run.completed"
      || event.type === "run.failed";
    const nextStatus = (
      (runLifecycleEvent ? event.snapshot?.status ?? event.delta?.status : undefined) ??
      (event.type === "optimizer.state.transitioned" ? event.delta?.to : undefined) ??
      (event.type === "gepa.run.finished" ? event.delta?.state : undefined) ??
      (event.type === "goex.run_finished" ? "completed" : undefined) ??
      (event.type === "goex.run_failed" ? "failed" : undefined)
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
    if (event.type === "gepa.run.started") {
      contract.container = {
        ...(contract.container ?? {}),
        url: optionalString(eventDelta.container_url) ?? contract.container?.url
      };
    }
    if (event.type === "container.task_info.loaded") {
      const task = objectRecord(eventDelta.task);
      const dataset = objectRecord(eventDelta.dataset);
      const datasetSplits = objectRecord(dataset.splits ?? eventDelta.splits);
      const trainSplit = objectRecord(datasetSplits.train);
      const selectionSplit = objectRecord(datasetSplits.selection);
      const heldoutSplit = objectRecord(datasetSplits.heldout);
      contract.task = {
        id: optionalString(task.id ?? task.task_id ?? eventDelta.task_id),
        name: optionalString(task.name ?? eventDelta.task_name),
        objective: optionalString(eventDelta.objective),
        description: optionalString(task.description),
        family: optionalString(task.task_family),
        version: optionalString(task.version),
        outputKind: optionalString(eventDelta.output_kind)
      };
      contract.dataset = {
        source: optionalString(dataset.source ?? task.benchmark),
        config: optionalString(dataset.config),
        revision: optionalString(dataset.revision),
        digest: optionalString(dataset.dataset_digest),
        rowCount: missingNumber(dataset.row_count),
        labelCount: missingNumber(dataset.label_count),
        splits: {
          train: missingNumber(trainSplit.count),
          selection: missingNumber(selectionSplit.count),
          heldout: missingNumber(heldoutSplit.count)
        }
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
      const taskPools = objectRecord(eventDelta.task_pools);
      contract.splits = {
        train: contract.splits?.train ?? (Array.isArray(taskPools.pareto) ? taskPools.pareto.length : undefined),
        minibatch: missingNumber(eventDelta.minibatch_rows),
        reflection: missingNumber(eventDelta.reflection_rows),
        pareto: missingNumber(eventDelta.pareto_rows),
        heldout: missingNumber(eventDelta.heldout_rows)
      };
    }
    if (event.type === "container.contract.verified") {
      const refs = Array.isArray(eventDelta.policy_refs) ? eventDelta.policy_refs : [];
      const policy = refs.find((value) => value && typeof value === "object" && !Array.isArray(value)) as Record<string, unknown> | undefined;
      const evaluator = objectRecord(eventDelta.evaluator ?? objectRecord(eventDelta.evaluation).evaluator);
      const dataset = objectRecord(eventDelta.dataset);
      const datasetSplits = objectRecord(dataset.splits);
      const trainSplit = objectRecord(datasetSplits.train);
      const selectionSplit = objectRecord(datasetSplits.selection);
      const heldoutSplit = objectRecord(datasetSplits.heldout);
      contract.dataset = {
        ...(contract.dataset ?? {}),
        source: optionalString(dataset.source) ?? contract.dataset?.source,
        config: optionalString(dataset.config) ?? contract.dataset?.config,
        revision: optionalString(dataset.revision) ?? contract.dataset?.revision,
        digest: optionalString(dataset.dataset_digest) ?? contract.dataset?.digest,
        rowCount: missingNumber(dataset.row_count) ?? contract.dataset?.rowCount,
        labelCount: missingNumber(dataset.label_count) ?? contract.dataset?.labelCount,
        splits: {
          train: missingNumber(trainSplit.count) ?? contract.dataset?.splits?.train,
          selection: missingNumber(selectionSplit.count) ?? contract.dataset?.splits?.selection,
          heldout: missingNumber(heldoutSplit.count) ?? contract.dataset?.splits?.heldout
        }
      };
      contract.container = {
        ...(contract.container ?? {}),
        verified: true,
        specId: optionalString(eventDelta.container_spec_id),
        workshopInstance: optionalString(eventDelta.workshop_instance),
        credentialMode: optionalString(eventDelta.credential_mode),
        evaluatorId: optionalString(
          evaluator.evaluator_id ?? evaluator.evaluatorId ?? eventDelta.evaluator_id ?? eventDelta.evaluation_plan_ref
        ),
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
      const trainIds = Array.isArray(details.train_ids) ? details.train_ids : undefined;
      const heldoutIds = Array.isArray(details.heldout_ids) ? details.heldout_ids : undefined;
      if (trainIds || heldoutIds) {
        contract.splits = {
          ...(contract.splits ?? {}),
          train: trainIds?.length ?? contract.splits?.train,
          heldout: heldoutIds?.length ?? contract.splits?.heldout
        };
      }
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
    if (event.type === "runtime.job.completed" || event.type === "runtime.throughput.warning") {
      const configuredRolloutWorkers = missingNumber(event.delta?.configured_rollout_workers);
      const staticRolloutWorkers = missingNumber(event.delta?.static_rollout_workers);
      const estimatedEffectiveConcurrency = missingNumber(event.delta?.estimated_effective_concurrency);
      const rolloutSubmissionMode = optionalString(event.delta?.rollout_submission_mode);
      if (configuredRolloutWorkers != null) runtime.configuredRolloutWorkers = configuredRolloutWorkers;
      if (staticRolloutWorkers != null) runtime.staticRolloutWorkers = staticRolloutWorkers;
      if (estimatedEffectiveConcurrency != null) runtime.estimatedEffectiveConcurrency = estimatedEffectiveConcurrency;
      if (rolloutSubmissionMode) runtime.rolloutSubmissionMode = rolloutSubmissionMode;

      // Result events arrive in a burst after each dispatch. Use the runtime's
      // completed-batch measurement so the UI does not manufacture a huge
      // throughput number from a few nearly simultaneous journal appends.
      const observedPerSecond = missingNumber(event.delta?.observed_uncached_rollouts_per_second);
      const cacheMisses = missingNumber(event.delta?.cache_misses);
      const wallSeconds = missingNumber(event.delta?.wall_seconds);
      if (observedPerSecond != null) {
        runtime.rolloutsPerMinute = observedPerSecond * 60;
      } else if (cacheMisses != null && cacheMisses > 0 && wallSeconds != null && wallSeconds > 0) {
        runtime.rolloutsPerMinute = cacheMisses * 60 / wallSeconds;
      }
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
          seed: event.delta?.seed,
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
    if ((event.type === "sft.checkpoint.created" || event.type === "training.checkpoint.created") && event.item) {
      checkpoints.push({
        id: event.item.id,
        status: event.item.status ?? "created",
        ready: false,
        selected: false,
        promoted: false,
        ...(event.item.raw ?? {})
      });
    }
    if ((event.type === "sft.checkpoint.ready" || event.type === "training.checkpoint.ready") && event.item) {
      const id = event.item.id;
      const existing = checkpoints.find((ckpt) => ckpt.id === id);
      if (existing) {
        existing.status = event.item.status ?? "ready";
        existing.ready = true;
      } else {
        checkpoints.push({ id, status: "ready", ready: true, selected: false, promoted: false, ...(event.item.raw ?? {}) });
      }
    }
    if (
      event.type === "sft.checkpoint.selected" ||
      event.type === "sft.checkpoint.promoted" ||
      event.type === "training.cispo.checkpoint.promoted"
    ) {
      const id = String(event.item?.id ?? event.delta?.checkpoint_id ?? event.delta?.checkpointId ?? "");
      if (id) {
        const claimed = event.delta?.uplift_claimed === true
          || event.delta?.improvement_verdict === "improvement_demonstrated";
        const existing = checkpoints.find((ckpt) => ckpt.id === id);
        if (existing) {
          existing.selected = true;
          existing.promoted = claimed;
          if (claimed) existing.status = "promoted";
        } else {
          checkpoints.push({
            id,
            status: claimed ? "promoted" : (event.item?.status ?? "selected"),
            selected: true,
            promoted: claimed,
            ...(event.item?.raw ?? {})
          });
        }
        summary = {
          ...summary,
          selectedCheckpointId: id,
          improvementVerdict: event.delta?.improvement_verdict ?? summary.improvementVerdict,
          ...(claimed ? { promotedCheckpointId: id } : {})
        };
      }
    }
    if (event.type === "sft.step.metrics" || event.type === "sft.training.metrics" || event.type === "training.metrics" || event.type === "cispo.training.metrics" || event.type === "cispo.update.completed" || event.type === "cispo.step.metrics") {
      const step = missingNumber(event.delta?.step ?? event.delta?.global_step);
      const epoch = missingNumber(event.delta?.epoch);
      const trainLoss = missingNumber(event.delta?.train_loss ?? event.delta?.trainLoss ?? event.delta?.loss);
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
      const groupSize = missingNumber(event.delta?.group_size ?? event.delta?.groupSize);
      const rewardVariance = missingNumber(event.delta?.reward_variance ?? event.delta?.rewardVariance);
      const advantageMean = missingNumber(event.delta?.advantage_mean ?? event.delta?.advantageMean);
      const advantageStd = missingNumber(event.delta?.advantage_std ?? event.delta?.advantageStd ?? event.delta?.advantage_sd);
      const optimizerStep = missingNumber(event.delta?.optimizer_step ?? event.delta?.optimizerStep);
      if (groupSize != null) cispoGroupSize = Math.max(cispoGroupSize ?? 0, groupSize);
      if (rewardVariance != null) cispoRewardVariance = rewardVariance;
      if (advantageMean != null) cispoAdvantageMean = advantageMean;
      if (advantageStd != null) cispoAdvantageStd = advantageStd;
      if (optimizerStep != null) cispoOptimizerSteps = Math.max(cispoOptimizerSteps, optimizerStep);
      else if (step != null && run.algorithmId === "cispo") cispoOptimizerSteps = Math.max(cispoOptimizerSteps, 1);
    }
    if (event.type === "cispo.clip.identity") {
      const clip = (event.delta?.clip && typeof event.delta.clip === "object" && !Array.isArray(event.delta.clip)
        ? event.delta.clip as Record<string, unknown>
        : event.delta) ?? {};
      cispoClipLow = missingNumber(clip.clip_low ?? clip.clipLow ?? clip.eps_low ?? clip.low) ?? null;
      cispoClipHigh = missingNumber(clip.clip_high ?? clip.clipHigh ?? clip.eps_high ?? clip.high) ?? null;
    }
    if (event.type === "cispo.no_learning_signal") {
      cispoNoLearningSignal = true;
    }
    if (event.type === "cispo.zero_advantage.detected") {
      cispoZeroAdvantageGroups += 1;
    }
    if (event.type === "cispo.rollout_group.completed") {
      const rewards = Array.isArray(event.delta?.rewards) ? event.delta.rewards : [];
      const advantages = Array.isArray(event.delta?.advantages) ? event.delta.advantages : [];
      const observedGroupSize = Math.max(rewards.length, advantages.length);
      if (observedGroupSize > 0) cispoGroupSize = Math.max(cispoGroupSize ?? 0, observedGroupSize);
      const rewardVariance = missingNumber(event.delta?.reward_variance ?? event.delta?.rewardVariance);
      const advantageMean = missingNumber(event.delta?.meanAdvantage ?? event.delta?.advantage_mean);
      if (rewardVariance != null) cispoRewardVariance = rewardVariance;
      if (advantageMean != null) cispoAdvantageMean = advantageMean;
      if (rewardVariance != null && rewardVariance > 0) cispoLearningSignalGroups += 1;
      cispoRolloutGroups.push({
        id: String(event.delta?.group_id ?? event.delta?.groupId ?? `group-${event.sequenceNumber}`),
        iteration: missingNumber(event.delta?.iteration),
        label: typeof event.delta?.label === "string" ? event.delta.label : null,
        rewardMean: missingNumber(event.delta?.reward_mean ?? event.delta?.rewardMean),
        rewardVariance,
        size: observedGroupSize,
        sequence: event.sequenceNumber
      });
    }
    if (event.type === "training.warm_start" || event.type === "cispo.warm_start") {
      const id = event.delta?.training_artifact_id ?? event.delta?.trainingArtifactId ?? event.item?.id;
      if (typeof id === "string" && id) cispoWarmStartArtifactId = id;
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
    // ── Phase A · baseline ──────────────────────────────────────────────
    // Per-seed rollouts of the unchanged student, plus an optional summary
    // that carries the frozen split digest.
    if (event.type === "sft.baseline_rollout.completed") {
      const entry = seedResult({ ...(event.delta ?? {}), ...(event.item?.raw ?? {}) });
      if (entry) {
        baselineSeeds.set(entry.seed, entry);
        // Baseline and terminal use the same frozen seeds. Keeping the base
        // arm here lets the later heldout rollouts form an honest paired
        // before/after comparison without duplicating provider inference.
        comparisonBase.set(entry.seed, entry);
      }
    }
    if (
      event.type === "sft.baseline_evaluation.completed" ||
      event.type === "sft.baseline_eval.completed"
    ) {
      const payload = (event.snapshot ?? event.delta ?? {}) as Record<string, unknown>;
      const digest = payload.split_digest ?? payload.seed_manifest_digest;
      if (typeof digest === "string") baselineSplitDigest = digest;
      for (const entry of armDetails(payload).seeds) baselineSeeds.set(entry.seed, entry);
    }

    // ── Phase B · teacher collection ────────────────────────────────────
    if (event.type === "sft.teacher_rollout.completed") teacherRolloutCount += 1;

    // ── Phase C · curation ──────────────────────────────────────────────
    if (
      event.type === "sft.curation.candidate_evaluated" ||
      event.type === "sft.curation.case_completed"
    ) {
      const candidate = curationCandidate({ ...(event.delta ?? {}), ...(event.item?.raw ?? {}) });
      if (candidate) curationCandidates.set(candidate.id, candidate);
    }
    if (event.type === "sft.curation.completed" || event.type === "sft.curation.validated") {
      const payload = (event.snapshot ?? event.delta ?? {}) as Record<string, unknown>;
      curationFunnel = { ...curationFunnel, ...payload };
      const list = payload.candidates;
      if (Array.isArray(list)) {
        for (const raw of list) {
          const candidate = curationCandidate(raw);
          if (candidate) curationCandidates.set(candidate.id, candidate);
        }
      }
    }

    // ── Phase G · paired heldout comparison ─────────────────────────────
    // `sft.heldout_evaluation.*` is the canonical name; `sft.heldout_eval.*`
    // is the older alias that shipped in fixtures. Accept both so a producer
    // on either name is projected instead of silently dropped.
    if (
      event.type === "sft.heldout_evaluation.completed" ||
      event.type === "sft.heldout_eval.completed"
    ) {
      const payload = (event.snapshot ?? event.delta ?? {}) as Record<string, unknown>;
      const digest = payload.split_digest ?? payload.seed_manifest_digest;
      if (typeof digest === "string") comparisonSplitDigest = digest;
      const baseArm = armDetails(payload.base);
      const trainedArm = armDetails(payload.trained ?? payload.sft ?? payload.promoted);
      if (baseArm.label) comparisonBaseLabel = baseArm.label;
      if (trainedArm.label) comparisonTrainedLabel = trainedArm.label;
      for (const entry of baseArm.seeds) comparisonBase.set(entry.seed, entry);
      for (const entry of trainedArm.seeds) comparisonTrained.set(entry.seed, entry);
    }
    if (event.type === "sft.heldout_rollout.completed") {
      const payload = { ...(event.delta ?? {}), ...(event.item?.raw ?? {}) } as Record<string, unknown>;
      const entry = seedResult(payload);
      const arm = String(payload.arm ?? payload.policy ?? payload.label ?? "");
      if (entry && arm) {
        if (arm === "base") comparisonBase.set(entry.seed, entry);
        else comparisonTrained.set(entry.seed, entry);
      }
    }

    if (
      event.type === "sft.checkpoint_eval.completed" ||
      event.type === "sft.heldout_evaluation.completed" ||
      event.type === "sft.heldout_eval.completed" ||
      event.type === "sft.checkpoint_evaluation.completed" ||
      event.type === "training.evaluation.completed"
    ) {
      // A paired heldout payload carries arms, not a scalar metric; it is
      // already projected into `comparison`. Only summarize events that
      // actually report a metric or score.
      const nestedEvaluation = event.delta?.evaluation && typeof event.delta.evaluation === "object" && !Array.isArray(event.delta.evaluation)
        ? event.delta.evaluation as Record<string, unknown>
        : {};
      const scalar = { ...(event.delta ?? {}), ...nestedEvaluation };
      const hasScalar = scalar.metric != null
        || scalar.score != null
        || scalar.accuracy != null
        || scalar.calibration_accuracy != null;
      if (!hasScalar && event.type.includes("heldout")) continue;
      const kind = String(event.delta?.kind ?? "");
      const role = event.delta?.role ?? (
        kind.includes("checkpoint") || event.type.includes("checkpoint")
          ? "checkpoint"
          : kind.includes("heldout") || event.type.includes("heldout")
            ? "heldout"
            : event.delta?.phase ?? "selection"
      );
      evaluations.push({
        sequence: event.sequenceNumber,
        role,
        measurementOnly: Boolean(event.delta?.measurementOnly),
        ...(event.delta ?? {}),
        ...nestedEvaluation,
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
    if (runtime.rolloutsPerMinute == null && recentCompletionTimes.length >= 2) {
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
  } else if (run.algorithmId === "eval") {
    const planCandidates = Array.isArray(evalPlan.candidates)
      ? (evalPlan.candidates as Array<Record<string, unknown>>).map((entry) => ({
          id: String(entry.id ?? ""),
          label: String(entry.label ?? entry.id ?? ""),
          isBaseline: entry.is_baseline === true
        }))
      : [];
    projected.eval = {
      candidates: planCandidates,
      // Stable order: screening before confirmation, baseline first inside a
      // stage, so a candidate does not jump rows as later events land.
      scorecards: [...evalScorecards.values()].sort((a, b) => {
        if (a.stage !== b.stage) return a.stage === "screen" ? -1 : 1;
        if (a.isBaseline !== b.isBaseline) return a.isBaseline ? -1 : 1;
        return a.label.localeCompare(b.label);
      }),
      trials: [...evalTrials.values()].sort((a, b) => a.id.localeCompare(b.id)),
      rollouts: [...evalRollouts.values()].sort((a, b) => b.sequence - a.sequence),
      selection: evalSelection,
      seedLedger: evalLedger,
      manifestDigest: optionalString(evalPlan.manifest_digest) ?? null,
      candidateSetId: optionalString(evalPlan.candidate_set_id) ?? null,
      evidenceDir: evalEvidenceDir,
      plannedTrials: optionalNumber(evalPlan.planned_trials) ?? 0,
      parallelism: numberOrNull(evalPlan.parallelism),
      globalCapacity: numberOrNull(evalPlan.global_capacity),
      paused: evalPaused
    };
  } else if (run.algorithmId === "sft" || run.algorithmId === "cispo") {
    const candidates = [...curationCandidates.values()];
    const acceptedCount = candidates.filter((candidate) => candidate.accepted).length;
    const rejectionsByReason: Record<string, number> = {};
    for (const candidate of candidates) {
      if (candidate.accepted) continue;
      const reason = candidate.reason ?? "unspecified";
      rejectionsByReason[reason] = (rejectionsByReason[reason] ?? 0) + 1;
    }
    const achievementsCovered = [...new Set(
      candidates.filter((candidate) => candidate.accepted).flatMap((candidate) => candidate.achievements ?? [])
    )].sort();
    const seedsCovered = new Set(
      candidates.filter((candidate) => candidate.accepted && candidate.seed != null).map((candidate) => candidate.seed)
    ).size;
    // A count the producer never reported stays null. Only fall back to a
    // derived count when candidates were actually observed.
    const reported = (key: string): number | null => numberOrNull(curationFunnel[key]);
    const pairedSeeds = [...new Set([...comparisonBase.keys(), ...comparisonTrained.keys()])];

    projected.sft = {
      curves,
      points,
      checkpoints,
      evaluations,
      campaigns,
      dataset,
      compute,
      examples,
      lineage,
      baseline: baselineSeeds.size > 0 || baselineSplitDigest
        ? { splitDigest: baselineSplitDigest, seeds: [...baselineSeeds.values()] }
        : undefined,
      curation: {
        collected: reported("collected") ?? (teacherRolloutCount > 0 ? teacherRolloutCount : null),
        considered: reported("considered") ?? (candidates.length > 0 ? candidates.length : null),
        accepted: reported("accepted") ?? (candidates.length > 0 ? acceptedCount : null),
        rejected: reported("rejected") ?? (candidates.length > 0 ? candidates.length - acceptedCount : null),
        rejectionsByReason,
        seedsCovered: candidates.length > 0 ? seedsCovered : null,
        achievementsCovered,
        candidates
      },
      comparison: pairedSeeds.length > 0
        ? {
            splitDigest: comparisonSplitDigest,
            baseLabel: comparisonBaseLabel ?? "Base student",
            trainedLabel: comparisonTrainedLabel ?? "Promoted checkpoint",
            pairs: pairedSeeds
              .sort((a, b) => (Number(a) - Number(b)) || a.localeCompare(b))
              .map((seed) => ({
                seed,
                base: comparisonBase.get(seed) ?? null,
                trained: comparisonTrained.get(seed) ?? null
              }))
          }
        : undefined
    };
    if (run.algorithmId === "cispo") {
      const warmStart = cispoWarmStartArtifactId
        ?? (typeof lineage.parentArtifactId === "string" ? lineage.parentArtifactId : null)
        ?? (typeof lineage.warmStartArtifactId === "string" ? lineage.warmStartArtifactId : null);
      projected.cispo = {
        objective: typeof run.objective === "string" && run.objective
          ? run.objective
          : "CISPO clipped-importance policy optimization",
        clipLow: cispoClipLow,
        clipHigh: cispoClipHigh,
        groupSize: cispoGroupSize,
        rewardVariance: cispoRewardVariance,
        advantageMean: cispoAdvantageMean,
        advantageStd: cispoAdvantageStd,
        optimizerSteps: cispoOptimizerSteps > 0 ? cispoOptimizerSteps : points.length,
        warmStartArtifactId: warmStart,
        checkpointIds: checkpoints.map((ckpt) => String(ckpt.id ?? "")).filter(Boolean),
        rolloutGroups: cispoRolloutGroups,
        zeroAdvantageGroups: cispoZeroAdvantageGroups,
        learningSignalGroups: cispoLearningSignalGroups,
        noLearningSignal: cispoNoLearningSignal
      };
    }
  } else if (projectDag) {
    projected.dag = finalizeDagState([...dagNodes.values()], dagName, dagSequence);
  }

  return projected;
}

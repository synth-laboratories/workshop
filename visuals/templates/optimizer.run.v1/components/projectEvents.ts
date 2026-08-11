/** Project optimizer_event.v1 fixtures into shared + algorithm slices at a cursor. */

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
  usageDelta?: Record<string, number>;
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
};

export type ProjectedState = {
  cursorSeq: number;
  summary: Record<string, unknown>;
  timeline: Array<Record<string, unknown>>;
  usage: Record<string, number>;
  logs: Array<Record<string, unknown>>;
  artifacts: unknown[];
  execution: { bindings: Array<Record<string, unknown>> };
  gepa?: {
    candidates: Array<Record<string, unknown>>;
    frontier: Array<Record<string, unknown>>;
    reflections: Array<Record<string, unknown>>;
  };
  goex?: {
    board: Record<string, unknown>;
    themes: Array<Record<string, unknown>>;
  };
  sft?: {
    curves: {
      steps: number[];
      epochs: number[];
      trainLoss: number[];
      validationLoss: number[];
      learningRate: number[];
    };
    checkpoints: Array<Record<string, unknown>>;
    evaluations: Array<Record<string, unknown>>;
    dataset: Record<string, unknown>;
    compute: Record<string, unknown>;
    examples: Array<Record<string, unknown>>;
    lineage?: Record<string, unknown>;
  };
};

export function projectAtCursor(
  run: OptimizerRun,
  events: OptimizerEvent[],
  atSeq?: number
): ProjectedState {
  const maxSeq = atSeq ?? Math.max(0, ...events.map((e) => e.sequenceNumber), run.cursorSeq ?? 0);
  const visible = events
    .filter((e) => e.sequenceNumber <= maxSeq)
    .sort((a, b) => a.sequenceNumber - b.sequenceNumber);

  const usage = {
    costUsd: 0,
    promptTokens: 0,
    completionTokens: 0,
    rollouts: 0,
    wallTimeMs: 0
  };
  const timeline: Array<Record<string, unknown>> = [];
  const logs: Array<Record<string, unknown>> = [];
  const artifacts: unknown[] = [];
  const candidates = new Map<string, Record<string, unknown>>();
  let frontier: Array<Record<string, unknown>> = [];
  const reflections: Array<Record<string, unknown>> = [];
  let board: Record<string, unknown> = { phase: "idle", tick: 0 };
  const themes: Array<Record<string, unknown>> = [];
  const checkpoints: Array<Record<string, unknown>> = [];
  const evaluations: Array<Record<string, unknown>> = [];
  const curves = {
    steps: [] as number[],
    epochs: [] as number[],
    trainLoss: [] as number[],
    validationLoss: [] as number[],
    learningRate: [] as number[]
  };
  let dataset: Record<string, unknown> = { splits: {} };
  let compute: Record<string, unknown> = {};
  let examples: Array<Record<string, unknown>> = [];
  let lineage: Record<string, unknown> = {};
  let status = run.status;
  let summary = { ...(run.summary ?? {}) };

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
    if (event.usageDelta) {
      usage.costUsd += Number(event.usageDelta.cost_usd ?? event.usageDelta.costUsd ?? 0);
      usage.promptTokens += Number(event.usageDelta.prompt_tokens ?? 0);
      usage.completionTokens += Number(event.usageDelta.completion_tokens ?? 0);
      usage.rollouts += Number(event.usageDelta.rollouts ?? 0);
      usage.wallTimeMs += Number(event.usageDelta.wall_time_ms ?? 0);
    }
    const nextStatus = (event.snapshot?.status ?? event.delta?.status) as string | undefined;
    if (nextStatus) status = nextStatus;
    if (event.snapshot?.summary && typeof event.snapshot.summary === "object") {
      summary = { ...summary, ...(event.snapshot.summary as Record<string, unknown>) };
    }
    if (typeof event.snapshot?.bestScore === "number") summary.bestScore = event.snapshot.bestScore;

    if (
      event.type.includes("candidate.") ||
      event.type === "gepa.candidate.updated"
    ) {
      const id = event.item?.id;
      if (id) {
        candidates.set(id, {
          id,
          status: event.item?.status,
          ...(event.item?.raw ?? {}),
          ...(event.delta ?? {}),
          sequence: event.sequenceNumber
        });
      }
    }
    if (event.type.startsWith("frontier.") || event.type === "gepa.frontier.updated") {
      const cells = event.snapshot?.cells ?? event.delta?.cells;
      if (Array.isArray(cells)) frontier = cells as Array<Record<string, unknown>>;
    }
    if (event.type === "gepa.reflection" || event.type === "proposer.completed") {
      reflections.push({
        sequence: event.sequenceNumber,
        occurredAt: event.occurredAt,
        message: event.delta?.message,
        ...(event.delta ?? {})
      });
    }
    if (event.type.includes("board.updated")) {
      board = { ...(event.snapshot ?? event.delta ?? {}) };
    }
    if (event.type.includes("theme.updated")) {
      themes.push({ sequence: event.sequenceNumber, ...(event.delta ?? {}) });
    }
    if (event.type === "sft.checkpoint.created" && event.item) {
      checkpoints.push({ id: event.item.id, status: event.item.status, ...(event.item.raw ?? {}) });
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
    if (event.type === "sft.step.metrics") {
      if (typeof event.delta?.step === "number") curves.steps.push(event.delta.step);
      if (typeof event.delta?.epoch === "number") curves.epochs.push(event.delta.epoch);
      if (typeof event.delta?.train_loss === "number") curves.trainLoss.push(event.delta.train_loss);
      if (typeof event.delta?.validation_loss === "number") {
        curves.validationLoss.push(event.delta.validation_loss);
      }
      if (typeof event.delta?.learning_rate === "number") {
        curves.learningRate.push(event.delta.learning_rate);
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
      event.type === "sft.heldout_eval.completed"
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

  if (run.algorithmId === "gepa") {
    projected.gepa = {
      candidates: [...candidates.values()],
      frontier,
      reflections
    };
  } else if (run.algorithmId === "go-ex") {
    projected.goex = { board, themes };
  } else if (run.algorithmId === "sft") {
    projected.sft = { curves, checkpoints, evaluations, dataset, compute, examples, lineage };
  }

  return projected;
}

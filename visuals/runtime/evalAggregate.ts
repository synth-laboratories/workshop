type Json = Record<string, unknown>;

export type EvalAggregateV1 = {
  schemaVersion: "eval.aggregate.v1";
  runId: string;
  asOfSequence: number;
  projectionRevision: number;
  lifecycle: string;
  work: Json;
  evidence: Json;
  selection: string;
  meanReward: number | null;
  scoredTrials: number;
  evaluatorEvidence: number;
  traceCount: number;
  evidenceRefCount: number;
};

export type EvalAggregateWorkFacts = {
  rolloutCount: number | null;
  terminalCount: number;
  running: number;
  queued: number;
  failed: number;
  started: number;
};

function object(value: unknown): Json | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Json : null;
}

function count(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : 0;
}

/** Accept only the revision-addressed backend aggregate for the bound run. */
export function evalAggregateV1(value: unknown, runId?: string | null): EvalAggregateV1 | null {
  const aggregate = object(value);
  if (
    aggregate?.schemaVersion !== "eval.aggregate.v1"
    || typeof aggregate.runId !== "string"
    || (runId && aggregate.runId !== runId)
    || typeof aggregate.projectionRevision !== "number"
    || typeof aggregate.asOfSequence !== "number"
    || !object(aggregate.work)
  ) {
    return null;
  }
  return aggregate as EvalAggregateV1;
}

/** Work counts are formatted from the canonical aggregate, never raw rows. */
export function evalAggregateWorkFacts(aggregate: EvalAggregateV1): EvalAggregateWorkFacts {
  const planned = typeof aggregate.work.planned === "number" && Number.isFinite(aggregate.work.planned)
    ? aggregate.work.planned
    : null;
  const running = count(aggregate.work.running);
  const queued = count(aggregate.work.queued);
  const failed = count(aggregate.work.failed) + count(aggregate.work.cancelled);
  const terminalCount = count(aggregate.work.succeeded) + failed;
  return {
    rolloutCount: planned,
    terminalCount,
    running,
    queued,
    failed,
    started: terminalCount + running
  };
}

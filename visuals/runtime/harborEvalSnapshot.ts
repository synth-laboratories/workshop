/**
 * Terminal restoration for the Harbor eval templates.
 *
 * `live.harbor_eval.v1` was built around a connected SSE input. Reopening a
 * settled run replays nothing — the producer's stream is closed — so the
 * template sat at `connecting` with `0/0` events while the binding it was
 * holding already contained a complete `synth.experiment.overview.v1`
 * snapshot of the same run (five terminal trials, five sealed traces, an
 * honest `usage: unavailable`).
 *
 * This module projects that persisted snapshot into the same shape the live
 * fold produces, so a reopened visual renders from evidence instead of
 * waiting for a stream that will never speak again.
 */

type Json = Record<string, unknown>;

export type HarborSnapshotTrial = {
  id: string;
  /** Task identity, never a bare seed. */
  label: string;
  taskInstanceId: string | null;
  seed: number | null;
  status: string;
  reward: number | null;
  /** Producer-declared terminal reason, already de-prefixed for reading. */
  stopReason: string | null;
  traceId: string | null;
  /** Trace workstation minted for this rollout, when one exists. */
  workbenchVisualId: string | null;
};

export type HarborSnapshotUsage = {
  /** Rendered token statement. `null` means the producer reported nothing. */
  tokens: string | null;
  cost: string | null;
  /** False whenever tokens or cost are unknown rather than measured. */
  complete: boolean;
};

export type HarborEvalSnapshot = {
  runId: string | null;
  title: string | null;
  /** `terminal` only when the producer said so. Never inferred from silence. */
  lifecycle: "terminal" | "running" | "unknown";
  status: string | null;
  work: {
    planned: number;
    succeeded: number;
    failed: number;
    running: number;
    queued: number;
    cancelled: number;
  };
  elapsed: string | null;
  usage: HarborSnapshotUsage;
  trials: HarborSnapshotTrial[];
  evidence: { completeness: string | null; reason: string | null; refCount: number };
  meanReward: number | null;
  limitations: string[];
  assessment: { summary: string | null; nextStep: string | null };
  runtime: {
    provider: string | null;
    model: string | null;
    policy: string | null;
    parallelism: number | null;
  };
};

function object(value: unknown): Json | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Json : null;
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : null;
}

function integer(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? Math.trunc(value) : 0;
}

function finite(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/**
 * A producer stop reason is written `code: prose`. The code is the machine
 * fact and the prose is the sentence a reader needs; keep both, in that order,
 * rather than truncating to either one.
 */
function stopReason(value: unknown): string | null {
  const reason = object(value);
  if (reason) {
    const code = text(reason.reason) ?? text(reason.code);
    const detail = text(reason.detail) ?? text(reason.message);
    if (code && detail) return `${code}: ${detail}`;
    return code ?? detail;
  }
  return text(value);
}

/** The task name a reader recognises, from the fullest identity available. */
function trialLabel(row: Json): string {
  const instance = text(row.taskInstanceId);
  const label = text(row.label);
  if (label) return label;
  if (instance) return instance.split("/").at(-1) ?? instance;
  const seed = finite(row.seed);
  return seed == null ? "trial" : `seed ${seed}`;
}

function trialRows(overview: Json): HarborSnapshotTrial[] {
  const candidates = [
    object(overview.results)?.rollouts,
    object(overview.traces)?.items,
    overview.records
  ];
  const rows = candidates.find(Array.isArray) as unknown[] | undefined;
  if (!rows) return [];
  return rows.flatMap((value, index) => {
    const row = object(value);
    if (!row) return [];
    return [{
      id: text(row.id) ?? text(row.rolloutId) ?? text(row.trialId) ?? `trial_${index}`,
      label: trialLabel(row),
      taskInstanceId: text(row.taskInstanceId),
      seed: finite(row.seed),
      status: text(row.status) ?? text(row.reportedStatus) ?? "unknown",
      reward: finite(row.reward),
      stopReason: stopReason(row.stopReason) ?? stopReason(row.error),
      traceId: text(row.traceId),
      workbenchVisualId: text(row.visualId)
    }];
  });
}

/**
 * Progress carries pre-rendered statements like `unavailable / $16.00`. Those
 * words are the producer's own honesty about a gap and must survive; only an
 * absent field becomes `null`.
 */
function usageFrom(progress: Json | null): HarborSnapshotUsage {
  const tokens = text(progress?.usage);
  const cost = text(progress?.cost);
  const unknown = (value: string | null) =>
    value != null && /unavailable|unknown|pending|not reported/i.test(value);
  return {
    tokens,
    cost,
    complete: tokens != null && cost != null && !unknown(tokens) && !unknown(cost)
  };
}

/**
 * Project a persisted `synth.experiment.overview.v1` document.
 *
 * Returns `null` for anything else, so a template can fall back to its live
 * fold rather than rendering an invented terminal state.
 */
export function harborEvalSnapshot(value: unknown): HarborEvalSnapshot | null {
  const overview = object(value);
  if (overview?.schemaVersion !== "synth.experiment.overview.v1") return null;
  const aggregate = object(overview.aggregate);
  const progress = object(overview.progress);
  const work = object(aggregate?.work);
  const evidence = object(aggregate?.evidence);
  const runtime = object(overview.runtime);
  const assessment = object(overview.assessment);
  const lifecycle = text(aggregate?.lifecycle);
  const trials = trialRows(overview);
  const stateCounts = object(progress?.stateCounts);
  return {
    runId: text(aggregate?.runId),
    title: text(overview.title),
    lifecycle: lifecycle === "terminal" ? "terminal" : lifecycle === "running" ? "running" : "unknown",
    status: text(overview.status) ?? text(progress?.phase),
    work: {
      planned: integer(work?.planned ?? progress?.total ?? trials.length),
      succeeded: integer(work?.succeeded ?? stateCounts?.completed),
      failed: integer(work?.failed ?? stateCounts?.failed),
      running: integer(work?.running ?? stateCounts?.running),
      queued: integer(work?.queued ?? stateCounts?.queued),
      cancelled: integer(work?.cancelled ?? stateCounts?.cancelled)
    },
    elapsed: text(progress?.elapsed),
    usage: usageFrom(progress),
    trials,
    evidence: {
      completeness: text(evidence?.completeness),
      reason: text(evidence?.reason),
      refCount: integer(aggregate?.evidenceRefCount)
    },
    meanReward: finite(aggregate?.meanReward),
    limitations: Array.isArray(overview.limitations)
      ? overview.limitations.flatMap((entry) => (text(entry) ? [text(entry) as string] : []))
      : [],
    assessment: {
      summary: text(assessment?.summary),
      nextStep: text(assessment?.nextStep)
    },
    runtime: {
      provider: text(runtime?.provider),
      model: text(runtime?.model),
      policy: text(runtime?.policy),
      parallelism: finite(runtime?.parallelism)
    }
  };
}

/** The trace workstation a reader should be sent to from a terminal snapshot. */
export function snapshotWorkbenchVisualId(snapshot: HarborEvalSnapshot | null): string | null {
  return snapshot?.trials.find((trial) => trial.workbenchVisualId)?.workbenchVisualId ?? null;
}

/** Decode producer-projected eval fields. No local billing or lifecycle inference. */
export type PhaseClock = { elapsed_seconds: number | null; limit_seconds: number | null; scope: string };
export type BenchmarkObservation = {
  lane: string;
  phase: string;
  observed_at: string;
  work: PhaseClock;
  verify: PhaseClock;
  usage: {
    input_tokens: number | null;
    output_tokens: number | null;
    total_tokens: number | null;
    cost_usd: number | null;
    source: string;
    coverage: string;
    status: "unknown" | "provisional" | "final";
  };
  calls: number | null;
  call_limit: number | null;
  call_limit_unbounded: boolean;
  limit_scope: string;
  spend_limit_usd: number | null;
  spend_enforcement: string;
  steps: number | null;
  step_limit: number | null;
  score: number | null;
  scientific_status: string;
  stop_reason: string;
  terminal: boolean;
  failed: boolean;
  artifact_path: string | null;
};

type Row = Record<string, unknown>;
const object = (v: unknown): Row | null => v !== null && typeof v === "object" && !Array.isArray(v) ? v as Row : null;
const text = (v: unknown): string => typeof v === "string" ? v : "";
const finite = (v: unknown): number | null => typeof v === "number" && Number.isFinite(v) ? v : null;
function clock(v: unknown): PhaseClock {
  const row = object(v);
  return { elapsed_seconds: finite(row?.elapsed_seconds), limit_seconds: finite(row?.limit_seconds), scope: text(row?.scope) || "phase" };
}

export function decodeBenchmarkObservation(value: unknown): BenchmarkObservation | null {
  const row = object(value);
  if (!row || !text(row.lane) || !text(row.observed_at) || !text(row.phase)) return null;
  const usage = object(row.usage);
  return {
    lane: text(row.lane), phase: text(row.phase), observed_at: text(row.observed_at),
    work: clock(row.work), verify: clock(row.verify),
    usage: {
      input_tokens: finite(usage?.input_tokens), output_tokens: finite(usage?.output_tokens),
      total_tokens: finite(usage?.total_tokens), cost_usd: finite(usage?.cost_usd),
      source: text(usage?.source), coverage: text(usage?.coverage),
      status: usage?.status === "final" ? "final" : usage?.status === "provisional" ? "provisional" : "unknown"
    },
    calls: finite(row.calls), call_limit: finite(row.call_limit), call_limit_unbounded: row.call_limit_unbounded === true, limit_scope: text(row.limit_scope),
    spend_limit_usd: finite(row.spend_limit_usd), spend_enforcement: text(row.spend_enforcement),
    steps: finite(row.steps), step_limit: finite(row.step_limit), score: finite(row.score),
    scientific_status: text(row.scientific_status), stop_reason: text(row.stop_reason),
    terminal: row.terminal === true, failed: row.failed === true,
    artifact_path: text(row.artifact_path) || null
  };
}

/** The existing evals snapshot carries the same already-projected lane rows. */
export function benchmarkSnapshotRows(value: unknown): BenchmarkObservation[] {
  const snapshot = object(value);
  if (snapshot?.schema_version !== "evals.live-rollout.v1" || !Array.isArray(snapshot.lanes)) return [];
  return snapshot.lanes.flatMap((lane) => {
    const decoded = decodeBenchmarkObservation(object(lane)?.benchmark_observation);
    return decoded ? [decoded] : [];
  });
}

/** Select the newest producer snapshot per lane; do not sum cumulative counters. */
export function latestBenchmarkObservations(rows: BenchmarkObservation[]): BenchmarkObservation[] {
  const latest = new Map<string, BenchmarkObservation>();
  for (const row of rows) {
    const previous = latest.get(row.lane);
    if (previous && (previous.observed_at > row.observed_at || (previous.terminal && !row.terminal))) continue;
    latest.set(row.lane, row);
  }
  return [...latest.values()].sort((a, b) => a.lane.localeCompare(b.lane));
}

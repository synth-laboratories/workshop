/**
 * Eval workspace template: sticky run header, plan → screen → prune → confirm
 * → select stages, the candidate comparison, the trial matrix, sealed evidence,
 * and a collapsed debug section with the raw event scrubber, usage, artifacts,
 * and execution bindings.
 */

import { OptimizerFamilyShell } from "../../_shared/optimizer.run.v1/components/FamilyShell.tsx";
import {
  ArtifactList,
  EventLog,
  ExecutionBindings,
  GlobalTimeline,
  UsageCards
} from "../../_shared/optimizer.run.v1/components/RunChrome.tsx";
import { EvalWorkspace } from "../../_shared/optimizer.run.v1/overlays/eval/EvalWorkspace.tsx";
import { useMemo, type ReactNode } from "react";
import {
  CollectionBrowser,
  useCollectionPage,
  type RunCollectionsClient,
  type RunCollectionRowLike
} from "../../_shared/optimizer.run.v1/components/workspace/CollectionBrowser.tsx";
import type { VisualBinding } from "../../../../runtime/types.ts";
import type {
  EvalScorecard,
  EvalState,
  EvalTrial,
  OptimizerEvent,
  OptimizerRun,
  ProjectedState
} from "../../_shared/optimizer.run.v1/components/projectEvents.ts";
export type AnalysisCampaign = {
  campaignId?: string;
  status?: string;
  label?: string;
  domain?: string;
  coverage?: { jobs?: number; sealed?: number; abstained?: number; failed?: number };
};

export type ShellProps = {
  title?: string;
  lede?: string;
  data?: unknown;
  optimizer_run?: unknown;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
  events?: OptimizerEvent[];
  run?: OptimizerRun;
  loadError?: string;
  analysisCampaigns?: AnalysisCampaign[];
};

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function finite(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function strings(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

function metricRows(value: unknown): EvalScorecard["metrics"] {
  if (Array.isArray(value)) {
    return value.map((entry) => {
      const row = record(entry);
      return {
        metric: typeof row.metric === "string" ? row.metric : "score",
        mean: finite(row.mean),
        min: finite(row.min),
        max: finite(row.max),
        count: finite(row.count) ?? 0
      };
    });
  }
  const source = record(value);
  return Object.entries(source).map(([metric, measured]) => ({
    metric,
    mean: finite(measured),
    min: finite(measured),
    max: finite(measured),
    count: finite(measured) == null ? 0 : 1
  }));
}

function scorecard(value: unknown): EvalScorecard | null {
  const summary = record(value);
  const details = record(summary.details);
  const candidateId = typeof summary.candidateId === "string"
    ? summary.candidateId
    : typeof details.candidateId === "string"
      ? details.candidateId
      : typeof details.candidate_id === "string"
        ? details.candidate_id
        : null;
  if (!candidateId) return null;
  const trialCounts = record(details.trials);
  const total = finite(trialCounts.total ?? details.totalTrials ?? details.total_trials) ?? 0;
  const valid = finite(trialCounts.valid ?? details.validTrials ?? details.valid_trials) ?? 0;
  const failed = finite(trialCounts.failed ?? details.failedTrials ?? details.failed_trials) ?? Math.max(0, total - valid);
  return {
    candidateId,
    label: typeof summary.label === "string" ? summary.label : typeof details.label === "string" ? details.label : candidateId,
    stage: typeof summary.stage === "string" ? summary.stage : typeof details.stage === "string" ? details.stage : "all",
    isBaseline: summary.isBaseline === true || details.isBaseline === true || details.is_baseline === true,
    trials: { total, valid, failed },
    metrics: metricRows(details.metrics ?? (summary.score == null ? undefined : { score: summary.score })),
    gateFailures: Object.fromEntries(Object.entries(record(details.gateFailures ?? details.gate_failures)).filter(([, count]) => finite(count) != null)) as Record<string, number>,
    pairedLift: finite(details.pairedLift ?? details.paired_lift ?? summary.score),
    pairedTrials: finite(details.pairedTrials ?? details.paired_trials) ?? 0,
    eliminatedAt: typeof details.eliminatedAt === "string" ? details.eliminatedAt : typeof details.eliminated_at === "string" ? details.eliminated_at : null,
    eliminationReason: typeof details.eliminationReason === "string" ? details.eliminationReason : typeof details.elimination_reason === "string" ? details.elimination_reason : null,
    costUsd: finite(summary.costUsd ?? details.costUsd ?? details.cost_usd),
    policyStepFraction: finite(details.policyStepFraction ?? details.policy_step_fraction),
    budgetExhaustedTrials: finite(details.budgetExhaustedTrials ?? details.budget_exhausted_trials) ?? 0
  };
}

function trial(row: RunCollectionRowLike): EvalTrial | null {
  if (row.kind !== "eval_trial") return null;
  const details = record(row.details);
  const metrics = Object.fromEntries(
    Object.entries(record(details.metrics)).filter(([, value]) => finite(value) != null)
  ) as Record<string, number>;
  return {
    id: typeof details.id === "string" ? details.id : row.itemId,
    candidateId: typeof details.candidateId === "string" ? details.candidateId : row.parentId ?? undefined,
    stage: typeof details.stage === "string" ? details.stage : row.label ?? undefined,
    seed: finite(details.seed) ?? undefined,
    scenario: typeof details.scenario === "string" ? details.scenario : undefined,
    status: typeof details.status === "string" ? details.status : row.status ?? "unknown",
    benchmarkStatus: typeof details.benchmarkStatus === "string" ? details.benchmarkStatus : null,
    valid: typeof details.valid === "boolean" ? details.valid : undefined,
    metrics,
    missingGates: strings(details.missingGates),
    missingArtifacts: strings(details.missingArtifacts),
    evidenceDir: typeof details.evidenceDir === "string" ? details.evidenceDir : undefined
  };
}

function EvalWorkspaceFromCollections({
  projected,
  run,
  collections,
  debug
}: {
  projected: ProjectedState;
  run: OptimizerRun;
  collections?: RunCollectionsClient;
  debug: ReactNode;
}) {
  const candidatePage = useCollectionPage(collections, "candidates", { limit: 100 });
  const evaluationPage = useCollectionPage(collections, "evaluations", { limit: 100 });
  const hydrated = useMemo<ProjectedState>(() => {
    if (!projected.eval || (!candidatePage.page && !evaluationPage.page)) return projected;
    const candidateRows = candidatePage.page?.rows ?? [];
    const candidates = candidateRows.map((row) => {
      const details = record(row.details);
      return {
        id: typeof details.id === "string" ? details.id : row.itemId,
        label: row.label ?? (typeof details.label === "string" ? details.label : row.itemId),
        isBaseline: Array.isArray(details.scorecards)
          && details.scorecards.some((entry) => record(entry).isBaseline === true)
      };
    });
    const scorecards = candidateRows.flatMap((row) => {
      const values = record(row.details).scorecards;
      return Array.isArray(values) ? values.map(scorecard).filter((item): item is EvalScorecard => item != null) : [];
    });
    const trials = (evaluationPage.page?.rows ?? []).map(trial).filter((item): item is EvalTrial => item != null);
    const next: EvalState = {
      ...projected.eval,
      candidates: candidates.length > 0 ? candidates : projected.eval.candidates,
      scorecards: scorecards.length > 0 ? scorecards : projected.eval.scorecards,
      trials: trials.length > 0 ? trials : projected.eval.trials
    };
    return { ...projected, eval: next };
  }, [candidatePage.page, evaluationPage.page, projected]);
  return <EvalWorkspace projected={hydrated} run={run} debug={debug} />;
}

export function Shell(props: ShellProps) {
  return (
    <OptimizerFamilyShell
      {...props}
      templateId="optimizer.eval.live.v1"
      kicker="EVAL"
      testId="visual-optimizer-eval-live"
      chrome="workspace"
    >
      {({ run, projected, cursor, collections }) => (
        <EvalWorkspaceFromCollections
          projected={projected}
          run={run}
          analysisCampaigns={props.analysisCampaigns}
          collections={collections}
          debug={
            <>
              <GlobalTimeline
                events={projected.timeline.map((e) => ({
                  sequence: Number(e.sequence),
                  type: String(e.type),
                  occurredAt: String(e.occurredAt)
                }))}
                cursorIndex={cursor.index}
                onScrub={cursor.onScrub}
                followLive={cursor.followLive}
                terminal={cursor.terminal}
                onFollowLive={cursor.onFollowLive}
              />
              <UsageCards usage={projected.usage} />
              <CollectionBrowser client={collections} collection="candidates" title="Durable candidates and scorecards" testId="eval-durable-candidates" />
              <CollectionBrowser client={collections} collection="evaluations" title="Measured trials and scorecards" descending testId="eval-durable-evaluations" />
              <CollectionBrowser client={collections} collection="rollouts" title="Rollout evidence" descending testId="eval-durable-rollouts" />
              <CollectionBrowser client={collections} collection="evidence_refs" title="Sealed evidence references" testId="eval-durable-evidence" />
              <EventLog entries={projected.logs} />
              <ArtifactList artifacts={projected.artifacts} />
              <ExecutionBindings bindings={projected.execution.bindings} />
            </>
          }
        />
      )}
    </OptimizerFamilyShell>
  );
}

export default Shell;

/**
 * GEPA workspace template: sticky run header, semantic stage timeline,
 * frontier + candidate canvas, evaluation browser, proposer traces, optional
 * Luna-vs-Sol comparison, and a collapsed debug section holding the raw
 * event scrubber, artifacts, usage, and execution bindings.
 */

import { OptimizerFamilyShell } from "../../_shared/optimizer.run.v1/components/FamilyShell.tsx";
import {
  ArtifactList,
  EventLog,
  ExecutionBindings,
  GlobalTimeline,
  UsageCards
} from "../../_shared/optimizer.run.v1/components/RunChrome.tsx";
import { useMemo, type ReactNode } from "react";
import { GepaWorkspace, type GepaComparisonPayload } from "../../_shared/optimizer.run.v1/overlays/gepa/GepaWorkspace.tsx";
import {
  CollectionBrowser,
  useCollectionPage,
  type RunCollectionsClient,
  type RunCollectionRowLike
} from "../../_shared/optimizer.run.v1/components/workspace/CollectionBrowser.tsx";
import type { VisualBinding } from "../../../../runtime/types.ts";
import type {
  GepaEvaluation,
  GepaProposerTrace,
  GepaState,
  OptimizerEvent,
  OptimizerRun,
  ProjectedState
} from "../../_shared/optimizer.run.v1/components/projectEvents.ts";

export type ShellProps = {
  title?: string;
  lede?: string;
  data?: unknown;
  optimizer_run?: unknown;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
  events?: OptimizerEvent[];
  run?: OptimizerRun;
  loadError?: string;
  visualId?: string;
  revision?: number;
  sourceDigest?: string;
  collections?: RunCollectionsClient;
  /** A comparable sibling run (e.g. the other half of a Luna vs Sol pair). */
  comparison?: GepaComparisonPayload | null;
};

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function finite(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function evaluation(row: RunCollectionRowLike): GepaEvaluation {
  const details = record(row.details);
  const rolloutId = typeof details.rolloutId === "string"
    ? details.rolloutId
    : typeof details.rollout_id === "string"
      ? details.rollout_id
      : row.itemId;
  return {
    candidateId: typeof details.candidateId === "string" ? details.candidateId : row.parentId ?? undefined,
    sequence: row.ordinal + 1,
    ref: { kind: "container_rollout", id: rolloutId, role: "candidate_evaluation" },
    stage: typeof details.stage === "string" ? details.stage : row.label ?? undefined,
    exampleId: typeof details.exampleId === "string" ? details.exampleId : undefined,
    reward: finite(details.reward) ?? row.score ?? null,
    costUsd: finite(details.costUsd) ?? row.costUsd ?? undefined
  };
}

function proposerCall(row: RunCollectionRowLike): GepaProposerTrace {
  const details = record(row.details);
  return {
    generation: finite(details.generation) ?? row.ordinal,
    sequence: row.ordinal + 1,
    status: row.status ?? "completed",
    model: typeof details.model === "string" ? details.model : row.label ?? undefined,
    provider: typeof details.provider === "string" ? details.provider : undefined,
    proposalCount: finite(details.proposalCount) ?? finite(details.proposal_count) ?? finite(row.score),
    costUsd: finite(details.costUsd) ?? finite(details.cost_usd) ?? row.costUsd ?? undefined,
    candidateIds: [],
    steps: []
  };
}

function GepaWorkspaceFromCollections({
  projected,
  run,
  collections,
  comparison,
  selectedCandidate,
  setSelectedCandidate,
  visualId,
  revision,
  sourceDigest,
  debug
}: {
  projected: ProjectedState;
  run: OptimizerRun;
  collections?: RunCollectionsClient;
  comparison?: GepaComparisonPayload | null;
  selectedCandidate: string | null;
  setSelectedCandidate: (id: string | null) => void;
  visualId?: string;
  revision?: number;
  sourceDigest?: string;
  debug: ReactNode;
}) {
  // Main product panels subscribe to bounded durable pages. The projection's
  // growing arrays are intentionally absent from the wire view.
  const evaluationPage = useCollectionPage(collections, "evaluations", { limit: 100, descending: true });
  const proposerPage = useCollectionPage(collections, "proposer_calls", { limit: 100 });
  const candidatePage = useCollectionPage(collections, "candidates", { limit: 100 });
  const hydrated = useMemo<ProjectedState>(() => {
    if (!projected.gepa || (!candidatePage.page && !evaluationPage.page && !proposerPage.page)) return projected;
    const existing = new Map(projected.gepa.candidates.map((candidate) => [String(candidate.id), candidate]));
    const candidates = candidatePage.page?.rows.map((row) => {
      const details = record(row.details);
      const id = typeof details.id === "string" ? details.id : row.itemId;
      const accepted = typeof details.gateAccepted === "boolean" ? details.gateAccepted : undefined;
      const trainReward = finite(details.trainReward);
      return {
        ...existing.get(id),
        ...details,
        candidateId: id,
        id,
        train_reward: trainReward,
        status: accepted === true
          ? "accepted"
          : accepted === false
            ? trainReward == null ? "rejected_minibatch" : "rejected_full_train"
            : trainReward == null ? "registered" : "full_train_evaluated"
      };
    });
    const next: GepaState = {
      ...projected.gepa,
      candidates: candidates?.length ? candidates : projected.gepa.candidates,
      evaluations: evaluationPage.page ? evaluationPage.page.rows.map(evaluation) : projected.gepa.evaluations,
      proposerTraces: proposerPage.page ? proposerPage.page.rows.map(proposerCall) : projected.gepa.proposerTraces
    };
    return { ...projected, gepa: next };
  }, [candidatePage.page, evaluationPage.page, projected, proposerPage.page]);
  return (
    <GepaWorkspace
      projected={hydrated}
      run={run}
      comparison={comparison}
      selectedCandidate={selectedCandidate}
      setSelectedCandidate={setSelectedCandidate}
      visualId={visualId}
      visualRevision={revision}
      sourceDigest={sourceDigest}
      evaluationTotal={evaluationPage.page?.total}
      evaluationPageState={evaluationPage.status}
      proposerCallTotal={proposerPage.page?.total}
      debug={debug}
    />
  );
}

export function Shell(props: ShellProps) {
  return (
    <OptimizerFamilyShell
      {...props}
      templateId="optimizer.gepa.live.v1"
      kicker="GEPA"
      testId="visual-optimizer-gepa-live"
      chrome="workspace"
    >
      {({ run, projected, collections, selectedCandidate, setSelectedCandidate, cursor }) => (
        <GepaWorkspaceFromCollections
          projected={projected}
          run={run}
          collections={collections}
          comparison={props.comparison}
          selectedCandidate={selectedCandidate}
          setSelectedCandidate={setSelectedCandidate}
          visualId={props.visualId}
          revision={props.revision}
          sourceDigest={props.sourceDigest}
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
              <EventLog entries={projected.logs} />
              <ArtifactList artifacts={projected.artifacts} />
              <ExecutionBindings bindings={projected.execution.bindings} />
              {/*
                The durable read model, paged on intent. These read the
                backend collections for the bound run — one explicit page at a
                time, one item on selection — and never the projection arrays
                or the raw journal.
              */}
              <CollectionBrowser client={collections} collection="candidates" title="Durable candidates" testId="gepa-durable-candidates" />
              <CollectionBrowser client={collections} collection="rollouts" title="Durable rollouts" descending testId="gepa-durable-rollouts" />
              <CollectionBrowser client={collections} collection="proposer_calls" title="Durable proposer calls" testId="gepa-durable-proposer-calls" />
            </>
          }
        />
      )}
    </OptimizerFamilyShell>
  );
}

export default Shell;

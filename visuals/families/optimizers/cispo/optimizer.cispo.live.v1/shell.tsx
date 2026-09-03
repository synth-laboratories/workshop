/**
 * CISPO workspace template: clip identity, group advantages, aligned curves,
 * checkpoints, and rollout groups. Hydrates Workshop collections
 * (`metric_points`, `candidates`, `evaluations`, `rollouts`) instead of
 * reconstructing charts from the raw journal.
 */

import { OptimizerFamilyShell } from "../../_shared/optimizer.run.v1/components/FamilyShell.tsx";
import {
  ArtifactList,
  EventLog,
  ExecutionBindings,
  GlobalTimeline,
  UsageCards
} from "../../_shared/optimizer.run.v1/components/RunChrome.tsx";
import { SftWorkspace } from "../../_shared/optimizer.run.v1/overlays/sft/SftWorkspace.tsx";
import { useMemo, type ReactNode } from "react";
import {
  CollectionBrowser,
  useCollectionPage,
  type RunCollectionsClient
} from "../../_shared/optimizer.run.v1/components/workspace/CollectionBrowser.tsx";
import type { VisualBinding } from "../../../../runtime/types.ts";
import type {
  OptimizerEvent,
  OptimizerRun,
  ProjectedState
} from "../../_shared/optimizer.run.v1/components/projectEvents.ts";
import { projectedScalar } from "./collectionHydration.ts";

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
};

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function finite(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function CispoWorkspaceFromCollections({
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
  const metricPage = useCollectionPage(
    collections,
    "metric_points",
    { limit: 100, descending: true }
  );
  const hydrated = useMemo<ProjectedState>(() => {
    if (!projected.sft || !metricPage.page) return projected;
    const points = metricPage.page.rows
      .map((row) => record(row.details))
      .map((point) => ({
        step: finite(point.step ?? point.update) ?? 0,
        ...(finite(point.epoch) == null ? {} : { epoch: finite(point.epoch) }),
        ...(finite(point.trainLoss ?? point.train_loss ?? point.loss) == null
          ? {}
          : { trainLoss: finite(point.trainLoss ?? point.train_loss ?? point.loss) }),
        ...(finite(point.validationLoss ?? point.validation_loss) == null
          ? {}
          : { validationLoss: finite(point.validationLoss ?? point.validation_loss) }),
        ...(finite(point.learningRate ?? point.learning_rate) == null
          ? {}
          : { learningRate: finite(point.learningRate ?? point.learning_rate) })
      }))
      .filter((point) => point.step > 0)
      .sort((left, right) => left.step - right.step);
    if (points.length === 0) return projected;
    const latest = record(metricPage.page.rows.at(-1)?.details);
    const cispo = projected.cispo
      ? {
          ...projected.cispo,
          rolloutGroups: projected.cispo.rolloutGroups ?? [],
          zeroAdvantageGroups: projected.cispo.zeroAdvantageGroups ?? 0,
          learningSignalGroups: projected.cispo.learningSignalGroups ?? 0,
          clippedTokenFraction: projected.cispo.clippedTokenFraction ?? null,
          importanceRatioMean: projected.cispo.importanceRatioMean ?? null,
          klProxy: projected.cispo.klProxy ?? null,
          groupSize: projectedScalar(
            projected.cispo.groupSize,
            latest.group_size ?? latest.groupSize ?? latest.group_count
          ),
          rewardVariance: projectedScalar(
            projected.cispo.rewardVariance,
            latest.reward_variance ?? latest.rewardVariance
          ),
          advantageMean: projectedScalar(
            projected.cispo.advantageMean,
            latest.advantage_mean ?? latest.advantageMean
          ),
          advantageStd: projectedScalar(
            projected.cispo.advantageStd,
            latest.advantage_std ?? latest.advantageStd
          ),
          optimizerSteps: projectedScalar(
            projected.cispo.optimizerSteps,
            latest.optimizer_step ?? latest.optimizerStep ?? latest.update
          ) ?? 0
        }
      : projected.cispo;
    return {
      ...projected,
      cispo,
      sft: {
        ...projected.sft,
        points,
        curves: {
          steps: points.map((point) => point.step),
          epochs: points.flatMap((point) => point.epoch == null ? [] : [point.epoch]),
          trainLoss: points.flatMap((point) => point.trainLoss == null ? [] : [point.trainLoss]),
          validationLoss: points.flatMap((point) => point.validationLoss == null ? [] : [point.validationLoss]),
          learningRate: points.flatMap((point) => point.learningRate == null ? [] : [point.learningRate])
        }
      }
    };
  }, [metricPage.page, projected]);
  return <SftWorkspace projected={hydrated} run={run} debug={debug} />;
}

export function Shell(props: ShellProps) {
  return (
    <OptimizerFamilyShell
      {...props}
      templateId="optimizer.cispo.live.v1"
      kicker="CISPO"
      testId="visual-optimizer-cispo-live"
      chrome="workspace"
    >
      {({ run, projected, cursor, collections }) => (
        <CispoWorkspaceFromCollections
          projected={projected}
          run={run}
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
              <CollectionBrowser client={collections} collection="candidates" title="Durable checkpoints" testId="cispo-durable-checkpoints" />
              <CollectionBrowser client={collections} collection="metric_points" title="Training metric series" descending testId="cispo-durable-metrics" />
              <CollectionBrowser client={collections} collection="evaluations" title="Checkpoint and heldout evaluations" descending testId="cispo-durable-evaluations" />
              <CollectionBrowser client={collections} collection="rollouts" title="Rollout groups" descending testId="cispo-durable-rollouts" />
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

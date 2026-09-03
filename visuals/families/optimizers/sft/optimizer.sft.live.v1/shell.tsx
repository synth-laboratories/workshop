/**
 * SFT workspace template: sticky run header, dataset → training → checkpoint
 * → evaluation → promotion stages, aligned curves, checkpoint campaigns in
 * the scalable rollout browser, and a collapsed debug section with the raw
 * event scrubber, usage, artifacts, and execution bindings.
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

function SftWorkspaceFromCollections({
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
        step: finite(point.step) ?? 0,
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
    return {
      ...projected,
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
      templateId="optimizer.sft.live.v1"
      kicker="Training"
      testId="visual-optimizer-sft-live"
      chrome="workspace"
    >
      {({ run, projected, cursor, collections }) => (
        <SftWorkspaceFromCollections
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
              <CollectionBrowser client={collections} collection="candidates" title="Durable checkpoints" testId="sft-durable-checkpoints" />
              <CollectionBrowser client={collections} collection="metric_points" title="Training metric series" descending testId="sft-durable-metrics" />
              <CollectionBrowser client={collections} collection="evaluations" title="Checkpoint and heldout evaluations" descending testId="sft-durable-evaluations" />
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

/**
 * DAG workspace template: sticky run header, per-node stages, node table
 * with honest cost (null stays "—"), and a collapsed debug section with the
 * raw event scrubber, usage, artifacts, and execution bindings.
 */

import { OptimizerFamilyShell } from "../optimizer.run.v1/components/FamilyShell.tsx";
import {
  ArtifactList,
  EventLog,
  ExecutionBindings,
  GlobalTimeline,
  UsageCards
} from "../optimizer.run.v1/components/RunChrome.tsx";
import { DagWorkspace } from "../optimizer.run.v1/overlays/dag/DagWorkspace.tsx";
import type { VisualBinding } from "../../runtime/types.ts";
import type { OptimizerEvent, OptimizerRun } from "../optimizer.run.v1/components/projectEvents.ts";

export type ShellProps = {
  title?: string;
  lede?: string;
  data?: unknown;
  optimizer_run?: unknown;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
  events?: OptimizerEvent[];
  run?: OptimizerRun;
  loadError?: string;
};

export function Shell(props: ShellProps) {
  return (
    <OptimizerFamilyShell
      {...props}
      templateId="optimizer.dag.live.v1"
      kicker="DAG"
      testId="visual-optimizer-dag-live"
      chrome="workspace"
    >
      {({ run, projected, cursor }) => (
        <DagWorkspace
          projected={projected}
          run={run}
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
            </>
          }
        />
      )}
    </OptimizerFamilyShell>
  );
}

export default Shell;

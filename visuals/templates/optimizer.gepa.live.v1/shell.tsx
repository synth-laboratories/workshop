/**
 * GEPA workspace template: sticky run header, semantic stage timeline,
 * frontier + candidate canvas, evaluation browser, proposer traces, optional
 * Luna-vs-Sol comparison, and a collapsed debug section holding the raw
 * event scrubber, artifacts, usage, and execution bindings.
 */

import { OptimizerFamilyShell } from "../optimizer.run.v1/components/FamilyShell.tsx";
import {
  ArtifactList,
  EventLog,
  ExecutionBindings,
  GlobalTimeline,
  UsageCards
} from "../optimizer.run.v1/components/RunChrome.tsx";
import { GepaWorkspace, type GepaComparisonPayload } from "../optimizer.run.v1/overlays/gepa/GepaWorkspace.tsx";
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
  /** A comparable sibling run (e.g. the other half of a Luna vs Sol pair). */
  comparison?: GepaComparisonPayload | null;
};

export function Shell(props: ShellProps) {
  return (
    <OptimizerFamilyShell
      {...props}
      templateId="optimizer.gepa.live.v1"
      kicker="GEPA"
      testId="visual-optimizer-gepa-live"
      chrome="workspace"
    >
      {({ run, projected, selectedCandidate, setSelectedCandidate, cursor }) => (
        <GepaWorkspace
          projected={projected}
          run={run}
          comparison={props.comparison}
          selectedCandidate={selectedCandidate}
          setSelectedCandidate={setSelectedCandidate}
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

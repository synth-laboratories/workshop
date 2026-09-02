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
import { GepaWorkspace, type GepaComparisonPayload } from "../../_shared/optimizer.run.v1/overlays/gepa/GepaWorkspace.tsx";
import { CollectionBrowser } from "../../_shared/optimizer.run.v1/components/workspace/CollectionBrowser.tsx";
import type { VisualBinding } from "../../../../runtime/types.ts";
import type { OptimizerEvent, OptimizerRun } from "../../_shared/optimizer.run.v1/components/projectEvents.ts";

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
      {({ run, projected, collections, selectedCandidate, setSelectedCandidate, cursor }) => (
        <GepaWorkspace
          projected={projected}
          run={run}
          comparison={props.comparison}
          selectedCandidate={selectedCandidate}
          setSelectedCandidate={setSelectedCandidate}
          visualId={props.visualId}
          visualRevision={props.revision}
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

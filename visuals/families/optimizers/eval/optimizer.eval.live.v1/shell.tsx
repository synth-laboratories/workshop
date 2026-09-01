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
import type { VisualBinding } from "../../../../runtime/types.ts";
import type { OptimizerEvent, OptimizerRun } from "../../_shared/optimizer.run.v1/components/projectEvents.ts";

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

export function Shell(props: ShellProps) {
  return (
    <OptimizerFamilyShell
      {...props}
      templateId="optimizer.eval.live.v1"
      kicker="EVAL"
      testId="visual-optimizer-eval-live"
      chrome="workspace"
    >
      {({ run, projected, cursor }) => (
        <EvalWorkspace
          projected={projected}
          run={run}
          analysisCampaigns={props.analysisCampaigns}
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

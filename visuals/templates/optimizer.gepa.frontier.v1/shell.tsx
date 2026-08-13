/** Focused frontier view — the same search canvas the GEPA workspace renders. */

import { OptimizerFamilyShell } from "../optimizer.run.v1/components/FamilyShell.tsx";
import { FrontierPanel } from "../optimizer.run.v1/overlays/gepa/FrontierPanel.tsx";
import { CandidateList } from "../optimizer.run.v1/overlays/gepa/CandidateBoard.tsx";
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
      templateId="optimizer.gepa.frontier.v1"
      kicker="GEPA · frontier"
      testId="visual-optimizer-gepa-frontier"
      showTimeline={false}
    >
      {({ projected, selectedCandidate, setSelectedCandidate }) => (
        projected.gepa ? (
          <div data-testid="gepa-frontier-plot">
            <FrontierPanel gepa={projected.gepa} selectedId={selectedCandidate} onSelect={setSelectedCandidate} />
            <CandidateList gepa={projected.gepa} selectedId={selectedCandidate} onSelect={setSelectedCandidate} />
          </div>
        ) : null
      )}
    </OptimizerFamilyShell>
  );
}

export default Shell;

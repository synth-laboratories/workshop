/** Focused candidate view — list plus the workspace's diff-capable inspector. */

import { OptimizerFamilyShell } from "../../_shared/optimizer.run.v1/components/FamilyShell.tsx";
import { CandidateInspector, CandidateList } from "../../_shared/optimizer.run.v1/overlays/gepa/CandidateBoard.tsx";
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
};

export function Shell(props: ShellProps) {
  return (
    <OptimizerFamilyShell
      {...props}
      templateId="optimizer.gepa.candidate.v1"
      kicker="GEPA · candidate"
      testId="visual-optimizer-gepa-candidate"
      showTimeline={false}
    >
      {({ projected, selectedCandidate, setSelectedCandidate }) => {
        const gepa = projected.gepa;
        if (!gepa) return null;
        const fallback = gepa.candidates.find((candidate) => String(candidate.id) === gepa.incumbentId) ??
          gepa.candidates.at(-1);
        const selectedId = selectedCandidate ?? (fallback ? String(fallback.id) : null);
        return (
          <div className="sv-workspace-canvas" data-testid="gepa-prompt-diff" style={{ marginTop: 14 }}>
            <CandidateList gepa={gepa} selectedId={selectedId} onSelect={setSelectedCandidate} />
            <CandidateInspector gepa={gepa} selectedId={selectedId} onSelect={setSelectedCandidate} />
          </div>
        );
      }}
    </OptimizerFamilyShell>
  );
}

export default Shell;

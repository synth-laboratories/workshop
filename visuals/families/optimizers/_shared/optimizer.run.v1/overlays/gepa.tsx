/**
 * GEPA overlay for the generic optimizer.run.v1 template: the workspace
 * canvas in embedded mode (the generic shell already renders its own header,
 * timeline, usage, and event log). The full experience with the sticky run
 * header and debug tab lives in optimizer.gepa.live.v1.
 */

import type { OptimizerRun, ProjectedState } from "../components/projectEvents.ts";
import { GepaWorkspace } from "./gepa/GepaWorkspace.tsx";

export function GepaOverlay({
  state,
  selectedId,
  onSelect
}: {
  state: ProjectedState;
  selectedId?: string | null;
  onSelect?: (id: string | null) => void;
}) {
  if (!state.gepa) return null;
  const run: OptimizerRun = {
    id: String(state.summary.id ?? "optimizer run"),
    algorithmId: "gepa",
    status: String(state.summary.status ?? "unknown")
  };
  return (
    <GepaWorkspace
      projected={state}
      run={run}
      embedded
      selectedCandidate={selectedId ?? null}
      setSelectedCandidate={(id) => onSelect?.(id)}
    />
  );
}

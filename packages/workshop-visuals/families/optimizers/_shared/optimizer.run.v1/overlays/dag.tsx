/**
 * DAG overlay for the generic optimizer.run.v1 template: the DAG workspace in
 * embedded mode (the generic shell renders its own header, timeline, usage,
 * and event log). The full experience lives in optimizer.dag.live.v1.
 */

import type { OptimizerRun, ProjectedState } from "../components/projectEvents.ts";
import { DagWorkspace } from "./dag/DagWorkspace.tsx";

export function DagOverlay({ state }: { state: ProjectedState }) {
  if (!state.dag) return null;
  const run: OptimizerRun = {
    id: String(state.summary.id ?? "optimizer run"),
    algorithmId: String(state.summary.algorithmId ?? "dag"),
    status: String(state.summary.status ?? "unknown")
  };
  return <DagWorkspace projected={state} run={run} embedded />;
}

/**
 * SFT overlay for the generic optimizer.run.v1 template: the SFT workspace in
 * embedded mode (the generic shell renders its own header, timeline, usage,
 * and event log). The full experience lives in optimizer.sft.live.v1.
 */

import type { OptimizerRun, ProjectedState } from "../components/projectEvents.ts";
import { SftWorkspace } from "./sft/SftWorkspace.tsx";

export function SftOverlay({ state }: { state: ProjectedState }) {
  if (!state.sft) return null;
  const run: OptimizerRun = {
    id: String(state.summary.id ?? "optimizer run"),
    algorithmId: "sft",
    status: String(state.summary.status ?? "unknown")
  };
  return <SftWorkspace projected={state} run={run} embedded />;
}

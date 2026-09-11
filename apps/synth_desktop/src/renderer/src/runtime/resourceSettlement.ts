/** Backend f67bc0fb source contract. Only fresh scoped reads may enter this
 * projection; stop receipts and cached snapshots must not be substituted. */
export type ResourceSettlementObservation = {
  run_id: string;
  observed_at: string;
  coverage: "untracked" | "explicit-v1";
  settled: boolean;
  registered_tree_settled?: boolean;
  coverage_complete?: boolean;
  scope_kind?: "root_tree" | "owned_subtree" | null;
  root_run_id?: string | null;
  edge_id?: string | null;
  pending?: number | null;
  unknown?: number | null;
  confirmed?: number | null;
  excluded?: number | null;
  root_confirmed?: boolean | null;
};
export type SettlementPresentation = {
  state: "unavailable" | "untracked" | "unknown" | "pending" | "partial" | "settled_root" | "settled_subtree";
  label: string;
  pending: number | null;
  unknown: number | null;
  observedAt: string | null;
};
/** View model for the default-gated Cloud cleanup status. Null counts remain
 * unknown, and child cleanup can never complete a root's presentation. */
export function resourceSettlementPresentation(
  requestedRunId: string,
  observation: ResourceSettlementObservation | null
): SettlementPresentation {
  const unavailable: SettlementPresentation = {state:"unavailable",label:"Cleanup evidence unavailable",pending:null,unknown:null,observedAt:null};
  if (!observation || observation.run_id !== requestedRunId || !Number.isFinite(Date.parse(observation.observed_at))) return unavailable;
  const pending = observation.pending ?? null;
  const unknown = observation.unknown ?? null;
  if ([pending,unknown].some(value => value !== null && (!Number.isSafeInteger(value) || value < 0))) return unavailable;
  const result = (state: SettlementPresentation["state"], label: string): SettlementPresentation => ({state,label,pending,unknown,observedAt:observation.observed_at});
  if (observation.coverage === "untracked") {
    return observation.settled ? unavailable : result("untracked","Cleanup coverage unavailable");
  }
  if (observation.coverage !== "explicit-v1") return unavailable;
  const root = observation.scope_kind === "root_tree" && observation.root_run_id === requestedRunId && !observation.edge_id;
  const subtree = observation.scope_kind === "owned_subtree" && !!observation.root_run_id && !!observation.edge_id;
  if (!root && !subtree) return unavailable;
  if (observation.settled) {
    if (!observation.coverage_complete || !observation.registered_tree_settled || pending !== 0 || (unknown !== null && unknown !== 0)) return unavailable;
    return root ? result("settled_root","Run resource cleanup confirmed") : result("settled_subtree","Owned subtree cleanup confirmed");
  }
  if (unknown !== null && unknown > 0) return result("unknown","Cleanup outcome unknown");
  if (pending !== null && pending > 0) return result("pending","Cleanup awaiting confirmation");
  if (observation.registered_tree_settled && !observation.coverage_complete) return result("partial","Tracked resource cleanup confirmed; coverage incomplete");
  return result("unknown","Cleanup awaiting evidence");
}

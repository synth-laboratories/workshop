/** Pure DAG workspace derivations, importable from node tests. */

import { formatMissingUsd } from "../../../../../../runtime/liveStream.ts";
import type { DagNodeState, DagState } from "../../components/projectEvents.ts";
import type { WorkspaceStage } from "../../components/workspace/WorkspaceChrome.tsx";

export type { DagNodeState, DagState };

const STAGE_STATUS: Record<string, WorkspaceStage["status"]> = {
  planned: "pending",
  cancelled: "pending",
  canceled: "pending",
  running: "active",
  paused: "active",
  sealed: "completed",
  failed: "failed"
};

function nodeLabel(id: string): string {
  return id.replaceAll("_", " ");
}

function partitionDetail(node: DagNodeState): string | undefined {
  if (node.partitionsSealed == null && node.partitionsTotal == null) return undefined;
  const sealed = node.partitionsSealed ?? 0;
  const total = node.partitionsTotal;
  return total != null ? `${sealed}/${total}` : `${sealed} sealed`;
}

/** One stage per DAG node; status is pending / active / completed / failed. */
export function dagStages(dag: DagState, _status: string): WorkspaceStage[] {
  return dag.nodes.map((node) => ({
    id: node.id,
    label: nodeLabel(node.id),
    status: STAGE_STATUS[node.status] ?? "pending",
    detail: partitionDetail(node)
  }));
}

export function formatNodeCost(node: DagNodeState): string {
  return node.costUsd == null ? "—" : formatMissingUsd(node.costUsd);
}

export function formatKnownSpend(dag: DagState): string {
  const partial = dag.nodes.reduce(
    (sum, node) => (node.costUsd == null ? sum : sum + node.costUsd),
    0
  );
  const hasPartial = dag.nodes.some((node) => node.costUsd != null);
  if (dag.missingMeterCount > 0) {
    const known = hasPartial ? formatMissingUsd(partial) : "—";
    return `known ${known} · ${dag.missingMeterCount} missing`;
  }
  const spend = dag.knownCostUsd == null ? "—" : formatMissingUsd(dag.knownCostUsd);
  if (dag.unmeteredCount > 0) return `${spend} · ${dag.unmeteredCount} unmetered`;
  return spend;
}

export function formatWallSeconds(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "—";
  if (value < 60) return `${value.toFixed(1)}s`;
  const minutes = Math.floor(value / 60);
  const seconds = Math.round(value % 60);
  return `${minutes}m ${seconds}s`;
}

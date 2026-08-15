/**
 * DAG workspace on the shared optimizer chrome. Node table + stage rail for
 * a local experiment DAG. Missing / unmetered cost renders as "—"; the
 * headline never invents a total when a sealed metered node lacks a receipt.
 *
 * Styling comes from visuals/chrome/tokens.css. No Craftax env frames.
 */

import { useMemo, type ReactNode } from "react";
import type { OptimizerRun, ProjectedState } from "../../components/projectEvents.ts";
import {
  StageTimeline,
  WorkspaceHeader,
  type WorkspaceMetric
} from "../../components/workspace/WorkspaceChrome.tsx";
import {
  dagStages,
  formatKnownSpend,
  formatNodeCost,
  formatWallSeconds
} from "./model.ts";

function statusChip(status: string): { text: string; tone?: "live" | "ok" | "bad" | "warn"; dot: boolean } {
  if (status === "failed") return { text: "Failed", tone: "bad", dot: false };
  if (["canceled", "cancelled"].includes(status)) return { text: "Canceled", tone: "warn", dot: false };
  if (["completed", "succeeded", "sealed"].includes(status)) return { text: "Completed", tone: "ok", dot: false };
  if (status === "paused") return { text: "Paused", tone: "warn", dot: false };
  if (status === "queued") return { text: "Queued", tone: "warn", dot: false };
  if (["created", "pending", "planned", "loading"].includes(status)) {
    return { text: status[0].toUpperCase() + status.slice(1), dot: false };
  }
  return { text: "Running", tone: "live", dot: true };
}

function partitionCell(sealed?: number, total?: number): string {
  if (sealed == null && total == null) return "—";
  if (total == null) return String(sealed ?? 0);
  return `${sealed ?? 0}/${total}`;
}

export function DagWorkspace({
  projected,
  run,
  debug,
  embedded = false
}: {
  projected: ProjectedState;
  run: OptimizerRun;
  debug?: ReactNode;
  embedded?: boolean;
}) {
  const dag = projected.dag;
  const status = String(projected.summary.status ?? run.status ?? "");
  const stages = useMemo(
    () => (dag ? dagStages(dag, status) : []),
    [dag, status]
  );

  if (!dag) return null;

  const chip = statusChip(status);
  const sealedCount = dag.nodes.filter((node) => node.status === "sealed").length;
  const spend = formatKnownSpend(dag);
  const headline = dag.dag ?? run.algorithmId ?? "DAG";
  const metrics: WorkspaceMetric[] = [
    {
      label: "Known spend",
      value: spend,
      title: dag.missingMeterCount > 0
        ? "A sealed metered node is missing cost; this is not a complete total."
        : dag.unmeteredCount > 0
          ? "Sum of metered node receipts. Unmetered nodes are counted, not zeroed."
          : undefined,
      testId: "dag-known-spend"
    },
    { label: "Unmetered", value: String(dag.unmeteredCount) },
    { label: "Nodes", value: `${sealedCount}/${dag.nodes.length} sealed` },
    { label: "Cursor", value: String(projected.cursorSeq) }
  ];

  return (
    <div className="sv-workspace" data-testid="dag-workspace">
      <WorkspaceHeader
        statusText={chip.text}
        statusTone={chip.tone}
        live={chip.dot}
        headline={headline}
        metrics={metrics}
        testId="dag-run-header"
      />
      <StageTimeline stages={stages} testId="dag-stage-timeline" />

      <section className="sv-panel" aria-label="DAG nodes" data-testid="dag-node-table">
        <div className="sv-panel-head">
          <h4>Nodes</h4>
          <span className="sv-mono">{dag.nodes.length}</span>
        </div>
        <div className="sv-panel-body">
          {dag.nodes.length === 0 ? (
            <p className="sv-empty">Nodes appear as the runner emits node and partition events.</p>
          ) : (
            <table className="sv-table">
              <thead>
                <tr>
                  <th scope="col">Node</th>
                  <th scope="col">Status</th>
                  <th scope="col">Partitions</th>
                  <th scope="col">Wall</th>
                  <th scope="col">Cost</th>
                </tr>
              </thead>
              <tbody>
                {dag.nodes.map((node) => (
                  <tr key={node.id} data-testid={`dag-node-${node.id}`}>
                    <td className="sv-mono">{node.id}</td>
                    <td>{node.status}</td>
                    <td className="sv-mono">{partitionCell(node.partitionsSealed, node.partitionsTotal)}</td>
                    <td className="sv-mono">{formatWallSeconds(node.wallSeconds)}</td>
                    <td className="sv-mono" data-testid={`dag-node-cost-${node.id}`}>
                      {formatNodeCost(node)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </section>

      {debug && !embedded ? (
        <details data-testid="dag-debug">
          <summary className="sv-mono">Debug · raw events, artifacts, usage</summary>
          {debug}
        </details>
      ) : null}
    </div>
  );
}

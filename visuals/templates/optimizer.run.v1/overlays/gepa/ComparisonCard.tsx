/**
 * Side-by-side summary of two comparable GEPA runs (the Luna vs Sol pairing),
 * so comparing them never means memorizing two long documents.
 */

import { formatMissingNumber } from "../../../../runtime/liveStream.ts";
import type { GepaState } from "../../components/projectEvents.ts";
import { decisionText, elapsedLabel, limitOf } from "./model.ts";

export type ComparisonColumn = {
  runId: string;
  label: string;
  gepa: GepaState;
};

function proposalSummary(gepa: GepaState): string {
  const proposals = gepa.candidates.filter((candidate) => candidate.parentId != null);
  if (proposals.length === 0) return "no proposals yet";
  return proposals
    .map((candidate) => decisionText(candidate) ?? String(candidate.status ?? "pending"))
    .join("; ");
}

function row(label: string, values: string[]): { label: string; values: string[] } {
  return { label, values };
}

export function ComparisonCard({ columns }: { columns: ComparisonColumn[] }) {
  if (columns.length < 2) return null;
  const rows = [
    row("Proposer model", columns.map(({ gepa }) => gepa.models.proposer ?? "—")),
    row("Best train score", columns.map(({ gepa }) => formatMissingNumber(gepa.best?.trainReward))),
    row("Heldout", columns.map(({ gepa }) =>
      gepa.heldout?.skipped ? "skipped" : formatMissingNumber(gepa.heldout?.reward ?? gepa.best?.heldoutReward))),
    row("Proposal outcome", columns.map(({ gepa }) => proposalSummary(gepa))),
    row("Minibatch (proposal vs parent)", columns.map(({ gepa }) => {
      const proposal = gepa.candidates.find((candidate) => candidate.parentId != null);
      const mb = proposal?.minibatchReward;
      const parent = proposal?.parentMinibatchReward;
      return typeof mb === "number" && typeof parent === "number"
        ? `${mb.toFixed(2)} vs ${parent.toFixed(2)}`
        : "—";
    })),
    row("Rollouts", columns.map(({ gepa }) => {
      const limit = limitOf(gepa, "total_rollouts");
      const spent = Math.max(limit?.spent ?? 0, gepa.rolloutsCompleted);
      return spent > 0 ? `${Math.round(spent)}${limit?.max != null ? ` / ${Math.round(limit.max)}` : ""}` : "—";
    })),
    row("Proposer tokens", columns.map(({ gepa }) => {
      const tokens = gepa.proposerTraces.reduce((sum, trace) =>
        sum + (typeof trace.usage?.total_tokens === "number" ? (trace.usage.total_tokens as number) : 0), 0);
      return tokens > 0 ? tokens.toLocaleString() : "—";
    })),
    row("Proposal wall time", columns.map(({ gepa }) => {
      const seconds = gepa.proposerTraces.reduce((sum, trace) => sum + (trace.wallSeconds ?? 0), 0);
      return seconds > 0 ? `${seconds.toFixed(1)} s` : "—";
    })),
    row("Frontier", columns.map(({ gepa }) =>
      `${gepa.frontier.length} member${gepa.frontier.length === 1 ? "" : "s"}${gepa.best?.candidateId ? ` · best ${gepa.best.candidateId}` : ""}`)),
    row("Elapsed", columns.map(({ gepa }) => elapsedLabel(gepa.timing, gepa.activity.terminal)))
  ];
  return (
    <section className="sv-section" aria-label="Run comparison" data-testid="gepa-comparison">
      <div className="sv-section-head">
        <h3>Compare runs</h3>
        <span className="sv-mono">{columns.map((column) => column.label).join(" vs ")}</span>
      </div>
      <div style={{ overflowX: "auto" }}>
        <table className="sv-table">
          <thead>
            <tr>
              <th scope="col" aria-label="Metric" />
              {columns.map((column) => (
                <th key={column.runId} scope="col">
                  {column.label}
                  <span className="sv-mono" style={{ display: "block", fontWeight: 400, textTransform: "none", letterSpacing: 0 }}>
                    {column.runId}
                  </span>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((entry) => (
              <tr key={entry.label}>
                <th scope="row" style={{ fontSize: 11, textTransform: "none", letterSpacing: 0 }}>{entry.label}</th>
                {entry.values.map((value, index) => (
                  <td key={`${entry.label}-${columns[index].runId}`} style={{ fontSize: 12 }}>{value}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

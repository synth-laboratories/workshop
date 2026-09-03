import { VisualChrome, MetricStrip } from "../../../chrome/VisualChrome.tsx";
import type { VisualBinding } from "../../../runtime/types.ts";

type TraceRow = {
  traceDigest: string;
  model?: string | null;
  provider?: string | null;
  benchmark?: string | null;
  taskId?: string | null;
  lifecycleStatus?: string | null;
  captureStatus?: string | null;
  reward?: number | null;
  costUsd?: number | null;
  eventCount?: number;
  toolCallCount?: number;
  errorCount?: number;
  durationMs?: number | null;
  startedAt?: string | null;
  hasMedia?: boolean;
  hasEvidence?: boolean;
};

type QuerySnapshot = {
  snapshotId?: string;
  queryAst?: Record<string, unknown>;
  resultIds?: string[];
  resultCount?: number;
  facets?: { rows?: TraceRow[] };
  resultDigest?: string;
  queriedAt?: string;
  truncated?: boolean;
};

export type ShellProps = {
  title?: string;
  lede?: string;
  result?: QuerySnapshot;
  data?: QuerySnapshot;
  bindings?: VisualBinding[];
};

const EMPTY: QuerySnapshot = { resultCount: 0, facets: { rows: [] } };

function asSnapshot(raw: unknown): QuerySnapshot {
  if (raw && typeof raw === "object") return raw as QuerySnapshot;
  return EMPTY;
}

/** A missing measurement stays missing; it is never rendered as zero. */
function num(value: number | null | undefined, digits = 2): string {
  return typeof value === "number" && Number.isFinite(value) ? value.toFixed(digits) : "—";
}

function when(iso: string | null | undefined): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}

/**
 * Render the query itself, not a prose summary of it. The reader has to be
 * able to tell which rows they are looking at without trusting a caption.
 */
function describeFilters(ast: Record<string, unknown> | undefined): string[] {
  if (!ast) return [];
  const parts: string[] = [];
  const where = (ast.where ?? {}) as Record<string, unknown>;
  for (const [key, value] of Object.entries(where)) {
    if (value == null) continue;
    if (Array.isArray(value)) {
      if (value.length) parts.push(`${key}: ${value.join(", ")}`);
    } else if (typeof value === "object") {
      const bounds = Object.entries(value as Record<string, unknown>)
        .filter(([, bound]) => bound != null)
        .map(([bound, at]) => `${bound} ${String(at)}`);
      if (bounds.length) parts.push(`${key}: ${bounds.join(" and ")}`);
    } else {
      parts.push(`${key}: ${String(value)}`);
    }
  }
  if (typeof ast.text === "string" && ast.text.trim()) parts.push(`text: "${ast.text}"`);
  const order = Array.isArray(ast.orderBy) ? (ast.orderBy as Array<Record<string, string>>) : [];
  for (const entry of order) parts.push(`sorted by ${entry.field} ${entry.direction ?? "desc"}`);
  return parts;
}

export function Shell(props: ShellProps) {
  const snapshot = asSnapshot(props.data ?? props.result);
  const rows = snapshot.facets?.rows ?? [];
  const filters = describeFilters(snapshot.queryAst);
  const count = snapshot.resultCount ?? rows.length;

  return (
    <VisualChrome
      kicker="Traces · query result"
      title={props.title ?? "Trace catalog"}
      lede={props.lede}
      testId="visual-trace-catalog"
      footer="trace.catalog.v1"
    >
      <MetricStrip
        metrics={[
          { label: "Matched", value: String(count) },
          { label: "Retrieved", value: when(snapshot.queriedAt) },
          { label: "With evidence", value: String(rows.filter((row) => row.hasEvidence).length) }
        ]}
      />

      <section className="sv-section" aria-label="Query provenance">
        <div className="sv-section-head">
          <h3>Filter</h3>
          <span className="sv-mono">{snapshot.snapshotId ?? "unsaved"}</span>
        </div>
        {filters.length ? (
          <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
            {filters.map((line) => (
              <li key={line} className="sv-mono">
                {line}
              </li>
            ))}
          </ul>
        ) : (
          <p style={{ margin: 0, fontSize: 12, color: "var(--sv-text-faint)" }}>
            No filter — every indexed trace, newest first.
          </p>
        )}
        {snapshot.truncated ? (
          <p
            data-testid="trace-catalog-truncated"
            style={{ margin: "8px 0 0", fontSize: 12, color: "#c2553f" }}
          >
            Capped at {count}. More traces match this filter than are shown.
          </p>
        ) : null}
      </section>

      <section className="sv-section" aria-label="Matching traces">
        <div className="sv-section-head">
          <h3>Traces</h3>
          <span className="sv-mono">{snapshot.resultDigest ?? "—"}</span>
        </div>
        {rows.length === 0 ? (
          <p
            data-testid="trace-catalog-empty"
            style={{ margin: 0, fontSize: 12, color: "var(--sv-text-faint)" }}
          >
            Nothing matched{filters.length ? " this filter" : ""}
            {snapshot.queriedAt ? ` as of ${when(snapshot.queriedAt)}` : ""}.
          </p>
        ) : (
          <div style={{ overflowX: "auto" }}>
            <table
              data-testid="trace-catalog-table"
              style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}
            >
              <caption className="sv-sr-only">
                {count} traces matching the filter above, retrieved {when(snapshot.queriedAt)}
              </caption>
              <thead>
                <tr>
                  {["Trace", "Benchmark", "Model", "Status", "Reward", "Events", "Started"].map(
                    (heading) => (
                      <th
                        key={heading}
                        scope="col"
                        style={{
                          textAlign: heading === "Reward" || heading === "Events" ? "right" : "left",
                          padding: "6px 8px",
                          borderBottom: "1px solid var(--sv-border, #e1e4e6)",
                          whiteSpace: "nowrap"
                        }}
                      >
                        {heading}
                      </th>
                    )
                  )}
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => (
                  <tr key={row.traceDigest} data-trace-digest={row.traceDigest}>
                    <td style={{ padding: "6px 8px", whiteSpace: "nowrap" }}>
                      <button
                        type="button"
                        className="sv-mono"
                        data-synth-open-trace={row.traceDigest}
                        style={{
                          border: 0,
                          background: "none",
                          padding: 0,
                          color: "#f05f22",
                          cursor: "pointer"
                        }}
                      >
                        {row.traceDigest.replace(/^sha256:/, "").slice(0, 12)}
                      </button>
                    </td>
                    <td style={{ padding: "6px 8px" }}>{row.benchmark ?? "—"}</td>
                    <td style={{ padding: "6px 8px" }}>{row.model ?? "—"}</td>
                    <td style={{ padding: "6px 8px" }}>{row.lifecycleStatus ?? "—"}</td>
                    <td className="sv-mono" style={{ padding: "6px 8px", textAlign: "right" }}>
                      {num(row.reward)}
                    </td>
                    <td className="sv-mono" style={{ padding: "6px 8px", textAlign: "right" }}>
                      {row.eventCount ?? "—"}
                    </td>
                    <td style={{ padding: "6px 8px", whiteSpace: "nowrap" }}>
                      {when(row.startedAt)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </VisualChrome>
  );
}

export default Shell;

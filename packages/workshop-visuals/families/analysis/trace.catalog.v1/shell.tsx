import { useEffect, useState } from "react";
import { useVisualState, useVisualSessionClient } from "@synth/visuals-react";
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
  querySchemaVersion?: string;
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
  if (snapshot.querySchemaVersion === "synth.trace-query.v2") return <ResearchTable key={snapshot.snapshotId} snapshot={snapshot} />;
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
                        data-reference-kind="trace" data-reference-value={row.traceDigest}
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


function ResearchTable({ snapshot }: { snapshot: QuerySnapshot }) {
  const visualSession = useVisualSessionClient();
  const storageKey = `workshop.research.catalog:${snapshot.snapshotId}:${snapshot.resultDigest}`;
  const restore = () => {
    try { return JSON.parse(sessionStorage.getItem(storageKey) ?? "{}"); } catch { return {}; }
  };
  const [page, setPage] = useVisualState("catalog.page", () => { const saved = visualSession ? 0 : restore().page; return Number.isInteger(saved) && saved >= 0 && saved * 50 < (snapshot.facets?.rows?.length ?? 0) ? saved : 0; }, {type:"number",minimum:0});
  const [selected, setSelected] = useVisualState<number | null>("catalog.selected", () => { const saved = visualSession ? null : restore().selected; return Number.isInteger(saved) && saved >= 0 && saved < (snapshot.facets?.rows?.length ?? 0) ? saved : null; }, {type:"number"});
  useEffect(() => { if(visualSession)return;try { sessionStorage.setItem(storageKey, JSON.stringify({ page, selected })); } catch { /* Storage may be unavailable in embedded previews. */ } }, [storageKey, page, selected,visualSession]);
  const rows = (snapshot.facets?.rows ?? []) as Array<Record<string, unknown>>;
  const size = 50;
  const columns = ["jobId", "trialId", "model", "effort", "environment", "taskId", "seed", "reward", "rewardMean", "rewardDelta", "matchStatus", "measuredCount", "missingCount", "actorId", "eventType", "label", "score", "reviewState", "analysisState", "traceAvailability"]
    .filter(key => rows.some(row => row[key] != null));
  const show = (value: unknown) => value == null ? "—" : typeof value === "object" ? JSON.stringify(value) : String(value);
  return <VisualChrome kicker="Evaluation research" title="Saved query results" testId="visual-trace-research" footer="trace.catalog.v1">
    <MetricStrip metrics={[{label:"Results",value:String(snapshot.resultCount ?? rows.length)},{label:"Page",value:`${page+1} / ${Math.max(1,Math.ceil(rows.length/size))}`}]} />
    <details><summary>Query and provenance</summary><pre style={{whiteSpace:"pre-wrap"}}>{JSON.stringify({snapshotId:snapshot.snapshotId,resultDigest:snapshot.resultDigest,query:snapshot.queryAst},null,2)}</pre></details>
    <div style={{overflowX:"auto"}}><table className="sv-table"><thead><tr>{columns.map(key=><th key={key}>{key}</th>)}<th>Evidence</th></tr></thead>
      <tbody>{rows.slice(page*size,(page+1)*size).map((row,i)=><tr key={snapshot.resultIds?.[page*size+i] ?? page*size+i}>
        {columns.map(key=><td key={key}>{show(row[key])}</td>)}<td>
          <button onClick={()=>setSelected(page*size+i)}>Inspect result</button>
          {typeof row.traceDigest === "string" && row.traceAvailability === "available" && <button data-reference-kind="trace" data-reference-value={row.traceDigest}>Open trace</button>}
        </td></tr>)}</tbody></table></div>
    {!rows.length && <p>No matching results.</p>}
    <nav aria-label="Query result pages"><button disabled={page===0} onClick={()=>setPage((p: number)=>p-1)}>Previous</button><button disabled={(page+1)*size>=rows.length} onClick={()=>setPage((p: number)=>p+1)}>Next</button></nav>
    {selected != null && <section aria-label="Selected evidence"><h3>Result evidence</h3><p className="sv-mono">{snapshot.resultIds?.[selected]}</p><pre style={{whiteSpace:"pre-wrap",maxHeight:400,overflow:"auto"}}>{JSON.stringify(rows[selected],null,2)}</pre><p>Source selectors resolve through the trace tool’s source operation. Annotations and comparison visuals are optional.</p></section>}
  </VisualChrome>;
}

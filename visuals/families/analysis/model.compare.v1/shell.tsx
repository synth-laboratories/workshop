import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import { UnresolvedInputNotice } from "../../../chrome/UnresolvedInputNotice.tsx";
import { formatMissingNumber, formatMissingUsd } from "../../../runtime/liveStream.ts";
import { resolveTemplateInput } from "../../../runtime/resolvedInput.ts";
import type { VisualBinding } from "../../../runtime/types.ts";
import compareFixture from "../../../fixtures/model_compare.json";

type CompareRow = {
  model: string;
  /** Required for a non-comparative run catalog. */
  benchmark?: string;
  effort?: string;
  /**
   * A producer that reported no achievements, no cost, or no success rate
   * leaves the field null. Null renders as missing; it never becomes a zero,
   * which would read as "free" or "never succeeded".
   */
  mean_achievements?: number | null;
  mean_reward?: number | null;
  cost_usd?: number | null;
  success_rate?: number | null;
  sparkline?: number[];
};

type ComparePayload = {
  metric?: string;
  /**
   * `like_for_like` means every row shares one evaluation contract and may be
   * ranked. `run_catalog` is descriptive only: each row is rendered as its own
   * benchmark card and no cross-row winner is inferred.
   */
  comparison_kind?: "like_for_like" | "run_catalog";
  rows: CompareRow[];
};

export type ShellProps = {
  title?: string;
  lede?: string;
  comparison?: ComparePayload;
  data?: ComparePayload;
  bindings?: VisualBinding[];
};

/** A payload this table can render, or `null`. Never the bundled example. */
function asCompare(raw: unknown): ComparePayload | null {
  if (!raw || typeof raw !== "object") return null;
  const candidate = raw as ComparePayload;
  if (!Array.isArray(candidate.rows) || candidate.rows.length === 0) return null;
  if (
    candidate.comparison_kind !== undefined
    && candidate.comparison_kind !== "like_for_like"
    && candidate.comparison_kind !== "run_catalog"
  ) {
    return null;
  }
  // Every cell in this table is `toFixed`ed. A row missing its numbers is not
  // a row this template can render, and used to be replaced by an example one.
  // A row must name a model and report at least one measurement. Everything
  // else may be honestly absent.
  const numeric = (value: unknown) => typeof value === "number" && Number.isFinite(value);
  if (
    !candidate.rows.every(
      (row) =>
        typeof row?.model === "string"
        && row.model.trim().length > 0
        && [row.mean_achievements, row.mean_reward, row.cost_usd, row.success_rate].some(numeric)
    )
  ) {
    return null;
  }
  if (
    candidate.comparison_kind === "run_catalog"
    && !candidate.rows.every((row) => typeof row.benchmark === "string" && row.benchmark.trim().length > 0)
  ) {
    return null;
  }
  return candidate;
}

type RankMetric = "mean_achievements" | "mean_reward" | "cost_usd" | "success_rate";

function rankMetric(metric: string | undefined): RankMetric | null {
  return ["mean_achievements", "mean_reward", "cost_usd", "success_rate"].includes(metric ?? "")
    ? metric as RankMetric
    : null;
}

function CatalogMetric({ label, value }: { label: string; value: string }) {
  return (
    <div style={{ minWidth: 112 }}>
      <div className="sv-label">{label}</div>
      <div className="sv-mono" style={{ marginTop: 4, fontSize: 15 }}>{value}</div>
    </div>
  );
}

function Spark({ values, label }: { values: number[]; label: string }) {
  if (!values.length) return null;
  const max = Math.max(...values);
  const min = Math.min(...values);
  const span = Math.max(max - min, 0.01);
  const w = 72;
  const h = 24;
  const pts = values
    .map((v, i) => {
      const x = (i / Math.max(values.length - 1, 1)) * (w - 2) + 1;
      const y = h - 2 - ((v - min) / span) * (h - 4);
      return `${x},${y}`;
    })
    .join(" ");

  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      width={w}
      height={h}
      role="img"
      aria-label={`${label} sparkline from ${values[0]} to ${values[values.length - 1]}`}
    >
      <polyline fill="none" stroke="#f05f22" strokeWidth="1.5" points={pts} />
    </svg>
  );
}

export function Shell(props: ShellProps) {
  const resolved = resolveTemplateInput<ComparePayload>({
    input: "comparison",
    candidates: [props.data, props.comparison],
    bindings: props.bindings,
    accept: asCompare,
    fixture: compareFixture
  });

  if (resolved.status === "unresolved") {
    return (
      <VisualChrome
        kicker="Model comparison"
        title={props.title ?? "Multi-model table"}
        lede={props.lede}
        testId="visual-model-compare"
        footer="model.compare.v1"
      >
        <UnresolvedInputNotice unresolved={resolved.unresolved} />
      </VisualChrome>
    );
  }

  const data = resolved.value;
  if (data.comparison_kind === "run_catalog") {
    return (
      <VisualChrome
        kicker="Evaluation run catalog"
        title={props.title ?? "Retained evaluation runs"}
        lede="Each card uses its benchmark's own scoring contract. Values are not comparable or ranked across cards."
        testId="visual-model-compare"
        footer="model.compare.v1 · descriptive catalog"
      >
        <div role="list" aria-label="Benchmark-local evaluation runs" style={{ display: "grid", gap: 12 }}>
          {data.rows.map((row) => (
            <section
              role="listitem"
              key={`${row.benchmark}-${row.model}-${row.effort ?? ""}`}
              style={{ border: "1px solid var(--sv-border)", borderRadius: 10, padding: 14 }}
            >
              <div className="sv-label">{row.benchmark}</div>
              <h3 style={{ margin: "5px 0 12px", fontSize: 16 }}>{row.model}</h3>
              <div style={{ display: "flex", flexWrap: "wrap", gap: "12px 26px", alignItems: "end" }}>
                <CatalogMetric label="Effort" value={row.effort ?? "—"} />
                <CatalogMetric
                  label="Mean achievements"
                  value={typeof row.mean_achievements === "number" ? row.mean_achievements.toFixed(1) : "—"}
                />
                <CatalogMetric label="Mean reward" value={formatMissingNumber(row.mean_reward)} />
                <CatalogMetric label="Reported cost" value={formatMissingUsd(row.cost_usd)} />
                <CatalogMetric
                  label="Positive-reward rate"
                  value={typeof row.success_rate === "number" ? `${Math.round(row.success_rate * 100)}%` : "—"}
                />
                {row.sparkline ? <Spark values={row.sparkline} label={`${row.model} within-benchmark trend`} /> : null}
              </div>
            </section>
          ))}
        </div>
      </VisualChrome>
    );
  }

  const primaryMetric = rankMetric(data.metric);
  const ranked = primaryMetric
    ? data.rows.filter((row) => typeof row[primaryMetric] === "number")
    : [];
  const best = [...ranked].sort((a, b) => {
    const left = a[primaryMetric!] as number;
    const right = b[primaryMetric!] as number;
    return primaryMetric === "cost_usd" ? left - right : right - left;
  })[0];

  return (
    <VisualChrome
      kicker="Model comparison"
      title={props.title ?? "Multi-model table"}
      lede={props.lede ?? (data.metric ? `Primary metric: ${data.metric}` : undefined)}
      testId="visual-model-compare"
      footer="model.compare.v1"
    >
      <div style={{ overflowX: "auto" }}>
        <table className="sv-table" aria-label="Model comparison">
          <thead>
            <tr>
              <th scope="col">Model</th>
              <th scope="col">Effort</th>
              <th scope="col">Achievements</th>
              <th scope="col">Reward</th>
              <th scope="col">Cost</th>
              <th scope="col">Success</th>
              <th scope="col">Trend</th>
            </tr>
          </thead>
          <tbody>
            {data.rows.map((row) => {
              const accent = row === best;
              return (
                <tr key={`${row.model}-${row.effort ?? ""}`}>
                  <td>
                    <strong style={{ color: accent ? "var(--sv-accent)" : undefined }}>
                      {row.model}
                    </strong>
                  </td>
                  <td className="sv-mono">{row.effort ?? "—"}</td>
                  <td className="sv-mono">
                    {typeof row.mean_achievements === "number" ? row.mean_achievements.toFixed(1) : "—"}
                  </td>
                  <td className="sv-mono">{formatMissingNumber(row.mean_reward)}</td>
                  <td className="sv-mono">{formatMissingUsd(row.cost_usd)}</td>
                  <td className="sv-mono">
                    {typeof row.success_rate === "number" ? `${Math.round(row.success_rate * 100)}%` : "—"}
                  </td>
                  <td>
                    {row.sparkline ? (
                      <Spark values={row.sparkline} label={row.model} />
                    ) : (
                      "—"
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </VisualChrome>
  );
}

export default Shell;

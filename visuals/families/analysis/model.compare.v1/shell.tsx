import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import { UnresolvedInputNotice } from "../../../chrome/UnresolvedInputNotice.tsx";
import { formatMissingNumber, formatMissingUsd } from "../../../runtime/liveStream.ts";
import { resolveTemplateInput } from "../../../runtime/resolvedInput.ts";
import type { VisualBinding } from "../../../runtime/types.ts";
import compareFixture from "../../../fixtures/model_compare.json";

type CompareRow = {
  model: string;
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
  return candidate;
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
  const ranked = data.rows.filter((row) => typeof row.mean_reward === "number");
  const best = [...ranked].sort((a, b) => (b.mean_reward ?? 0) - (a.mean_reward ?? 0))[0];

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
              const accent = row.model === best?.model;
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

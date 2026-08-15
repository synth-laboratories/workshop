import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import type { VisualBinding } from "../../../runtime/types.ts";
import compareFixture from "../../../fixtures/model_compare.json";

type CompareRow = {
  model: string;
  effort?: string;
  mean_achievements: number;
  mean_reward: number;
  cost_usd: number;
  success_rate: number;
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

function asCompare(raw: unknown): ComparePayload {
  if (raw && typeof raw === "object" && Array.isArray((raw as ComparePayload).rows)) {
    return raw as ComparePayload;
  }
  return compareFixture as ComparePayload;
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
  const data = asCompare(props.data ?? props.comparison ?? compareFixture);
  const best = [...data.rows].sort((a, b) => b.mean_achievements - a.mean_achievements)[0];

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
                  <td className="sv-mono">{row.mean_achievements.toFixed(1)}</td>
                  <td className="sv-mono">{row.mean_reward.toFixed(2)}</td>
                  <td className="sv-mono">${row.cost_usd.toFixed(2)}</td>
                  <td className="sv-mono">{Math.round(row.success_rate * 100)}%</td>
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

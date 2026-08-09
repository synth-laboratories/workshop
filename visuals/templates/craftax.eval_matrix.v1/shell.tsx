import { VisualChrome, MetricStrip } from "../../chrome/VisualChrome.tsx";
import type { EvalMatrixPoint, VisualBinding } from "../../runtime/types.ts";
import matrixFixture from "../../fixtures/craftax_matrix_slice.json";
import { chunkAchievements, rateFor, type MatrixSlice } from "./components/matrixUtils.ts";

export type ShellProps = {
  title?: string;
  lede?: string;
  matrix?: MatrixSlice;
  bindings?: VisualBinding[];
  /** When Desktop has already resolved the matrix slot. */
  data?: MatrixSlice;
};

function asSlice(raw: unknown): MatrixSlice {
  if (raw && typeof raw === "object" && "points" in (raw as object)) {
    return raw as MatrixSlice;
  }
  return matrixFixture as MatrixSlice;
}

function formatCostUsd(cost: number): string {
  if (cost === 0) return "$0.00";
  if (cost < 0.01) return `$${cost.toFixed(5).replace(/0+$/, "")}`;
  return `$${cost.toFixed(2)}`;
}

function ParetoChart({ points }: { points: EvalMatrixPoint[] }) {
  const maxAch = Math.max(...points.map((p) => p.achievements), 1);
  const maxCost = Math.max(...points.map((p) => p.cost_usd), 0.01);

  return (
    <div className="pareto-plot" role="img" aria-label="Pareto chart of achievements versus cost per rollout">
      <svg viewBox="0 0 320 200" width="100%" style={{ maxHeight: 220 }}>
        <defs>
          <linearGradient id="svParetoFill" x1="0" y1="1" x2="0" y2="0">
            <stop offset="0%" stopColor="rgba(240,95,34,0.08)" />
            <stop offset="100%" stopColor="rgba(240,95,34,0)" />
          </linearGradient>
        </defs>
        {[40, 80, 120, 160].map((y) => (
          <line key={y} x1="36" y1={y} x2="300" y2={y} stroke="#e8eaee" strokeWidth="1" />
        ))}
        {[80, 140, 200, 260].map((x) => (
          <line key={x} x1={x} y1="20" x2={x} y2="168" stroke="#eef0f3" strokeWidth="1" />
        ))}
        <path
          d="M70 150 C 110 120, 150 95, 190 70 S 250 48, 280 40"
          fill="none"
          stroke="rgba(240,95,34,0.45)"
          strokeWidth="2"
        />
        {points.map((m) => {
          const x = 50 + (m.cost_usd / maxCost) * 230;
          const y = 160 - (m.achievements / maxAch) * 130;
          return (
            <g key={`${m.model}-${m.effort ?? ""}`}>
              <circle
                cx={x}
                cy={y}
                r={m.accent ? 8 : 6}
                fill={m.accent ? "#f05f22" : "#5c6573"}
              />
              <text
                x={x}
                y={y - 12}
                textAnchor="middle"
                fill="#5c6573"
                fontSize="9"
                fontFamily="var(--sv-mono)"
              >
                {m.model}
              </text>
            </g>
          );
        })}
        <text x="168" y="192" textAnchor="middle" fill="#8b93a1" fontSize="10">
          inference cost / rollout
        </text>
        <text
          x="14"
          y="100"
          textAnchor="middle"
          fill="#8b93a1"
          fontSize="10"
          transform="rotate(-90 14 100)"
        >
          achievements
        </text>
      </svg>
    </div>
  );
}

function AchievementMatrix({
  achievements,
  points,
  families
}: {
  achievements: string[];
  points: EvalMatrixPoint[];
  families?: string[];
}) {
  const primary = points.find((p) => p.accent) ?? points[0];
  const rows = chunkAchievements(achievements, 6);

  return (
    <div role="group" aria-label="Achievement matrix">
      {rows.map((row, ri) => (
        <div
          key={ri}
          style={{ display: "flex", gap: 4, marginBottom: 4 }}
          role="row"
        >
          {row.map((cell) => {
            const heat = rateFor(primary, cell);
            return (
              <div
                key={cell}
                role="gridcell"
                aria-label={`${cell}: ${Math.round(heat * 100)} percent`}
                title={`${cell} · ${Math.round(heat * 100)}%`}
                style={{
                  flex: 1,
                  minWidth: 0,
                  padding: "8px 4px",
                  borderRadius: 6,
                  textAlign: "center",
                  fontSize: 10,
                  fontFamily: "var(--sv-mono)",
                  background: `rgba(240, 95, 34, ${0.12 + heat * 0.72})`,
                  color: heat > 0.45 ? "#fff" : "#5a3a28"
                }}
              >
                {cell}
              </div>
            );
          })}
        </div>
      ))}
      {families?.length ? (
        <div
          style={{
            display: "flex",
            flexWrap: "wrap",
            gap: 8,
            marginTop: 10,
            color: "var(--sv-text-faint)",
            fontSize: 11
          }}
          aria-label="Achievement families"
        >
          {families.map((f) => (
            <span key={f}>{f}</span>
          ))}
        </div>
      ) : null}
    </div>
  );
}

export function Shell(props: ShellProps) {
  const slice = asSlice(props.data ?? props.matrix ?? matrixFixture);
  const points = slice.points;
  const accent = points.find((p) => p.accent) ?? points[0];

  return (
    <VisualChrome
      kicker="Open-ended agents · Craftax"
      title={props.title ?? slice.title ?? "Craftax eval matrix"}
      lede={props.lede}
      testId="visual-craftax-eval-matrix"
      footer="craftax.eval_matrix.v1 · usesynth.ai/evals/craftax"
    >
      <section className="sv-section">
        <div className="sv-section-head">
          <h3>Cost vs performance</h3>
          <span>ACH ↑ · $ / rollout →</span>
        </div>
        <ParetoChart points={points} />
        <MetricStrip
          metrics={[
            {
              label: accent?.model ?? "Top",
              value: `${accent?.achievements?.toFixed?.(1) ?? "—"} ach`
            },
            {
              label: "$ / rollout",
              value: accent ? formatCostUsd(accent.cost_usd) : "—"
            },
            {
              label: "Models",
              value: String(points.length)
            }
          ]}
        />
      </section>

      <section className="sv-section">
        <div className="sv-section-head">
          <h3>Per-achievement breakdown</h3>
          <span>
            {slice.achievements.length} achievements · {(accent ?? points[0])?.model ?? "cohort"}
          </span>
        </div>
        <AchievementMatrix
          achievements={slice.achievements}
          points={points}
          families={slice.families}
        />
      </section>
    </VisualChrome>
  );
}

export default Shell;

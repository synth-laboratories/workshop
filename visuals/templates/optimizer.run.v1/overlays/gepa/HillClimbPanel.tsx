import type { GepaState } from "../../components/projectEvents.ts";
import { candidateName } from "./model.ts";

const W = 430;
const H = 190;
const PAD = { left: 46, right: 18, top: 18, bottom: 34 };

function finiteScore(candidate: Record<string, unknown>): number | undefined {
  for (const value of [candidate.train_reward, candidate.score, candidate.minibatchReward]) {
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  return undefined;
}

export function HillClimbPanel({ gepa, onSelect }: { gepa: GepaState; onSelect?: (id: string) => void }) {
  const points = gepa.candidates
    .map((candidate) => ({
      candidate,
      id: String(candidate.id ?? ""),
      sequence: typeof candidate.sequence === "number" ? candidate.sequence : 0,
      score: finiteScore(candidate)
    }))
    .filter((point): point is typeof point & { score: number } => point.score != null)
    .sort((a, b) => a.sequence - b.sequence);
  if (points.length === 0) return null;
  const scores = points.map((point) => point.score);
  let min = Math.min(...scores);
  let max = Math.max(...scores);
  if (min === max) { min -= 0.05; max += 0.05; }
  const margin = Math.max(0.02, (max - min) * 0.12);
  min -= margin;
  max += margin;
  const x = (index: number) => PAD.left + (points.length === 1 ? 0.5 : index / (points.length - 1)) * (W - PAD.left - PAD.right);
  const y = (score: number) => PAD.top + ((max - score) / (max - min)) * (H - PAD.top - PAD.bottom);
  let runningBest = Number.NEGATIVE_INFINITY;
  const best = points.map((point) => {
    runningBest = Math.max(runningBest, point.score);
    return runningBest;
  });
  const scorePath = points.map((point, index) => `${index === 0 ? "M" : "L"}${x(index)},${y(point.score)}`).join(" ");
  const bestPath = best.map((score, index) => `${index === 0 ? "M" : "L"}${x(index)},${y(score)}`).join(" ");

  return (
    <section className="sv-section" aria-label="GEPA hill climb" data-testid="gepa-hill-climb" style={{ marginTop: 0 }}>
      <div className="sv-section-head">
        <h3>Hill climb</h3>
        <span className="sv-mono">{points.length} scored candidates · best {Math.max(...scores).toFixed(3)}</span>
      </div>
      <div style={{ border: "1px solid var(--sv-border)", borderRadius: 9, padding: "8px 10px" }}>
        <svg viewBox={`0 0 ${W} ${H}`} width="100%" role="img" aria-label="Candidate score and best score through the GEPA search" style={{ display: "block", maxWidth: 650, margin: "0 auto" }}>
          {[0, 0.5, 1].map((fraction) => {
            const score = min + (max - min) * fraction;
            return <g key={fraction}>
              <line x1={PAD.left} y1={y(score)} x2={W - PAD.right} y2={y(score)} stroke="var(--sv-border)" />
              <text x={PAD.left - 7} y={y(score) + 3} textAnchor="end" fontSize="9" fill="var(--sv-text-faint)">{score.toFixed(2)}</text>
            </g>;
          })}
          <path d={scorePath} fill="none" stroke="#6d7480" strokeWidth="1.5" />
          <path d={bestPath} fill="none" stroke="var(--sv-accent)" strokeWidth="2.5" />
          {points.map((point, index) => (
            <g key={point.id} role="button" tabIndex={0} style={{ cursor: "pointer" }}
              aria-label={`${candidateName(point.candidate)} scored ${point.score.toFixed(3)}`}
              onClick={() => onSelect?.(point.id)}
              onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") onSelect?.(point.id); }}>
              <circle cx={x(index)} cy={y(point.score)} r="5" fill={point.score === best[index] ? "var(--sv-accent)" : "#6d7480"} />
              <text x={x(index)} y={H - 13} textAnchor="middle" fontSize="9" fill="var(--sv-text-faint)">{index + 1}</text>
            </g>
          ))}
          <text x={(PAD.left + W - PAD.right) / 2} y={H - 1} textAnchor="middle" fontSize="10" fill="var(--sv-text-muted)">scored candidate order</text>
        </svg>
        <div style={{ display: "flex", gap: 14, color: "var(--sv-text-muted)", fontSize: 10.5 }}>
          <span><b style={{ color: "var(--sv-accent)" }}>—</b> best so far</span><span><b style={{ color: "#6d7480" }}>—</b> candidate score</span>
        </div>
      </div>
    </section>
  );
}

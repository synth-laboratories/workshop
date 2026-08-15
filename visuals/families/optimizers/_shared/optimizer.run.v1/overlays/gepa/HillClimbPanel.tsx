import type { GepaState } from "../../components/projectEvents.ts";
import { candidateName, candidatePalette, candidateGeneration, incumbentCandidateIds, orderedScoredCandidates } from "./model.ts";

const W = 430;
const H = 190;
const PAD = { left: 46, right: 18, top: 18, bottom: 34 };

export function HillClimbPanel({ gepa, onSelect }: { gepa: GepaState; onSelect?: (id: string) => void }) {
  const points = orderedScoredCandidates(gepa);
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
  const pointIndex = new Map(points.map((point, index) => [point.id, index]));
  const incumbents = incumbentCandidateIds(gepa).flatMap((id) => {
    const index = pointIndex.get(id);
    return index == null ? [] : [{ ...points[index], index }];
  });
  const incumbentPath = incumbents.map((point, index) => `${index === 0 ? "M" : "L"}${x(point.index)},${y(point.score)}`).join(" ");
  const incumbentSet = new Set(incumbents.map((point) => point.id));
  const generationLegend = [...new Map(points.map((point) => {
    const generation = candidateGeneration(point.candidate);
    return [generation == null ? "seed" : `gen-${generation}`, { label: generation == null ? "Seed" : `Generation ${generation}`, ...candidatePalette(point.candidate) }];
  })).values()];

  return (
    <section className="sv-section" aria-label="GEPA hill climb" data-testid="gepa-hill-climb" style={{ marginTop: 0 }}>
      <div className="sv-section-head">
        <h3>Incumbent hill climb</h3>
        <span className="sv-mono">{points.length} train-scored · {incumbents.length} incumbent{incumbents.length === 1 ? "" : "s"} · best {Math.max(...incumbents.map((point) => point.score), ...scores).toFixed(3)}</span>
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
          {incumbentPath ? <path d={incumbentPath} fill="none" stroke="var(--sv-accent)" strokeWidth="2.5" /> : null}
          {points.map((point, index) => {
            const palette = candidatePalette(point.candidate);
            return (
            <g key={point.id} role="button" tabIndex={0} style={{ cursor: "pointer" }}
              aria-label={`${candidateName(point.candidate)} scored ${point.score.toFixed(3)}`}
              onClick={() => onSelect?.(point.id)}
              onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") onSelect?.(point.id); }}>
              <circle cx={x(index)} cy={y(point.score)} r={incumbentSet.has(point.id) ? 6 : 4.5} fill={palette.color} stroke={incumbentSet.has(point.id) ? "var(--sv-accent)" : "white"} strokeWidth={incumbentSet.has(point.id) ? 2.5 : 1.5} opacity={incumbentSet.has(point.id) ? 1 : .72} />
              <text x={x(index)} y={H - 13} textAnchor="middle" fontSize="9" fill="var(--sv-text-faint)">{index + 1}</text>
            </g>
            );
          })}
          <text x={(PAD.left + W - PAD.right) / 2} y={H - 1} textAnchor="middle" fontSize="10" fill="var(--sv-text-muted)">scored candidate order</text>
        </svg>
        <div style={{ display: "flex", gap: 14, color: "var(--sv-text-muted)", fontSize: 10.5 }}>
          <span><b style={{ color: "var(--sv-accent)" }}>—</b> authoritative incumbent trajectory</span>
          {generationLegend.map((entry) => <span key={entry.label}><b style={{ display: "inline-block", width: 8, height: 8, borderRadius: "50%", background: entry.color, marginRight: 4 }} />{entry.label}</span>)}
        </div>
        <p style={{ margin: "7px 0 0", color: "var(--sv-text-faint)", fontSize: 10.5 }}>Dots are complete full-train evaluations. The line connects only candidates accepted as incumbents; rejected siblings never become progress.</p>
      </div>
    </section>
  );
}

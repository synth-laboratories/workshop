/**
 * Pareto search canvas: every evaluated candidate on the quality × coverage
 * plane, frontier membership, incumbent, parent links, and honest states for
 * zero / one / many members. Rejected and dominated candidates stay visible —
 * the chart explains the search, not just the survivors.
 */

import type { GepaState } from "../../components/projectEvents.ts";
import {
  candidateName,
  candidatePoint,
  metricsByCandidate,
  statusLabel,
  type CandidateRecord
} from "./model.ts";

const PLOT = { left: 46, right: 396, top: 18, bottom: 208 };

function x(coverage: number): number {
  return PLOT.left + coverage * (PLOT.right - PLOT.left);
}

function y(quality: number): number {
  return PLOT.bottom - quality * (PLOT.bottom - PLOT.top);
}

export function FrontierPanel({
  gepa,
  selectedId,
  onSelect
}: {
  gepa: GepaState;
  selectedId?: string | null;
  onSelect?: (id: string) => void;
}) {
  const metrics = metricsByCandidate(gepa.evaluations);
  const frontierIds = new Set(gepa.frontier.map((cell) => String(cell.candidateId)));
  const incumbentId = gepa.incumbentId ?? gepa.best?.candidateId;

  const plotted = gepa.candidates.map((candidate) => {
    const id = String(candidate.id ?? "");
    const point = candidatePoint(candidate, metrics.get(id));
    return { candidate, id, point };
  });
  const placeable = plotted.filter(({ point }) => point.quality != null && point.coverage != null);
  const unplaced = plotted.filter(({ point }) => point.quality == null || point.coverage == null);

  const edges = placeable.flatMap(({ candidate, id, point }) => {
    const parentId = candidate.parentId == null ? null : String(candidate.parentId);
    if (!parentId) return [];
    const parent = placeable.find((entry) => entry.id === parentId);
    if (!parent) return [];
    return [{
      key: `${parentId}->${id}`,
      x1: x(parent.point.coverage!),
      y1: y(parent.point.quality!),
      x2: x(point.coverage!),
      y2: y(point.quality!)
    }];
  });

  const caption = (() => {
    if (gepa.candidates.length === 0) return "No candidates yet — the seed prompt is evaluated first.";
    if (placeable.length === 0) return "Candidates are registered but no rollouts have scored yet.";
    if (placeable.length === 1) {
      const only = placeable[0];
      const rejectedCount = plotted.filter(({ candidate }) => String(candidate.status ?? "").startsWith("rejected")).length;
      return `The frontier holds one candidate: ${candidateName(only.candidate)} at score ${only.point.quality!.toFixed(2)}, solving ${(only.point.coverage! * 100).toFixed(0)}% of examples.${rejectedCount > 0 ? ` ${rejectedCount} proposal${rejectedCount === 1 ? "" : "s"} failed to beat it.` : " Proposals must beat it to join."}`;
    }
    return null;
  })();

  return (
    <section className="sv-section" aria-label="Pareto frontier" data-testid="gepa-pareto-frontier" style={{ marginTop: 0 }}>
      <div className="sv-section-head">
        <h3>Search space</h3>
        <span className="sv-mono">
          {gepa.frontier.length} frontier · {gepa.candidates.length} candidate{gepa.candidates.length === 1 ? "" : "s"}
        </span>
      </div>
      <div style={{ border: "1px solid var(--sv-border)", borderRadius: 9, padding: "8px 10px" }}>
        <svg viewBox="0 0 430 248" width="100%" role="img" aria-label="Candidate quality versus example coverage">
          {[0, 0.25, 0.5, 0.75, 1].map((tick) => (
            <g key={`grid-${tick}`}>
              <line x1={x(tick)} y1={PLOT.top} x2={x(tick)} y2={PLOT.bottom} stroke="var(--sv-border)" strokeWidth="1" />
              <line x1={PLOT.left} y1={y(tick)} x2={PLOT.right} y2={y(tick)} stroke="var(--sv-border)" strokeWidth="1" />
              <text x={x(tick)} y={PLOT.bottom + 14} textAnchor="middle" fontSize="9" fill="var(--sv-text-faint)">
                {(tick * 100).toFixed(0)}%
              </text>
              <text x={PLOT.left - 8} y={y(tick) + 3} textAnchor="end" fontSize="9" fill="var(--sv-text-faint)">
                {tick.toFixed(2)}
              </text>
            </g>
          ))}
          <text x={(PLOT.left + PLOT.right) / 2} y={240} textAnchor="middle" fontSize="10" fill="var(--sv-text-muted)">
            examples solved (coverage)
          </text>
          <text x={12} y={(PLOT.top + PLOT.bottom) / 2} textAnchor="middle" fontSize="10" fill="var(--sv-text-muted)" transform={`rotate(-90 12 ${(PLOT.top + PLOT.bottom) / 2})`}>
            score (mean reward)
          </text>
          {edges.map((edge) => (
            <line key={edge.key} x1={edge.x1} y1={edge.y1} x2={edge.x2} y2={edge.y2} stroke="var(--sv-border-strong)" strokeWidth="1.2" strokeDasharray="3 3" />
          ))}
          {placeable.map(({ candidate, id, point }) => {
            const rejected = String(candidate.status ?? "").startsWith("rejected");
            const evaluating = candidate.status === "evaluating";
            const onFrontier = frontierIds.has(id);
            const isIncumbent = id === incumbentId;
            const isSelected = id === selectedId;
            const cx = x(point.coverage!);
            const cy = y(point.quality!);
            const fill = onFrontier ? "var(--sv-accent)" : rejected ? "var(--sv-surface)" : "#5c6573";
            const stroke = rejected ? "#b23830" : onFrontier ? "var(--sv-accent)" : "#5c6573";
            return (
              <g
                key={id}
                role="button"
                tabIndex={0}
                aria-label={`${candidateName(candidate)} · ${statusLabel(candidate.status)} · score ${point.quality!.toFixed(2)} · coverage ${(point.coverage! * 100).toFixed(0)}%`}
                data-testid={`frontier-point-${id}`}
                onClick={() => onSelect?.(id)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") onSelect?.(id);
                }}
                style={{ cursor: "pointer" }}
              >
                {isIncumbent ? <circle cx={cx} cy={cy} r={10} fill="none" stroke="var(--sv-accent)" strokeWidth="1.5" /> : null}
                {isSelected ? <circle cx={cx} cy={cy} r={13} fill="none" stroke="var(--sv-border-strong)" strokeWidth="1.5" /> : null}
                <circle cx={cx} cy={cy} r={6} fill={fill} stroke={stroke} strokeWidth="1.5" strokeDasharray={evaluating ? "2 2" : undefined} />
                {rejected ? (
                  <>
                    <line x1={cx - 3} y1={cy - 3} x2={cx + 3} y2={cy + 3} stroke="#b23830" strokeWidth="1.4" />
                    <line x1={cx - 3} y1={cy + 3} x2={cx + 3} y2={cy - 3} stroke="#b23830" strokeWidth="1.4" />
                  </>
                ) : null}
                <text x={cx} y={cy - 14} textAnchor="middle" fontSize="9" fill="var(--sv-text-muted)">
                  {candidateName(candidate)}
                </text>
              </g>
            );
          })}
        </svg>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 12, marginTop: 4, fontSize: 10.5, color: "var(--sv-text-muted)" }} aria-hidden="true">
          <span><span style={{ display: "inline-block", width: 8, height: 8, borderRadius: 4, background: "var(--sv-accent)", marginRight: 4 }} />frontier</span>
          <span><span style={{ display: "inline-block", width: 8, height: 8, borderRadius: 4, background: "#5c6573", marginRight: 4 }} />evaluated</span>
          <span><span style={{ display: "inline-block", width: 8, height: 8, borderRadius: 4, border: "1.5px solid #b23830", marginRight: 4 }} />rejected</span>
          <span><span style={{ display: "inline-block", width: 8, height: 8, borderRadius: 4, border: "1.5px solid var(--sv-accent)", marginRight: 4 }} />incumbent ring</span>
          <span>dashes link parent → child</span>
        </div>
        {caption ? <p style={{ margin: "8px 0 0", fontSize: 12, color: "var(--sv-text-muted)" }}>{caption}</p> : null}
        {unplaced.length > 0 ? (
          <p style={{ margin: "6px 0 0", fontSize: 11.5, color: "var(--sv-text-faint)" }}>
            Not plotted (no scored rollouts yet):{" "}
            {unplaced.map(({ candidate, id }, index) => (
              <button
                key={id}
                type="button"
                className="sv-btn"
                style={{ padding: "1px 7px", fontSize: 11, marginLeft: index === 0 ? 0 : 4 }}
                onClick={() => onSelect?.(id)}
              >
                {candidateName(candidate)}
              </button>
            ))}
          </p>
        ) : null}
      </div>
    </section>
  );
}

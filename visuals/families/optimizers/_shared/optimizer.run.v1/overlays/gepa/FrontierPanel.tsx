/**
 * GEPA's frontier is a frontier over per-example reward vectors, not a generic
 * reward × coverage trade-off curve. A candidate remains selectable when no
 * other train-evaluated candidate is at least as good on every example and
 * strictly better on one. The cell map also explains the paper's parent
 * selector: candidates receive credit for examples on which they are best.
 */

import type { GepaState } from "../../components/projectEvents.ts";
import { candidateName, candidatePalette, statusLabel, type CandidateRecord } from "./model.ts";

type CandidateRow = {
  candidate: CandidateRecord;
  id: string;
  scores: Map<string, number>;
  mean?: number;
  wins: number;
  onFrontier: boolean;
};

function isTrainSelectable(candidate: CandidateRecord): boolean {
  return String(candidate.source ?? "") === "seed" ||
    ["accepted", "full_train_evaluated", "rejected_full_train"].includes(String(candidate.status ?? ""));
}

function fullTrainScores(gepa: GepaState, candidateId: string): Map<string, number> {
  const scores = new Map<string, number>();
  for (const evaluation of gepa.evaluations) {
    if (evaluation.candidateId !== candidateId || evaluation.reward == null || !evaluation.exampleId) continue;
    if (!["seed_full_train", "candidate_full_train"].includes(evaluation.stage ?? "")) continue;
    scores.set(evaluation.exampleId, evaluation.reward);
  }
  return scores;
}

function cellColor(reward: number, winner: boolean): string {
  if (winner) return "var(--sv-accent)";
  if (reward > 0) return "var(--sv-border-strong)";
  return "var(--sv-surface-muted)";
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
  const frontierIds = new Set(gepa.frontier.map((member) => String(member.candidateId)));
  const selectable = gepa.candidates.filter(isTrainSelectable);
  const allExamples = [...new Set(selectable.flatMap((candidate) =>
    [...fullTrainScores(gepa, String(candidate.id ?? "")).keys()]
  ))].sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
  const bestByExample = new Map<string, number>();
  for (const exampleId of allExamples) {
    const values = selectable
      .map((candidate) => fullTrainScores(gepa, String(candidate.id ?? "")).get(exampleId))
      .filter((value): value is number => value != null);
    if (values.length) bestByExample.set(exampleId, Math.max(...values));
  }
  const rows: CandidateRow[] = selectable.map((candidate) => {
    const id = String(candidate.id ?? "");
    const scores = fullTrainScores(gepa, id);
    const values = [...scores.values()];
    const wins = [...scores].filter(([exampleId, reward]) =>
      Math.abs(reward - (bestByExample.get(exampleId) ?? Number.POSITIVE_INFINITY)) <= Number.EPSILON
    ).length;
    return {
      candidate,
      id,
      scores,
      mean: values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : undefined,
      wins,
      onFrontier: frontierIds.has(id)
    };
  }).sort((a, b) => Number(b.onFrontier) - Number(a.onFrontier) || b.wins - a.wins);
  const pending = gepa.candidates.filter((candidate) => !isTrainSelectable(candidate));
  const frontierRows = rows.filter((row) => row.onFrontier);
  const frontierPending = frontierIds.size === 0 && !gepa.activity.terminal;
  const frontierCoverage = new Set(frontierRows.flatMap((row) =>
    [...row.scores].filter(([, reward]) => reward > 0).map(([exampleId]) => exampleId)
  )).size;
  const progress = gepa.frontierHistory.filter((snapshot) => snapshot.totalExamples != null && (snapshot.optimisticSolved != null || snapshot.bestCandidateSolved != null)).filter((snapshot, index, snapshots) => {
    const previous = snapshots[index - 1];
    return !previous || snapshot.bestCandidateId !== previous.bestCandidateId || snapshot.bestCandidateSolved !== previous.bestCandidateSolved || snapshot.optimisticSolved !== previous.optimisticSolved || snapshot.frontierSize !== previous.frontierSize;
  });

  return (
    <section className="sv-section" aria-label="GEPA Pareto frontier" data-testid="gepa-pareto-frontier" style={{ marginTop: 0 }}>
      <div className="sv-section-head">
        <h3>GEPA Pareto frontier</h3>
        <span className="sv-mono">{frontierRows.length} member{frontierRows.length === 1 ? "" : "s"} · {allExamples.length} example dimensions</span>
      </div>
      <p style={{ margin: "0 0 9px", color: "var(--sv-text-muted)", fontSize: 11.5 }}>
        Non-dominated per-example reward vectors. Orange cells mark examples where a candidate is currently best; aggregate mean is context, not a Pareto axis.
      </p>
      {progress.length ? (
        <div data-testid="gepa-explore-exploit" style={{ border: "1px solid var(--sv-border)", borderRadius: 9, padding: "10px 12px", marginBottom: 10 }}>
          <div style={{ display: "flex", justifyContent: "space-between", gap: 10, alignItems: "baseline" }}>
            <strong style={{ fontSize: 12 }}>Explore ↔ exploit</strong>
            <span className="sv-mono" style={{ color: "var(--sv-text-faint)", fontSize: 9.5 }}>{progress.at(-1)?.coverageSemantics?.replaceAll("_", " ") ?? "per-example reward"}</span>
          </div>
          <div style={{ display: "grid", gap: 7, marginTop: 9 }}>
            {progress.map((snapshot, index) => {
              const total = snapshot.totalExamples ?? 0;
              const optimistic = snapshot.optimisticSolved ?? 0;
              const incumbent = snapshot.bestCandidateSolved ?? 0;
              return (
            <div key={snapshot.sequence} className="sv-gepa-explore-row" style={{ display: "grid", gap: 9, alignItems: "center" }}>
              <span className="sv-gepa-explore-label" style={{ fontSize: 10.5 }}>{index === 0 ? "Seed" : `Generation ${snapshot.generation ?? index - 1}`}</span>
              <span className="sv-gepa-explore-meter" style={{ position: "relative", height: 13, borderRadius: 99, background: "var(--sv-surface-muted)", overflow: "hidden" }} title={`Optimistic union ${optimistic}/${total}; incumbent ${incumbent}/${total}`}>
                <span style={{ position: "absolute", inset: 0, width: total ? `${optimistic / total * 100}%` : "0%", background: "var(--sv-border-strong)" }} />
                <span style={{ position: "absolute", inset: 0, width: total ? `${incumbent / total * 100}%` : "0%", background: "var(--sv-accent)" }} />
              </span>
              <span className="sv-gepa-explore-receipt sv-mono" style={{ fontSize: 9.5, color: "var(--sv-text-muted)" }}>{incumbent}/{total} incumbent · {optimistic}/{total} ever</span>
                </div>
              );
            })}
          </div>
          <p style={{ margin: "8px 0 0", color: "var(--sv-text-faint)", fontSize: 10.5 }}>Orange is what one deployable incumbent solves (exploitation). Gray extension is the optimistic union ever solved by retained frontier members (exploration); it is not a deployable score.</p>
        </div>
      ) : null}
      <div style={{ border: "1px solid var(--sv-border)", borderRadius: 9, overflow: "hidden" }}>
        <div className="sv-gepa-frontier-grid" style={{ display: "grid", gap: 10, padding: "7px 10px", background: "var(--sv-surface-muted)", borderBottom: "1px solid var(--sv-border)", color: "var(--sv-text-faint)", fontSize: 9.5, textTransform: "uppercase", letterSpacing: ".07em" }}>
          <span>Candidate / selection credit</span><span>Per-example reward vector</span>
        </div>
        {rows.map((row) => {
          const palette = candidatePalette(row.candidate);
          return (
          <button
            key={row.id}
            type="button"
            onClick={() => onSelect?.(row.id)}
            data-testid={`frontier-point-${row.id}`}
            aria-pressed={row.id === selectedId}
            aria-label={`${candidateName(row.candidate)} · ${row.onFrontier ? "Pareto member" : "dominated"} · ${row.wins} winning example cells`}
            className="sv-gepa-frontier-grid"
            style={{ display: "grid", gap: 10, width: "100%", padding: "9px 10px", border: 0, borderLeft: `4px solid ${palette.color}`, borderBottom: "1px solid var(--sv-border)", background: row.id === selectedId ? palette.tint : "var(--sv-surface)", color: "var(--sv-text)", textAlign: "left", cursor: "pointer" }}
          >
            <span>
              <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
                <strong style={{ fontSize: 11.5 }}>{candidateName(row.candidate)}</strong>
                <span className="sv-chip" data-tone={row.onFrontier ? "ok" : frontierPending ? "live" : undefined}>{row.onFrontier ? "frontier" : frontierPending ? "scoring" : "dominated"}</span>
              </span>
              <span className="sv-mono" style={{ display: "block", marginTop: 4, color: "var(--sv-text-muted)", fontSize: 9.5 }}>
                {row.wins} best cells · mean {row.mean?.toFixed(3) ?? "—"}
              </span>
            </span>
            <span style={{ display: "grid", gridTemplateColumns: `repeat(${Math.max(1, allExamples.length)}, minmax(5px, 1fr))`, gap: 2, alignSelf: "center" }}>
              {allExamples.map((exampleId) => {
                const reward = row.scores.get(exampleId);
                const winner = reward != null && Math.abs(reward - (bestByExample.get(exampleId) ?? Number.POSITIVE_INFINITY)) <= Number.EPSILON;
                return <span key={exampleId} title={`${exampleId}: ${reward == null ? "missing" : reward.toFixed(3)}${winner ? " · best" : ""}`} style={{ height: 16, minWidth: 5, borderRadius: 2, border: "1px solid var(--sv-border)", background: reward == null ? "transparent" : cellColor(reward, winner), opacity: row.onFrontier ? 1 : .55 }} />;
              })}
            </span>
          </button>
          );
        })}
        {!rows.length ? <p style={{ margin: 0, padding: 12, color: "var(--sv-text-faint)", fontSize: 12 }}>The seed becomes frontier-eligible after its complete train evaluation.</p> : null}
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 12, marginTop: 7, color: "var(--sv-text-muted)", fontSize: 10.5 }}>
        <span><span style={{ display: "inline-block", width: 10, height: 10, borderRadius: 2, background: "var(--sv-accent)", marginRight: 4 }} />best on example</span>
        <span><span style={{ display: "inline-block", width: 10, height: 10, borderRadius: 2, background: "var(--sv-border-strong)", marginRight: 4 }} />positive, not best</span>
        <span><span style={{ display: "inline-block", width: 10, height: 10, borderRadius: 2, border: "1px solid var(--sv-border)", marginRight: 4 }} />zero / missing</span>
      </div>
      <p style={{ margin: "8px 0 0", color: "var(--sv-text-muted)", fontSize: 11.5 }}>
        Frontier coverage: {frontierCoverage}/{allExamples.length || "—"} examples solved by at least one retained candidate. This is distinct from the single best candidate's mean reward.
      </p>
      {pending.length ? (
        <p style={{ margin: "6px 0 0", color: "var(--sv-text-faint)", fontSize: 11 }}>
          {gepa.activity.terminal ? "Not train-evaluated before the run ended" : "Awaiting complete full-train vectors"}: {pending.map((candidate) => `${candidateName(candidate)} (${statusLabel(candidate.status)})`).join(", ")}.
        </p>
      ) : null}
    </section>
  );
}

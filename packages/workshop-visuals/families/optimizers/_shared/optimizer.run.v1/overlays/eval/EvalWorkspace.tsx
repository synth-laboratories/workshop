/**
 * Eval workspace on the shared optimizer chrome. One surface for the whole
 * measurement: plan → screen → prune → confirm → select, with the candidate
 * comparison as the centre of gravity and the trial matrix underneath it.
 *
 * Three invariants shape everything here:
 *   1. Orchestration completing is never presented as a promotion. The run
 *      status and the selection status are separate chips, always both shown.
 *   2. A metric no valid trial produced renders "—" and is counted as missing.
 *      A failed container is failed evidence, never a zero reward.
 *   3. A candidate is a row. The page never collapses the comparison into one
 *      aggregate number, because the comparison *is* the product.
 *
 * Styling comes from visuals/chrome/tokens.css. No literal sizes or colors.
 */

import type { ReactNode } from "react";
import { Identifier } from "../../../../../../chrome/Identifier.tsx";
import type { EvalState, OptimizerRun, ProjectedState } from "../../components/projectEvents.ts";
import {
  StageTimeline,
  WorkspaceHeader,
  type WorkspaceMetric
} from "../../components/workspace/WorkspaceChrome.tsx";
import {
  EVAL_TERMINAL_STATUSES,
  SELECTION_TONE,
  evalComparison,
  evalStages,
  trialCounts,
  trialsForStage,
  type EvalComparisonRow
} from "./model.ts";

/** Signed value with an explicit sign, so direction survives a screenshot. */
function signed(value: number | null | undefined, digits = 3): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "—";
  return `${value > 0 ? "+" : value < 0 ? "−" : "±"}${Math.abs(value).toFixed(digits)}`;
}

function fixed(value: number | null | undefined, digits = 3): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "—";
  return value.toFixed(digits);
}

function usd(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "—";
  return `$${value.toFixed(4)}`;
}

function shortDigest(digest: string): string {
  const hash = digest.includes(":") ? digest.slice(digest.indexOf(":") + 1) : digest;
  return `${digest.slice(0, digest.indexOf(":") + 1)}${hash.slice(0, 12)}`;
}

function runChip(status: string, paused: boolean): {
  text: string;
  tone?: "live" | "ok" | "bad" | "warn";
  dot: boolean;
} {
  if (status === "failed") return { text: "Failed", tone: "bad", dot: false };
  if (["cancelled", "canceled"].includes(status)) return { text: "Cancelled", tone: "warn", dot: false };
  if (EVAL_TERMINAL_STATUSES.includes(status)) return { text: "Completed", tone: "ok", dot: false };
  if (paused || status === "paused") return { text: "Paused", tone: "warn", dot: false };
  if (status === "queued") return { text: "Queued", tone: "warn", dot: false };
  return { text: "Running", tone: "live", dot: true };
}

function Panel({
  title,
  aside,
  children,
  testId
}: {
  title: string;
  aside?: ReactNode;
  children: ReactNode;
  testId?: string;
}) {
  return (
    <section className="sv-panel" aria-label={title} data-testid={testId}>
      <div className="sv-panel-head">
        <h4>{title}</h4>
        {aside ? <span className="sv-mono">{aside}</span> : null}
      </div>
      <div className="sv-panel-body">{children}</div>
    </section>
  );
}

/* ── Live Craftax evidence ─────────────────────────────────────────────── */

function LiveRolloutsPanel({ state }: { state: EvalState }) {
  if (state.rollouts.length === 0) return null;
  const hero = state.rollouts.find((rollout) => rollout.status === "running" && rollout.frame)
    ?? state.rollouts.find((rollout) => rollout.frame)
    ?? state.rollouts[0];
  const rewardValues = state.rollouts
    .map((rollout) => rollout.rewardTotal)
    .filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  const rewardMax = Math.max(0.000001, ...rewardValues.map((value) => Math.abs(value)));
  const achievementCounts = new Map<string, number>();
  for (const rollout of state.rollouts) {
    for (const achievement of rollout.achievements) {
      achievementCounts.set(achievement, (achievementCounts.get(achievement) ?? 0) + 1);
    }
  }
  const resources = Object.entries(hero.resources)
    .filter(([, value]) => value !== 0)
    .sort((a, b) => Math.abs(b[1]) - Math.abs(a[1]))
    .slice(0, 8);

  return (
    <Panel
      title="Live Craftax rollouts"
      aside={`${state.rollouts.filter((rollout) => rollout.status === "running").length} live · ${state.rollouts.length} seen`}
      testId="eval-craftax-live"
    >
      <div className="sv-craftax-live-grid">
        <figure className="sv-craftax-frame" data-testid="eval-craftax-frame">
          {hero.frame ? (
            <img
              src={hero.frame.dataUrl}
              alt={`Craftax world for seed ${hero.seed ?? "unknown"} at step ${hero.ply}`}
            />
          ) : (
            <div className="sv-craftax-frame-empty">Waiting for the first rendered engine frame…</div>
          )}
          <figcaption>
            <span className="sv-chip" data-tone={hero.status === "running" ? "live" : "ok"}>
              {hero.status}
            </span>
            <strong>seed {hero.seed ?? "?"}</strong>
            <span className="sv-mono">step {hero.ply}</span>
            <span className="sv-mono">reward {fixed(hero.rewardTotal, 4)}</span>
          </figcaption>
        </figure>

        <div className="sv-craftax-telemetry">
          <section>
            <h5 className="sv-subhead">Latest model decision</h5>
            <p className="sv-craftax-actions">
              {hero.actions.length ? hero.actions.join(" → ") : "waiting for actions"}
            </p>
            {hero.policyReason ? <p className="sv-note">{hero.policyReason}</p> : null}
          </section>
          <section>
            <h5 className="sv-subhead">Inventory</h5>
            <div className="sv-craftax-stats">
              {resources.length
                ? resources.map(([name, value]) => (
                    <span key={name}><strong>{value}</strong>{name.replaceAll("_", " ")}</span>
                  ))
                : <span className="sv-empty">No collected resources yet</span>}
            </div>
          </section>
          <section>
            <h5 className="sv-subhead">Achievement frequency</h5>
            <div className="sv-craftax-achievements" data-testid="eval-achievement-frequency">
              {[...achievementCounts.entries()].length
                ? [...achievementCounts.entries()]
                    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
                    .map(([name, count]) => (
                      <span key={name} className="sv-tag" data-tone="ok">
                        {name.replaceAll("_", " ")} ×{count}
                      </span>
                    ))
                : <span className="sv-empty">No achievements unlocked yet</span>}
            </div>
          </section>
        </div>
      </div>

      <section className="sv-craftax-distribution" data-testid="eval-reward-distribution">
        <div className="sv-rail-row">
          <h5 className="sv-subhead">Reward distribution</h5>
          <span className="sv-mono">{rewardValues.length} observed rollouts</span>
        </div>
        <div className="sv-craftax-bars" role="img" aria-label="Reward distribution across observed rollouts">
          {rewardValues.length
            ? rewardValues.map((value, index) => (
                <span key={index} title={`reward ${value}`}>
                  <i style={{ height: `${Math.max(4, Math.abs(value) / rewardMax * 100)}%` }} />
                  <small>{fixed(value, 3)}</small>
                </span>
              ))
            : <span className="sv-empty">Rewards appear as rollouts advance.</span>}
        </div>
      </section>

      <div className="sv-craftax-rollout-strip" data-testid="eval-rollout-strip">
        {state.rollouts.slice(0, 10).map((rollout) => (
          <article key={rollout.trialId} data-status={rollout.status}>
            {rollout.frame ? <img src={rollout.frame.dataUrl} alt="" aria-hidden="true" /> : null}
            <div>
              <strong>seed {rollout.seed ?? "?"}</strong>
              <span className="sv-mono">step {rollout.ply}</span>
              <span className="sv-mono">r {fixed(rollout.rewardTotal, 3)}</span>
              <small>{rollout.achievements.length} achievements</small>
            </div>
          </article>
        ))}
      </div>
    </Panel>
  );
}

/* ── Verdict ────────────────────────────────────────────────────────────── */

function VerdictPanel({ state, runStatus }: { state: EvalState; runStatus: string }) {
  const selection = state.selection;
  const terminal = EVAL_TERMINAL_STATUSES.includes(runStatus);
  if (!selection) {
    return (
      <Panel title="Selection" testId="eval-verdict">
        <p className="sv-empty">
          {terminal
            ? "The run reached a terminal state without issuing a selection decision."
            : "Scoring is still in progress. No candidate has been selected."}
        </p>
      </Panel>
    );
  }
  const tone = SELECTION_TONE[selection.status] ?? { tone: "warn" as const, label: selection.status };
  const winner = state.scorecards.find(
    (card) => card.candidateId === selection.winnerId && card.stage !== "screen"
  ) ?? state.scorecards.find((card) => card.candidateId === selection.winnerId);
  return (
    <Panel
      title="Selection"
      aside={selection.primaryMetric ? `primary ${selection.primaryMetric}` : undefined}
      testId="eval-verdict"
    >
      <div className="sv-rail-row">
        <span className="sv-chip" data-tone={tone.tone} data-testid="eval-selection-status">
          {tone.label}
        </span>
        {winner ? (
          <strong data-testid="eval-selection-winner">{winner.label}</strong>
        ) : (
          <span style={{ color: "var(--sv-text-muted)" }}>no winner</span>
        )}
      </div>
      <dl className="sv-kv">
        <dt>Paired lift</dt>
        <dd data-testid="eval-selection-lift">
          {signed(selection.lift)} <span className="sv-mono">/ required {signed(selection.minLift)}</span>
        </dd>
        <dt>Why</dt>
        <dd data-testid="eval-selection-reason">{selection.reason}</dd>
      </dl>
      {selection.status === "promoted" ? (
        <p className="sv-note" data-testid="eval-promotion-note">
          A promotion is a measurement, not an action. Nothing has been replaced.
        </p>
      ) : null}
    </Panel>
  );
}

/* ── Candidate comparison ───────────────────────────────────────────────── */

function ComparisonPanel({ state }: { state: EvalState }) {
  const rows = evalComparison(state);
  const primary = state.selection?.primaryMetric ?? state.scorecards[0]?.metrics[0]?.metric ?? "";
  if (rows.length === 0) {
    return (
      <Panel title="Candidates" testId="eval-comparison">
        <p className="sv-empty">No candidate has been scored yet.</p>
      </Panel>
    );
  }
  const stages = [...new Set(rows.map((row) => row.stage))];
  return (
    <Panel title="Candidates" aside={primary ? `by ${primary}` : undefined} testId="eval-comparison">
      {stages.map((stage) => (
        <div key={stage} data-testid={`eval-comparison-stage-${stage}`}>
          <h5 className="sv-subhead">{stage}</h5>
          <table className="sv-table">
            <thead>
              <tr>
                <th scope="col">Candidate</th>
                <th scope="col">{primary || "primary"}</th>
                <th scope="col">Lift</th>
                <th scope="col">Valid</th>
                <th scope="col">Failed</th>
                <th scope="col">Played</th>
                <th scope="col">Cost</th>
              </tr>
            </thead>
            <tbody>
              {rows
                .filter((row) => row.stage === stage)
                .map((row: EvalComparisonRow) => (
                  <tr
                    key={row.key}
                    data-testid={`eval-candidate-${row.candidateId}-${row.stage}`}
                    data-eliminated={row.eliminationReason ? "true" : undefined}
                    data-winner={row.isWinner ? "true" : undefined}
                  >
                    <th scope="row">
                      {row.label}
                      {row.isBaseline ? <span className="sv-tag">baseline</span> : null}
                      {row.isWinner ? <span className="sv-tag" data-tone="ok">winner</span> : null}
                      {row.eliminationReason ? (
                        <span className="sv-tag" data-tone="warn" title={row.eliminationReason}>
                          eliminated
                        </span>
                      ) : null}
                    </th>
                    <td className="sv-mono">{fixed(row.primary)}</td>
                    <td className="sv-mono" data-direction={row.lift == null ? "unknown" : row.lift > 0 ? "up" : row.lift < 0 ? "down" : "flat"}>
                      {row.isBaseline ? "—" : `${signed(row.lift)}`}
                      {!row.isBaseline && row.pairedTrials > 0 ? (
                        <span className="sv-hint"> ({row.pairedTrials} paired)</span>
                      ) : null}
                    </td>
                    <td className="sv-mono">{row.valid}</td>
                    <td className="sv-mono" data-tone={row.failed > 0 ? "bad" : undefined}>{row.failed}</td>
                    {/*
                      Share of the scored episodes the candidate's own policy
                      chose. Anything short of 100% means a fallback finished
                      the episode and part of the score above is not the
                      candidate's, so it is flagged rather than left to the
                      reader to infer from the call count.
                    */}
                    <td
                      className="sv-mono"
                      data-tone={
                        row.policyStepFraction != null && row.policyStepFraction < 1
                          ? "warn"
                          : undefined
                      }
                    >
                      {row.policyStepFraction == null
                        ? "—"
                        : `${Math.round(row.policyStepFraction * 100)}%`}
                      {row.budgetExhaustedTrials > 0 ? (
                        <span
                          className="sv-tag"
                          data-tone="warn"
                          title={`${row.budgetExhaustedTrials} of ${row.valid} scored trials ran out of budget; a fallback policy finished the episode`}
                        >
                          {row.budgetExhaustedTrials} capped
                        </span>
                      ) : null}
                    </td>
                    <td className="sv-mono">{usd(row.costUsd)}</td>
                  </tr>
                ))}
            </tbody>
          </table>
        </div>
      ))}
    </Panel>
  );
}

/* ── Trial matrix ───────────────────────────────────────────────────────── */

function MatrixPanel({ state }: { state: EvalState }) {
  const counts = trialCounts(state);
  const stages = [...new Set(state.trials.map((trial) => trial.stage ?? ""))].filter(Boolean);
  return (
    <Panel
      title="Trial matrix"
      aside={`${counts.terminal}/${counts.planned || counts.terminal} terminal`}
      testId="eval-matrix"
    >
      {state.trials.length === 0 ? (
        <p className="sv-empty">No trial has been dispatched yet.</p>
      ) : (
        stages.map((stage) => {
          const trials = trialsForStage(state, stage);
          const byCandidate = new Map<string, typeof trials>();
          for (const trial of trials) {
            const key = trial.candidateId ?? "unknown";
            byCandidate.set(key, [...(byCandidate.get(key) ?? []), trial]);
          }
          return (
            <div key={stage} data-testid={`eval-matrix-stage-${stage}`}>
              <h5 className="sv-subhead">{stage}</h5>
              {[...byCandidate.entries()].map(([candidateId, rows]) => {
                const label = state.candidates.find((c) => c.id === candidateId)?.label ?? candidateId;
                return (
                  <div key={candidateId} className="sv-matrix-row">
                    <span className="sv-matrix-label">{label}</span>
                    <span className="sv-matrix-cells">
                      {rows
                        .slice()
                        .sort((a, b) => (a.seed ?? 0) - (b.seed ?? 0))
                        .map((trial) => (
                          <span
                            key={trial.id}
                            className="sv-matrix-cell"
                            data-status={trial.status}
                            data-valid={trial.valid ? "true" : "false"}
                            data-testid={`eval-trial-${trial.id}`}
                            title={
                              `seed ${trial.seed ?? "?"} · ${trial.status}` +
                              (trial.benchmarkStatus ? ` · ${trial.benchmarkStatus}` : "") +
                              (trial.missingArtifacts.length
                                ? ` · missing ${trial.missingArtifacts.join(", ")}`
                                : "")
                            }
                          >
                            {trial.seed ?? "?"}
                          </span>
                        ))}
                    </span>
                  </div>
                );
              })}
            </div>
          );
        })
      )}
      {counts.failed > 0 ? (
        <p className="sv-note" data-tone="bad" data-testid="eval-failed-note">
          {counts.failed} trial{counts.failed === 1 ? "" : "s"} produced no usable evidence. They are
          retained as failures and excluded from scoring — not counted as zero.
        </p>
      ) : null}
    </Panel>
  );
}

/* ── Evidence ───────────────────────────────────────────────────────────── */

function EvidencePanel({ state }: { state: EvalState }) {
  const ledger = state.seedLedger;
  return (
    <Panel title="Sealed evidence" testId="eval-evidence">
      <dl className="sv-kv">
        {state.candidateSetId ? (
          <>
            <dt>Candidate set</dt>
            <dd><Identifier value={state.candidateSetId} /></dd>
          </>
        ) : null}
        {state.manifestDigest ? (
          <>
            <dt>Input manifest</dt>
            <dd className="sv-mono" data-testid="eval-manifest-digest">
              {shortDigest(state.manifestDigest)}
            </dd>
          </>
        ) : null}
        {ledger ? (
          <>
            <dt>Screening seeds</dt>
            <dd className="sv-mono">{ledger.screening.join(", ") || "—"}</dd>
            <dt>Confirmation seeds</dt>
            <dd className="sv-mono">{ledger.confirmation.join(", ") || "none"}</dd>
            <dt>Scenarios</dt>
            <dd className="sv-mono">{ledger.scenarios.join(", ") || "—"}</dd>
          </>
        ) : null}
        {state.evidenceDir ? (
          <>
            <dt>Evidence</dt>
            <dd><Identifier value={state.evidenceDir} /></dd>
          </>
        ) : null}
      </dl>
      {ledger && ledger.confirmation.some((seed) => ledger.screening.includes(seed)) ? (
        <p className="sv-note" data-tone="bad">
          Confirmation seeds overlap screening seeds; this comparison is not held out.
        </p>
      ) : null}
    </Panel>
  );
}

/* ── Shell ──────────────────────────────────────────────────────────────── */

export type AnalysisCampaign = {
  campaignId?: string;
  status?: string;
  label?: string;
  domain?: string;
  coverage?: { jobs?: number; sealed?: number; abstained?: number; failed?: number };
};

function AnnotationCampaignPanel({ campaigns }: { campaigns: AnalysisCampaign[] }) {
  if (campaigns.length === 0) return null;
  return (
    <Panel title="Annotation campaigns" aside={`${campaigns.length} campaign${campaigns.length === 1 ? "" : "s"}`} testId="eval-annotation-campaigns">
      <ul className="sv-stack" style={{ listStyle: "none", margin: 0, padding: 0, gap: 8 }}>
        {campaigns.map((campaign) => {
          const jobs = campaign.coverage?.jobs ?? 0;
          const sealed = campaign.coverage?.sealed ?? 0;
          const status = campaign.status ?? "submitted";
          const progress =
            status === "running" && jobs > 0
              ? `annotating ${sealed}/${jobs}`
              : status === "submitted"
                ? jobs > 0
                  ? `submitted ${jobs}`
                  : "submitted"
                : `${status} · ${sealed}/${jobs} sealed`;
          return (
            <li key={campaign.campaignId ?? campaign.label ?? "campaign"} data-testid={`eval-annotation-campaign-${campaign.campaignId ?? "unknown"}`}>
              <strong>{campaign.label ?? campaign.domain ?? campaign.campaignId ?? "campaign"}</strong>
              {" · "}
              <span className="sv-mono">{progress}</span>
            </li>
          );
        })}
      </ul>
    </Panel>
  );
}

export function EvalWorkspace({
  projected,
  run,
  debug,
  analysisCampaigns = []
}: {
  projected: ProjectedState;
  run: OptimizerRun;
  debug?: ReactNode;
  analysisCampaigns?: AnalysisCampaign[];
}) {
  const state = projected.eval;
  const status = String(projected.summary.status ?? run.status ?? "running");
  const paused = state?.paused === true;
  const chip = runChip(status, paused);
  const counts = state
    ? trialCounts(state)
    : { planned: 0, terminal: 0, valid: 0, failed: 0, running: 0, queued: 0 };
  const selection = state?.selection;
  const selectionTone = selection ? SELECTION_TONE[selection.status] : undefined;

  const metrics: WorkspaceMetric[] = [
    { label: "Trials", value: `${counts.terminal}/${counts.planned || counts.terminal}` },
    { label: "Valid", value: String(counts.valid) },
    { label: "Failed", value: String(counts.failed), title: "Trials with no usable evidence" },
    { label: "Running", value: String(counts.running) },
    {
      label: "Cost",
      value: usd(projected.usage.cost_usd ?? null),
      title: "Provider-reported tokens at the recipe's declared rate"
    },
    {
      label: "Selection",
      value: selectionTone?.label ?? "—",
      title: selection?.reason
    }
  ];

  return (
    <div className="sv-workspace sv-stack" data-testid="eval-workspace">
      <WorkspaceHeader
        statusText={chip.text}
        statusTone={chip.tone}
        live={chip.dot}
        headline={String(projected.summary.objective ?? run.objective ?? "Candidate evaluation")}
        // Orchestration and promotion are different claims and always both shown.
        detail={
          selection
            ? `run ${chip.text.toLowerCase()} · selection ${selection.status}`
            : `run ${chip.text.toLowerCase()} · selection pending`
        }
        metrics={metrics}
        testId="eval-workspace-header"
      />
      <StageTimeline stages={evalStages(state, status)} testId="eval-stages" />
      <AnnotationCampaignPanel campaigns={analysisCampaigns} />
      {state ? (
        <>
          <LiveRolloutsPanel state={state} />
          <VerdictPanel state={state} runStatus={status} />
          <ComparisonPanel state={state} />
          <MatrixPanel state={state} />
          <EvidencePanel state={state} />
        </>
      ) : (
        <p className="sv-empty">
          This run has emitted no eval events yet. The matrix appears once the worker seals its
          input manifest.
        </p>
      )}
      {debug ? <details><summary className="sv-micro-label">Raw run data</summary>{debug}</details> : null}
    </div>
  );
}

export default EvalWorkspace;

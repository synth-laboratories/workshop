/**
 * SFT workspace on the shared optimizer chrome. Covers the full uplift
 * experiment as one linked surface: baseline → collection/curation → dataset →
 * training → checkpoints → campaigns → promotion → paired heldout comparison.
 *
 * Two invariants shape everything here:
 *   1. A ready checkpoint is never presented as promoted, and training success
 *      is never presented as uplift. Only the paired heldout comparison can
 *      license an uplift claim.
 *   2. Missing measurements render as "—" and are counted as missing. Nothing
 *      is imputed as zero, because an unmeasured rollout is not a failed one.
 *
 * Styling comes from visuals/chrome/tokens.css. No literal sizes or colors.
 */

import { useMemo } from "react";
import type { ReactNode } from "react";
import { Identifier } from "../../../../../../chrome/Identifier.tsx";
import { formatMissingNumber, formatMissingUsd } from "../../../../../../runtime/liveStream.ts";
import { optimizerFailureDetail } from "../../components/projectEvents.ts";
import type { OptimizerRun, ProjectedState } from "../../components/projectEvents.ts";
import {
  counted,
  NotEnoughData,
  StageTimeline,
  WorkspaceHeader,
  type WorkspaceMetric
} from "../../components/workspace/WorkspaceChrome.tsx";
import { RolloutBrowser, type RolloutGroup, type RolloutRow } from "../../components/workspace/RolloutBrowser.tsx";
import {
  SFT_TERMINAL_STATUSES,
  sftAggregateBaseline,
  sftComparison,
  sftCurationFunnel,
  sftDistinctEvaluations,
  sftDistribution,
  sftHeldoutSummary,
  sftEffectiveStatus,
  sftMissingPrerequisites,
  sftStages,
  type SftComparison,
  type SftHeldoutSummary,
  type SftPrerequisite,
  type SftState
} from "./model.ts";

const TERMINAL_STATUSES = SFT_TERMINAL_STATUSES;

function statusChip(
  status: string,
  verdict?: string,
  claimReady = false
): { text: string; tone?: "live" | "ok" | "bad" | "warn"; dot: boolean } {
  if (status === "failed") return { text: "Failed", tone: "bad", dot: false };
  if (["canceled", "cancelled"].includes(status)) return { text: "Canceled", tone: "warn", dot: false };
  if (TERMINAL_STATUSES.includes(status)) {
    if (verdict === "improvement_demonstrated" || claimReady) {
      return { text: "Completed · improvement demonstrated", tone: "ok", dot: false };
    }
    if (verdict === "inconclusive") {
      return { text: "Completed · evaluation inconclusive", tone: "warn", dot: false };
    }
    return { text: "Completed · no measured improvement", tone: "warn", dot: false };
  }
  if (status === "queued") return { text: "Queued", tone: "warn", dot: false };
  if (["created", "pending", "loading"].includes(status)) {
    return { text: status[0].toUpperCase() + status.slice(1), dot: false };
  }
  return { text: "Running", tone: "live", dot: true };
}

/** Signed value with an explicit sign, so direction survives a screenshot. */
function signed(value: number | null | undefined, digits = 2): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "—";
  return `${value > 0 ? "+" : value < 0 ? "−" : "±"}${Math.abs(value).toFixed(digits)}`;
}

/** Show a digest by its hash, not by its algorithm prefix. */
function shortDigest(digest: string): string {
  const hash = digest.includes(":") ? digest.slice(digest.indexOf(":") + 1) : digest;
  return `${digest.slice(0, digest.indexOf(":") + 1)}${hash.slice(0, 12)}`;
}

function percent(value: number | null | undefined, digits = 0): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "—";
  return `${(value * 100).toFixed(digits)}%`;
}

/**
 * A difference between two rates, in percentage points.
 *
 * The heldout panel reported the same accuracy three ways: the selection score
 * as `80.3%`, the base and promoted arms as `0.80` and `0.82`, and the uplift
 * between those arms as `+0.02`. A reader then has to know that the `+0.02`
 * under two percentages means two points, and that it is not two percent.
 *
 * So rates render as rates and their differences render in points. The unit is
 * spelled out because "+2%" and "+2 pp" mean different things and only one of
 * them is true here. Reward means keep their own raw scale -- they are not
 * rates, and `signed` still serves them.
 */
function points(value: number | null | undefined, digits = 1): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "—";
  const sign = value > 0 ? "+" : value < 0 ? "−" : "±";
  return `${sign}${Math.abs(value * 100).toFixed(digits)} pp`;
}

function direction(value: number | null | undefined): "up" | "down" | "flat" | "unknown" {
  if (typeof value !== "number" || !Number.isFinite(value)) return "unknown";
  if (value > 0) return "up";
  if (value < 0) return "down";
  return "flat";
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

/* ── Phase A · baseline ─────────────────────────────────────────────────── */

function BaselinePanel({ sft, isCispo }: { sft: SftState; isCispo?: boolean }) {
  const baseline = sft.baseline;
  const aggregate = sftAggregateBaseline(sft);
  const distribution = useMemo(
    () => sftDistribution((baseline?.seeds ?? []).map((seed) => seed.reward)),
    [baseline]
  );
  return (
    <Panel
      title={isCispo ? "Baseline — policy before CISPO" : "Baseline — unchanged student"}
      aside={baseline?.splitDigest ? `split ${shortDigest(baseline.splitDigest)}` : undefined}
      testId={isCispo ? "cispo-baseline" : "sft-baseline"}
    >
      {(!baseline || baseline.seeds.length === 0) && !aggregate ? (
        <p className="sv-empty">No baseline evaluation has been emitted yet.</p>
      ) : baseline && baseline.seeds.length > 0 ? (
        <>
          <dl className="sv-kv">
            <dt>Seeds scored</dt>
            <dd>
              {distribution.scored} / {distribution.n}
              {distribution.missing > 0 ? ` · ${distribution.missing} missing` : ""}
            </dd>
            <dt>Mean reward</dt><dd>{formatMissingNumber(distribution.mean)}</dd>
            <dt>Median</dt><dd>{formatMissingNumber(distribution.median)}</dd>
            <dt>Std. deviation</dt><dd>{formatMissingNumber(distribution.sd)}</dd>
            <dt>Range</dt>
            <dd>{formatMissingNumber(distribution.min)} … {formatMissingNumber(distribution.max)}</dd>
          </dl>
          <details>
            <summary className="sv-mono">Per-seed baseline rollouts</summary>
            <table className="sv-table">
              <thead>
                <tr>
                  <th scope="col">Seed</th><th scope="col">Reward</th>
                  <th scope="col">Steps</th><th scope="col">Achievements</th><th scope="col">Rollout</th>
                </tr>
              </thead>
              <tbody>
                {baseline.seeds.map((seed) => (
                  <tr key={seed.seed}>
                    <td className="sv-mono">{seed.seed}</td>
                    <td className="sv-mono">{formatMissingNumber(seed.reward)}</td>
                    <td className="sv-mono">{formatMissingNumber(seed.steps, 0)}</td>
                    <td className="sv-mono">{seed.achievements?.length ?? "—"}</td>
                    <td>{seed.rolloutId ? <Identifier value={seed.rolloutId} max={22} /> : "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </details>
        </>
      ) : aggregate ? (
        /* `sftAggregateBaseline` only matches a `selection`-role record, so this
           is the unchanged policy on the split the run uses to CHOOSE a
           checkpoint. The heldout panel scores that same unchanged policy on the
           locked heldout split, and the two disagree by ordinary split-to-split
           variation — the 2026-09-02 Banking77 SFT run read 79.50% on selection
           and 81.25% on heldout, 400 examples each. Naming the split on both
           surfaces is what stops a reader treating that gap as a reporting error
           and discarding the uplift claim that rests on the heldout arm. */
        <dl className="sv-kv">
          <dt>Split</dt><dd>selection · not the locked heldout split</dd>
          <dt>Examples scored</dt><dd>{formatMissingNumber(aggregate.n, 0)}</dd>
          <dt>Selection {aggregate.metric}</dt><dd>{percent(aggregate.score, 1)}</dd>
          <dt>Policy</dt><dd>unchanged base</dd>
          <dt>Checkpoint</dt>
          <dd>{aggregate.checkpointId ? <Identifier value={aggregate.checkpointId} max={28} /> : "—"}</dd>
        </dl>
      ) : null}
    </Panel>
  );
}

/* ── Phases B/C · collection and curation ───────────────────────────────── */

function CurationPanel({ sft }: { sft: SftState }) {
  const funnel = useMemo(() => sftCurationFunnel(sft), [sft]);
  const hasAnything = funnel.steps.some((step) => step.count != null) || funnel.accepted.length > 0;
  const hasVersionedDataset = typeof sft.dataset.digest === "string" && sft.dataset.digest.length > 0;
  const widest = Math.max(1, ...funnel.steps.map((step) => step.count ?? 0));
  return (
    <Panel
      title="Collection & curation"
      aside={funnel.acceptanceRate != null ? `${percent(funnel.acceptanceRate)} retained` : undefined}
      testId="sft-curation"
    >
      {!hasAnything ? (
        <p className="sv-empty">
          {hasVersionedDataset
            ? "Direct supervised corpus supplied with a durable dataset digest; teacher collection and curation are not part of this recipe."
            : "No teacher collection or curation has been reported yet."}
        </p>
      ) : (
        <>
          <div className="sv-stack-tight sv-stack">
            {funnel.steps.map((step) => (
              <div key={step.id} className="sv-stack-tight sv-stack">
                <div className="sv-legend">
                  <span>{step.label}</span>
                  <span className="sv-mono" style={{ marginLeft: "auto" }}>
                    {step.count == null ? "not reported" : step.count}
                  </span>
                </div>
                <div className="sv-bar" role="img" aria-label={`${step.label}: ${step.count ?? "not reported"}`}>
                  <span data-tone="accent" style={{ width: `${((step.count ?? 0) / widest) * 100}%` }} />
                </div>
              </div>
            ))}
          </div>

          {funnel.achievementsCovered.length > 0 ? (
            <div className="sv-stack-tight sv-stack">
              <span className="sv-micro-label">Achievement coverage in retained set</span>
              <div className="sv-coverage">
                {funnel.achievementsCovered.map((name) => <code key={name}>{name}</code>)}
              </div>
            </div>
          ) : null}

          {funnel.seedsCovered != null ? (
            <p className="sv-mono">{funnel.seedsCovered} distinct collection seeds represented</p>
          ) : null}

          {funnel.topRejections.length > 0 ? (
            <div className="sv-stack-tight sv-stack">
              <span className="sv-micro-label">Why candidates were rejected</span>
              <table className="sv-table">
                <thead><tr><th scope="col">Reason</th><th scope="col">Count</th></tr></thead>
                <tbody>
                  {funnel.topRejections.map((row) => (
                    <tr key={row.reason}>
                      <td>{row.reason}</td>
                      <td className="sv-mono">{row.count}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : null}

          {funnel.accepted.length + funnel.rejected.length > 0 ? (
            <details data-testid="sft-curation-candidates">
              <summary className="sv-mono">
                Inspect {funnel.accepted.length} accepted · {funnel.rejected.length} rejected
              </summary>
              <table className="sv-table">
                <thead>
                  <tr>
                    <th scope="col">Candidate</th><th scope="col">Seed</th><th scope="col">Reward</th>
                    <th scope="col">Score</th><th scope="col">Decision</th><th scope="col">Reason</th>
                  </tr>
                </thead>
                <tbody>
                  {[...funnel.accepted, ...funnel.rejected].map((candidate) => (
                    <tr key={candidate.id}>
                      <td><Identifier value={candidate.id} max={22} /></td>
                      <td className="sv-mono">{candidate.seed ?? "—"}</td>
                      <td className="sv-mono">{formatMissingNumber(candidate.reward)}</td>
                      <td className="sv-mono">{formatMissingNumber(candidate.score)}</td>
                      <td>
                        <span className="sv-chip" data-tone={candidate.accepted ? "ok" : undefined}>
                          {candidate.accepted ? "Accepted" : "Rejected"}
                        </span>
                      </td>
                      <td>{candidate.reason ?? "—"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </details>
          ) : null}
        </>
      )}
    </Panel>
  );
}

/* ── Phase E · training ─────────────────────────────────────────────────── */

function CurvesPanel({
  sft,
  metricSeries
}: {
  sft: SftState;
  metricSeries?: { status: string; total?: number; error?: string };
}) {
  const points = sft.points;
  const maxStep = Math.max(1, ...points.map((point) => point.step));
  const losses = points.flatMap((point) =>
    [point.trainLoss, point.validationLoss].filter((value): value is number => typeof value === "number")
  );
  const maxLoss = Math.max(1e-6, ...losses);
  const x = (step: number) => 40 + (step / maxStep) * 340;
  const y = (loss: number) => 128 - (Math.min(loss, maxLoss) / maxLoss) * 104;
  const path = (key: "trainLoss" | "validationLoss") => points
    .filter((point) => typeof point[key] === "number")
    .map((point, index) => `${index === 0 ? "M" : "L"} ${x(point.step).toFixed(1)} ${y(point[key] as number).toFixed(1)}`)
    .join(" ");
  const latest = points.at(-1);
  return (
    <Panel
      title="Training curves"
      aside={counted(metricSeries?.total ?? points.length, "durable record")}
      testId="sft-live-curves"
    >
      {metricSeries && metricSeries.status === "error" ? (
        <p className="sv-callout" data-tone="warn" role="status">
          Detailed metric series unavailable; showing the latest projected step. {metricSeries.error ?? ""}
        </p>
      ) : null}
      {points.length === 0 ? (
        <p className="sv-empty">Loss metrics stream here once the training job reports its first step.</p>
      ) : losses.length === 0 ? (
        <NotEnoughData
          have={0}
          need={2}
          noun="loss sample"
          detail={`${counted(points.length, "durable step record")} reached step ${latest?.step ?? "—"}, but the provider did not emit train or validation loss.`}
          testId="sft-curves-loss-unavailable"
        />
      ) : points.length < 2 ? (
        // One dot on a full pair of axes reads as a broken chart. State the
        // sample instead, and switch to the plot when a trend exists.
        <NotEnoughData
          have={points.length}
          need={2}
          noun="metric sample"
          detail={latest ? `step ${latest.step} · train ${formatMissingNumber(latest.trainLoss)} · val ${formatMissingNumber(latest.validationLoss)}` : undefined}
          testId="sft-curves-single-sample"
        />
      ) : (
        <>
          <svg viewBox="0 0 400 150" width="100%" role="img" aria-label="Train and validation loss by step">
            {[0, 0.5, 1].map((tick) => (
              <g key={tick}>
                <line x1={40} y1={128 - tick * 104} x2={380} y2={128 - tick * 104} stroke="var(--sv-border)" />
                <text x={34} y={131 - tick * 104} textAnchor="end" fontSize="8" fill="var(--sv-text-faint)">
                  {(tick * maxLoss).toFixed(2)}
                </text>
              </g>
            ))}
            <text x={210} y={146} textAnchor="middle" fontSize="9" fill="var(--sv-text-muted)">step (max {maxStep})</text>
            {path("trainLoss") ? <path d={path("trainLoss")} fill="none" stroke="var(--sv-accent)" strokeWidth="1.8" /> : null}
            {path("validationLoss") ? <path d={path("validationLoss")} fill="none" stroke="var(--sv-series-b)" strokeWidth="1.8" strokeDasharray="4 3" /> : null}
            {points.map((point) => (
              <g key={point.step}>
                {typeof point.trainLoss === "number" ? <circle cx={x(point.step)} cy={y(point.trainLoss)} r={3} fill="var(--sv-accent)" /> : null}
                {typeof point.validationLoss === "number" ? <circle cx={x(point.step)} cy={y(point.validationLoss)} r={3} fill="var(--sv-series-b)" /> : null}
              </g>
            ))}
          </svg>
          <div className="sv-legend" aria-hidden="true">
            <span><i className="sv-swatch" style={{ background: "var(--sv-accent)" }} />train loss</span>
            <span><i className="sv-swatch" style={{ background: "var(--sv-series-b)" }} />validation loss (dashed)</span>
          </div>
          <details>
            <summary className="sv-mono">Per-step records</summary>
            <table className="sv-table">
              <thead>
                <tr><th scope="col">Step</th><th scope="col">Epoch</th><th scope="col">Train</th><th scope="col">Val</th><th scope="col">LR</th></tr>
              </thead>
              <tbody>
                {points.map((point) => (
                  <tr key={point.step}>
                    <td className="sv-mono">{point.step}</td>
                    <td className="sv-mono">{formatMissingNumber(point.epoch, 0)}</td>
                    <td className="sv-mono">{formatMissingNumber(point.trainLoss)}</td>
                    <td className="sv-mono">{formatMissingNumber(point.validationLoss)}</td>
                    <td className="sv-mono">{typeof point.learningRate === "number" ? point.learningRate.toExponential(1) : "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </details>
        </>
      )}
    </Panel>
  );
}

function CheckpointRail({ sft, promotedCheckpointId }: { sft: SftState; promotedCheckpointId?: string }) {
  return (
    <Panel title="Checkpoints" aside={String(sft.checkpoints.length)} testId="sft-checkpoint-rail">
      {sft.checkpoints.length === 0 ? (
        <p className="sv-empty">Checkpoints appear as training emits them.</p>
      ) : (
        <div role="list" className="sv-stack-tight sv-stack">
          {sft.checkpoints.map((ckpt) => {
            const id = String(ckpt.id ?? "");
            const claimed = ckpt.promoted === true;
            const selected = ckpt.selected === true || claimed || id === promotedCheckpointId;
            const ready = ckpt.ready === true;
            return (
              <div key={id} role="listitem" className="sv-rail-row" data-promoted={claimed}>
                <Identifier value={id} max={30} />
                {ckpt.step != null ? <span className="sv-mono">step {String(ckpt.step)}</span> : null}
                <span className="sv-rail-row-end">
                  <span className="sv-chip" data-tone={ready ? "ok" : undefined}>
                    {ready ? "Ready" : String(ckpt.status ?? "created")}
                  </span>
                  <span
                    className="sv-chip"
                    data-tone={claimed ? "ok" : selected ? "warn" : undefined}
                    title="Selection is not uplift. A green Promoted chip requires improvement_demonstrated."
                  >
                    {claimed ? "Promoted · uplift claimed" : selected ? "Selected" : "Not selected"}
                  </span>
                </span>
              </div>
            );
          })}
        </div>
      )}
    </Panel>
  );
}

/* ── Phase G · paired heldout comparison ────────────────────────────────── */

function ComparisonPanel({
  comparison,
  aggregate
}: {
  comparison: SftComparison | null;
  aggregate: SftHeldoutSummary | null;
}) {
  if (!comparison && aggregate) {
    // Both arms here live on the locked heldout split, and the base arm is
    // reconstructed as trained − uplift because services report the selected
    // checkpoint's score and the paired delta but never the base score itself.
    // It is therefore a different measurement from the baseline panel's number,
    // which is the same unchanged policy on the selection split. Each is
    // authoritative for its own split; only this pair can license an uplift
    // claim, and leaving them unlabelled invites a reader to read the gap as a
    // contradiction and distrust the claim.
    return (
      <Panel
        title="Heldout comparison — base vs selected"
        aside="locked heldout split"
        testId="sft-comparison"
      >
        <div className="sv-arms">
          <div className="sv-arm" data-arm="base">
            <span className="sv-micro-label">unchanged base · heldout</span>
            <strong>{percent(aggregate.baseScore, 1)}</strong>
          </div>
          <div className="sv-arm" data-arm="trained">
            <span className="sv-micro-label">{aggregate.checkpointId ?? "selected checkpoint"}</span>
            <strong>{percent(aggregate.trainedScore, 1)}</strong>
          </div>
        </div>
        <dl className="sv-kv" data-testid="sft-uplift">
          <dt>Paired examples</dt><dd>{aggregate.paired}</dd>
          <dt>Accuracy uplift</dt>
          <dd><span className="sv-delta" data-dir={direction(aggregate.absoluteUplift)}>{points(aggregate.absoluteUplift)}</span></dd>
          <dt>95% paired CI</dt>
          <dd>{aggregate.upliftCi ? `${points(aggregate.upliftCi[0])} … ${points(aggregate.upliftCi[1])}` : "—"}</dd>
          <dt>Verdict</dt><dd>{aggregate.verdict ?? "not reported"}</dd>
          <dt>Uplift claim</dt><dd>{aggregate.claimReady ? "supported" : "not established"}</dd>
        </dl>
        <p className="sv-note">
          Both arms are scored on the locked heldout split, and the base arm is
          derived as the selected checkpoint's score minus the reported paired
          uplift rather than measured on its own. The baseline panel reports the
          same unchanged policy on the selection split, so the two base numbers
          differ; neither one corrects the other.
        </p>
      </Panel>
    );
  }
  if (!comparison) {
    return (
      <Panel title="Heldout comparison — base vs promoted" testId="sft-comparison">
        <p className="sv-empty">
          No paired heldout evaluation has been emitted, so <strong>no uplift is claimed</strong>.
        </p>
      </Panel>
    );
  }
  const dir = direction(comparison.absoluteUplift);
  const decided = comparison.wins + comparison.losses + comparison.ties;
  const share = (count: number) => (decided === 0 ? 0 : (count / decided) * 100);
  return (
    <Panel
      title="Heldout comparison — base vs promoted"
      aside={comparison.splitDigest ? `split ${shortDigest(comparison.splitDigest)}` : undefined}
      testId="sft-comparison"
    >
      <div className="sv-arms">
        <div className="sv-arm" data-arm="base">
          <span className="sv-micro-label">{comparison.baseLabel}</span>
          <strong>{formatMissingNumber(comparison.baseMean)}</strong>
          <span className="sv-mono">
            median {formatMissingNumber(comparison.baseMedian)} · sd {formatMissingNumber(comparison.baseSd)}
          </span>
          <span className="sv-mono">success {percent(comparison.baseSuccessRate)}</span>
        </div>
        <div className="sv-arm" data-arm="trained">
          <span className="sv-micro-label">{comparison.trainedLabel}</span>
          <strong>{formatMissingNumber(comparison.trainedMean)}</strong>
          <span className="sv-mono">
            median {formatMissingNumber(comparison.trainedMedian)} · sd {formatMissingNumber(comparison.trainedSd)}
          </span>
          <span className="sv-mono">success {percent(comparison.trainedSuccessRate)}</span>
        </div>
      </div>

      <dl className="sv-kv" data-testid="sft-uplift">
        <dt>Paired seeds</dt>
        <dd>
          {comparison.paired}
          {comparison.unpaired > 0 ? ` · ${comparison.unpaired} unpaired (excluded, not zeroed)` : ""}
        </dd>
        <dt>Mean uplift</dt>
        <dd>
          <span className="sv-delta" data-dir={dir}>{signed(comparison.absoluteUplift)}</span>
        </dd>
        <dt>Relative</dt><dd>{percent(comparison.relativeUplift, 1)}</dd>
        <dt>95% CI of the paired difference</dt>
        <dd>
          {comparison.upliftCi
            ? `${signed(comparison.upliftCi[0])} … ${signed(comparison.upliftCi[1])}`
            : "— (needs ≥2 paired seeds)"}
        </dd>
        <dt>Mean episode length</dt>
        <dd>
          {formatMissingNumber(comparison.baseMeanSteps, 0)} → {formatMissingNumber(comparison.trainedMeanSteps, 0)}
        </dd>
      </dl>

      {decided > 0 ? (
        <div className="sv-stack-tight sv-stack">
          <span className="sv-micro-label">Paired per-seed outcomes</span>
          <div
            className="sv-bar"
            role="img"
            aria-label={`${comparison.wins} wins, ${comparison.ties} ties, ${comparison.losses} losses`}
          >
            <span data-tone="ok" style={{ width: `${share(comparison.wins)}%` }} />
            <span data-tone="flat" style={{ width: `${share(comparison.ties)}%` }} />
            <span data-tone="bad" style={{ width: `${share(comparison.losses)}%` }} />
          </div>
          <div className="sv-legend">
            <span><i className="sv-swatch" style={{ background: "var(--sv-ok-fg)" }} />{comparison.wins} trained wins</span>
            <span><i className="sv-swatch" style={{ background: "var(--sv-border-strong)" }} />{comparison.ties} ties</span>
            <span><i className="sv-swatch" style={{ background: "var(--sv-bad-fg)" }} />{comparison.losses} base wins</span>
          </div>
        </div>
      ) : null}

      {comparison.achievementsGained.length + comparison.achievementsLost.length > 0 ? (
        <div className="sv-stack-tight sv-stack">
          <span className="sv-micro-label">Achievement coverage delta</span>
          <div className="sv-coverage">
            {comparison.achievementsGained.map((name) => (
              <code key={`gain-${name}`} data-state="gained" title="Reached by the promoted checkpoint only">+{name}</code>
            ))}
            {comparison.achievementsLost.map((name) => (
              <code key={`lost-${name}`} data-state="lost" title="Reached by the base student only">−{name}</code>
            ))}
          </div>
        </div>
      ) : null}

      <details data-testid="sft-paired-matrix">
        <summary className="sv-mono">Paired seed matrix</summary>
        <table className="sv-table">
          <thead>
            <tr>
              <th scope="col">Seed</th>
              <th scope="col">{comparison.baseLabel}</th>
              <th scope="col">{comparison.trainedLabel}</th>
              <th scope="col">Δ</th>
              <th scope="col">Outcome</th>
            </tr>
          </thead>
          <tbody>
            {comparison.rows.map((row) => (
              <tr key={row.seed}>
                <td className="sv-mono">{row.seed}</td>
                <td className="sv-mono">{formatMissingNumber(row.baseReward)}</td>
                <td className="sv-mono">{formatMissingNumber(row.trainedReward)}</td>
                <td className="sv-mono">{row.delta == null ? "—" : signed(row.delta)}</td>
                <td>
                  <span className="sv-outcome" data-outcome={row.outcome}>
                    {row.outcome === "unpaired" ? "unpaired" : row.outcome}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </details>
    </Panel>
  );
}

/* ── Selection evidence and provenance ──────────────────────────────────── */

function EvaluationSummaries({ sft }: { sft: SftState }) {
  const evaluations = sftDistinctEvaluations(sft);
  const selection = evaluations.filter((evaluation) => String(evaluation.role ?? evaluation.split) !== "heldout");
  const heldout = evaluations.filter((evaluation) => String(evaluation.role ?? evaluation.split) === "heldout");
  if (selection.length === 0 && heldout.length === 0) return null;
  const rows = (list: Array<Record<string, unknown>>) =>
    list.map((evaluation, index) => (
      <tr key={`${String(evaluation.role ?? "evaluation")}-${index}`}>
        <td>{String(evaluation.role ?? "evaluation")}</td>
        <td className="sv-mono">{String(evaluation.checkpoint_id ?? evaluation.checkpointId ?? "—")}</td>
        <td className="sv-mono">{String(evaluation.step ?? "—")}</td>
        <td className="sv-mono">{String(evaluation.metric ?? (evaluation.calibration_accuracy != null ? "calibration accuracy" : evaluation.accuracy != null ? "accuracy" : "—"))}</td>
        <td className="sv-mono">{String(evaluation.score ?? evaluation.calibration_accuracy ?? evaluation.accuracy ?? "—")}</td>
        <td className="sv-mono">{String(evaluation.n ?? evaluation.sampleCount ?? evaluation.sample_count ?? "—")}</td>
      </tr>
    ));
  return (
    <Panel
      title="Evaluation summaries"
      aside="selection drives promotion · heldout is measurement only"
      testId="sft-evaluations"
    >
      <table className="sv-table">
        <thead>
          <tr>
            <th scope="col">Role</th><th scope="col">Checkpoint</th><th scope="col">Step</th>
            <th scope="col">Metric</th><th scope="col">Value</th><th scope="col">N</th>
          </tr>
        </thead>
        <tbody>
          {rows(selection)}
          {rows(heldout)}
        </tbody>
      </table>
    </Panel>
  );
}

function ProvenancePanel({ sft }: { sft: SftState }) {
  const splits = (sft.dataset.splits as Record<string, { count?: number; digest?: string }> | undefined) ?? {};
  const lineage = sft.lineage ?? {};
  const hasLineage = Object.keys(lineage).length > 0;
  const compute = sft.compute;
  if (Object.keys(splits).length === 0 && !hasLineage && Object.keys(compute).length === 0) return null;
  return (
    <Panel title="Provenance" testId="sft-provenance">
      {Object.keys(splits).length > 0 ? (
        <div className="sv-stack-tight sv-stack">
          <span className="sv-micro-label">Dataset splits</span>
          <table className="sv-table">
            <thead><tr><th scope="col">Split</th><th scope="col">Rows</th><th scope="col">Digest</th></tr></thead>
            <tbody>
              {Object.entries(splits).map(([name, split]) => (
                <tr key={name}>
                  <td className="sv-mono">{name}</td>
                  <td className="sv-mono">{split.count ?? "—"}</td>
                  <td>{split.digest ? <Identifier value={String(split.digest)} label="digest" max={20} /> : "—"}</td>
                </tr>
              ))}
              {sft.dataset.rejected != null ? (
                <tr><td className="sv-mono">rejected</td><td className="sv-mono">{String(sft.dataset.rejected)}</td><td>—</td></tr>
              ) : null}
            </tbody>
          </table>
        </div>
      ) : null}

      {hasLineage ? (
        <div className="sv-stack-tight sv-stack">
          <span className="sv-micro-label">Lineage</span>
          <dl className="sv-kv">
            <dt>Base model</dt><dd>{String(lineage.baseModel ?? "—")}</dd>
            <dt>Adapter</dt><dd>{String(lineage.adapter ?? "—")}</dd>
            <dt>Checkpoint</dt><dd>{String(lineage.checkpointId ?? "—")}</dd>
            {lineage.digest ? (
              <>
                <dt>Digest</dt>
                <dd><Identifier value={String(lineage.digest)} label="digest" max={20} /></dd>
              </>
            ) : null}
          </dl>
        </div>
      ) : null}

      {Object.keys(compute).length > 0 ? (
        <p className="sv-mono">
          {String(compute.provider ?? "—")} · {String(compute.gpu ?? "—")}
          {compute.utilization != null ? ` · util ${Number(compute.utilization).toFixed(2)}` : ""}
          {compute.tokensPerSec != null ? ` · ${String(compute.tokensPerSec)} tok/s` : ""}
        </p>
      ) : null}
    </Panel>
  );
}

function CispoIdentityPanel({
  cispo
}: {
  cispo: NonNullable<ProjectedState["cispo"]>;
}) {
  const clip =
    cispo.clipLow != null || cispo.clipHigh != null
      ? `${formatMissingNumber(cispo.clipLow)} … ${formatMissingNumber(cispo.clipHigh)}`
      : "—";
  return (
    <Panel title="CISPO identity" testId="cispo-identity">
      <dl className="sv-kv">
        <dt>Objective</dt>
        <dd>{cispo.objective}</dd>
        <dt>Clip bounds</dt>
        <dd className="sv-mono">{clip}</dd>
        <dt>Group size</dt>
        <dd className="sv-mono">{formatMissingNumber(cispo.groupSize, 0)}</dd>
        <dt>Optimizer steps</dt>
        <dd className="sv-mono">{String(cispo.optimizerSteps)}</dd>
        <dt>Warm-start artifact</dt>
        <dd>{cispo.warmStartArtifactId ? <Identifier value={cispo.warmStartArtifactId} max={28} /> : "—"}</dd>
        <dt>Checkpoint lineage</dt>
        <dd className="sv-mono">
          {cispo.checkpointIds.length > 0 ? cispo.checkpointIds.join(" → ") : "—"}
        </dd>
      </dl>
    </Panel>
  );
}

/**
 * CISPO's distinguishing evidence. The generic training sequence below is the
 * same one SFT uses; what makes a CISPO run readable is whether the group
 * produced usable advantage at all, so that question gets its own panel at the
 * top of the canvas rather than a row inside an identity card.
 */
function CispoLearningSignalPanel({
  cispo
}: {
  cispo: NonNullable<ProjectedState["cispo"]>;
}) {
  const variance = cispo.rewardVariance;
  const rolloutGroups = cispo.rolloutGroups ?? [];
  const learningSignalGroups = cispo.learningSignalGroups
    ?? rolloutGroups.filter((group) => group.rewardVariance != null && group.rewardVariance > 0).length;
  const zeroAdvantageGroups = cispo.zeroAdvantageGroups ?? 0;
  const verdict = cispo.noLearningSignal && learningSignalGroups === 0
    ? { text: "Stopped truthfully — uniform group, no fabricated advantage", tone: "warn" as const }
    : learningSignalGroups > 0
      ? {
          text: `${learningSignalGroups} of ${rolloutGroups.length} groups carried reward variation`,
          tone: "ok" as const
        }
      : zeroAdvantageGroups > 0
        ? {
            text: `${zeroAdvantageGroups} uniform group${zeroAdvantageGroups === 1 ? "" : "s"} so far; waiting for usable variation`,
            tone: "warn" as const
          }
    : typeof variance === "number" && Number.isFinite(variance)
      ? variance > 0
        ? { text: "Rewards vary within the group, so advantage is defined", tone: "ok" as const }
        : { text: "Zero reward variance — the group is uniform and carries no gradient", tone: "warn" as const }
      : { text: "Reward variance has not been reported yet", tone: undefined };
  // The chip counts groups across the whole run; every row below it is a
  // scalar from the most recent update (collectionHydration reads them off the
  // last metric point). Unlabelled, the two read as a contradiction -- "17 of
  // 18 groups carried reward variation" directly above "Reward variance 0.00"
  // looks like one of them is broken, when in fact the newest group is the one
  // uniform group. Saying which update the numbers describe is the whole fix.
  return (
    <Panel title="Learning signal" aside="latest update" testId="cispo-learning-signal">
      <p className="sv-stack-tight">
        <span className="sv-chip sv-chip-wrap" data-tone={verdict.tone}>{verdict.text}</span>
      </p>
      <dl className="sv-kv">
        <dt>Reward variance</dt>
        <dd className="sv-mono">{formatMissingNumber(cispo.rewardVariance)}</dd>
        <dt>Advantage</dt>
        <dd className="sv-mono">
          {formatMissingNumber(cispo.advantageMean)} ± {formatMissingNumber(cispo.advantageStd)}
        </dd>
        <dt>Clipping</dt>
        <dd className="sv-mono">
          {cispo.clipLow != null || cispo.clipHigh != null
            ? `${formatMissingNumber(cispo.clipLow)} … ${formatMissingNumber(cispo.clipHigh)}`
            : "—"}
        </dd>
        <dt>Tokens clipped</dt>
        <dd className="sv-mono">
          {cispo.clippedTokenFraction == null ? "—" : percent(cispo.clippedTokenFraction, 1)}
        </dd>
        <dt>Mean ratio</dt>
        <dd className="sv-mono">{formatMissingNumber(cispo.importanceRatioMean)}</dd>
        <dt>KL proxy</dt>
        <dd className="sv-mono">{formatMissingNumber(cispo.klProxy)}</dd>
        <dt>Group size</dt>
        <dd className="sv-mono">{formatMissingNumber(cispo.groupSize, 0)}</dd>
        <dt>Rollout groups</dt>
        <dd className="sv-mono">{String(rolloutGroups.length)}</dd>
        <dt>Uniform groups</dt>
        <dd className="sv-mono">{String(zeroAdvantageGroups)}</dd>
      </dl>
    </Panel>
  );
}

function PrerequisitesPanel({
  missing,
  isCispo,
  failure
}: {
  missing: SftPrerequisite[];
  isCispo?: boolean;
  /** Why the run ended, when it ended badly. */
  failure?: string;
}) {
  // A rejected run has not stalled part-way through a sequence; it never ran.
  // Listing its untouched stages as "what is still needed" tells the reader to
  // go collect evidence when the actual next step is to fix the rejection, and
  // the reason is the one thing the surface must not omit.
  if (failure) {
    return (
      <Panel
        title="Why this run failed"
        aside="no stage was reached"
        testId={isCispo ? "cispo-failure" : "sft-failure"}
      >
        <p className="sv-failure-detail">{failure}</p>
        <p className="sv-empty">
          The stages below are empty because the job was rejected before it
          started, not because it is still in progress.
        </p>
      </Panel>
    );
  }
  if (missing.length === 0) return null;
  return (
    <Panel
      title="What is still needed"
      aside={`${missing.length} outstanding`}
      testId={isCispo ? "cispo-prerequisites" : "sft-prerequisites"}
    >
      <ol className="sv-checklist">
        {missing.map((item) => (
          <li key={item.id} data-testid={`sft-prerequisite-${item.id}`}>
            <strong>{item.label}</strong>
            <span>{item.why}</span>
          </li>
        ))}
      </ol>
    </Panel>
  );
}

/* ── Workspace ──────────────────────────────────────────────────────────── */

export function SftWorkspace({
  projected,
  run,
  debug,
  metricSeries,
  embedded = false
}: {
  projected: ProjectedState;
  run: OptimizerRun;
  debug?: ReactNode;
  metricSeries?: { status: string; total?: number; error?: string };
  embedded?: boolean;
}) {
  const sft = projected.sft;
  const cispo = projected.cispo;
  const isCispo = run.algorithmId === "cispo";
  const reportedStatus = String(projected.summary.status ?? run.status ?? "");
  const status = sft ? sftEffectiveStatus(sft, reportedStatus) : reportedStatus;
  const nested = (projected.summary.summary as Record<string, unknown> | undefined) ?? {};
  const promotedCheckpointId = typeof nested.promotedCheckpointId === "string" ? nested.promotedCheckpointId : undefined;
  const stages = useMemo(() => {
    if (!sft) return [];
    const base = sftStages(sft, status, promotedCheckpointId);
    if (!isCispo || !cispo) return base;
    return base
      .filter((stage) => ["baseline", "training", "checkpoints", "evaluation", "heldout"].includes(stage.id))
      .map((stage) => stage.id === "training"
        ? {
            ...stage,
            label: "Rollout groups + updates",
            detail: `${(cispo.rolloutGroups ?? []).length} groups · ${cispo.optimizerSteps} updates`
          }
        : stage);
  }, [sft, status, promotedCheckpointId, isCispo, cispo]);
  const comparison = useMemo(() => (sft ? sftComparison(sft) : null), [sft]);
  const missingPrerequisites = useMemo(() => {
    const missing = sft ? sftMissingPrerequisites(sft) : [];
    // CISPO learns from online rollout groups; SFT teacher collection and
    // curation are not prerequisites for this algorithm.
    return isCispo ? missing.filter((item) => item.id !== "collection") : missing;
  }, [sft, isCispo]);
  const heldoutSummary = useMemo(() => (sft ? sftHeldoutSummary(sft) : null), [sft]);
  const campaignData = useMemo(() => {
    if (!sft) return { groups: [] as RolloutGroup[], rows: [] as RolloutRow[] };
    const groups: RolloutGroup[] = [];
    const rows: RolloutRow[] = [];
    if (isCispo && cispo) {
      const seenIterations = new Set<number | null>();
      for (const group of cispo.rolloutGroups ?? []) {
        if (!seenIterations.has(group.iteration)) {
          seenIterations.add(group.iteration);
          groups.push({
            key: `iteration-${group.iteration ?? "unknown"}`,
            title: group.iteration == null ? "Iteration" : `Iteration ${group.iteration}`,
            subtitle: "CISPO rollout groups"
          });
        }
        rows.push({
          id: group.id,
          groupKey: `iteration-${group.iteration ?? "unknown"}`,
          sequence: group.sequence,
          stage: group.label ?? undefined,
          reward: group.rewardMean ?? undefined,
          completed: group.completed
        });
      }
      return { groups, rows };
    }
    for (const campaign of sft.campaigns) {
      groups.push({
        key: campaign.id,
        title: campaign.checkpointId ? `Checkpoint ${campaign.checkpointId}` : campaign.id,
        subtitle: [campaign.splitRole, campaign.status].filter(Boolean).join(" · ") || undefined
      });
      for (const child of campaign.children) {
        const reward = child.attributes?.reward;
        const cost = child.attributes?.cost_usd ?? child.attributes?.costUsd;
        rows.push({
          id: child.id,
          groupKey: campaign.id,
          sequence: 0,
          stage: campaign.splitRole,
          reward: typeof reward === "number" ? reward : reward === null ? null : undefined,
          costUsd: typeof cost === "number" ? cost : undefined,
          streamId: child.attributes?.stream_id,
          rewardUrl: child.attributes?.reward_url
        });
      }
    }
    return { groups, rows };
  }, [sft, isCispo, cispo]);

  if (!sft) return null;

  const improvementVerdict = typeof nested.improvementVerdict === "string"
    ? nested.improvementVerdict
    : undefined;
  const chip = statusChip(status, improvementVerdict, heldoutSummary?.claimReady === true);
  const terminal = TERMINAL_STATUSES.includes(status);
  // `run.error` is set only when the host already knew the failure; for a
  // live or replayed run the reason arrives on the event stream, and the
  // projection recovers it there.
  const failureDetail = status === "failed"
    ? optimizerFailureDetail(run.error) ?? (typeof projected.summary.failureDetail === "string" ? projected.summary.failureDetail : undefined)
    : undefined;
  const latest = sft.points.at(-1);
  const aggregateBaseline = sftAggregateBaseline(sft);
  const readyCount = sft.checkpoints.filter((ckpt) => ckpt.ready === true || ckpt.promoted === true).length;
  const costUsd = projected.usage.costUsd;
  const activeStage = stages.find((stage) => stage.status === "active");
  const upliftClaimed = sft.checkpoints.some((ckpt) => ckpt.promoted === true)
    || improvementVerdict === "improvement_demonstrated"
    || heldoutSummary?.claimReady === true;
  const selectedId = typeof nested.selectedCheckpointId === "string"
    ? nested.selectedCheckpointId
    : typeof sft.lineage?.selectedCheckpointId === "string"
      ? sft.lineage.selectedCheckpointId
      : promotedCheckpointId;
  const headline = terminal
    ? status === "failed"
      ? "Training failed"
      : heldoutSummary
        ? heldoutSummary.claimReady
          ? `Heldout uplift ${points(heldoutSummary.absoluteUplift)} over ${heldoutSummary.paired} paired examples`
          : `Completed · heldout ${heldoutSummary.verdict ?? "inconclusive"} — no uplift claimed`
      : upliftClaimed && comparison
        ? `Heldout uplift ${signed(comparison.absoluteUplift)} over ${comparison.paired} paired seeds`
        : improvementVerdict === "inconclusive"
          ? "Completed · evaluation inconclusive — no uplift claimed"
          : "Completed · no measured improvement — selection is not uplift"
    : status === "queued"
      ? "Waiting for an accelerator — queued honestly, not running"
      : activeStage
        ? `${activeStage.label}${activeStage.detail ? ` · ${activeStage.detail}` : ""}`
        : "Preparing run";

  // Tiering, not truncation: the header line carries the outcome and the two or
  // three values that drive the next decision; everything else stays reachable
  // one click away under "Run details". A flat chip wall hid all of them.
  const metrics: WorkspaceMetric[] = [
    ...(isCispo && cispo
      ? ([
          { label: "Algorithm", value: "CISPO", tier: "detail" },
          {
            label: "Clip",
            value:
              cispo.clipLow != null || cispo.clipHigh != null
                ? `${formatMissingNumber(cispo.clipLow)} … ${formatMissingNumber(cispo.clipHigh)}`
                : "—",
            tier: "primary"
          },
          { label: "Group size", value: formatMissingNumber(cispo.groupSize, 0), tier: "primary" },
          { label: "Reward var", value: formatMissingNumber(cispo.rewardVariance), tier: "detail" },
          {
            label: "Advantage",
            value: `${formatMissingNumber(cispo.advantageMean)} ± ${formatMissingNumber(cispo.advantageStd)}`,
            tier: "primary"
          },
          { label: "Opt. steps", value: String(cispo.optimizerSteps), tier: "detail" },
          { label: "Warm start", value: cispo.warmStartArtifactId ?? "none", tier: "detail" }
        ] satisfies WorkspaceMetric[])
      : []),
    {
      tier: "primary",
      label: "Heldout uplift",
      // Two different units share this slot: a paired accuracy difference is
      // in points, a paired reward-mean difference is not a rate at all. The
      // titles below already say which one is being reported.
      value: heldoutSummary
        ? points(heldoutSummary.absoluteUplift)
        : comparison
          ? signed(comparison.absoluteUplift)
          : "not measured",
      title: heldoutSummary
        ? `Paired accuracy difference over ${heldoutSummary.paired} examples, selected checkpoint minus base.`
        : comparison
          ? `Paired mean difference over ${comparison.paired} seeds, trained minus base.`
        : "Requires a paired base-vs-promoted run on untouched heldout seeds."
    },
    {
      // Phase A only. The heldout base arm is a different split and lives in
      // the comparison panel; conflating them would misreport both.
      tier: "detail",
      label: "Baseline mean",
      value: aggregateBaseline
        ? percent(aggregateBaseline.score, 1)
        : formatMissingNumber(sftDistribution((sft.baseline?.seeds ?? []).map((seed) => seed.reward)).mean),
      title: aggregateBaseline
        ? "Unchanged student on the selection split, not on the locked heldout split."
        : "Unchanged student on the frozen baseline seeds."
    },
    { tier: isCispo ? "detail" : "primary", label: "Step / epoch", value: `${formatMissingNumber(latest?.step, 0)} / ${formatMissingNumber(latest?.epoch, 0)}` },
    { tier: "detail", label: "Train loss", value: formatMissingNumber(latest?.trainLoss) },
    { tier: "detail", label: "Val loss", value: formatMissingNumber(latest?.validationLoss) },
    { tier: isCispo ? "detail" : "primary", label: "Checkpoints", value: sft.checkpoints.length ? `${readyCount}/${sft.checkpoints.length} ready` : "—" },
    {
      tier: "detail",
      label: "Selected",
      value: selectedId ?? "none yet",
      title: "Selection retains a checkpoint. It is not an uplift claim."
    },
    {
      tier: isCispo ? "detail" : "primary",
      label: "Uplift",
      value: upliftClaimed ? "demonstrated" : "not claimed",
      title: "Green only when the canonical verdict demonstrates improvement or the paired heldout evaluation is claim-ready. Zero-score or no-baseline cohorts cannot claim uplift."
    },
    {
      tier: "detail",
      label: "Cost",
      value: costUsd != null && costUsd > 0 ? formatMissingUsd(costUsd) : "unavailable",
      title: costUsd != null && costUsd > 0 ? undefined : "No usable cost telemetry from this run"
    },
    ...(nested.baseModel || sft.lineage?.baseModel
      ? ([{ tier: "detail", label: "Base model", value: String(nested.baseModel ?? sft.lineage?.baseModel) }] satisfies WorkspaceMetric[])
      : [])
  ];

  return (
    <div className="sv-workspace" data-testid={isCispo ? "cispo-workspace" : "sft-workspace"}>
      {!embedded ? (
        <WorkspaceHeader
          statusText={chip.text}
          statusTone={chip.tone}
          live={chip.dot}
          headline={headline}
          metrics={metrics}
          testId={isCispo ? "cispo-run-header" : "sft-run-header"}
        />
      ) : null}
      <StageTimeline stages={stages} testId={isCispo ? "cispo-stage-timeline" : "sft-stage-timeline"} />

      {isCispo && cispo ? (
        <div className="sv-workspace-canvas">
          <CispoIdentityPanel cispo={cispo} />
          <CispoLearningSignalPanel cispo={cispo} />
        </div>
      ) : isCispo ? (
        // A CISPO run that ends before emitting a single CISPO event still has
        // to say it was CISPO. Falling through to the shared SFT grammar
        // rendered a surface indistinguishable from an SFT run -- same stages,
        // same panels, no clip bounds, no group size, nothing naming the
        // algorithm whose behaviour the reader came to check.
        <Panel title="CISPO identity" aside="not reported" testId="cispo-identity-unreported">
          <p className="sv-empty">
            This run is CISPO, but it ended before reporting clip bounds, group
            size, reward variance, or advantage. The sections below are the
            training stages CISPO shares with SFT; nothing here describes the
            clipped-importance behaviour itself.
          </p>
        </Panel>
      ) : null}

      <PrerequisitesPanel missing={missingPrerequisites} isCispo={isCispo} failure={failureDetail} />

      {/* CISPO's unit of work is the rollout group, so it leads; SFT's is the
          curated dataset, so its baseline and curation lead instead. Below the
          fork both families share the same training/evidence/provenance tail. */}
      {isCispo ? (
        <RolloutBrowser
          groups={campaignData.groups}
          rows={campaignData.rows}
          emptyText="Rollout groups appear here with per-rollout reward and cost as the producer emits them."
          testId="sft-live-campaigns"
        />
      ) : (
        <div className="sv-workspace-canvas">
          <BaselinePanel sft={sft} isCispo={isCispo} />
          <CurationPanel sft={sft} />
        </div>
      )}

      <div className="sv-workspace-canvas">
      <CurvesPanel sft={sft} metricSeries={metricSeries} />
        <CheckpointRail sft={sft} promotedCheckpointId={promotedCheckpointId} />
      </div>

      {isCispo ? null : (
        <RolloutBrowser
          groups={campaignData.groups}
          rows={campaignData.rows}
          emptyText="Checkpoint evaluation campaigns appear here with per-rollout reward and cost as the producer emits them."
          testId="sft-live-campaigns"
        />
      )}

      <ComparisonPanel comparison={comparison} aggregate={heldoutSummary} />
      <EvaluationSummaries sft={sft} />

      {isCispo ? (
        <div className="sv-workspace-canvas">
          <BaselinePanel sft={sft} isCispo={isCispo} />
          <CurationPanel sft={sft} />
        </div>
      ) : null}

      <ProvenancePanel sft={sft} />

      {debug ? (
        <details data-testid="sft-debug">
          <summary className="sv-mono">Debug · raw events, artifacts, usage</summary>
          {debug}
        </details>
      ) : null}
    </div>
  );
}

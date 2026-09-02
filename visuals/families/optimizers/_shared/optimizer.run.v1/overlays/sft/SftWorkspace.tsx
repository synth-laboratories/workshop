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
import type { OptimizerRun, ProjectedState } from "../../components/projectEvents.ts";
import {
  StageTimeline,
  WorkspaceHeader,
  type WorkspaceMetric
} from "../../components/workspace/WorkspaceChrome.tsx";
import { RolloutBrowser, type RolloutGroup, type RolloutRow } from "../../components/workspace/RolloutBrowser.tsx";
import {
  SFT_TERMINAL_STATUSES,
  sftComparison,
  sftCurationFunnel,
  sftDistribution,
  sftStages,
  type SftComparison,
  type SftState
} from "./model.ts";

const TERMINAL_STATUSES = SFT_TERMINAL_STATUSES;

function statusChip(
  status: string,
  verdict?: string
): { text: string; tone?: "live" | "ok" | "bad" | "warn"; dot: boolean } {
  if (status === "failed") return { text: "Failed", tone: "bad", dot: false };
  if (["canceled", "cancelled"].includes(status)) return { text: "Canceled", tone: "warn", dot: false };
  if (TERMINAL_STATUSES.includes(status)) {
    if (verdict === "improvement_demonstrated") {
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
      {!baseline || baseline.seeds.length === 0 ? (
        <p className="sv-empty">
          No baseline evaluation has been emitted. The untrained student must be scored on the frozen
          baseline seeds before training, or there is nothing to measure uplift against.
        </p>
      ) : (
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
      )}
    </Panel>
  );
}

/* ── Phases B/C · collection and curation ───────────────────────────────── */

function CurationPanel({ sft }: { sft: SftState }) {
  const funnel = useMemo(() => sftCurationFunnel(sft), [sft]);
  const hasAnything = funnel.steps.some((step) => step.count != null) || funnel.accepted.length > 0;
  const widest = Math.max(1, ...funnel.steps.map((step) => step.count ?? 0));
  return (
    <Panel
      title="Collection & curation"
      aside={funnel.acceptanceRate != null ? `${percent(funnel.acceptanceRate)} retained` : undefined}
      testId="sft-curation"
    >
      {!hasAnything ? (
        <p className="sv-empty">
          No teacher collection or curation has been reported. Trajectories must be sealed and then
          accepted or rejected with an explicit reason before a dataset can claim provenance.
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

function CurvesPanel({ sft }: { sft: SftState }) {
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
  return (
    <Panel title="Training curves" aside={`${points.length} aligned records`} testId="sft-live-curves">
      {points.length === 0 ? (
        <p className="sv-empty">Loss metrics stream here once the training job reports its first step.</p>
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

function ComparisonPanel({ comparison }: { comparison: SftComparison | null }) {
  if (!comparison) {
    return (
      <Panel title="Heldout comparison — base vs promoted" testId="sft-comparison">
        <p className="sv-empty">
          No paired heldout evaluation has been emitted. Training completing, a checkpoint reaching
          <strong> ready</strong>, and even a promotion decision are <strong>not</strong> uplift. The promoted
          checkpoint and the unchanged base student must both run the untouched heldout seeds before
          any uplift can be reported.
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
  const selection = sft.evaluations.filter((evaluation) => String(evaluation.role ?? evaluation.split) !== "heldout");
  const heldout = sft.evaluations.filter((evaluation) => String(evaluation.role ?? evaluation.split) === "heldout");
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
        <dt>Reward variance</dt>
        <dd className="sv-mono">{formatMissingNumber(cispo.rewardVariance)}</dd>
        <dt>Advantage mean</dt>
        <dd className="sv-mono">{formatMissingNumber(cispo.advantageMean)}</dd>
        <dt>Advantage std</dt>
        <dd className="sv-mono">{formatMissingNumber(cispo.advantageStd)}</dd>
        <dt>Optimizer steps</dt>
        <dd className="sv-mono">{String(cispo.optimizerSteps)}</dd>
        <dt>Warm-start artifact</dt>
        <dd>{cispo.warmStartArtifactId ? <Identifier value={cispo.warmStartArtifactId} max={28} /> : "—"}</dd>
        <dt>Checkpoint lineage</dt>
        <dd className="sv-mono">
          {cispo.checkpointIds.length > 0 ? cispo.checkpointIds.join(" → ") : "—"}
        </dd>
        {cispo.noLearningSignal ? (
          <>
            <dt>Learning signal</dt>
            <dd>Stopped truthfully — uniform group, no fabricated advantage</dd>
          </>
        ) : null}
      </dl>
    </Panel>
  );
}

/* ── Workspace ──────────────────────────────────────────────────────────── */

export function SftWorkspace({
  projected,
  run,
  debug,
  embedded = false
}: {
  projected: ProjectedState;
  run: OptimizerRun;
  debug?: ReactNode;
  embedded?: boolean;
}) {
  const sft = projected.sft;
  const cispo = projected.cispo;
  const isCispo = run.algorithmId === "cispo";
  const status = String(projected.summary.status ?? run.status ?? "");
  const nested = (projected.summary.summary as Record<string, unknown> | undefined) ?? {};
  const promotedCheckpointId = typeof nested.promotedCheckpointId === "string" ? nested.promotedCheckpointId : undefined;
  const stages = useMemo(
    () => sft ? sftStages(sft, status, promotedCheckpointId) : [],
    [sft, status, promotedCheckpointId]
  );
  const comparison = useMemo(() => (sft ? sftComparison(sft) : null), [sft]);
  const campaignData = useMemo(() => {
    if (!sft) return { groups: [] as RolloutGroup[], rows: [] as RolloutRow[] };
    const groups: RolloutGroup[] = [];
    const rows: RolloutRow[] = [];
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
  }, [sft]);

  if (!sft) return null;

  const improvementVerdict = typeof nested.improvementVerdict === "string"
    ? nested.improvementVerdict
    : undefined;
  const chip = statusChip(status, improvementVerdict);
  const terminal = TERMINAL_STATUSES.includes(status);
  const latest = sft.points.at(-1);
  const readyCount = sft.checkpoints.filter((ckpt) => ckpt.ready === true || ckpt.promoted === true).length;
  const costUsd = projected.usage.costUsd;
  const activeStage = stages.find((stage) => stage.status === "active");
  const upliftClaimed = sft.checkpoints.some((ckpt) => ckpt.promoted === true) || improvementVerdict === "improvement_demonstrated";
  const selectedId = typeof nested.selectedCheckpointId === "string"
    ? nested.selectedCheckpointId
    : typeof sft.lineage?.selectedCheckpointId === "string"
      ? sft.lineage.selectedCheckpointId
      : promotedCheckpointId;
  const headline = terminal
    ? status === "failed"
      ? "Training failed"
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

  const metrics: WorkspaceMetric[] = [
    ...(isCispo && cispo
      ? [
          { label: "Algorithm", value: "CISPO" },
          {
            label: "Clip",
            value:
              cispo.clipLow != null || cispo.clipHigh != null
                ? `${formatMissingNumber(cispo.clipLow)} … ${formatMissingNumber(cispo.clipHigh)}`
                : "—"
          },
          { label: "Group size", value: formatMissingNumber(cispo.groupSize, 0) },
          { label: "Reward var", value: formatMissingNumber(cispo.rewardVariance) },
          {
            label: "Advantage",
            value: `${formatMissingNumber(cispo.advantageMean)} ± ${formatMissingNumber(cispo.advantageStd)}`
          },
          { label: "Opt. steps", value: String(cispo.optimizerSteps) },
          { label: "Warm start", value: cispo.warmStartArtifactId ?? "none" }
        ]
      : []),
    {
      label: "Heldout uplift",
      value: comparison ? signed(comparison.absoluteUplift) : "not measured",
      title: comparison
        ? `Paired mean difference over ${comparison.paired} seeds, trained minus base.`
        : "Requires a paired base-vs-promoted run on untouched heldout seeds."
    },
    {
      // Phase A only. The heldout base arm is a different split and lives in
      // the comparison panel; conflating them would misreport both.
      label: "Baseline mean",
      value: formatMissingNumber(sftDistribution((sft.baseline?.seeds ?? []).map((seed) => seed.reward)).mean),
      title: "Unchanged student on the frozen baseline seeds."
    },
    { label: "Step / epoch", value: `${formatMissingNumber(latest?.step, 0)} / ${formatMissingNumber(latest?.epoch, 0)}` },
    { label: "Train loss", value: formatMissingNumber(latest?.trainLoss) },
    { label: "Val loss", value: formatMissingNumber(latest?.validationLoss) },
    { label: "Checkpoints", value: sft.checkpoints.length ? `${readyCount}/${sft.checkpoints.length} ready` : "—" },
    {
      label: "Selected",
      value: selectedId ?? "none yet",
      title: "Selection retains a checkpoint. It is not an uplift claim."
    },
    {
      label: "Uplift",
      value: upliftClaimed ? "demonstrated" : "not claimed",
      title: "Green only when improvement_verdict is improvement_demonstrated. Zero-score or no-baseline cohorts cannot claim uplift."
    },
    {
      label: "Cost",
      value: costUsd != null && costUsd > 0 ? formatMissingUsd(costUsd) : "unavailable",
      title: costUsd != null && costUsd > 0 ? undefined : "No usable cost telemetry from this run"
    },
    ...(nested.baseModel || sft.lineage?.baseModel
      ? [{ label: "Base model", value: String(nested.baseModel ?? sft.lineage?.baseModel) }]
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

      {isCispo && cispo ? <CispoIdentityPanel cispo={cispo} /> : null}

      <div className="sv-workspace-canvas">
        <BaselinePanel sft={sft} isCispo={isCispo} />
        <CurationPanel sft={sft} />
      </div>

      <div className="sv-workspace-canvas">
        <CurvesPanel sft={sft} />
        <CheckpointRail sft={sft} promotedCheckpointId={promotedCheckpointId} />
      </div>

      <RolloutBrowser
        groups={campaignData.groups}
        rows={campaignData.rows}
        emptyText="Checkpoint evaluation campaigns appear here with per-rollout reward and cost as the producer emits them."
        testId="sft-live-campaigns"
      />

      <ComparisonPanel comparison={comparison} />
      <EvaluationSummaries sft={sft} />
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

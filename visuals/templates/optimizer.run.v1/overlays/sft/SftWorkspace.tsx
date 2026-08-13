/**
 * SFT workspace on the shared optimizer chrome: sticky run header, semantic
 * stage timeline (dataset → training → checkpoints → evaluation → promotion),
 * aligned training curves, checkpoint rail where promotion is never conflated
 * with "ready", and checkpoint-evaluation campaigns in the scalable rollout
 * browser. Honest about queued runs and providers that have disappeared.
 */

import { useMemo } from "react";
import type { ReactNode } from "react";
import { Identifier } from "../../../../chrome/Identifier.tsx";
import { formatMissingNumber, formatMissingUsd } from "../../../../runtime/liveStream.ts";
import type { OptimizerRun, ProjectedState } from "../../components/projectEvents.ts";
import {
  StageTimeline,
  WorkspaceHeader,
  type WorkspaceMetric
} from "../../components/workspace/WorkspaceChrome.tsx";
import { RolloutBrowser, type RolloutGroup, type RolloutRow } from "../../components/workspace/RolloutBrowser.tsx";
import { SFT_TERMINAL_STATUSES, sftStages, type SftState } from "./model.ts";

const TERMINAL_STATUSES = SFT_TERMINAL_STATUSES;

function statusChip(status: string): { text: string; tone?: "live" | "ok" | "bad" | "warn"; dot: boolean } {
  if (status === "failed") return { text: "Failed", tone: "bad", dot: false };
  if (["canceled", "cancelled"].includes(status)) return { text: "Canceled", tone: "warn", dot: false };
  if (TERMINAL_STATUSES.includes(status)) return { text: "Completed", tone: "ok", dot: false };
  if (status === "queued") return { text: "Queued", tone: "warn", dot: false };
  if (["created", "pending", "loading"].includes(status)) {
    return { text: status[0].toUpperCase() + status.slice(1), dot: false };
  }
  return { text: "Running", tone: "live", dot: true };
}

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
    <section className="sv-section" aria-label="Training curves" data-testid="sft-live-curves" style={{ marginTop: 0 }}>
      <div className="sv-section-head">
        <h3>Training curves</h3>
        <span className="sv-mono">{points.length} aligned records</span>
      </div>
      {points.length === 0 ? (
        <p style={{ color: "var(--sv-text-faint)", fontSize: 12, margin: 0 }}>
          Loss metrics stream here once the training job reports its first step.
        </p>
      ) : (
        <div style={{ border: "1px solid var(--sv-border)", borderRadius: 9, padding: "8px 10px" }}>
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
            {path("validationLoss") ? <path d={path("validationLoss")} fill="none" stroke="#5c6573" strokeWidth="1.8" strokeDasharray="4 3" /> : null}
            {points.map((point) => (
              <g key={point.step}>
                {typeof point.trainLoss === "number" ? <circle cx={x(point.step)} cy={y(point.trainLoss)} r={3} fill="var(--sv-accent)" /> : null}
                {typeof point.validationLoss === "number" ? <circle cx={x(point.step)} cy={y(point.validationLoss)} r={3} fill="#5c6573" /> : null}
              </g>
            ))}
          </svg>
          <div style={{ display: "flex", gap: 12, fontSize: 10.5, color: "var(--sv-text-muted)" }} aria-hidden="true">
            <span><span style={{ display: "inline-block", width: 8, height: 8, borderRadius: 4, background: "var(--sv-accent)", marginRight: 4 }} />train loss</span>
            <span><span style={{ display: "inline-block", width: 8, height: 8, borderRadius: 4, background: "#5c6573", marginRight: 4 }} />validation loss (dashed)</span>
          </div>
          <details style={{ marginTop: 6 }}>
            <summary style={{ cursor: "pointer", fontSize: 11, color: "var(--sv-text-muted)" }}>Per-step records</summary>
            <table className="sv-table" style={{ marginTop: 6 }}>
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
        </div>
      )}
    </section>
  );
}

function CheckpointRail({ sft, promotedCheckpointId }: { sft: SftState; promotedCheckpointId?: string }) {
  return (
    <section className="sv-section" aria-label="Checkpoints" data-testid="sft-checkpoint-rail" style={{ marginTop: 14 }}>
      <div className="sv-section-head">
        <h3>Checkpoints</h3>
        <span className="sv-mono">{sft.checkpoints.length}</span>
      </div>
      {sft.checkpoints.length === 0 ? (
        <p style={{ color: "var(--sv-text-faint)", fontSize: 12, margin: 0 }}>Checkpoints appear as training emits them.</p>
      ) : (
        <div role="list" style={{ display: "grid", gap: 6 }}>
          {sft.checkpoints.map((ckpt) => {
            const id = String(ckpt.id ?? "");
            const promoted = ckpt.promoted === true || id === promotedCheckpointId;
            const ready = ckpt.ready === true;
            return (
              <div key={id} role="listitem" style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 8, padding: "8px 11px", border: "1px solid var(--sv-border)", borderRadius: 9 }}>
                <Identifier value={id} max={30} />
                {ckpt.step != null ? <span className="sv-mono" style={{ color: "var(--sv-text-faint)" }}>step {String(ckpt.step)}</span> : null}
                <span style={{ marginLeft: "auto", display: "inline-flex", gap: 5 }}>
                  <span className="sv-chip" data-tone={ready ? "ok" : undefined}>{ready ? "Ready" : String(ckpt.status ?? "created")}</span>
                  <span className="sv-chip" data-tone={promoted ? "ok" : undefined} title="Promotion is a distinct decision; a ready checkpoint is not promoted.">
                    {promoted ? "Promoted" : "Not promoted"}
                  </span>
                </span>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

function EvaluationSummaries({ sft }: { sft: SftState }) {
  const selection = sft.evaluations.filter((evaluation) => String(evaluation.role ?? evaluation.split) !== "heldout");
  const heldout = sft.evaluations.filter((evaluation) => String(evaluation.role ?? evaluation.split) === "heldout");
  const row = (evaluation: Record<string, unknown>, index: number) => (
    <li key={index} className="sv-mono" style={{ fontSize: 12 }}>
      {String(evaluation.split ?? evaluation.role ?? "selection")} · {String(evaluation.metric ?? "metric")}={String(evaluation.score ?? "—")}
      {evaluation.accuracy != null ? ` · acc=${String(evaluation.accuracy)}` : ""}
    </li>
  );
  if (selection.length === 0 && heldout.length === 0) return null;
  return (
    <section className="sv-section" aria-label="Evaluation summaries">
      <div className="sv-section-head">
        <h3>Evaluation summaries</h3>
        <span className="sv-mono">selection drives promotion · heldout is measurement only</span>
      </div>
      <div className="sv-workspace-canvas">
        <div>
          <strong style={{ fontSize: 11, color: "var(--sv-text-muted)", textTransform: "uppercase", letterSpacing: ".06em" }}>Selection</strong>
          <ul style={{ margin: "5px 0 0", paddingLeft: 18 }}>{selection.map(row)}{selection.length === 0 ? <li style={{ opacity: 0.7, fontSize: 12 }}>None yet.</li> : null}</ul>
        </div>
        <div>
          <strong style={{ fontSize: 11, color: "var(--sv-text-muted)", textTransform: "uppercase", letterSpacing: ".06em" }}>Heldout (measurement only)</strong>
          <ul style={{ margin: "5px 0 0", paddingLeft: 18 }}>{heldout.map(row)}{heldout.length === 0 ? <li style={{ opacity: 0.7, fontSize: 12 }}>Not evaluated yet.</li> : null}</ul>
        </div>
      </div>
    </section>
  );
}

function ProvenancePanel({ sft }: { sft: SftState }) {
  const splits = (sft.dataset.splits as Record<string, { count?: number; digest?: string }> | undefined) ?? {};
  const lineage = sft.lineage ?? {};
  const hasLineage = Object.keys(lineage).length > 0;
  const compute = sft.compute;
  if (Object.keys(splits).length === 0 && !hasLineage && Object.keys(compute).length === 0) return null;
  return (
    <section className="sv-section" aria-label="Dataset, compute, and lineage">
      <div className="sv-section-head"><h3>Provenance</h3></div>
      <div className="sv-workspace-canvas">
        <div>
          {Object.keys(splits).length > 0 ? (
            <>
              <strong style={{ fontSize: 11, color: "var(--sv-text-muted)", textTransform: "uppercase", letterSpacing: ".06em" }}>Dataset</strong>
              <ul style={{ margin: "5px 0 0", paddingLeft: 18, fontSize: 12 }}>
                {Object.entries(splits).map(([name, split]) => (
                  <li key={name} style={{ display: "flex", gap: 8, alignItems: "baseline" }}>
                    <span>{name}: {split.count ?? "—"} rows</span>
                    {split.digest ? <Identifier value={String(split.digest)} label="digest" max={18} /> : null}
                  </li>
                ))}
                {sft.dataset.rejected != null ? <li>rejected rows: {String(sft.dataset.rejected)}</li> : null}
              </ul>
            </>
          ) : null}
          {Object.keys(compute).length > 0 ? (
            <p className="sv-mono" style={{ margin: "10px 0 0", fontSize: 11, color: "var(--sv-text-muted)" }}>
              {String(compute.provider ?? "—")} · {String(compute.gpu ?? "—")}
              {compute.utilization != null ? ` · util ${Number(compute.utilization).toFixed(2)}` : ""}
              {compute.tokensPerSec != null ? ` · ${String(compute.tokensPerSec)} tok/s` : ""}
            </p>
          ) : null}
        </div>
        <div>
          {hasLineage ? (
            <>
              <strong style={{ fontSize: 11, color: "var(--sv-text-muted)", textTransform: "uppercase", letterSpacing: ".06em" }}>Lineage</strong>
              <p style={{ margin: "5px 0 0", fontSize: 12 }}>
                {String(lineage.baseModel ?? "—")} → {String(lineage.adapter ?? "—")} → {String(lineage.checkpointId ?? "—")}
                {lineage.digest ? <> · <Identifier value={String(lineage.digest)} label="digest" max={16} /></> : null}
              </p>
            </>
          ) : null}
        </div>
      </div>
    </section>
  );
}

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
  const status = String(projected.summary.status ?? run.status ?? "");
  const nested = (projected.summary.summary as Record<string, unknown> | undefined) ?? {};
  const promotedCheckpointId = typeof nested.promotedCheckpointId === "string" ? nested.promotedCheckpointId : undefined;
  const stages = useMemo(
    () => sft ? sftStages(sft, status, promotedCheckpointId) : [],
    [sft, status, promotedCheckpointId]
  );
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

  const chip = statusChip(status);
  const terminal = TERMINAL_STATUSES.includes(status);
  const latest = sft.points.at(-1);
  const readyCount = sft.checkpoints.filter((ckpt) => ckpt.ready === true || ckpt.promoted === true).length;
  const costUsd = projected.usage.costUsd;
  const activeStage = stages.find((stage) => stage.status === "active");
  const headline = terminal
    ? status === "failed" ? "Training failed" : "Run complete"
    : status === "queued"
      ? "Waiting for an accelerator — queued honestly, not running"
      : activeStage
        ? `${activeStage.label}${activeStage.detail ? ` · ${activeStage.detail}` : ""}`
        : "Preparing run";
  const metrics: WorkspaceMetric[] = [
    { label: "Step / epoch", value: `${formatMissingNumber(latest?.step, 0)} / ${formatMissingNumber(latest?.epoch, 0)}` },
    { label: "Train loss", value: formatMissingNumber(latest?.trainLoss) },
    { label: "Val loss", value: formatMissingNumber(latest?.validationLoss) },
    { label: "Checkpoints", value: sft.checkpoints.length ? `${readyCount}/${sft.checkpoints.length} ready` : "—" },
    {
      label: "Promoted",
      value: promotedCheckpointId ?? (sft.checkpoints.some((ckpt) => ckpt.promoted === true) ? "yes" : "none yet"),
      title: "Promotion requires an explicit event; ready checkpoints are not promoted."
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
    <div className="sv-workspace" data-testid="sft-workspace">
      {!embedded ? (
        <WorkspaceHeader
          statusText={chip.text}
          statusTone={chip.tone}
          live={chip.dot}
          headline={headline}
          metrics={metrics}
          testId="sft-run-header"
        />
      ) : null}
      <StageTimeline stages={stages} testId="sft-stage-timeline" />
      <div className="sv-workspace-canvas">
        <CurvesPanel sft={sft} />
        <div>
          <CheckpointRail sft={sft} promotedCheckpointId={promotedCheckpointId} />
        </div>
      </div>
      <RolloutBrowser
        groups={campaignData.groups}
        rows={campaignData.rows}
        emptyText="Checkpoint evaluation campaigns appear here with per-rollout reward and cost as the producer emits them."
        testId="sft-live-campaigns"
      />
      <EvaluationSummaries sft={sft} />
      <ProvenancePanel sft={sft} />
      {debug ? (
        <details data-testid="sft-debug">
          <summary style={{ width: "fit-content", cursor: "pointer", color: "var(--sv-text-muted)", fontSize: 12, fontWeight: 650 }}>
            Debug · raw events, artifacts, usage
          </summary>
          {debug}
        </details>
      ) : null}
    </div>
  );
}

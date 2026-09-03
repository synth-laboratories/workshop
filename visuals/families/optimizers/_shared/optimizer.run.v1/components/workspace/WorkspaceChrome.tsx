import type { ReactNode } from "react";

/**
 * Algorithm-agnostic optimizer workspace chrome: a run header and a semantic
 * stage timeline. GEPA assembles these today; SFT and GELO templates can feed
 * their own stages/lanes/metrics without changes here.
 *
 * Only the identity line is sticky. The metric block scrolls away with the
 * canvas, because a two-row pinned header covered the evidence it described.
 * Templates keep the block short by tiering metrics: two to four `primary`
 * values stay on the line, everything else folds into `Run details`.
 */

function renderMetric(metric: WorkspaceMetric) {
  return (
    <div
      key={metric.label}
      className="sv-workspace-metric"
      title={metric.title}
      data-testid={metric.testId}
      aria-hidden="true"
    >
      <span>{metric.label}</span>
      <strong>{metric.value}</strong>
    </div>
  );
}

export type WorkspaceMetric = {
  label: string;
  value: string;
  title?: string;
  testId?: string;
  /**
   * "primary" stays on the always-visible header line; "detail" folds into the
   * `Run details` disclosure. Untiered metrics stay primary so a template that
   * has not been triaged yet keeps showing everything it used to show.
   */
  tier?: "primary" | "detail";
};

export type WorkspaceLane = {
  id: string;
  label: string;
  active: boolean;
  detail?: string;
};

export function WorkspaceHeader({
  statusText,
  statusTone,
  live,
  headline,
  detail,
  metrics,
  lanes,
  receipt,
  testId
}: {
  statusText: string;
  statusTone?: "live" | "ok" | "bad" | "warn";
  live?: boolean;
  headline: string;
  detail?: string;
  metrics: WorkspaceMetric[];
  lanes?: WorkspaceLane[];
  receipt?: ReactNode;
  testId?: string;
}) {
  const primary = metrics.filter((metric) => (metric.tier ?? "primary") === "primary");
  const detailMetrics = metrics.filter((metric) => metric.tier === "detail");
  return (
    <header className="sv-workspace-header" data-testid={testId}>
      <div className="sv-workspace-identity">
        <span className="sv-chip" data-tone={statusTone} data-testid="workspace-status">
          {live ? <span className="sv-live-dot" aria-hidden="true" /> : null}
          {statusText}
        </span>
        <strong style={{ fontSize: 13 }} aria-live="polite" data-testid="workspace-headline">{headline}</strong>
        {detail ? <span style={{ color: "var(--sv-text-muted)", fontSize: 12 }}>{detail}</span> : null}
        {lanes && lanes.length > 0 ? (
          <span style={{ display: "inline-flex", gap: 6, marginLeft: "auto" }}>
            {lanes.map((lane) => (
              <span key={lane.id} className="sv-lane" data-active={lane.active} data-testid={`workspace-lane-${lane.id}`}>
                <strong style={{ fontSize: 11 }}>{lane.label}</strong>
                {lane.detail ? <span>{lane.detail}</span> : null}
              </span>
            ))}
          </span>
        ) : null}
      </div>
      <div
        className="sv-workspace-metrics"
        role="group"
        aria-label={`Run metrics: ${metrics.map((metric) => `${metric.label} ${metric.value}`).join("; ")}`}
      >
        {primary.map(renderMetric)}
        {detailMetrics.length > 0 ? (
          <details className="sv-workspace-metric-more" data-testid="workspace-run-details">
            <summary className="sv-mono" aria-label={`Run details: ${detailMetrics.length} further metrics`}>
              Run details · {detailMetrics.length}
            </summary>
            <div className="sv-workspace-metrics">{detailMetrics.map(renderMetric)}</div>
          </details>
        ) : null}
      </div>
      {receipt}
    </header>
  );
}

/**
 * One honest stat instead of chart furniture. A plot drawn from fewer points
 * than it needs reads as a broken chart, not as "no trend yet", so every
 * series-backed panel routes through here before it draws axes.
 */
export function NotEnoughData({
  have,
  need,
  noun,
  detail,
  testId
}: {
  have: number;
  need: number;
  /** Singular noun for one datum, e.g. "metric sample" or "example dimension". */
  noun: string;
  detail?: ReactNode;
  testId?: string;
}) {
  return (
    <div className="sv-not-enough" data-testid={testId}>
      <strong className="sv-mono">{have} {noun}{have === 1 ? "" : "s"}</strong>
      {detail ? <span className="sv-not-enough-detail">{detail}</span> : null}
      <span className="sv-not-enough-need">
        {need === 2 ? "A trend needs at least 2." : `At least ${need} are needed to plot this.`}
      </span>
    </div>
  );
}

export type WorkspaceStage = {
  id: string;
  label: string;
  status: string;
  detail?: string;
  startedAt?: string;
  endedAt?: string;
};

function stageDuration(stage: WorkspaceStage): string | null {
  if (!stage.startedAt || !stage.endedAt) return null;
  const ms = Date.parse(stage.endedAt) - Date.parse(stage.startedAt);
  if (!Number.isFinite(ms) || ms < 0) return null;
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

export function StageTimeline({
  stages,
  selected,
  onSelect,
  testId
}: {
  stages: WorkspaceStage[];
  selected?: string | null;
  onSelect?: (id: string | null) => void;
  testId?: string;
}) {
  return (
    <nav aria-label="Optimizer stages" data-testid={testId}>
      <ol className="sv-stageline">
        {stages.map((stage) => {
          const duration = stageDuration(stage);
          const isSelected = selected === stage.id;
          return (
            <li key={stage.id}>
              <button
                type="button"
                className="sv-stage"
                data-status={stage.status}
                data-selected={isSelected}
                data-testid={`stage-${stage.id}`}
                aria-pressed={isSelected}
                aria-current={stage.status === "active" ? "step" : undefined}
                title={stage.detail ?? undefined}
                onClick={() => onSelect?.(isSelected ? null : stage.id)}
              >
                <span className="sv-stage-dot" aria-hidden="true" />
                {stage.label}
                {stage.status === "skipped" ? <span style={{ fontSize: 10, color: "var(--sv-text-faint)" }}>skipped</span> : null}
                {duration && stage.status !== "skipped" ? (
                  <span className="sv-mono" style={{ fontSize: 10, color: "var(--sv-text-faint)" }}>{duration}</span>
                ) : null}
              </button>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

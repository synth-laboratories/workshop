/**
 * Algorithm-agnostic optimizer workspace chrome: a sticky run header and a
 * semantic stage timeline. GEPA assembles these today; SFT and GELO templates
 * can feed their own stages/lanes/metrics without changes here.
 */

export type WorkspaceMetric = {
  label: string;
  value: string;
  title?: string;
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
  testId
}: {
  statusText: string;
  statusTone?: "live" | "ok" | "bad" | "warn";
  live?: boolean;
  headline: string;
  detail?: string;
  metrics: WorkspaceMetric[];
  lanes?: WorkspaceLane[];
  testId?: string;
}) {
  return (
    <header className="sv-workspace-header" data-testid={testId}>
      <div className="sv-workspace-header-row">
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
      <div className="sv-workspace-metrics" role="group" aria-label="Run metrics">
        {metrics.map((metric) => (
          <div key={metric.label} className="sv-workspace-metric" title={metric.title}>
            <span>{metric.label}</span>
            <strong>{metric.value}</strong>
          </div>
        ))}
      </div>
    </header>
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

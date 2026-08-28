import type { ReactNode } from "react";
import "./tokens.css";

/** What a template says it actually rendered.
 *
 * Workshop harvests these as DOM data and decides readiness itself; nothing
 * here is a passing boolean, it is a count and a transport state. A template
 * that declares an `observationContract` in its manifest must publish one of
 * these on a rendered element, or `capture_review` has no evidence to attach
 * and the visual can never be certified.
 *
 * Static projections (a sealed trace, a finished report) are `terminal` the
 * moment they render: nothing further is coming.
 */
export type SurfaceObservation = {
  transportState: string;
  rolloutCount?: number;
  renderedFrameCount?: number;
  semanticEventCount?: number;
  terminal?: boolean;
  error?: string | null;
};

export function surfaceObservationAttributes(observation: SurfaceObservation) {
  return {
    "data-visual-transport-state": observation.transportState,
    "data-visual-rollout-count": observation.rolloutCount ?? 0,
    "data-visual-rendered-frame-count": observation.renderedFrameCount ?? 0,
    "data-visual-semantic-event-count": observation.semanticEventCount ?? 0,
    "data-visual-terminal": observation.terminal ? "true" : "false",
    "data-visual-error": observation.error ?? ""
  };
}

export type VisualChromeProps = {
  kicker?: string;
  title: string;
  lede?: string;
  live?: boolean;
  footer?: ReactNode;
  children: ReactNode;
  testId?: string;
  observation?: SurfaceObservation;
};

/** Shared light chrome wrapper for all genre templates. */
export function VisualChrome({
  kicker,
  title,
  lede,
  live,
  footer,
  children,
  testId,
  observation
}: VisualChromeProps) {
  return (
    <div
      className="synth-visual-root"
      data-testid={testId}
      data-synth-visual=""
      {...(observation ? surfaceObservationAttributes(observation) : {})}
    >
      <header>
        {kicker ? (
          <p className="sv-kicker">
            {live ? <span className="sv-live-dot" aria-hidden="true" /> : null}
            {kicker}
          </p>
        ) : null}
        <h2 className="sv-title">{title}</h2>
        {lede ? <p className="sv-lede">{lede}</p> : null}
      </header>
      {children}
      {footer ? (
        <footer
          style={{
            marginTop: 20,
            paddingTop: 12,
            borderTop: "1px solid var(--sv-border)",
            color: "var(--sv-text-faint)",
            fontSize: 11
          }}
        >
          {footer}
        </footer>
      ) : null}
    </div>
  );
}

export type Metric = { label: string; value: string };

export function MetricStrip({ metrics }: { metrics: Metric[] }) {
  return (
    <div className="sv-metrics" role="group" aria-label="Key metrics">
      {metrics.map((m) => (
        <div key={m.label} className="sv-metric">
          <span>{m.label}</span>
          <strong>{m.value}</strong>
        </div>
      ))}
    </div>
  );
}

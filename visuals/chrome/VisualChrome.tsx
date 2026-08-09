import type { ReactNode } from "react";
import "./tokens.css";

export type VisualChromeProps = {
  kicker?: string;
  title: string;
  lede?: string;
  live?: boolean;
  footer?: ReactNode;
  children: ReactNode;
  testId?: string;
};

/** Shared light chrome wrapper for all genre templates. */
export function VisualChrome({
  kicker,
  title,
  lede,
  live,
  footer,
  children,
  testId
}: VisualChromeProps) {
  return (
    <div className="synth-visual-root" data-testid={testId} data-synth-visual="">
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

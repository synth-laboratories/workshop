import type { LiveEvalEvent } from "../../runtime/types.ts";
import { formatMissingNumber } from "../../runtime/liveStream.ts";

const SCALAR_KEYS = [
  "reward",
  "mean_reward",
  "train_reward",
  "metric",
  "value",
  "train_loss"
] as const;

export type MetricsStrip = {
  count: number;
  scalarLabel: string;
  scalarValue: string;
};

function asNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

/** Reduce ingested envelopes to a count plus the last scalar reward/metric. */
export function reduceMetricsStrip(events: LiveEvalEvent[]): MetricsStrip {
  let scalarLabel = "Reward";
  let scalar: number | undefined;
  for (const event of events) {
    const payload = event.payload ?? {};
    for (const key of SCALAR_KEYS) {
      const next = asNumber(payload[key]);
      if (next != null) {
        scalarLabel = key === "train_loss" ? "Loss" : key === "metric" || key === "value" ? "Metric" : "Reward";
        scalar = next;
        break;
      }
    }
  }
  return {
    count: events.length,
    scalarLabel,
    scalarValue: formatMissingNumber(scalar)
  };
}

export function Metrics({ events }: { events: LiveEvalEvent[] }) {
  const strip = reduceMetricsStrip(events);
  return (
    <section className="sv-section" aria-label="Metrics" data-testid="compose-metrics">
      <div className="sv-section-head">
        <h3>Metrics</h3>
      </div>
      <div className="sv-metrics" role="group" aria-label="Reduced metrics">
        <div className="sv-metric">
          <span>Events</span>
          <strong data-testid="compose-metrics-count">{String(strip.count)}</strong>
        </div>
        <div className="sv-metric">
          <span>{strip.scalarLabel}</span>
          <strong data-testid="compose-metrics-scalar">{strip.scalarValue}</strong>
        </div>
      </div>
    </section>
  );
}

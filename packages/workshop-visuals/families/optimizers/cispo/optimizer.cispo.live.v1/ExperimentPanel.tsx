import { useCollectionPage, type RunCollectionsClient } from "../../_shared/optimizer.run.v1/components/workspace/CollectionBrowser.tsx";

const object = (value: unknown): Record<string, unknown> => value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
const number = (value: unknown) => typeof value === "number" && Number.isFinite(value) ? value : null;
const display = (value: unknown) => number(value)?.toFixed(4) ?? "Not reported";

/** Presentation only: backend projections own all status, selection and cost. */
export function ExperimentPanel({ experiment, collections }: { experiment?: Record<string, unknown>; collections?: RunCollectionsClient }) {
  const checkpoints = useCollectionPage(collections, "candidates", { limit: 20 });
  if (!experiment || Object.keys(experiment).length === 0) return null;
  const phase = object(experiment.phase);
  const budget = object(experiment.budget);
  const throughput = object(experiment.throughput);
  const latency = object(experiment.operation_latency);
  const evaluations = Array.isArray(experiment.evaluations) ? experiment.evaluations.map(object) : [];
  const points = Array.isArray(experiment.cost_points) ? experiment.cost_points.map(object).filter(p => number(p.sequence) != null && number(p.usd) != null) : [];
  const maxSequence = Math.max(1, ...points.map(p => Number(p.sequence)));
  const maxCost = Math.max(0.001, ...points.map(p => Number(p.usd)), number(budget.cap_usd) ?? 0);
  const line = points.map(p => `${10 + 580 * Number(p.sequence) / maxSequence},${110 - 100 * Number(p.usd) / maxCost}`).join(" ");
  return <section data-testid="cispo-experiment-panel" aria-label="Container experiment">
    <h2>Container experiment</h2>
    <p>Phase: {String(phase.id ?? "Preparing")} · State: {String(experiment.status ?? "Running")}. Pause drains the current phase.</p>
    {experiment.blocked_reason != null && <p role="status">Blocked: {String(experiment.blocked_reason)}. Uncertain work must be reconciled before continuing.</p>}
    <h3>Aggregate budget</h3>
    <p>${display(budget.counted_or_reserved_usd)} counted or reserved / ${display(budget.cap_usd)} cap. Not invoice-reconciled.</p>
    {points.length > 1 && <svg role="img" aria-label="Counted or reserved dollars over durable event sequence" viewBox="0 0 600 130" style={{ width: "100%", maxHeight: 180 }}>
      <title>Counted or reserved cost; reservations may decrease after settlement</title>
      <path d="M10 10 V110 H590" fill="none" stroke="currentColor" opacity=".3" />
      <polyline points={line} fill="none" stroke="currentColor" strokeWidth="2" />
      <text x="10" y="126" fontSize="10" fill="currentColor">Durable event sequence · downsampled</text>
    </svg>}
    <h3>Heldout comparisons</h3>
    {Object.keys(latency).length > 0 && <table aria-label="Completed operation latency"><thead><tr><th>Operation lane</th><th>Completed calls</th><th>Mean call seconds</th></tr></thead><tbody>
      {Object.entries(latency).map(([lane, value]) => <tr key={lane}><td>{lane}</td><td>{String(object(value).count)}</td><td>{display(object(value).mean_seconds)}</td></tr>)}
    </tbody></table>}
    {Object.keys(throughput).length > 0 && <p>Last training segment: {display(throughput.admitted_examples_per_second)} admitted examples/sec · {String(throughput.trained_groups)} trained groups · {String(throughput.stale_groups)} stale · {String(throughput.skipped_groups)} skipped · {display(throughput.phase_seconds)} seconds (including setup and checkpoint publication).</p>}
    <p>Training reward is not heldout uplift. Validation selects the checkpoint; the final panel measures it independently.</p>
    <table><thead><tr><th>Panel / update</th><th>Baseline</th><th>Trained</th><th>Paired delta / 95% interval</th><th>Protocol</th></tr></thead>
      <tbody>{evaluations.map(row => <tr key={String(row.evaluation_id)}>
        <td>{String(row.panel)} / {String(row.target_update)}</td><td>{display(row.baseline_mean)}</td><td>{display(row.trained_mean)}</td>
        <td>{display(row.mean_delta)} [{Array.isArray(row.paired_bootstrap_95_interval) ? row.paired_bootstrap_95_interval.map(display).join(", ") : "not reported"}]</td>
        <td><details><summary>Panel and judge identity</summary><code>{String(row.panel_digest)}</code><br /><code>{String(row.judge_protocol_digest)}</code></details></td>
      </tr>)}</tbody></table>
    <h3>Checkpoint lineage</h3>
    <p>Publication, provider availability, and selection are separate facts. A training-state reference alone does not authorize resume.</p>
    <table><thead><tr><th>Checkpoint</th><th>Parent</th><th>Publication</th><th>Artifacts</th><th>Availability</th></tr></thead><tbody>
      {(checkpoints.page?.rows ?? []).filter(row => row.kind === "rl_checkpoint").map(row => {
        const details = object(row.details), checkpoint = object(details.checkpoint), artifacts = object(checkpoint.artifacts);
        const health = object(details.artifact_health);
        return <tr key={row.itemId}><td><code>{row.itemId}</code></td><td><code>{String(checkpoint.parent_checkpoint_id ?? "Baseline")}</code></td>
          <td>{String(checkpoint.publication_status ?? "Unknown")}</td><td>{artifacts.sampler_weights ? "Sampler " : ""}{artifacts.training_state ? "+ training state" : "· no training state"}</td>
          <td>{health.recorded_at ? `Checked ${String(health.recorded_at)} — inspect artifact result below` : "Unverified"}</td></tr>;
      })}
    </tbody></table>
    <p>The checkpoint collection below provides full details and additional pages.</p>
  </section>;
}

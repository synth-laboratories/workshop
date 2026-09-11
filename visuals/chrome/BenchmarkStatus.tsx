import type { BenchmarkObservation, PhaseClock } from "../runtime/benchmarkObservation.ts";

const number = (value: number | null): string => value === null ? "—" : value.toLocaleString(undefined, { maximumFractionDigits: 2 });
const clock = (value: PhaseClock): string => `${number(value.elapsed_seconds)}/${number(value.limit_seconds)}s (${value.scope})`;

/** Common producer values. Domain boards and scientific reports remain separate. */
export function BenchmarkStatus({ rows }: { rows: BenchmarkObservation[] }) {
  if (!rows.length) return null;
  const work = rows.some((row) => row.work.elapsed_seconds !== null || row.work.limit_seconds !== null);
  const verify = rows.some((row) => row.verify.elapsed_seconds !== null || row.verify.limit_seconds !== null);
  const calls = rows.some((row) => row.calls !== null || row.call_limit !== null || row.call_limit_unbounded);
  const steps = rows.some((row) => row.steps !== null || row.step_limit !== null);
  return <section aria-label="Benchmark status" style={{ overflowX: "auto" }}>
    <table className="sv-table">
      <thead><tr>
        <th scope="col">Task / lane</th><th scope="col">State</th>
        {work && <th scope="col">Work</th>}{verify && <th scope="col">Verify</th>}
        {calls && <th scope="col">Calls</th>}{steps && <th scope="col">Steps</th>}
        <th scope="col">Score</th><th scope="col">Usage</th><th scope="col">Stop / evidence</th>
      </tr></thead>
      <tbody>{rows.map((row) => <tr key={row.lane}>
        <th scope="row">{row.lane}</th><td>{row.phase}</td>
        {work && <td>{clock(row.work)}</td>}{verify && <td>{clock(row.verify)}</td>}
        {calls && <td>{number(row.calls)}/{row.call_limit_unbounded ? "∞" : number(row.call_limit)} ({row.limit_scope})</td>}
        {steps && <td>{number(row.steps)}/{number(row.step_limit)}</td>}
        <td>{row.scientific_status === "ungraded" ? "UNGRADED" : number(row.score)}<br /><small>{row.scientific_status}</small></td>
        <td title={`${row.usage.source} · ${row.usage.coverage}`}>
          {number(row.usage.total_tokens)} tokens · ${number(row.usage.cost_usd)}/${number(row.spend_limit_usd)} · {row.usage.status}
          <br /><small>{row.spend_enforcement}</small>
        </td>
        <td>{row.stop_reason || "—"}<br /><small>Observed {row.observed_at}</small>
          {row.artifact_path && <details><summary>Evidence location</summary><code>{row.artifact_path}</code></details>}
        </td>
      </tr>)}</tbody>
    </table>
  </section>;
}

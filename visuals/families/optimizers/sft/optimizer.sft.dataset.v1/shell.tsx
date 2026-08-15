import { formatMissingNumber } from "../../../../runtime/liveStream.ts";
import { OptimizerFamilyShell } from "../../_shared/optimizer.run.v1/components/FamilyShell.tsx";
import type { VisualBinding } from "../../../../runtime/types.ts";
import type { OptimizerEvent, OptimizerRun } from "../../_shared/optimizer.run.v1/components/projectEvents.ts";

export type ShellProps = {
  title?: string;
  lede?: string;
  data?: unknown;
  optimizer_run?: unknown;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
  events?: OptimizerEvent[];
  run?: OptimizerRun;
  loadError?: string;
};

export function Shell(props: ShellProps) {
  return (
    <OptimizerFamilyShell
      {...props}
      templateId="optimizer.sft.dataset.v1"
      kicker="SFT · dataset"
      testId="visual-optimizer-sft-dataset"
    >
      {({ projected }) => {
        const dataset = projected.sft?.dataset ?? {};
        const splits = (dataset.splits as Record<string, { count?: number; digest?: string; role?: string }> | undefined) ?? {};
        return (
          <section className="sv-section" aria-label="Dataset splits" data-testid="sft-dataset">
            <div className="sv-section-head">
              <h3>Dataset</h3>
              <span className="sv-mono">{String(dataset.format ?? "—")}</span>
            </div>
            <p className="sv-mono" style={{ fontSize: 12 }} data-testid="sft-dataset-digest">
              dataset_digest {String(dataset.dataset_digest ?? dataset.datasetDigest ?? "—")}
            </p>
            <table className="sv-table">
              <thead>
                <tr>
                  <th scope="col">Split</th>
                  <th scope="col">Role</th>
                  <th scope="col">Count</th>
                  <th scope="col">Digest</th>
                </tr>
              </thead>
              <tbody>
                {Object.entries(splits).map(([name, split]) => (
                  <tr key={name}>
                    <td className="sv-mono">{name}</td>
                    <td className="sv-mono">{split.role ?? name}</td>
                    <td className="sv-mono">{formatMissingNumber(split.count, 0)}</td>
                    <td className="sv-mono">{split.digest ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="sv-mono" style={{ fontSize: 12 }}>
              rejected {formatMissingNumber(dataset.rejected, 0)}
            </p>
          </section>
        );
      }}
    </OptimizerFamilyShell>
  );
}

export default Shell;

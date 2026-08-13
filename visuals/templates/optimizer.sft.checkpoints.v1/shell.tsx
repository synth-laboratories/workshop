import { formatMissingNumber } from "../../runtime/liveStream.ts";
import { OptimizerFamilyShell } from "../optimizer.run.v1/components/FamilyShell.tsx";
import type { VisualBinding } from "../../runtime/types.ts";
import type { OptimizerEvent, OptimizerRun } from "../optimizer.run.v1/components/projectEvents.ts";

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
      templateId="optimizer.sft.checkpoints.v1"
      kicker="SFT · checkpoints"
      testId="visual-optimizer-sft-checkpoints"
    >
      {({ projected }) => {
        const checkpoints = projected.sft?.checkpoints ?? [];
        const evals = projected.sft?.evaluations ?? [];
        const promotedId = String(
          ((projected.summary.summary as Record<string, unknown> | undefined)?.promotedCheckpointId ?? "") ||
            (checkpoints.find((c) => c.promoted)?.id ?? "")
        );
        return (
          <section className="sv-section" aria-label="Checkpoint rail" data-testid="sft-checkpoint-rail">
            <div className="sv-section-head">
              <h3>Checkpoints</h3>
              <span className="sv-mono">ready ≠ promoted</span>
            </div>
            <p className="sv-lede">A checkpoint can be ready for eval while promotion is still a later decision.</p>
            <table className="sv-table">
              <thead>
                <tr>
                  <th scope="col">Id</th>
                  <th scope="col">Step</th>
                  <th scope="col">Ready</th>
                  <th scope="col">Promoted</th>
                  <th scope="col">Selection</th>
                </tr>
              </thead>
              <tbody>
                {checkpoints.map((ckpt) => {
                  const id = String(ckpt.id ?? "");
                  const selection = evals.find(
                    (evaluation) =>
                      String(evaluation.item && typeof evaluation.item === "object"
                        ? (evaluation.item as { raw?: { checkpointId?: string } }).raw?.checkpointId
                        : "") === id &&
                      String(evaluation.role ?? evaluation.split) !== "heldout"
                  );
                  return (
                    <tr key={id} data-testid={`sft-ckpt-${id}`}>
                      <td className="sv-mono">{id}</td>
                      <td className="sv-mono">{formatMissingNumber(ckpt.step, 0)}</td>
                      <td>{ckpt.ready || ckpt.status === "ready" ? "ready" : "—"}</td>
                      <td>{ckpt.promoted || id === promotedId ? "promoted" : "—"}</td>
                      <td className="sv-mono">{formatMissingNumber(selection?.score)}</td>
                    </tr>
                  );
                })}
                {checkpoints.length === 0 ? (
                  <tr>
                    <td colSpan={5} style={{ color: "var(--sv-text-faint)" }}>No checkpoints at this cursor.</td>
                  </tr>
                ) : null}
              </tbody>
            </table>
          </section>
        );
      }}
    </OptimizerFamilyShell>
  );
}

export default Shell;

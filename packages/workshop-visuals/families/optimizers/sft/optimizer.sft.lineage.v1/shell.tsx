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
      templateId="optimizer.sft.lineage.v1"
      kicker="SFT · lineage"
      testId="visual-optimizer-sft-lineage"
    >
      {({ projected }) => {
        const nested = (projected.summary.summary as Record<string, unknown> | undefined) ?? {};
        const lineage = projected.sft?.lineage ?? {};
        const base = String(lineage.baseModel ?? nested.baseModel ?? "—");
        const adapter = String(lineage.adapter ?? nested.adapter ?? "—");
        const deployable = String(
          lineage.deployable ?? lineage.checkpointId ?? nested.promotedCheckpointId ?? "—"
        );
        return (
          <section className="sv-section" aria-label="Model lineage" data-testid="sft-lineage">
            <div className="sv-section-head">
              <h3>Lineage</h3>
              <span className="sv-mono">{String(lineage.status ?? projected.summary.status ?? "—")}</span>
            </div>
            <ol style={{ display: "grid", gap: 8, margin: 0, padding: 0, listStyle: "none" }}>
              <li style={{ border: "1px solid var(--sv-border)", borderRadius: 8, padding: 10 }}>
                <span style={{ fontSize: 10, color: "var(--sv-text-muted)", textTransform: "uppercase" }}>Base</span>
                <div className="sv-mono">{base}</div>
              </li>
              <li style={{ textAlign: "center", color: "var(--sv-text-faint)" }}>↓</li>
              <li style={{ border: "1px solid var(--sv-border)", borderRadius: 8, padding: 10 }}>
                <span style={{ fontSize: 10, color: "var(--sv-text-muted)", textTransform: "uppercase" }}>Adapter</span>
                <div className="sv-mono">{adapter}</div>
              </li>
              <li style={{ textAlign: "center", color: "var(--sv-text-faint)" }}>↓</li>
              <li style={{ border: "1px solid var(--sv-border)", borderRadius: 8, padding: 10 }}>
                <span style={{ fontSize: 10, color: "var(--sv-text-muted)", textTransform: "uppercase" }}>Deployable</span>
                <div className="sv-mono">{deployable}</div>
                {lineage.digest ? <div className="sv-mono" style={{ color: "var(--sv-text-faint)" }}>{String(lineage.digest)}</div> : null}
              </li>
            </ol>
          </section>
        );
      }}
    </OptimizerFamilyShell>
  );
}

export default Shell;

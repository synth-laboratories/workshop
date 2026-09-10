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
      templateId="optimizer.sft.examples.v1"
      kicker="SFT · examples"
      testId="visual-optimizer-sft-examples"
    >
      {({ projected }) => {
        const examples = projected.sft?.examples ?? [];
        return (
          <section className="sv-section" aria-label="Paired examples" data-testid="sft-examples">
            <div className="sv-section-head">
              <h3>Baseline vs checkpoint</h3>
              <span className="sv-mono">{examples.length}</span>
            </div>
            {examples.map((example) => (
              <article key={String(example.id)} style={{ marginTop: 10, border: "1px solid var(--sv-border)", borderRadius: 8, padding: 10 }}>
                <div className="sv-mono" style={{ color: "var(--sv-text-muted)", marginBottom: 8 }}>
                  {String(example.intent ?? example.id ?? "—")}
                </div>
                <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
                  <div>
                    <strong>Baseline</strong>
                    <p style={{ margin: "4px 0 0", fontSize: 12 }}>{String(example.baseline ?? "—")}</p>
                  </div>
                  <div>
                    <strong>Checkpoint</strong>
                    <p style={{ margin: "4px 0 0", fontSize: 12 }}>
                      {String(example.checkpoint ?? example.ckpt ?? example.selected ?? "—")}
                    </p>
                  </div>
                </div>
              </article>
            ))}
            {examples.length === 0 ? (
              <p style={{ color: "var(--sv-text-faint)", fontSize: 12 }}>No paired examples at this cursor.</p>
            ) : null}
          </section>
        );
      }}
    </OptimizerFamilyShell>
  );
}

export default Shell;

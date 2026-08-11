import type { ProjectedState } from "../components/projectEvents.ts";

/** First-class SFT overlay for live recipes, imported runs, and fixtures. */
export function SftOverlay({ state }: { state: ProjectedState }) {
  const sft = state.sft;
  if (!sft) return null;
  const { curves, checkpoints, evaluations, dataset, compute, examples, lineage } = sft;
  const summary = (state.summary.summary as Record<string, unknown> | undefined) ?? {};
  const step = curves.steps[curves.steps.length - 1];
  const epoch = curves.epochs[curves.epochs.length - 1];
  const selectionEvals = evaluations.filter(
    (evaluation) => String(evaluation.role ?? evaluation.split) !== "heldout"
  );
  const heldoutEvals = evaluations.filter(
    (evaluation) => String(evaluation.role ?? evaluation.split) === "heldout"
  );
  const splits = (dataset.splits as Record<string, { count?: number; digest?: string }> | undefined) ?? {};

  return (
    <>
      <section className="sv-section" aria-label="SFT run header">
        <div className="sv-section-head">
          <h3>Training</h3>
          <span className="sv-mono">live · durable replay</span>
        </div>
        <dl
          style={{
            display: "grid",
            gridTemplateColumns: "auto 1fr",
            gap: "4px 12px",
            margin: 0,
            fontSize: 12
          }}
        >
          <dt>Base</dt>
          <dd className="sv-mono">{String(summary.baseModel ?? lineage?.baseModel ?? "—")}</dd>
          <dt>Adapter</dt>
          <dd className="sv-mono">{String(summary.adapter ?? lineage?.adapter ?? "—")}</dd>
          <dt>Backend</dt>
          <dd className="sv-mono">{String(summary.backend ?? compute.provider ?? "—")}</dd>
          <dt>Status</dt>
          <dd className="sv-mono">{String(state.summary.status ?? "—")}</dd>
          <dt>Step / epoch</dt>
          <dd className="sv-mono">
            {step ?? "—"} / {epoch ?? "—"}
          </dd>
          <dt>Cost</dt>
          <dd className="sv-mono">${(state.usage.costUsd ?? 0).toFixed(2)}</dd>
        </dl>
      </section>

      <section className="sv-section" aria-label="Training curves">
        <div className="sv-section-head">
          <h3>Training curves</h3>
          <span className="sv-mono">train · val · lr</span>
        </div>
        <div role="img" aria-label="Train and validation loss over steps">
          <svg viewBox="0 0 320 140" width="100%" style={{ maxHeight: 160 }}>
            {curves.trainLoss.map((loss, index) => {
              const x = 30 + (index / Math.max(curves.trainLoss.length - 1, 1)) * 260;
              const y = 120 - (Math.min(loss, 3) / 3) * 100;
              return <circle key={`t-${index}`} cx={x} cy={y} r={4} fill="#f05f22" />;
            })}
            {curves.validationLoss.map((loss, index) => {
              const x = 30 + (index / Math.max(curves.validationLoss.length - 1, 1)) * 260;
              const y = 120 - (Math.min(loss, 3) / 3) * 100;
              return <circle key={`v-${index}`} cx={x} cy={y} r={4} fill="#5c6573" />;
            })}
          </svg>
          <p className="sv-mono" style={{ fontSize: 11 }}>
            steps [{curves.steps.join(", ")}] · train [
            {curves.trainLoss.map((v) => v.toFixed(2)).join(", ")}] · val [
            {curves.validationLoss.map((v) => v.toFixed(2)).join(", ")}] · lr [
            {curves.learningRate.map((v) => v.toExponential(1)).join(", ")}]
          </p>
        </div>
      </section>

      <section className="sv-section" aria-label="Checkpoint rail">
        <div className="sv-section-head">
          <h3>Checkpoints</h3>
        </div>
        <ol style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
          {checkpoints.map((ckpt) => (
            <li key={String(ckpt.id)} className="sv-mono">
              {String(ckpt.id)} · step {String(ckpt.step ?? "—")}
              {ckpt.promoted || ckpt.status === "promoted" ? " · promoted" : ""}
              {ckpt.digest ? ` · ${String(ckpt.digest).slice(0, 18)}` : ""}
            </li>
          ))}
          {checkpoints.length === 0 ? <li style={{ opacity: 0.7 }}>No checkpoints yet.</li> : null}
        </ol>
      </section>

      <section className="sv-section" aria-label="Selection evaluations">
        <div className="sv-section-head">
          <h3>Selection evals</h3>
          <span className="sv-mono">affects promotion</span>
        </div>
        <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
          {selectionEvals.map((evaluation, index) => (
            <li key={index} className="sv-mono">
              {String(evaluation.split ?? "selection")} · {String(evaluation.metric ?? "metric")}=
              {String(evaluation.score ?? "—")}
              {evaluation.accuracy != null ? ` · acc=${String(evaluation.accuracy)}` : ""}
            </li>
          ))}
          {selectionEvals.length === 0 ? <li style={{ opacity: 0.7 }}>None yet.</li> : null}
        </ul>
      </section>

      <section className="sv-section" aria-label="Heldout evaluations">
        <div className="sv-section-head">
          <h3>Heldout measurement</h3>
          <span className="sv-mono">measurement only · not for promotion</span>
        </div>
        <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
          {heldoutEvals.map((evaluation, index) => (
            <li key={index} className="sv-mono">
              {String(evaluation.metric ?? "metric")}={String(evaluation.score ?? "—")}
              {evaluation.accuracy != null ? ` · acc=${String(evaluation.accuracy)}` : ""}
            </li>
          ))}
          {heldoutEvals.length === 0 ? <li style={{ opacity: 0.7 }}>Not evaluated yet.</li> : null}
        </ul>
      </section>

      <section className="sv-section" aria-label="Dataset splits">
        <div className="sv-section-head">
          <h3>Dataset</h3>
        </div>
        <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
          {Object.entries(splits).map(([name, split]) => (
            <li key={name} className="sv-mono">
              {name}: {split.count ?? "—"} · {String(split.digest ?? "").slice(0, 18) || "—"}
            </li>
          ))}
          {dataset.rejected != null ? (
            <li className="sv-mono">rejected: {String(dataset.rejected)}</li>
          ) : null}
        </ul>
      </section>

      <section className="sv-section" aria-label="Compute">
        <div className="sv-section-head">
          <h3>Compute</h3>
        </div>
        <p className="sv-mono" style={{ fontSize: 12, margin: 0 }}>
          {String(compute.provider ?? "—")} · {String(compute.gpu ?? "—")} · util=
          {compute.utilization != null ? Number(compute.utilization).toFixed(2) : "—"} · tok/s=
          {String(compute.tokensPerSec ?? "—")}
        </p>
      </section>

      <section className="sv-section" aria-label="Example comparisons">
        <div className="sv-section-head">
          <h3>Examples</h3>
          <span className="sv-mono">baseline vs selected</span>
        </div>
        <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
          {examples.map((example) => (
            <li key={String(example.id)} style={{ marginBottom: 8 }}>
              <div className="sv-mono">{String(example.intent ?? example.id)}</div>
              <div>baseline: {String(example.baseline ?? "—")}</div>
              <div>selected: {String(example.selected ?? "—")}</div>
            </li>
          ))}
          {examples.length === 0 ? <li style={{ opacity: 0.7 }}>No examples yet.</li> : null}
        </ul>
      </section>

      {lineage && Object.keys(lineage).length > 0 ? (
        <section className="sv-section" aria-label="Model lineage">
          <div className="sv-section-head">
            <h3>Lineage</h3>
          </div>
          <p className="sv-mono" style={{ fontSize: 12, margin: 0 }}>
            {String(lineage.baseModel ?? "—")} → {String(lineage.adapter ?? "—")} →{" "}
            {String(lineage.checkpointId ?? "—")}
          </p>
        </section>
      ) : null}
    </>
  );
}

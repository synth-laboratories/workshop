import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import type { VisualBinding } from "../../../runtime/types.ts";
import type { ReactNode } from "react";

type Progress = {
  phase?: string;
  completed?: number;
  total?: number;
  elapsed?: string;
  eta?: string;
  usage?: string;
  cost?: string;
};

type Metric = {
  label: string;
  value?: string | number | null;
  detail?: string;
  tone?: "positive" | "negative" | "neutral";
};

type Arm = {
  id: string;
  label?: string;
  status?: string;
  score?: string | number | null;
  detail?: string;
  baseline?: boolean;
  selected?: boolean;
  metrics?: Record<string, string | number | null>;
};

type ComparisonColumn = {
  id: string;
  label: string;
  format?: "number" | "percent" | "currency" | "duration";
  direction?: "higher" | "lower";
};

type Comparison = {
  primaryMetric?: string;
  columns?: ComparisonColumn[];
};

type Evidence = {
  id: string;
  title: string;
  kind?: string;
  status?: string;
  summary?: string;
  visualId?: string;
};

type Rollout = {
  id: string;
  label?: string;
  seed?: string | number;
  reward?: number | null;
  steps?: number | null;
  achievements?: number | null;
  stopReason?: string;
  status?: string;
  traceId?: string;
};

type TraceReference = {
  id: string;
  label?: string;
  traceId?: string;
  visualId?: string;
  seed?: string | number;
  reward?: number | null;
  steps?: number | null;
  stopReason?: string;
  summary?: string;
};

type ArtifactReference = {
  id: string;
  label?: string;
  kind?: string;
  path?: string;
  visualId?: string;
  traceId?: string;
  containerId?: string;
  summary?: string;
};

type ContextRecord = Record<string, string | number | boolean | null | undefined>;

type OptionalCollection<T> = {
  prominence?: "summary" | "detail";
  items?: T[];
};

type ExperimentResults = {
  metrics?: Metric[];
  rollouts?: Rollout[];
};

type HypothesisVerdict = "true" | "false" | "needs_more_analysis" | "unresolved";
type Confidence = "low" | "medium" | "high" | "overwhelming";

type HypothesisResolution = {
  id: string;
  claim: string;
  verdict?: HypothesisVerdict;
  confidence?: Confidence;
  why?: string;
};

type Assessment = {
  summary?: string;
  confidence?: Confidence;
  nextStep?: string;
};

type LineageNode = {
  id: string;
  label: string;
  kind?: string;
  status?: string;
};

type ExperimentOverview = {
  schemaVersion?: string;
  experimentId?: string;
  title?: string;
  question?: string;
  hypothesis?: string;
  hypotheses?: HypothesisResolution[];
  assessment?: Assessment;
  results?: ExperimentResults;
  traces?: OptionalCollection<TraceReference>;
  task?: ContextRecord;
  runtime?: ContextRecord;
  artifacts?: OptionalCollection<ArtifactReference>;
  provenance?: ContextRecord;
  status?: string;
  progress?: Progress;
  metrics?: Metric[];
  arms?: Arm[];
  comparison?: Comparison;
  evidence?: Evidence[];
  lineage?: LineageNode[];
  limitations?: string[];
};

export type ShellProps = {
  title?: string;
  lede?: string;
  experiment?: ExperimentOverview;
  data?: ExperimentOverview;
  bindings?: VisualBinding[];
};

const MISSING = "—";

enum ExperimentProjectionErrorCode {
  ModelMissing = "experiment_model_missing",
  ProgressMissing = "experiment_progress_missing",
  ProgressOutOfRange = "experiment_progress_out_of_range",
  TerminalPhaseMismatch = "experiment_terminal_phase_mismatch",
  TerminalRolloutsMissing = "experiment_terminal_rollouts_missing"
}

class ExperimentProjectionError extends Error {
  constructor(readonly code: ExperimentProjectionErrorCode, message: string) {
    super(message);
    this.name = "ExperimentProjectionError";
  }
}

function assertExperimentProjection(experiment: ExperimentOverview): void {
  // The shared template also renders descriptive, non-executable experiments.
  // A runtime harness marks the strict executable-evaluation contract.
  if (!text(experiment.runtime?.harness)) return;
  const status = experiment.status;
  const terminal = status === "completed" || status === "failed" || status === "cancelled";
  if (!text(experiment.runtime?.model)) {
    throw new ExperimentProjectionError(ExperimentProjectionErrorCode.ModelMissing, "Runtime model is required for an evaluation projection.");
  }
  if (!experiment.progress || experiment.progress.completed == null || experiment.progress.total == null) {
    throw new ExperimentProjectionError(ExperimentProjectionErrorCode.ProgressMissing, "Evaluation progress requires completed and total counts.");
  }
  if (experiment.progress.total <= 0 || experiment.progress.completed < 0 || experiment.progress.completed > experiment.progress.total) {
    throw new ExperimentProjectionError(ExperimentProjectionErrorCode.ProgressOutOfRange, "Evaluation progress counts are outside the declared plan.");
  }
  if (terminal && experiment.progress.phase !== status) {
    throw new ExperimentProjectionError(ExperimentProjectionErrorCode.TerminalPhaseMismatch, `Terminal status ${status} cannot use phase ${experiment.progress.phase ?? "missing"}.`);
  }
  if (terminal && (experiment.results?.rollouts?.length ?? 0) !== experiment.progress.total) {
    throw new ExperimentProjectionError(ExperimentProjectionErrorCode.TerminalRolloutsMissing, "A terminal evaluation must include one rollout result per planned trial.");
  }
}

function ProjectionFailure({ error, title }: { error: ExperimentProjectionError; title?: string }) {
  return <VisualChrome title={title ?? "Invalid experiment projection"} lede="The producer emitted an inconsistent evaluation record." testId="visual-experiment-overview-error">
    <section className="sv-section" role="alert" style={{ borderColor: "#b42318", background: "#fff5f5" }}>
      <strong style={{ color: "#b42318" }}>{error.code}</strong>
      <p style={{ marginBottom: 0 }}>{error.message}</p>
    </section>
  </VisualChrome>;
}

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function finiteNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function normalizeProgress(value: unknown): Progress | undefined {
  const raw = record(value);
  if (!raw) return undefined;
  return {
    phase: text(raw.phase),
    completed: finiteNumber(raw.completed),
    total: finiteNumber(raw.total),
    elapsed: text(raw.elapsed),
    eta: text(raw.eta),
    usage: text(raw.usage),
    cost: text(raw.cost)
  };
}

function normalizeOverview(value: unknown): ExperimentOverview | null {
  const raw = record(value);
  if (!raw) return null;
  const array = <T,>(field: unknown): T[] => Array.isArray(field) ? field.filter((item) => record(item)) as T[] : [];
  const hypotheses = array<HypothesisResolution>(raw.hypotheses).map((item) => ({
    id: text(item.id) ?? text(item.claim) ?? "hypothesis",
    claim: text(item.claim) ?? "Hypothesis not recorded",
    verdict: (["true", "false", "needs_more_analysis", "unresolved"] as const).includes(item.verdict as HypothesisVerdict) ? item.verdict : "unresolved",
    confidence: (["low", "medium", "high", "overwhelming"] as const).includes(item.confidence as Confidence) ? item.confidence : undefined,
    why: text(item.why)
  }));
  const assessment = record(raw.assessment);
  const results = record(raw.results);
  const collection = <T,>(value: unknown): OptionalCollection<T> => {
    if (Array.isArray(value)) return { items: value.filter((item) => record(item)) as T[] };
    const wrapper = record(value);
    return wrapper ? {
      prominence: wrapper.prominence === "summary" ? "summary" : "detail",
      items: array<T>(wrapper.items)
    } : { items: [] };
  };
  return {
    schemaVersion: text(raw.schemaVersion),
    experimentId: text(raw.experimentId),
    title: text(raw.title),
    question: text(raw.question),
    hypothesis: text(raw.hypothesis),
    hypotheses,
    assessment: assessment ? {
      summary: text(assessment.summary),
      confidence: (["low", "medium", "high", "overwhelming"] as const).includes(assessment.confidence as Confidence) ? assessment.confidence as Confidence : undefined,
      nextStep: text(assessment.nextStep)
    } : undefined,
    results: results ? {
      metrics: array<Metric>(results.metrics),
      rollouts: array<Rollout>(results.rollouts)
    } : undefined,
    traces: collection<TraceReference>(raw.traces),
    task: record(raw.task) as ContextRecord | undefined,
    runtime: record(raw.runtime) as ContextRecord | undefined,
    artifacts: collection<ArtifactReference>(raw.artifacts),
    provenance: record(raw.provenance) as ContextRecord | undefined,
    status: text(raw.status),
    progress: normalizeProgress(raw.progress),
    metrics: array<Metric>(raw.metrics),
    arms: array<Arm>(raw.arms),
    comparison: record(raw.comparison) as Comparison | undefined,
    evidence: array<Evidence>(raw.evidence),
    lineage: array<LineageNode>(raw.lineage),
    limitations: Array.isArray(raw.limitations) ? raw.limitations.map(text).filter((item): item is string => Boolean(item)) : []
  };
}

function metricNumber(arm: Arm, metricId: string): number | null {
  const value = arm.metrics?.[metricId];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function formatComparisonValue(value: string | number | null | undefined, format?: ComparisonColumn["format"]): string {
  if (value == null) return MISSING;
  if (typeof value !== "number") return value;
  if (format === "percent") return `${(value * 100).toFixed(1)}%`;
  if (format === "currency") return `$${value.toFixed(2)}`;
  if (format === "duration") return `${value.toFixed(value < 10 ? 1 : 0)}s`;
  return Number.isInteger(value) ? String(value) : value.toFixed(3);
}

function ComparisonTable({ arms, comparison }: { arms: Arm[]; comparison?: Comparison }) {
  const columns = Array.isArray(comparison?.columns)
    ? comparison.columns.filter((column) => text(column?.id) && text(column?.label))
    : [];
  if (!columns.length || !arms.length) return null;
  const primary = columns.find((column) => column.id === comparison?.primaryMetric) ?? columns[0];
  const observed = arms.map((arm) => metricNumber(arm, primary.id)).filter((value): value is number => value != null);
  const min = observed.length ? Math.min(...observed) : 0;
  const max = observed.length ? Math.max(...observed) : 0;
  const span = max - min;
  const best = observed.length ? (primary.direction === "lower" ? min : max) : null;

  return <section className="sv-section" aria-label="Run comparison">
    <div className="sv-section-head"><h3>Run comparison</h3><span>{primary.label} highlighted</span></div>
    <div style={{ overflowX: "auto" }}>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 10 }}>
        <thead><tr>{["Variant", ...columns.map((column) => column.label)].map((label) => <th key={label} scope="col" style={{ padding: "7px 8px", borderBottom: "1px solid var(--sv-border)", color: "var(--sv-text-faint)", textAlign: label === "Variant" ? "left" : "right", fontWeight: 600 }}>{label}</th>)}</tr></thead>
        <tbody>{arms.map((arm) => {
          const primaryValue = metricNumber(arm, primary.id);
          const relative = primaryValue == null ? 0 : span === 0 ? 100 : Math.max(8, (primaryValue - min) / span * 100);
          return <tr key={arm.id} style={{ background: arm.selected ? "#fff7f2" : undefined }}>
            <th scope="row" style={{ minWidth: 140, padding: "9px 8px", borderBottom: "1px solid var(--sv-border)", textAlign: "left", fontWeight: 600 }}>
              <span>{arm.label ?? arm.id}</span>{arm.baseline ? <small style={{ marginLeft: 5, color: "var(--sv-text-faint)" }}>baseline</small> : null}{arm.selected ? <small style={{ marginLeft: 5, color: "#c2410c" }}>selected</small> : null}
              {primaryValue != null ? <span aria-hidden style={{ display: "block", width: `${relative}%`, height: 3, marginTop: 6, borderRadius: 99, background: primaryValue === best ? "#18794e" : "#cbd2dc" }} /> : null}
            </th>
            {columns.map((column) => {
              const value = arm.metrics?.[column.id];
              const isBest = typeof value === "number" && value === best && column.id === primary.id;
              return <td key={column.id} style={{ padding: "9px 8px", borderBottom: "1px solid var(--sv-border)", textAlign: "right", fontFamily: "var(--sv-mono)", color: isBest ? "#18794e" : undefined, fontWeight: isBest ? 700 : 400 }}>{formatComparisonValue(value, column.format)}</td>;
            })}
          </tr>;
        })}</tbody>
      </table>
    </div>
    <p style={{ margin: "8px 0 0", color: "var(--sv-text-faint)", fontSize: 9 }}>Missing measurements are shown as {MISSING} and are never ranked.</p>
  </section>;
}

function statusTone(status?: string): string {
  const normalized = status?.toLowerCase();
  if (normalized === "completed" || normalized === "selected" || normalized === "passed") return "#18794e";
  if (normalized === "failed" || normalized === "aborted" || normalized === "excluded") return "#b42318";
  if (normalized === "running" || normalized === "evaluating") return "#c2410c";
  return "#697386";
}

function verdictLabel(verdict?: HypothesisVerdict): string {
  if (verdict === "true") return "True";
  if (verdict === "false") return "False";
  if (verdict === "needs_more_analysis") return "Needs more analysis";
  return "Unresolved";
}

function verdictTone(verdict?: HypothesisVerdict): string {
  if (verdict === "true") return "#18794e";
  if (verdict === "false") return "#b42318";
  if (verdict === "needs_more_analysis") return "#c2410c";
  return "#697386";
}

function Hypotheses({ legacyHypothesis, hypotheses }: { legacyHypothesis?: string; hypotheses: HypothesisResolution[] }) {
  const rows = hypotheses.length ? hypotheses : legacyHypothesis ? [{ id: "hypothesis", claim: legacyHypothesis, verdict: "unresolved" as const }] : [];
  return <section className="sv-section" aria-label="Hypothesis resolution" style={{ padding: 0, overflow: "hidden" }}>
    <div className="sv-section-head" style={{ padding: "8px 10px" }}><h3>Conclusion</h3><span>Claim · verdict · evidence</span></div>
    {rows.length ? <div>{rows.map((item) => <article key={item.id} style={{ display: "grid", gridTemplateColumns: "minmax(150px,1fr) auto", gap: "7px 12px", padding: "10px", borderTop: "1px solid var(--sv-border)" }}>
      <strong style={{ fontSize: 11 }}>{item.claim}</strong>
      <span style={{ color: verdictTone(item.verdict), fontSize: 10, fontWeight: 700 }}>{verdictLabel(item.verdict)}</span>
      <p style={{ gridColumn: "1 / -1", margin: 0, color: "var(--sv-text-faint)", fontSize: 10, lineHeight: 1.45 }}>{item.why ?? (item.verdict === "unresolved" ? "Waiting for evidence" : MISSING)}</p>
      <small style={{ color: "var(--sv-text-faint)", textTransform: "capitalize" }}>Confidence · {item.confidence ?? MISSING}</small>
    </article>)}</div> : <p style={{ margin: 0, padding: 10, color: "var(--sv-text-faint)", fontSize: 10 }}>No hypothesis has been recorded.</p>}
  </section>;
}

function OverviewStrip({ status, arms, model, progress }: { status: string; arms: Arm[]; model?: unknown; progress?: Progress }) {
  const items = [
    ["Status", status],
    ["Runs", arms.length || MISSING],
    ["Model", text(model) ?? MISSING],
    ["Progress", progress?.completed != null && progress?.total != null ? `${progress.completed}/${progress.total}` : MISSING]
  ];
  return <section className="sv-section" aria-label="Experiment summary" style={{ padding: "10px 12px", background: "#fffaf4", borderColor: "#e2d2c2" }}><dl style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit,minmax(90px,1fr))", gap: 10, margin: 0 }}>{items.map(([label, value]) => <div key={String(label)}><dt style={{ color: "var(--sv-text-faint)", fontSize: 8, letterSpacing: ".06em", textTransform: "uppercase" }}>{label}</dt><dd style={{ margin: "3px 0 0", overflow: "hidden", fontFamily: "var(--sv-mono)", fontSize: 10, textOverflow: "ellipsis", textTransform: label === "Status" ? "capitalize" : undefined, whiteSpace: "nowrap" }}>{String(value)}</dd></div>)}</dl></section>;
}

function Disclosure({ title, summary, children, defaultOpen = false }: { title: string; summary?: string; children: ReactNode; defaultOpen?: boolean }) {
  return <details className="sv-section" style={{ padding: 0 }} open={defaultOpen || undefined}>
    <summary style={{ display: "flex", alignItems: "center", gap: 7, padding: "6px 8px", cursor: "pointer", listStyle: "none" }}><strong style={{ fontSize: 10 }}>{title}</strong>{summary ? <span style={{ marginLeft: "auto", color: "var(--sv-text-faint)", fontSize: 8 }}>{summary}</span> : null}<span aria-hidden style={{ color: "#2563a7", fontSize: 8 }}>Show</span></summary>
    <div style={{ display: "grid", gap: 6, padding: "0 6px 6px" }}>{children}</div>
  </details>;
}

function AssessmentPanel({ assessment }: { assessment?: Assessment }) {
  if (!assessment?.summary && !assessment?.confidence && !assessment?.nextStep) return null;
  return <section className="sv-section" aria-label="Experiment assessment"><div className="sv-section-head"><h3>Assessment</h3><span>{assessment.confidence ? `${assessment.confidence} confidence` : "confidence not recorded"}</span></div><div style={{ display: "grid", gridTemplateColumns: "minmax(160px,1.4fr) minmax(130px,1fr)", gap: 8 }}><div><span style={{ color: "var(--sv-text-faint)", fontSize: 9, textTransform: "uppercase" }}>Interpretation</span><strong style={{ display: "block", marginTop: 3, fontSize: 11 }}>{assessment.summary ?? MISSING}</strong></div><div><span style={{ color: "var(--sv-text-faint)", fontSize: 9, textTransform: "uppercase" }}>Next experiment</span><strong style={{ display: "block", marginTop: 3, fontSize: 11 }}>{assessment.nextStep ?? MISSING}</strong></div></div></section>;
}

function display(value: unknown): string {
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  return text(value) ?? MISSING;
}

function ProgressPanel({ progress, status }: { progress?: Progress; status?: string }) {
  const completed = progress?.completed;
  const total = progress?.total;
  const determinate = completed != null && total != null && total > 0;
  const percent = determinate ? Math.max(0, Math.min(100, completed / total * 100)) : 0;
  return <section className="sv-section" aria-label="Experiment progress">
    <div style={{ display: "flex", justifyContent: "space-between", gap: 16, alignItems: "baseline" }}>
      <div><span style={{ fontSize: 10, color: "var(--sv-text-faint)", textTransform: "uppercase", letterSpacing: ".08em" }}>Progress</span><h3 style={{ margin: "3px 0 0", fontSize: 15 }}>{progress?.phase ?? status ?? "Planned"}</h3></div>
      <strong style={{ fontFamily: "var(--sv-mono)", fontSize: 12 }}>{determinate ? `${completed}/${total}` : MISSING}</strong>
    </div>
    <div role="progressbar" aria-valuemin={0} aria-valuemax={determinate ? total : undefined} aria-valuenow={determinate ? completed : undefined} aria-label="Experiment completion" style={{ height: 8, borderRadius: 99, overflow: "hidden", background: "#e8ebef", marginTop: 12 }}>
      <span style={{ display: "block", height: "100%", width: `${percent}%`, background: "#f05f22", transition: "width 180ms ease" }} />
    </div>
    <dl style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit,minmax(90px,1fr))", gap: 10, margin: "13px 0 0" }}>
      {[["Elapsed", progress?.elapsed], ["ETA", progress?.eta], ["Usage", progress?.usage], ["Cost", progress?.cost]].map(([label, value]) => <div key={label}><dt style={{ fontSize: 9, color: "var(--sv-text-faint)", textTransform: "uppercase" }}>{label}</dt><dd style={{ margin: "3px 0 0", fontFamily: "var(--sv-mono)", fontSize: 11 }}>{value ?? MISSING}</dd></div>)}
    </dl>
  </section>;
}

function Metrics({ metrics }: { metrics: Metric[] }) {
  if (!metrics.length) return null;
  return <section className="sv-section" aria-label="Experiment results"><div className="sv-metrics">{metrics.map((metric, index) => <div className="sv-metric" key={`${metric.label}-${index}`}><span>{metric.label}</span><strong style={{ color: metric.tone === "positive" ? "#18794e" : metric.tone === "negative" ? "#b42318" : undefined }}>{display(metric.value)}</strong>{metric.detail ? <small>{metric.detail}</small> : null}</div>)}</div></section>;
}

function Arms({ arms }: { arms: Arm[] }) {
  if (!arms.length) return <section className="sv-section"><h3>Variants</h3><p style={{ color: "var(--sv-text-faint)", fontSize: 11 }}>No variants have been recorded.</p></section>;
  return <section className="sv-section"><div className="sv-section-head"><h3>Variants</h3><span>{arms.length} total</span></div><div style={{ display: "grid", gap: 7 }}>{arms.map((arm) => <article key={arm.id} style={{ display: "grid", gridTemplateColumns: "12px minmax(100px,1fr) auto", gap: 9, alignItems: "center", padding: "10px 11px", border: `1px solid ${arm.selected ? "#f05f22" : "var(--sv-border)"}`, borderRadius: 8, background: arm.selected ? "#fff7f2" : "#fff" }}><span aria-hidden style={{ width: 8, height: 8, borderRadius: 99, background: arm.baseline ? "#7c8798" : arm.selected ? "#f05f22" : statusTone(arm.status) }} /><div style={{ minWidth: 0 }}><strong style={{ display: "block", fontSize: 11, overflow: "hidden", textOverflow: "ellipsis" }}>{arm.label ?? arm.id}{arm.baseline ? " · baseline" : ""}{arm.selected ? " · selected" : ""}</strong>{arm.detail ? <small style={{ color: "var(--sv-text-faint)" }}>{arm.detail}</small> : null}</div><div style={{ textAlign: "right" }}><strong style={{ display: "block", fontFamily: "var(--sv-mono)", fontSize: 12 }}>{display(arm.score)}</strong><small style={{ color: statusTone(arm.status), textTransform: "capitalize" }}>{arm.status ?? "recorded"}</small></div></article>)}</div></section>;
}

function EvidenceList({ evidence }: { evidence: Evidence[] }) {
  return <section className="sv-section" style={{ padding: 0, overflow: "hidden" }}>{evidence.length ? <div>{evidence.map((item) => <article key={item.id} data-visual-id={item.visualId} style={{ display: "grid", gridTemplateColumns: "minmax(130px,.7fr) minmax(200px,1.3fr) auto", gap: 8, alignItems: "baseline", padding: "6px 8px", borderBottom: "1px solid var(--sv-border)" }}><strong style={{ fontSize: 10 }}>{item.title}</strong><span style={{ minWidth: 0, color: "var(--sv-text-faint)", fontSize: 9 }}>{item.summary ?? "No summary"}</span><span style={{ color: statusTone(item.status), fontSize: 8, textTransform: "uppercase" }}>{item.status ?? item.kind ?? "evidence"}</span></article>)}</div> : <p style={{ margin: 0, padding: 8, color: "var(--sv-text-faint)", fontSize: 10 }}>No evidence has been attached.</p>}</section>;
}

function basename(path?: string): string | undefined {
  return path?.split(/[\\/]/).filter(Boolean).at(-1);
}

function ReferenceChip({ label, kind, value, containerId }: { label: string; kind: string; value?: string; containerId?: string }) {
  if (!value) return <span>{label}</span>;
  return <button type="button" data-reference-kind={kind} data-reference-value={value} data-reference-container-id={containerId} title={value} style={{ display: "inline-flex", alignItems: "center", gap: 4, maxWidth: "100%", padding: "3px 7px", border: "1px solid #d8c8b9", borderRadius: 6, background: "#fffaf4", color: "#5b4032", cursor: "pointer", fontFamily: "var(--sv-mono)", fontSize: 8 }}><span aria-hidden>{kind === "path" ? "▧" : "◈"}</span><span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{label}</span></button>;
}

function RolloutTable({ rollouts, containerId, unavailableTraceIds }: { rollouts: Rollout[]; containerId?: string; unavailableTraceIds?: Set<string> }) {
  if (!rollouts.length) return null;
  return <section className="sv-section" style={{ padding: 0, overflow: "hidden" }} aria-label="Rollout results"><table style={{ width: "100%", borderCollapse: "collapse", fontSize: 9 }}><thead><tr>{["Rollout", "Reward", "Steps", "Achievements", "Stop reason", "Trace"].map((label) => <th key={label} style={{ padding: "4px 6px", borderBottom: "1px solid var(--sv-border)", color: "var(--sv-text-faint)", textAlign: label === "Rollout" || label === "Stop reason" || label === "Trace" ? "left" : "right", fontSize: 8 }}>{label}</th>)}</tr></thead><tbody>{rollouts.map((rollout, index) => <tr key={rollout.id ?? index}><th scope="row" style={{ padding: "5px 6px", borderBottom: "1px solid var(--sv-border)", textAlign: "left" }}>{rollout.label ?? (rollout.seed != null ? `Seed ${rollout.seed}` : rollout.id)}</th><td style={{ padding: "5px 6px", textAlign: "right" }}>{rollout.reward ?? MISSING}</td><td style={{ padding: "5px 6px", textAlign: "right" }}>{rollout.steps ?? MISSING}</td><td style={{ padding: "5px 6px", textAlign: "right" }}>{rollout.achievements ?? MISSING}</td><td style={{ padding: "5px 6px", color: "var(--sv-text-faint)" }}>{rollout.stopReason ?? rollout.status ?? MISSING}</td><td style={{ padding: "5px 6px" }}>{rollout.traceId ? unavailableTraceIds?.has(rollout.traceId) ? <span title="A lite seal retains provenance but cannot be opened in the Trace V5 inspector." style={{ color: "var(--sv-text-faint)" }}>Unavailable</span> : <ReferenceChip label="Open trace" kind="trace" value={rollout.traceId} containerId={containerId} /> : MISSING}</td></tr>)}</tbody></table></section>;
}

function TraceList({ traces, containerId }: { traces: TraceReference[]; containerId?: string }) {
  return <section className="sv-section" style={{ padding: 0, overflow: "hidden" }}>{traces.map((trace, index) => { const unavailable = /lite seal|not self-contained/i.test(trace.summary ?? ""); return <article key={trace.id ?? index} style={{ display: "grid", gridTemplateColumns: "minmax(120px,.8fr) minmax(170px,1.3fr) auto", gap: 8, alignItems: "center", padding: "6px 8px", borderBottom: "1px solid var(--sv-border)" }}><div><strong style={{ display: "block", fontSize: 10 }}>{trace.label ?? (trace.seed != null ? `Seed ${trace.seed}` : trace.id)}</strong><small style={{ color: "var(--sv-text-faint)" }}>{[trace.reward != null ? `reward ${trace.reward}` : null, trace.steps != null ? `${trace.steps} steps` : null, trace.stopReason].filter(Boolean).join(" · ")}</small></div><span style={{ color: "var(--sv-text-faint)", fontSize: 9 }}>{trace.summary ?? "Trace evidence"}</span>{unavailable ? <span title="A lite seal retains provenance but cannot be opened in the Trace V5 inspector." style={{ color: "var(--sv-text-faint)", fontSize: 9 }}>Unavailable</span> : trace.traceId ? <ReferenceChip label="Open trace" kind="trace" value={trace.traceId} containerId={containerId} /> : trace.visualId ? <ReferenceChip label="Open visual" kind="visual" value={trace.visualId} /> : null}</article>;})}</section>;
}

function ArtifactList({ artifacts }: { artifacts: ArtifactReference[] }) {
  return <section className="sv-section" style={{ padding: 0, overflow: "hidden" }}>{artifacts.map((artifact, index) => {
    const reference = artifact.path ?? artifact.visualId ?? artifact.traceId ?? artifact.containerId;
    const kind = artifact.path ? "path" : artifact.visualId ? "visual" : artifact.traceId ? "trace" : artifact.containerId ? "container" : "artifact";
    return <article key={artifact.id ?? index} style={{ display: "grid", gridTemplateColumns: "minmax(120px,.7fr) minmax(150px,1.3fr) auto", gap: 8, alignItems: "center", padding: "6px 8px", borderBottom: "1px solid var(--sv-border)" }}><strong style={{ fontSize: 10 }}>{artifact.label ?? artifact.id}</strong><span style={{ color: "var(--sv-text-faint)", fontSize: 9 }}>{artifact.summary ?? artifact.kind ?? "Artifact"}</span>{reference ? <ReferenceChip label={artifact.path ? basename(artifact.path) ?? "Open file" : `Open ${kind}`} kind={kind} value={reference} /> : null}</article>;
  })}</section>;
}

function ContextGrid({ title, records }: { title: string; records: Array<{ label: string; data?: ContextRecord }> }) {
  const groups = records.map((group) => ({ ...group, entries: Object.entries(group.data ?? {}).filter(([, value]) => ["string", "number", "boolean"].includes(typeof value)) })).filter((group) => group.entries.length);
  if (!groups.length) return null;
  return <section className="sv-section" aria-label={title}>{groups.map((group) => <div key={group.label} style={{ marginBottom: 8 }}><div className="sv-section-head"><h3>{group.label}</h3><span>{group.entries.length} fields</span></div><dl style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit,minmax(150px,1fr))", gap: "5px 10px", margin: 0 }}>{group.entries.map(([key, value]) => <div key={key} style={{ minWidth: 0 }}><dt style={{ color: "var(--sv-text-faint)", fontSize: 8, textTransform: "uppercase" }}>{key.replace(/([a-z])([A-Z])/g, "$1 $2")}</dt><dd style={{ margin: "2px 0 0", overflow: "hidden", fontFamily: "var(--sv-mono)", fontSize: 9, textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={String(value)}>{String(value)}</dd></div>)}</dl></div>)}</section>;
}

function Lineage({ nodes }: { nodes: LineageNode[] }) {
  if (!nodes.length) return null;
  return <section className="sv-section"><div className="sv-section-head"><h3>Lineage</h3><span>ordered</span></div><ol aria-label="Experiment lineage" style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6, padding: 0, margin: 0, listStyle: "none" }}>{nodes.map((node, index) => <li key={node.id} style={{ display: "flex", alignItems: "center", gap: 6 }}><span style={{ border: "1px solid var(--sv-border)", borderRadius: 99, padding: "6px 9px", fontSize: 10, background: "#fff" }}>{node.label}<small style={{ marginLeft: 5, color: statusTone(node.status) }}>{node.kind ?? ""}</small></span>{index < nodes.length - 1 ? <span aria-hidden style={{ color: "var(--sv-text-faint)" }}>→</span> : null}</li>)}</ol></section>;
}

export function Shell(props: ShellProps) {
  const experiment = normalizeOverview(props.experiment ?? props.data);
  if (!experiment) return <VisualChrome title={props.title ?? "Experiment overview"} lede="No experiment projection was provided." testId="visual-experiment-overview"><></></VisualChrome>;
  try {
    assertExperimentProjection(experiment);
  } catch (error) {
    if (error instanceof ExperimentProjectionError) return <ProjectionFailure error={error} title={props.title ?? experiment.title} />;
    throw error;
  }
  const status = experiment.status ?? "planned";
  const progressSummary = experiment.progress?.completed != null && experiment.progress?.total != null ? `${experiment.progress.completed}/${experiment.progress.total} · ${experiment.progress.phase ?? status}` : status;
  const metrics = [...(experiment.metrics ?? []), ...(experiment.results?.metrics ?? [])];
  const rollouts = experiment.results?.rollouts ?? [];
  const traces = experiment.traces?.items ?? [];
  const unavailableTraceIds = new Set(traces.filter((trace) => /lite seal|not self-contained/i.test(trace.summary ?? "")).map((trace) => trace.traceId).filter((id): id is string => Boolean(id)));
  const artifacts = experiment.artifacts?.items ?? [];
  const hasResults = Boolean(experiment.progress || metrics.length || experiment.arms?.length || rollouts.length || experiment.assessment);
  const contextRecords = [{ label: "Task", data: experiment.task }, { label: "Runtime", data: experiment.runtime }, { label: "Provenance", data: experiment.provenance }];
  const hasContext = contextRecords.some((group) => Object.keys(group.data ?? {}).length);
  const hasMethod = Boolean(experiment.lineage?.length || experiment.limitations?.length);
  return <VisualChrome kicker={`Experiment · ${status}`} title={props.title ?? experiment.title ?? "Experiment overview"} lede={props.lede ?? experiment.question ?? experiment.hypothesis} testId="visual-experiment-overview">
    <OverviewStrip status={status} arms={experiment.arms ?? []} model={experiment.runtime?.model} progress={experiment.progress} />
    <Hypotheses legacyHypothesis={experiment.hypothesis} hypotheses={experiment.hypotheses ?? []} />
    {hasResults ? <Disclosure title="Comparison & results" summary={progressSummary} defaultOpen>
      {experiment.progress ? <ProgressPanel progress={experiment.progress} status={status} /> : null}
      <Metrics metrics={metrics} />
      <ComparisonTable arms={experiment.arms ?? []} comparison={experiment.comparison} />
      {experiment.arms?.length ? <Arms arms={experiment.arms} /> : null}
      <RolloutTable rollouts={rollouts} containerId={typeof experiment.runtime?.containerId === "string" ? experiment.runtime.containerId : undefined} unavailableTraceIds={unavailableTraceIds} />
      <AssessmentPanel assessment={experiment.assessment} />
    </Disclosure> : null}
    {experiment.evidence?.length ? <Disclosure title="Supporting evidence" summary={`${experiment.evidence.length} items`}><EvidenceList evidence={experiment.evidence} /></Disclosure> : null}
    {traces.length ? <Disclosure title="Traces" summary={`${traces.length} retained`} defaultOpen><TraceList traces={traces} containerId={typeof experiment.runtime?.containerId === "string" ? experiment.runtime.containerId : undefined} /></Disclosure> : null}
    {hasContext ? <Disclosure title="Run context" summary={[experiment.task?.name, experiment.runtime?.model].filter(Boolean).join(" · ") || "task · runtime · provenance"}><ContextGrid title="Run context" records={contextRecords} /></Disclosure> : null}
    {artifacts.length ? <Disclosure title="Artifacts" summary={`${artifacts.length} files and references`} defaultOpen={experiment.artifacts?.prominence === "summary"}><ArtifactList artifacts={artifacts} /></Disclosure> : null}
    {hasMethod ? <Disclosure title="Method & caveats" summary={experiment.experimentId ?? "details"}>
      <Lineage nodes={experiment.lineage ?? []} />
      {experiment.limitations?.length ? <section className="sv-section" style={{ background: "#fff7ed", borderRadius: 8, padding: 12 }}><h3 style={{ marginTop: 0 }}>Limitations</h3><ul style={{ marginBottom: 0, paddingLeft: 18, fontSize: 10 }}>{experiment.limitations.map((item) => <li key={item}>{item}</li>)}</ul></section> : null}
    </Disclosure> : null}
  </VisualChrome>;
}

export default Shell;

import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import type { VisualBinding } from "../../../runtime/types.ts";

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
};

type Evidence = {
  id: string;
  title: string;
  kind?: string;
  status?: string;
  summary?: string;
  visualId?: string;
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
  status?: string;
  progress?: Progress;
  metrics?: Metric[];
  arms?: Arm[];
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
  return {
    schemaVersion: text(raw.schemaVersion),
    experimentId: text(raw.experimentId),
    title: text(raw.title),
    question: text(raw.question),
    hypothesis: text(raw.hypothesis),
    status: text(raw.status),
    progress: normalizeProgress(raw.progress),
    metrics: array<Metric>(raw.metrics),
    arms: array<Arm>(raw.arms),
    evidence: array<Evidence>(raw.evidence),
    lineage: array<LineageNode>(raw.lineage),
    limitations: Array.isArray(raw.limitations) ? raw.limitations.map(text).filter((item): item is string => Boolean(item)) : []
  };
}

function statusTone(status?: string): string {
  const normalized = status?.toLowerCase();
  if (normalized === "completed" || normalized === "selected" || normalized === "passed") return "#18794e";
  if (normalized === "failed" || normalized === "aborted" || normalized === "excluded") return "#b42318";
  if (normalized === "running" || normalized === "evaluating") return "#c2410c";
  return "#697386";
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
  return <section className="sv-section"><div className="sv-section-head"><h3>Evidence</h3><span>{evidence.length} items</span></div>{evidence.length ? <div style={{ display: "grid", gap: 7 }}>{evidence.map((item) => <article key={item.id} data-visual-id={item.visualId} style={{ padding: "10px 11px", border: "1px solid var(--sv-border)", borderRadius: 8 }}><div style={{ display: "flex", justifyContent: "space-between", gap: 10 }}><strong style={{ fontSize: 11 }}>{item.title}</strong><span style={{ color: statusTone(item.status), fontSize: 9, textTransform: "uppercase" }}>{item.status ?? item.kind ?? "evidence"}</span></div>{item.summary ? <p style={{ margin: "5px 0 0", color: "var(--sv-text-faint)", fontSize: 10 }}>{item.summary}</p> : null}</article>)}</div> : <p style={{ color: "var(--sv-text-faint)", fontSize: 11 }}>No evidence has been attached.</p>}</section>;
}

function Lineage({ nodes }: { nodes: LineageNode[] }) {
  if (!nodes.length) return null;
  return <section className="sv-section"><div className="sv-section-head"><h3>Lineage</h3><span>ordered</span></div><ol aria-label="Experiment lineage" style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6, padding: 0, margin: 0, listStyle: "none" }}>{nodes.map((node, index) => <li key={node.id} style={{ display: "flex", alignItems: "center", gap: 6 }}><span style={{ border: "1px solid var(--sv-border)", borderRadius: 99, padding: "6px 9px", fontSize: 10, background: "#fff" }}>{node.label}<small style={{ marginLeft: 5, color: statusTone(node.status) }}>{node.kind ?? ""}</small></span>{index < nodes.length - 1 ? <span aria-hidden style={{ color: "var(--sv-text-faint)" }}>→</span> : null}</li>)}</ol></section>;
}

export function Shell(props: ShellProps) {
  const experiment = normalizeOverview(props.experiment ?? props.data);
  if (!experiment) return <VisualChrome title={props.title ?? "Experiment overview"} lede="No experiment projection was provided." testId="visual-experiment-overview"><></></VisualChrome>;
  const status = experiment.status ?? "planned";
  return <VisualChrome kicker={`Experiment · ${status}`} title={props.title ?? experiment.title ?? "Experiment overview"} lede={props.lede ?? experiment.question ?? experiment.hypothesis} testId="visual-experiment-overview">
    <section className="sv-section" style={{ padding: 12, borderRadius: 8, background: "#f6f7f9" }}><div style={{ display: "flex", justifyContent: "space-between", gap: 12 }}><div><span style={{ fontSize: 9, textTransform: "uppercase", color: "var(--sv-text-faint)" }}>Research question</span><strong style={{ display: "block", marginTop: 4, fontSize: 12 }}>{experiment.question ?? experiment.hypothesis ?? "Not recorded"}</strong></div><span style={{ color: statusTone(status), fontSize: 10, fontWeight: 700, textTransform: "uppercase" }}>{status}</span></div>{experiment.experimentId ? <code style={{ display: "block", marginTop: 8, color: "var(--sv-text-faint)", fontSize: 9 }}>{experiment.experimentId}</code> : null}</section>
    <ProgressPanel progress={experiment.progress} status={status} />
    <Metrics metrics={experiment.metrics ?? []} />
    <Arms arms={experiment.arms ?? []} />
    <EvidenceList evidence={experiment.evidence ?? []} />
    <Lineage nodes={experiment.lineage ?? []} />
    {experiment.limitations?.length ? <section className="sv-section" style={{ background: "#fff7ed", borderRadius: 8, padding: 12 }}><h3 style={{ marginTop: 0 }}>Limitations</h3><ul style={{ marginBottom: 0, paddingLeft: 18, fontSize: 10 }}>{experiment.limitations.map((item) => <li key={item}>{item}</li>)}</ul></section> : null}
  </VisualChrome>;
}

export default Shell;

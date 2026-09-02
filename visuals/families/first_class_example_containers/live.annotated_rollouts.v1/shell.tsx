import { useEffect, useMemo, useState } from "react";
import { MetricStrip, VisualChrome } from "../../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../../chrome/useLiveEvalStream.ts";
import { formatMissingNumber } from "../../../runtime/liveStream.ts";
import type { LiveTemplateProps } from "../../../runtime/replayClient.ts";
import type { LiveEvalEvent } from "../../../runtime/types.ts";
import {
  FINDING_KIND_ORDER,
  activeFindings,
  countByKind,
  eventDetail,
  labelTally,
  logicalTimeline,
  projectLanes,
  type Finding,
  type Lane,
  type LaneEvent,
} from "./project.ts";
import { TaskDetails, familyLabel, outcomeLabel, progressLabel, taskFamily } from "./adapters.tsx";

type StreamPayload = { run_id?: string; events?: LiveEvalEvent[]; sse_url?: string };
type Feed = "all" | "annotations" | "rollout";
type DetailTab = "rollout" | "trace" | "verifier";

type RunConfiguration = { container: string; containerDetail: string; policy: string; policyDetail: string; model: string; modelDetail: string };
function record(value: unknown): Record<string, unknown> { return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {}; }
function text(value: unknown): string | undefined { return typeof value === "string" && value.trim() ? value.trim() : undefined; }
function abbreviated(value: string | undefined): string | undefined { return value && value.length > 22 ? `${value.slice(0, 10)}…${value.slice(-8)}` : value; }

export function runConfiguration(events: LiveEvalEvent[], rawOptimizer: unknown): RunConfiguration {
  const slot = record(rawOptimizer);
  const run = Object.keys(record(slot.run)).length ? record(slot.run) : slot;
  const summary = record(run.summary);
  const bindings = Array.isArray(run.execution_bindings) ? run.execution_bindings.map(record) : [];
  const containerBinding = bindings.find((row) => row.kind === "container_http" || row.kind === "container") ?? {};
  const containerMeta = record(containerBinding.metadata);
  const unwrapped = events.map(unwrapRelayed);
  const opened = unwrapped.find((event) => event.kind === "trace.opened");
  const policyPayload = record(unwrapped.find((event) => event.kind === "policy.session.opened")?.payload);
  const policyRef = Object.keys(record(summary.policyRef)).length ? record(summary.policyRef) : record(opened?.payload.policy_ref);
  const containerId = text(summary.containerId) ?? text(containerBinding.id);
  const imageDigest = text(summary.containerImageDigest) ?? text(containerMeta.imageDigest);
  const harness = text(policyRef.harness) ?? text(policyPayload.harness);
  const config = text(policyRef.config) ?? text(policyPayload.config);
  const policyRevision = text(summary.policySourceRevision);
  const policyDigest = text(summary.policyConfigurationDigest);
  const provider = text(summary.provider) ?? text(policyPayload.provider);
  const model = text(summary.model) ?? text(policyPayload.model);
  const reasoning = text(policyPayload.reasoning_effort);
  return {
    container: containerId ?? "not reported",
    containerDetail: imageDigest ? `image ${abbreviated(imageDigest)}` : "image digest not reported",
    policy: [harness, config].filter(Boolean).join(" / ") || "not reported",
    policyDetail: policyRevision ? `revision ${abbreviated(policyRevision)}` : policyDigest ? `digest ${abbreviated(policyDigest)}` : "revision not reported",
    model: model ?? "not reported",
    modelDetail: [provider, reasoning].filter(Boolean).join(" · ") || "provider not reported",
  };
}

function RunConfigurationStrip({ configuration }: { configuration: RunConfiguration }) {
  const cells = [{ label: "Container", value: configuration.container, detail: configuration.containerDetail }, { label: "Policy", value: configuration.policy, detail: configuration.policyDetail }, { label: "Model", value: configuration.model, detail: configuration.modelDetail }];
  return <section data-testid="run-configuration" aria-label="Run configuration" style={{ display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 8, marginBottom: 12 }}>{cells.map((cell) => <div key={cell.label} style={{ minWidth: 0, padding: "9px 11px", border: "1px solid var(--sv-border)", borderRadius: 8, background: "var(--sv-canvas)" }}><span style={{ display: "block", marginBottom: 3, color: "var(--sv-text-faint)", fontSize: 9, fontWeight: 700, letterSpacing: ".08em", textTransform: "uppercase" }}>{cell.label}</span><strong className="sv-mono" title={cell.value} style={{ display: "block", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 11 }}>{cell.value}</strong><span className="sv-mono" title={cell.detail} style={{ display: "block", marginTop: 2, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: "var(--sv-text-faint)", fontSize: 9 }}>{cell.detail}</span></div>)}</section>;
}

const KIND_COLOR: Record<string, string> = {
  achievement: "#39a46b",
  milestone: "#2f6fdd",
  failure_mode: "#d84b3f",
  intent: "#8a5bd6",
  note: "#8c8c8c",
};

function displayTime(value: string) {
  if (!value) return "Waiting for an event";
  const parsed = new Date(value);
  return Number.isNaN(parsed.valueOf()) ? value : parsed.toLocaleString([], { month: "short", day: "numeric", hour: "numeric", minute: "2-digit", second: "2-digit" });
}

function FindingChip({ finding, showHistory }: { finding: Finding; showHistory: boolean }) {
  if (finding.status !== "provisional" && !showHistory) return null;
  const color = KIND_COLOR[finding.kind] ?? KIND_COLOR.note;
  const muted = finding.status !== "provisional";
  const title = [
    `${finding.kind}: ${finding.label}`,
    finding.step != null ? `step ${finding.step}` : null,
    finding.confidence != null ? `confidence ${finding.confidence.toFixed(2)}` : null,
    finding.basis ? `basis ${finding.basis}` : null,
    finding.status === "retracted" ? `retracted: ${finding.retractedReason ?? ""}` : null,
    finding.status === "superseded" ? `superseded by ${finding.supersededBy ?? ""}` : null,
    finding.sequences.length ? `evidence sequences ${finding.sequences.join(", ")}` : null,
    finding.logicalTime != null ? `logical time t=${finding.logicalTime}` : null,
    typeof finding.detail.rationale === "string" ? String(finding.detail.rationale) : null,
  ].filter(Boolean).join(" · ");
  return <span title={title} data-status={finding.status} className="sv-mono" style={{ display: "inline-flex", alignItems: "center", gap: 4, padding: "2px 7px", borderRadius: 999, fontSize: 10, border: `1px solid ${color}`, color: muted ? "var(--sv-text-faint)" : color, textDecoration: finding.status === "retracted" ? "line-through" : "none", opacity: muted ? 0.65 : 1 }}>
    <span style={{ width: 6, height: 6, borderRadius: 999, background: color }} />
    {finding.label}
    {finding.confidence != null && finding.kind !== "achievement" ? <span style={{ opacity: 0.7 }}>{Math.round(finding.confidence * 100)}%</span> : null}
    {finding.basis === "model" ? <span style={{ opacity: 0.7 }}>judge</span> : null}
  </span>;
}

function MarkerStrip({ lane }: { lane: Lane }) {
  const span = Math.max(lane.total ?? 0, lane.done, 1);
  return <div aria-label={`Annotation markers for ${lane.name}`} style={{ position: "relative", height: 14, margin: "8px 0 2px", background: "var(--sv-border)", borderRadius: 7 }}>
    <div style={{ position: "absolute", inset: 0, width: `${Math.min(100, lane.done / span * 100)}%`, background: "var(--sv-accent)", opacity: 0.25, borderRadius: 7 }} />
    {lane.markers.map((marker) => {
      const step = marker.step ?? lane.done;
      const left = Math.min(99, Math.max(0, step / span * 100));
      const color = KIND_COLOR[marker.kind] ?? KIND_COLOR.note;
      return <span key={`${marker.findingId}-${marker.sequence}`} title={`${marker.kind}: ${marker.label} @ step ${step}${marker.logicalTime != null ? ` · t=${marker.logicalTime}` : ""} (${marker.status})`} style={{ position: "absolute", top: 2, left: `${left}%`, width: 10, height: 10, marginLeft: -5, borderRadius: marker.kind === "failure_mode" ? 2 : 999, background: marker.status === "provisional" ? color : "transparent", border: `2px solid ${color}`, opacity: marker.status === "provisional" ? 1 : 0.45, transform: marker.kind === "failure_mode" ? "rotate(45deg)" : "none" }} />;
    })}
  </div>;
}

function RolloutBar({ lane, selected, onClick }: { lane: Lane; selected: boolean; onClick: () => void }) {
  const pct = lane.status === "finished" ? 100 : lane.total ? Math.min(100, lane.done / lane.total * 100) : lane.rolloutEvents ? 12 : 0;
  const findings = activeFindings(lane).length;
  return <button type="button" onClick={onClick} aria-pressed={selected} data-testid={`rollout-bar-${lane.name}`} style={{ width: "100%", display: "grid", gridTemplateColumns: "minmax(120px, 1.4fr) minmax(110px, 2fr) auto", gap: 10, alignItems: "center", padding: "8px 10px", border: selected ? "1px solid var(--sv-accent)" : "1px solid var(--sv-border)", borderRadius: 8, background: selected ? "#fff8f3" : "var(--sv-surface)", color: "var(--sv-text)", cursor: "pointer", textAlign: "left" }}>
    <span style={{ minWidth: 0 }}><strong style={{ display: "block", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 11 }}>{lane.name}</strong><span className="sv-mono" style={{ color: "var(--sv-text-faint)", fontSize: 9 }}>{familyLabel(lane)} · {progressLabel(lane)}</span></span>
    <span style={{ display: "grid", gap: 4 }}><span style={{ display: "block", height: 7, borderRadius: 9, overflow: "hidden", background: "var(--sv-border)" }}><span style={{ display: "block", width: `${pct}%`, height: "100%", background: lane.status === "failed" ? "#d84b3f" : "var(--sv-accent)", transition: "width 180ms ease" }} /></span><span className="sv-mono" style={{ color: "var(--sv-text-faint)", fontSize: 9, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{lane.lastAnnotation}</span></span>
    <span className="sv-mono" style={{ fontSize: 9, textAlign: "right", whiteSpace: "nowrap" }}>{outcomeLabel(lane)}<br />{findings} finding{findings === 1 ? "" : "s"}</span>
  </button>;
}

function EvidencePayload({ payload }: { payload: Record<string, unknown> }) {
  const rendered = JSON.stringify(payload, (_key, value) => {
    if (typeof value === "string" && value.length > 800) return `${value.slice(0, 800)}…`;
    if (Array.isArray(value) && value.length > 30) return [...value.slice(0, 30), `… ${value.length - 30} more`];
    return value;
  }, 2);
  return <details style={{ marginTop: 5 }}><summary className="sv-mono" style={{ cursor: "pointer", color: "var(--sv-text-faint)", fontSize: 9 }}>Inspect event evidence</summary><pre style={{ margin: "6px 0 0", padding: 8, maxHeight: 260, overflow: "auto", borderRadius: 6, background: "var(--sv-canvas)", border: "1px solid var(--sv-border)", whiteSpace: "pre-wrap", overflowWrap: "anywhere", fontSize: 9 }}>{rendered}</pre></details>;
}

function TraceList({ rows, empty }: { rows: LaneEvent[]; empty: string }) {
  const visible = rows.slice(-80);
  return <div>
    {rows.length > visible.length ? <p className="sv-mono" style={{ margin: "0 0 7px", color: "var(--sv-text-faint)", fontSize: 9 }}>Showing the latest {visible.length} of {rows.length} events.</p> : null}
    <ol style={{ listStyle: "none", margin: 0, padding: 0, display: "grid", gap: 0 }}>
      {visible.map((row, index) => <li key={`${row.stream}-${row.sequence ?? index}-${row.kind}`} style={{ display: "grid", gridTemplateColumns: "68px minmax(0, 1fr)", gap: 10, padding: "9px 0", borderTop: "1px solid var(--sv-border)" }}>
        <div className="sv-mono" style={{ color: "var(--sv-text-faint)", fontSize: 9, lineHeight: 1.45 }}><strong style={{ color: "var(--sv-text)" }}>{row.logicalTime != null ? `t=${row.logicalTime}` : "t=—"}</strong><br />seq {row.sequence ?? "—"}<br />{row.occurredAt ? row.occurredAt.slice(11, 19) : "—"}</div>
        <div style={{ minWidth: 0 }}><div style={{ display: "flex", gap: 7, alignItems: "baseline", flexWrap: "wrap" }}><strong className="sv-mono" style={{ fontSize: 10 }}>{row.kind}</strong><span style={{ fontSize: 10, color: "var(--sv-text-muted)" }}>{row.detail}</span>{row.stream === "annotation" && row.sourceSequence != null ? <span className="sv-mono" style={{ fontSize: 9, color: "var(--sv-text-faint)" }}>observed rollout seq {row.sourceSequence}</span> : null}</div><EvidencePayload payload={row.payload} /></div>
      </li>)}
      {!rows.length ? <li style={{ padding: "14px 0", color: "var(--sv-text-faint)", fontSize: 10 }}>{empty}</li> : null}
    </ol>
  </div>;
}

function RubricEvidence({ lane }: { lane: Lane }) {
  const grades = Array.isArray(lane.task.rubric_grades) ? lane.task.rubric_grades as Record<string, unknown>[] : [];
  return <div style={{ display: "grid", gap: 8 }}>
    <div className="sv-section-head" style={{ marginBottom: 0 }}><h4 style={{ margin: 0, fontSize: 11 }}>Rubric evidence</h4><span className="sv-mono">{grades.length ? `${grades.filter((row) => row.criteria_met === true).length}/${grades.length} met` : "no rubric rows"}</span></div>
    {grades.length ? <ol style={{ listStyle: "none", padding: 0, margin: 0, display: "grid", gap: 6 }}>{grades.map((grade, index) => {
      const criterion = String(grade.rubric_text ?? grade.criterion ?? grade.rubric_id ?? `Rubric ${index + 1}`);
      const explanation = grade.explanation ?? grade.rationale ?? grade.reason ?? grade.grader_feedback;
      const points = typeof grade.points === "number" ? grade.points : typeof grade.score === "number" ? grade.score : null;
      return <li key={`${String(grade.rubric_id ?? "rubric")}-${index}`} style={{ padding: "9px 10px", border: "1px solid var(--sv-border)", borderLeft: `3px solid ${grade.criteria_met ? "#39a46b" : "#d84b3f"}`, borderRadius: 7, background: "var(--sv-surface)" }}>
        <div style={{ display: "grid", gridTemplateColumns: "60px minmax(0, 1fr) auto", gap: 8, alignItems: "start", fontSize: 10 }}><strong style={{ color: grade.criteria_met ? "#238a57" : "#c2553f" }}>{grade.criteria_met ? "MET" : "UNMET"}</strong><span>{criterion}</span><span className="sv-mono">{points == null ? "—" : `${points} pts`}</span></div>
        {explanation != null ? <p style={{ margin: "6px 0 0 68px", color: "var(--sv-text-muted)", fontSize: 10 }}>{String(explanation)}</p> : null}
        <div className="sv-mono" style={{ margin: "5px 0 0 68px", color: "var(--sv-text-faint)", fontSize: 9 }}>{grade.logical_time != null ? `t=${String(grade.logical_time)} · ` : ""}{grade.rubric_id != null ? `rubric ${String(grade.rubric_id)}` : `rubric ${index + 1}`}</div>
      </li>;
    })}</ol> : <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: 10 }}>This rollout did not emit structured rubric grades. Verifier events and reward authority are still shown below when available.</p>}
  </div>;
}

function LaneCard({ lane, showHistory, streamBase }: { lane: Lane; showHistory: boolean; streamBase: URL | null }) {
  const [tab, setTab] = useState<DetailTab>("rollout");
  const pct = lane.total ? Math.min(100, lane.done / lane.total * 100) : 0;
  const active = activeFindings(lane);
  const counts = countByKind(active);
  const ordered = [...lane.findings].sort((a, b) => (a.sourceSequence ?? 0) - (b.sourceSequence ?? 0));
  const reward = lane.metrics.cumulative_reward ?? lane.reward;
  const judge = lane.metrics.judge_progress;
  const rolloutTrace = lane.trace.filter((row) => row.stream === "rollout" && !row.verifier);
  const verifierTrace = lane.trace.filter((row) => row.verifier);
  return <article data-testid={`lane-${lane.name}`} style={{ border: "1px solid var(--sv-border)", borderRadius: 10, padding: 12, background: lane.status === "running" ? "#fffaf7" : "var(--sv-surface)", display: "grid", gap: 8 }}>
    <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "baseline" }}>
      <strong style={{ fontSize: 13, overflow: "hidden", textOverflow: "ellipsis" }}>{lane.name}</strong>
      <span className="sv-mono" style={{ color: lane.status === "failed" ? "#c2553f" : "var(--sv-accent)", fontSize: 11 }}>{lane.status}{lane.annotationClosed ? " · annotations sealed" : lane.protocol ? " · annotating" : ""}</span>
    </div>
    <div style={{ height: 7, background: "var(--sv-border)", borderRadius: 8, overflow: "hidden" }}><div style={{ width: `${pct}%`, height: "100%", background: "var(--sv-accent)", transition: "width 180ms ease" }} /></div>
    <div style={{ display: "flex", justifyContent: "space-between", gap: 8, color: "var(--sv-text-muted)", fontSize: 11, flexWrap: "wrap" }}>
      <span className="sv-mono">{lane.done}{lane.total ? ` / ${lane.total}` : " steps"}</span>
      <span>reward <strong style={{ color: "var(--sv-text)" }}>{formatMissingNumber(reward)}</strong></span>
      <span>{lane.achievements.length} achievements</span>
      <span>{lane.calls} calls</span>
      {judge != null ? <span title="latest judge progress: 1 advancing, 0 stalled, -1 regressing">judge {judge > 0 ? "advancing" : judge < 0 ? "regressing" : "stalled"}</span> : null}
    </div>
    <div style={{ display: "flex", justifyContent: "space-between", gap: 10, alignItems: "end" }}><span className="sv-mono" style={{ fontSize: 10, color: "var(--sv-text-faint)" }}>{familyLabel(lane)} · {taskFamily(lane)}</span><span className="sv-mono" style={{ maxWidth: "68%", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 10, color: "var(--sv-text-faint)" }}>› {lane.last}</span></div>
    <nav aria-label="Rollout detail sections" style={{ display: "flex", gap: 6, padding: 4, borderRadius: 8, background: "var(--sv-canvas)", border: "1px solid var(--sv-border)" }}>{(["rollout", "trace", "verifier"] as DetailTab[]).map((option) => <button key={option} type="button" aria-pressed={tab === option} onClick={() => setTab(option)} style={{ flex: 1, border: tab === option ? "1px solid var(--sv-border)" : "1px solid transparent", borderRadius: 6, padding: "6px 8px", background: tab === option ? "var(--sv-surface)" : "transparent", color: tab === option ? "var(--sv-text)" : "var(--sv-text-muted)", boxShadow: tab === option ? "0 1px 2px rgba(0,0,0,.05)" : "none", cursor: "pointer", fontSize: 10, fontWeight: 700 }}>{option === "rollout" ? "Rollout" : option === "trace" ? `Trace · ${rolloutTrace.length}` : `Verifier & rubric · ${verifierTrace.length}`}</button>)}</nav>
    {tab === "rollout" ? <section aria-label="Rollout information" style={{ display: "grid", gap: 10 }}>
      <div className="sv-section-head" style={{ marginBottom: 0 }}><h4 style={{ margin: 0, fontSize: 11 }}>Rollout information</h4><span className="sv-mono">{familyLabel(lane)} · {outcomeLabel(lane)}</span></div>
      <MarkerStrip lane={lane} />
      <TaskDetails lane={lane} streamBase={streamBase} />
      <div style={{ display: "flex", gap: 10, fontSize: 10, color: "var(--sv-text-faint)", flexWrap: "wrap" }} className="sv-mono">
        {FINDING_KIND_ORDER.map((kind) => <span key={kind} style={{ color: counts[kind] ? KIND_COLOR[kind] : undefined }}>{counts[kind] ?? 0} {kind.replace("_", " ")}</span>)}
        <span>{lane.findings.filter((row) => row.status === "retracted").length} retracted</span>
        {lane.model.requested ? <span>judge {lane.model.completed}/{lane.model.requested}{lane.model.failed ? ` (${lane.model.failed} failed)` : ""}</span> : null}
        {lane.protocolErrors ? <span style={{ color: "#c2553f" }}>{lane.protocolErrors} protocol errors</span> : null}
      </div>
      <div style={{ display: "flex", gap: 5, flexWrap: "wrap" }} aria-label={`Findings for ${lane.name}`}>{ordered.map((finding) => <FindingChip key={finding.findingId} finding={finding} showHistory={showHistory} />)}{!lane.findings.length ? <span style={{ fontSize: 10, color: "var(--sv-text-faint)" }}>{lane.protocol ? "no findings yet" : "no protocol bound"}</span> : null}</div>
      <span className="sv-mono" style={{ fontSize: 10, color: "var(--sv-text-faint)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>◌ {lane.lastAnnotation}{lane.protocol?.revisionId ? ` · ${lane.protocol.protocolId ?? "protocol"} ${lane.protocol.revisionId}` : ""}</span>
    </section> : null}
    {tab === "trace" ? <section aria-label="Rollout trace"><div className="sv-section-head"><div><h4 style={{ margin: 0, fontSize: 11 }}>Rollout trace</h4><span style={{ fontSize: 9, color: "var(--sv-text-faint)" }}>Policy, environment, action, observation, and lifecycle evidence in logical order.</span></div><span className="sv-mono">{rolloutTrace.length} events</span></div><TraceList rows={rolloutTrace} empty="No rollout trace events have arrived." /></section> : null}
    {tab === "verifier" ? <section aria-label="Verifier and rubric information" style={{ display: "grid", gap: 14 }}><div className="sv-section-head" style={{ marginBottom: 0 }}><div><h4 style={{ margin: 0, fontSize: 11 }}>Verifier & rubric</h4><span style={{ fontSize: 9, color: "var(--sv-text-faint)" }}>Structured criteria first; verifier and grader calls remain inspectable below.</span></div><span className="sv-mono">{outcomeLabel(lane)}</span></div><div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(130px, 1fr))", gap: 8, padding: 9, border: "1px solid var(--sv-border)", borderRadius: 7 }}><span><strong style={{ display: "block", fontSize: 9, color: "var(--sv-text-faint)" }}>PROTOCOL</strong><span className="sv-mono" style={{ fontSize: 10 }}>{lane.protocol?.protocolId ?? "not bound"}</span></span><span><strong style={{ display: "block", fontSize: 9, color: "var(--sv-text-faint)" }}>REVISION</strong><span className="sv-mono" style={{ fontSize: 10 }}>{lane.protocol?.revisionId ?? "—"}</span></span><span><strong style={{ display: "block", fontSize: 9, color: "var(--sv-text-faint)" }}>MODEL</strong><span className="sv-mono" style={{ fontSize: 10 }}>{lane.protocol?.model ?? "deterministic / unspecified"}</span></span><span><strong style={{ display: "block", fontSize: 9, color: "var(--sv-text-faint)" }}>CALLS</strong><span className="sv-mono" style={{ fontSize: 10 }}>{lane.model.completed}/{lane.model.requested} complete</span></span></div><RubricEvidence lane={lane} /><div><div className="sv-section-head"><h4 style={{ margin: 0, fontSize: 11 }}>Verifier trace</h4><span className="sv-mono">{verifierTrace.length} events</span></div><TraceList rows={verifierTrace} empty="No separate verifier or grader trace was emitted for this rollout." /></div></section> : null}
  </article>;
}

export type ShellProps = LiveTemplateProps & { title?: string; lede?: string; stream?: StreamPayload; optimizer_run?: unknown };

export function Shell(props: ShellProps) {
  const stream = props.stream ?? {};
  const declaredStreamCount = props.replay?.streams.length ?? 0;
  const { events, state, error, ready } = useLiveEvalStream({
    replay: props.replay,
    fixtureEvents: declaredStreamCount > 0 ? undefined : stream.events,
    replayMs: 240,
    visualId: props.visualId,
    revision: props.revision,
  });
  const live = state === "live";
  const hasSource = declaredStreamCount > 0 || Boolean(stream.events);
  const [globalCursor, setGlobalCursor] = useState<number | null>(null);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const [feed, setFeed] = useState<Feed>("annotations");
  const [showHistory, setShowHistory] = useState(false);
  const [selectedRollout, setSelectedRollout] = useState<string | null>(null);
  const timeline = useMemo(() => logicalTimeline(events), [events]);
  const configuration = useMemo(() => runConfiguration(events, props.optimizer_run), [events, props.optimizer_run]);
  const selectedGlobal = globalCursor == null ? timeline.length - 1 : Math.max(0, Math.min(globalCursor, timeline.length - 1));
  const selectedMoment = timeline[selectedGlobal];
  const visibleTimeline = useMemo(() => timeline.slice(0, selectedGlobal + 1), [timeline, selectedGlobal]);
  const visibleEvents = useMemo(() => visibleTimeline.map((row) => row.event), [visibleTimeline]);
  const lanes = useMemo(() => projectLanes(visibleEvents), [visibleEvents]);
  const done = lanes.filter((lane) => lane.status === "finished").length;
  const active = lanes.flatMap(activeFindings);
  const counts = countByKind(active);
  const retracted = lanes.reduce((sum, lane) => sum + lane.findings.filter((row) => row.status === "retracted").length, 0);
  const judge = lanes.reduce((sum, lane) => sum + lane.model.requested, 0);
  const failureTally = labelTally(lanes, "failure_mode");
  const milestoneTally = labelTally(lanes, "milestone");
  const recent = visibleTimeline.filter((row) => feed === "all" || (feed === "annotations") === (row.stream === "annotation")).slice(-12).reverse();
  const streamBase = stream.sse_url ? new URL(stream.sse_url, window.location.href) : null;
  const selectedLane = lanes.find((lane) => lane.name === selectedRollout);

  useEffect(() => {
    if (!playing || !timeline.length) return;
    const timer = window.setInterval(() => {
      setGlobalCursor((current) => {
        const index = current == null ? -1 : current;
        if (index >= timeline.length - 1) {
          setPlaying(false);
          return timeline.length - 1;
        }
        return index + 1;
      });
    }, Math.max(40, 360 / speed));
    return () => window.clearInterval(timer);
  }, [playing, speed, timeline.length]);

  const seek = (index: number) => {
    setPlaying(false);
    setGlobalCursor(Math.max(0, Math.min(index, timeline.length - 1)));
  };

  const togglePlayback = () => {
    if (!timeline.length) return;
    if (!playing && selectedGlobal >= timeline.length - 1) setGlobalCursor(-1);
    setPlaying((value) => !value);
  };

  return <VisualChrome kicker="Container eval · annotated rollouts" live={live} title={props.title ?? "Annotated Rollouts"} lede={props.lede ?? "Rollout evidence and provisional annotations on one replay clock. Choose a rollout for task-specific detail; return to All rollouts for the aggregate."} testId="visual-live-annotated-rollouts" footer="Annotated Rollouts · live.annotated_rollouts.v1 · synth.trace-stream-event.v1 + synth.live-annotation-stream.v1">
    <RunConfigurationStrip configuration={configuration} />
    <MetricStrip metrics={[
      { label: "Rollouts", value: `${done}/${lanes.length || "—"} done` },
      { label: "Achievements", value: String(counts.achievement ?? 0) },
      { label: "Milestones", value: String(counts.milestone ?? 0) },
      { label: "Failure modes", value: String(counts.failure_mode ?? 0) },
      { label: "Retracted", value: String(retracted) },
      { label: "Judge calls", value: String(judge) },
      { label: "Stream", value: !hasSource ? "awaiting source" : !ready ? "connecting" : live ? "receiving" : done ? "complete" : "waiting" },
    ]} />
    <section className="sv-section" aria-label="Evaluation replay">
      <div className="sv-section-head"><div><h3>Logical evaluation time</h3><span className="sv-mono" style={{ fontSize: 10, color: "var(--sv-text-faint)" }}>one tick per rollout or annotation event</span></div><div style={{ textAlign: "right" }}><strong className="sv-mono">{selectedMoment ? `t=${selectedMoment.logicalTime} / ${timeline.length}` : "Waiting for an event"}</strong><br />{selectedMoment ? <time className="sv-mono" style={{ fontSize: 10, color: "var(--sv-text-faint)" }}>{displayTime(selectedMoment.occurredAt)}</time> : null}</div></div>
      <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginBottom: 7 }}>
        <button onClick={togglePlayback} disabled={!timeline.length}>{playing ? "Pause" : "Play"}</button>
        <button onClick={() => seek(selectedGlobal - 1)} disabled={!timeline.length || selectedGlobal <= 0}>Previous</button>
        <button onClick={() => seek(selectedGlobal + 1)} disabled={!timeline.length || selectedGlobal >= timeline.length - 1}>Next</button>
        <select aria-label="Logical replay speed" value={speed} onChange={(event) => setSpeed(Number(event.currentTarget.value))}><option value={0.5}>0.5×</option><option value={1}>1×</option><option value={2}>2×</option><option value={4}>4×</option><option value={8}>8×</option></select>
        <button onClick={() => { setPlaying(false); setGlobalCursor(null); }} disabled={!timeline.length || globalCursor == null}>Follow live</button>
      </div>
      <input type="range" min={0} max={Math.max(0, timeline.length - 1)} value={Math.max(0, selectedGlobal)} onChange={(event) => seek(Number(event.currentTarget.value))} disabled={!timeline.length} aria-label="Replay the complete evaluation by logical time" style={{ width: "100%" }} />
      {selectedMoment ? <div className="sv-mono" aria-live="polite" style={{ marginTop: 7, padding: "7px 9px", border: "1px solid var(--sv-border)", borderRadius: 7, display: "grid", gridTemplateColumns: "auto minmax(80px, .7fr) minmax(160px, 2fr)", gap: 9, fontSize: 10 }}><strong>t={selectedMoment.logicalTime}</strong><span>{selectedMoment.stream} · {selectedMoment.lane}</span><span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{eventDetail(selectedMoment.event)} · stream seq {selectedMoment.streamSequence ?? "—"}{selectedMoment.stream === "annotation" ? ` · observed rollout seq ${selectedMoment.sourceSequence ?? "—"}` : ""}</span></div> : null}
    </section>
    {error ? <p role="alert" style={{ color: "#c2553f" }}>{error}</p> : null}
    <section className="sv-section" aria-label="Summary across rollouts">
      <div className="sv-section-head"><h3>Across rollouts</h3><span className="sv-mono">provisional · observe-only</span></div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))", gap: 12, fontSize: 11 }}>
        <div>
          <strong style={{ fontSize: 11, color: KIND_COLOR.failure_mode }}>Failure modes</strong>
          <ol style={{ margin: "4px 0 0", padding: 0, listStyle: "none" }}>{failureTally.slice(0, 8).map((row) => <li key={row.label} className="sv-mono" style={{ display: "flex", justifyContent: "space-between", gap: 8, padding: "2px 0" }}><span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{row.label}</span><span>{row.lanes} lanes · {row.count}</span></li>)}{!failureTally.length ? <li style={{ color: "var(--sv-text-faint)" }}>none active</li> : null}</ol>
        </div>
        <div>
          <strong style={{ fontSize: 11, color: KIND_COLOR.milestone }}>Milestones reached</strong>
          <ol style={{ margin: "4px 0 0", padding: 0, listStyle: "none" }}>{milestoneTally.slice(0, 8).map((row) => <li key={row.label} className="sv-mono" style={{ display: "flex", justifyContent: "space-between", gap: 8, padding: "2px 0" }}><span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{row.label}</span><span>{row.lanes} lanes</span></li>)}{!milestoneTally.length ? <li style={{ color: "var(--sv-text-faint)" }}>none yet</li> : null}</ol>
        </div>
      </div>
    </section>
    <section className="sv-section" aria-label="Rollout lanes" aria-live="polite">
      <div className="sv-section-head"><div style={{ display: "flex", gap: 9, alignItems: "center" }}><h3>{selectedLane ? selectedLane.name : "All rollouts"}</h3>{selectedLane ? <button type="button" onClick={() => setSelectedRollout(null)}>← All rollouts</button> : null}</div><label className="sv-mono" style={{ fontSize: 10, display: "flex", gap: 6, alignItems: "center" }}><input type="checkbox" checked={showHistory} onChange={(event) => setShowHistory(event.currentTarget.checked)} /> show superseded and retracted</label></div>
      {selectedLane ? <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 2fr) minmax(260px, .85fr)", gap: 12, alignItems: "start" }}>
        <LaneCard key={selectedLane.name} lane={selectedLane} showHistory={showHistory} streamBase={streamBase} />
        <aside aria-label="Related rollout information" style={{ display: "grid", gap: 7, position: "sticky", top: 8 }}><strong style={{ fontSize: 11 }}>Related info</strong>{lanes.map((lane) => <RolloutBar key={lane.name} lane={lane} selected={lane.name === selectedLane.name} onClick={() => setSelectedRollout(lane.name)} />)}</aside>
      </div> : <div style={{ display: "grid", gap: 7 }}>
        {lanes.map((lane) => <RolloutBar key={lane.name} lane={lane} selected={false} onClick={() => setSelectedRollout(lane.name)} />)}
        {!lanes.length ? <div style={{ padding: 20, color: "var(--sv-text-faint)" }}>Waiting for the first rollout…</div> : null}
      </div>}
    </section>
    <section className="sv-section" aria-label="Recent activity">
      <div className="sv-section-head"><h3>Activity</h3><div role="tablist" className="sv-mono" style={{ display: "flex", gap: 8, fontSize: 10 }}>{(["annotations", "rollout", "all"] as Feed[]).map((option) => <button key={option} role="tab" aria-selected={feed === option} onClick={() => setFeed(option)} style={{ background: feed === option ? "var(--sv-accent)" : "transparent", color: feed === option ? "#fff" : "var(--sv-text-muted)", border: "1px solid var(--sv-border)", borderRadius: 999, padding: "2px 8px", cursor: "pointer" }}>{option}</button>)}</div></div>
      <ol style={{ listStyle: "none", margin: 0, padding: 0 }}>{recent.map((row) => <li key={`logical-${row.logicalTime}`} style={{ display: "grid", gridTemplateColumns: "58px 66px minmax(90px, 0.7fr) 2fr", gap: 10, padding: "7px 0", borderTop: "1px solid var(--sv-border)", fontSize: 11 }}><strong className="sv-mono">t={row.logicalTime}</strong><time className="sv-mono" title={displayTime(row.occurredAt)} style={{ color: "var(--sv-text-faint)" }}>{row.occurredAt.slice(11, 19)}</time><strong style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{row.lane}</strong><span className="sv-mono" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: row.stream === "annotation" ? (KIND_COLOR[String(row.event.payload.kind ?? "")] ?? "var(--sv-text)") : "var(--sv-text-muted)" }}>{row.stream === "annotation" ? "◌ " : "· "}{eventDetail(row.event)}</span></li>)}{!recent.length ? <li style={{ padding: 8, color: "var(--sv-text-faint)" }}>Nothing in this feed yet.</li> : null}</ol>
    </section>
  </VisualChrome>;
}

export default Shell;

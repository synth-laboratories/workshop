import { useEffect, useMemo, useRef, useState } from "react";
import { MetricStrip, surfaceObservationAttributes, VisualChrome } from "../../../chrome/VisualChrome.tsx";

type InspectorItem = {
  item_id: string; kind: string; title?: string; status?: string; sequence?: number;
  lane_id?: string; occurred_at?: string; visibility?: string;
  detail?: Record<string, unknown>; source_selector?: Record<string, unknown>;
};
type Lane = {
  lane_id: string; display_name?: string; role?: string; actor_kind?: string;
  detail?: { status?: string; coverage?: Record<string, unknown> };
};
type InspectorPayload = {
  schema_version: string; trace_id: string; trace_digest: string; evidence_digest?: string;
  visual?: {
    items?: InspectorItem[]; lanes?: Lane[]; run_id?: string; task_id?: string;
    state?: string; visibility_ceiling?: string; losses?: unknown[];
    summary?: Record<string, unknown>; usage?: Record<string, unknown>;
  };
};

type AnalysisFinding = {
  id: string;
  label?: string;
  status?: string;
  target?: { id?: string; selector?: string };
  targetSelector?: string;
  summary?: string;
};

export type ShellProps = {
  title?: string;
  lede?: string;
  projection?: InspectorPayload;
  data?: InspectorPayload;
  analysisFindings?: AnalysisFinding[];
};

type CraftaxAction = { step?: number; action?: string; transition?: string; reason?: string };
type CraftaxAchievement = { step?: number; name?: string };
type CraftaxRollout = {
  lane?: string; rollout_id?: string; model?: string; provider?: string; reasoning_effort?: string;
  seed?: number; reward?: number; env_steps?: number; stopped_on?: string;
  usage?: Record<string, unknown>; actions?: CraftaxAction[]; achievements?: CraftaxAchievement[];
  model_calls?: unknown[];
};
type CraftaxSummary = {
  schema_version?: string; paired?: boolean; cost_provenance?: string; rollouts?: CraftaxRollout[];
};

type Family = "message" | "tool" | "thought" | "model" | "span" | "evidence" | "system";
function family(item: InspectorItem): Family {
  const kind = item.kind;
  if (kind.startsWith("evidence.")) return "evidence";
  if (kind.startsWith("span.")) return "span";
  if (kind.includes("command_") || kind.startsWith("tool.")) return "tool";
  if (kind.includes("reasoning") || kind.includes("thought")) return "thought";
  if (kind.startsWith("message.") || kind === "codex.agent_message") return "message";
  if (kind.startsWith("model_call.") || kind.includes("turn_")) return "model";
  return "system";
}
const FAMILY_META: Record<Family, { label: string; glyph: string; tint: string }> = {
  message: { label: "Message", glyph: "◆", tint: "#eaf3ff" },
  tool: { label: "Tool", glyph: ">_", tint: "#eef8f1" },
  thought: { label: "Thought", glyph: "✦", tint: "#f6f0ff" },
  model: { label: "Model", glyph: "◌", tint: "#f2f4f7" },
  span: { label: "Span", glyph: "↔", tint: "#f2f4f7" },
  evidence: { label: "Evidence", glyph: "✓", tint: "#fff4e9" },
  system: { label: "Event", glyph: "·", tint: "#f7f7f8" }
};
function object(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}
function text(value: unknown): string { return typeof value === "string" ? value : ""; }
function native(item: InspectorItem) { return object(item.detail?.native); }
function primary(item: InspectorItem): string {
  const detail = item.detail ?? {}; const n = native(item);
  const nested = object(detail.payload);
  for (const value of [n.text, n.command, detail.reply, detail.action, detail.message, nested.reason, detail.content, detail.text, detail.rationale, detail.task_id]) {
    if (typeof value === "string" && value.trim()) return value;
  }
  if (typeof detail.score === "number") return `Score ${detail.score}`;
  if (typeof detail.call_index === "number") return `Model call ${detail.call_index}`;
  return item.title ?? item.kind;
}

function number(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}
function compactAction(value?: string): string {
  return (value ?? "—").replace(/^move_/i, "").replaceAll("_", " ").toLowerCase();
}
function tokens(value: unknown): string {
  const amount = number(value); return amount == null ? "—" : amount.toLocaleString();
}
function usd(value: unknown): string {
  const amount = number(value); return amount == null ? "—" : `$${amount.toFixed(4)}`;
}

function CraftaxComparison({ summary }: { summary: CraftaxSummary }) {
  const rollouts = summary.rollouts ?? [];
  if (!rollouts.length) return null;
  const maxReward = Math.max(...rollouts.map((rollout) => number(rollout.reward) ?? Number.NEGATIVE_INFINITY));
  const maxStep = Math.max(0, ...rollouts.flatMap((rollout) => (rollout.actions ?? []).map((action) => number(action.step) ?? 0)));
  const steps = Array.from({ length: maxStep }, (_, index) => index + 1);
  return <section className="sv-section" aria-label="Craftax policy comparison" data-testid="craftax-policy-comparison" style={{ marginTop: 14 }}>
    <div className="sv-section-head">
      <h3>Craftax policy comparison</h3>
      <span className="sv-mono">{summary.paired ? "paired seed" : "rollouts"} · real LLM policy</span>
    </div>
    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(210px, 1fr))", gap: 9 }}>
      {rollouts.map((rollout) => {
        const usage = rollout.usage ?? {}; const isWinner = number(rollout.reward) === maxReward;
        return <article key={rollout.rollout_id ?? rollout.lane} style={{ padding: 12, border: `1px solid ${isWinner ? "#8dcfaf" : "var(--sv-border)"}`, borderRadius: 10, background: isWinner ? "#f2fbf6" : "var(--sv-surface)" }}>
          <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 8 }}>
            <div><strong style={{ textTransform: "capitalize" }}>{rollout.reasoning_effort ?? rollout.lane ?? "policy"}</strong><div className="sv-mono" style={{ fontSize: 9, color: "var(--sv-text-faint)", marginTop: 2 }}>{rollout.model ?? "model"}</div></div>
            <div style={{ textAlign: "right" }}><span style={{ fontSize: 22, fontWeight: 800 }}>{rollout.reward ?? "—"}</span><div style={{ fontSize: 9.5, textTransform: "uppercase", letterSpacing: ".08em", color: "var(--sv-text-faint)" }}>reward</div></div>
          </div>
          <dl style={{ display: "grid", gridTemplateColumns: "repeat(2, 1fr)", gap: 8, margin: "12px 0 0" }}>
            {[["tokens", tokens(usage.total_tokens)], ["cost", usd(usage.estimated_usd)], ["calls", String(usage.calls ?? rollout.model_calls?.length ?? "—")], ["steps", String(rollout.env_steps ?? rollout.actions?.length ?? "—")]].map(([label, value]) => <div key={label}><dt style={{ fontSize: 9.5, textTransform: "uppercase", color: "var(--sv-text-faint)" }}>{label}</dt><dd className="sv-mono" style={{ margin: "3px 0 0", fontSize: 10, fontWeight: 700 }}>{value}</dd></div>)}
          </dl>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginTop: 10 }}>
            {(rollout.achievements ?? []).map((achievement, index) => <span key={`${achievement.step}-${achievement.name}-${index}`} title={`step ${achievement.step ?? "?"}`} style={{ padding: "3px 6px", borderRadius: 12, background: "#fff4d8", color: "#6b4d00", fontSize: 9 }}>★ {achievement.name || "achievement"}</span>)}
            {!rollout.achievements?.length ? <span style={{ fontSize: 9, color: "var(--sv-text-faint)" }}>No achievements</span> : null}
          </div>
        </article>;
      })}
    </div>
    {steps.length ? <div style={{ marginTop: 12, overflowX: "auto", border: "1px solid var(--sv-border)", borderRadius: 10 }}>
      <div style={{ minWidth: 150 + steps.length * 46 }}>
        <div style={{ display: "grid", gridTemplateColumns: `150px repeat(${steps.length}, 46px)`, background: "var(--sv-surface-muted, #f7f7f8)", borderBottom: "1px solid var(--sv-border)" }}>
          <div style={{ padding: "7px 9px", fontSize: 9, fontWeight: 700 }}>Aligned action trace</div>
          {steps.map((step) => <div key={step} className="sv-mono" style={{ padding: "7px 2px", textAlign: "center", fontSize: 9.5, color: "var(--sv-text-faint)" }}>{step}</div>)}
        </div>
        {rollouts.map((rollout) => {
          const actionByStep = new Map((rollout.actions ?? []).map((action) => [action.step, action]));
          const achievementSteps = new Set((rollout.achievements ?? []).map((achievement) => achievement.step));
          return <div key={rollout.rollout_id ?? rollout.lane} style={{ display: "grid", gridTemplateColumns: `150px repeat(${steps.length}, 46px)`, borderBottom: "1px solid var(--sv-border)" }}>
            <div style={{ padding: "8px 9px", fontSize: 10 }}><strong style={{ textTransform: "capitalize" }}>{rollout.reasoning_effort ?? rollout.lane}</strong><div style={{ color: "var(--sv-text-faint)", fontSize: 9.5 }}>{rollout.reward ?? "—"} reward</div></div>
            {steps.map((step) => { const action = actionByStep.get(step); return <div key={step} title={`Step ${step}: ${action?.action ?? "no action"}${action?.transition ? ` · ${action.transition}` : ""}${action?.reason ? ` · ${action.reason}` : ""}`} style={{ position: "relative", padding: "8px 2px", borderLeft: "1px solid var(--sv-border)", textAlign: "center", fontSize: 9.5, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", background: achievementSteps.has(step) ? "#fff8df" : "transparent" }}>{compactAction(action?.action)}{achievementSteps.has(step) ? <span aria-label="achievement" style={{ position: "absolute", top: 1, right: 2, color: "#b67a00" }}>★</span> : null}</div>; })}
          </div>;
        })}
      </div>
    </div> : null}
  </section>;
}
function output(item: InspectorItem): string {
  const n = native(item); const detail = item.detail ?? {};
  const value = n.aggregated_output ?? detail.output ?? detail.result;
  return typeof value === "string" ? value : value == null ? "" : JSON.stringify(value, null, 2);
}
function time(value?: string): string {
  if (!value) return "—";
  const date = new Date(value); return Number.isNaN(date.valueOf()) ? value : date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}
function duration(items: InspectorItem[]): string {
  const stamps = items.map((item) => Date.parse(item.occurred_at ?? "")).filter(Number.isFinite);
  if (stamps.length < 2) return "—";
  const seconds = Math.max(0, Math.round((Math.max(...stamps) - Math.min(...stamps)) / 1000));
  return seconds < 60 ? `${seconds}s` : seconds < 3600 ? `${Math.floor(seconds / 60)}m ${seconds % 60}s` : `${Math.floor(seconds / 3600)}h ${Math.floor(seconds % 3600 / 60)}m`;
}
function statusColor(status?: string) {
  return /error|fail|invalid/i.test(status ?? "") ? "#b84235" : /pass|ok|complete|decisive|valid/i.test(status ?? "") ? "#238558" : "#77808d";
}
function DetailValue({ value }: { value: unknown }) {
  if (value == null) return <span>—</span>;
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") return <span>{String(value)}</span>;
  return <code style={{ fontSize: 10, whiteSpace: "pre-wrap" }}>{JSON.stringify(value, null, 2)}</code>;
}

function EvidenceReviewSummary({ evidence, digestBound }: { evidence: InspectorItem[]; digestBound: boolean }) {
  if (!evidence.length) return <section className="sv-section" aria-label="Evidence review summary" data-testid="trace-evidence-summary" style={{ marginTop: 14, borderLeft: "4px solid #9aa3b2" }}>
    <div className="sv-section-head"><h3>Evidence review</h3><span className="sv-mono">none captured</span></div>
    <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: 11 }}>This trace has no evaluator evidence to review.</p>
  </section>;
  const decisive = evidence.filter((item) => /pass|fail|decisive|complete|valid/i.test(item.status ?? "")).length;
  return <section className="sv-section" aria-label="Evidence review summary" data-testid="trace-evidence-summary" style={{ marginTop: 14, borderLeft: `4px solid ${decisive === evidence.length ? "#238558" : "#b67a00"}` }}>
    <div className="sv-section-head">
      <h3>Evidence review</h3>
      <span className="sv-mono">{decisive}/{evidence.length} decisive · {digestBound ? "digest bound" : "unbound"}</span>
    </div>
    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(min(100%, 210px), 1fr))", gap: 8 }}>
      {evidence.slice(0, 3).map((item) => {
        const detail = item.detail ?? {};
        const rationale = text(detail.rationale) || text(detail.verdict) || primary(item);
        return <article key={item.item_id} style={{ minWidth: 0, padding: 10, border: "1px solid var(--sv-border)", borderRadius: 9, background: "var(--sv-surface)" }}>
          <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 8 }}>
            <strong style={{ minWidth: 0, fontSize: 11, overflowWrap: "anywhere" }}>{item.title ?? item.kind}</strong>
            <span style={{ flex: "0 0 auto", color: statusColor(item.status), fontSize: 9, fontWeight: 800, textTransform: "uppercase" }}>{item.status ?? "recorded"}</span>
          </div>
          <p style={{ margin: "6px 0 0", color: "var(--sv-text-faint)", fontSize: 10, lineHeight: 1.4, overflowWrap: "anywhere" }}>{rationale}</p>
          {number(detail.score) != null ? <div className="sv-mono" style={{ marginTop: 6, fontSize: 10 }}>score {number(detail.score)}</div> : null}
        </article>;
      })}
    </div>
    {evidence.length > 3 ? <p className="sv-mono" style={{ margin: "8px 0 0", color: "var(--sv-text-faint)", fontSize: 9 }}>+ {evidence.length - 3} more in Evidence</p> : null}
  </section>;
}

function EventCard({ item, expanded, onToggle, findings = [] }: { item: InspectorItem; expanded: boolean; onToggle: () => void; findings?: AnalysisFinding[] }) {
  const meta = FAMILY_META[family(item)]; const body = primary(item); const toolOutput = output(item);
  const command = family(item) === "tool"; const isLong = body.length > 360 || toolOutput.length > 260;
  const cited = findings.filter((finding) => {
    const targetId = finding.target?.id ?? "";
    const selector = finding.targetSelector ?? finding.target?.selector ?? "";
    return targetId === item.item_id || selector.includes(item.item_id) || String(item.source_selector?.entity_id ?? "") === targetId;
  });
  return <article id={`trace-${item.item_id}`} data-testid={`trace-item-${item.item_id}`} style={{ display: "grid", gridTemplateColumns: "48px minmax(0,1fr)", gap: 10, scrollMarginTop: 12 }}>
    <aside style={{ textAlign: "right", paddingTop: 12, color: "var(--sv-text-faint)" }}>
      <div className="sv-mono" style={{ fontSize: 10 }}>#{item.sequence ?? "·"}</div>
      <time style={{ fontSize: 9 }}>{time(item.occurred_at)}</time>
    </aside>
    <div style={{ border: `1px solid ${item.status && /error|fail/i.test(item.status) ? "#efb7af" : "var(--sv-border)"}`, borderRadius: 11, background: "var(--sv-surface)", overflow: "hidden" }}>
      <header style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 10px", background: meta.tint, borderBottom: "1px solid var(--sv-border)" }}>
        <span className="sv-mono" aria-hidden style={{ fontWeight: 800, color: "var(--sv-text)" }}>{meta.glyph}</span>
        <strong style={{ fontSize: 11 }}>{meta.label}</strong>
        <span className="sv-mono" style={{ fontSize: 9, color: "var(--sv-text-faint)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{item.kind}</span>
        {item.status ? <span style={{ marginLeft: "auto", fontSize: 9, color: statusColor(item.status), fontWeight: 700 }}>{item.status}</span> : null}
        {cited.length ? <span data-testid={`trace-item-findings-${item.item_id}`} style={{ fontSize: 9, fontWeight: 700, color: "var(--sv-accent)" }}>{cited.length} finding{cited.length === 1 ? "" : "s"}</span> : null}
      </header>
      <div style={{ padding: 11 }}>
        <div className={command ? "sv-mono" : undefined} style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere", fontSize: command ? 11 : 12, lineHeight: 1.5, maxHeight: expanded ? "none" : 150, overflow: "hidden" }}>{body}</div>
        {toolOutput ? <div style={{ marginTop: 9 }}>
          <div style={{ fontSize: 9, fontWeight: 700, letterSpacing: ".08em", textTransform: "uppercase", color: "var(--sv-text-faint)", marginBottom: 4 }}>Output</div>
          <pre style={{ margin: 0, padding: 9, borderRadius: 7, background: "#171a20", color: "#d9e1ea", fontSize: 10, lineHeight: 1.45, whiteSpace: "pre-wrap", overflowWrap: "anywhere", maxHeight: expanded ? 420 : 100, overflow: "auto" }}>{toolOutput || "No output"}</pre>
        </div> : null}
        {isLong ? <button className="sv-btn" type="button" onClick={onToggle} style={{ marginTop: 8, fontSize: 10 }}>{expanded ? "Show less" : "Show all"}</button> : null}
      </div>
    </div>
  </article>;
}

export function Shell({ title, lede, projection, data, analysisFindings = [] }: ShellProps) {
  const payload = projection ?? data; const visual = payload?.visual; const items = visual?.items ?? [];
  const [tab, setTab] = useState<"trace" | "evidence" | "metadata">("trace");
  const [density, setDensity] = useState<"focus" | "full">("focus");
  const [lane, setLane] = useState("all"); const [query, setQuery] = useState("");
  const [expandOutputs, setExpandOutputs] = useState(false); const [jump, setJump] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(new Set()); const listRef = useRef<HTMLDivElement>(null);
  const [playing, setPlaying] = useState(false); const [playbackIndex, setPlaybackIndex] = useState(0);
  const lanes = visual?.lanes ?? [];
  const focusedFamilies = new Set<Family>(["message", "tool", "thought", "evidence"]);
  const filtered = useMemo(() => items.filter((item) => {
    const needle = query.trim().toLowerCase();
    return (lane === "all" || item.lane_id === lane) &&
      (density === "full" || focusedFamilies.has(family(item)) || /error|fail/i.test(item.status ?? "")) &&
      (!needle || `${item.kind} ${item.title ?? ""} ${primary(item)} ${output(item)}`.toLowerCase().includes(needle));
  }), [density, items, lane, query]);
  const evidence = items.filter((item) => family(item) === "evidence");
  const tools = items.filter((item) => family(item) === "tool");
  const failures = items.filter((item) => /error|fail/i.test(item.status ?? ""));
  const usage = visual?.usage ?? {}; const summary = visual?.summary ?? {};
  const craftax = object(summary.craftax) as CraftaxSummary;

  useEffect(() => { if (expandOutputs) setExpanded(new Set(filtered.map((item) => item.item_id))); else setExpanded(new Set()); }, [expandOutputs]);
  useEffect(() => {
    if (!playing || !filtered.length) return;
    const current = filtered[Math.min(playbackIndex, filtered.length - 1)];
    document.getElementById(`trace-${current.item_id}`)?.scrollIntoView({ behavior: "smooth", block: "center" });
    const timer = window.setTimeout(() => {
      if (playbackIndex + 1 >= filtered.length) {
        setPlaying(false);
        setPlaybackIndex(0);
      } else {
        setPlaybackIndex((index) => index + 1);
      }
    }, 800);
    return () => window.clearTimeout(timer);
  }, [filtered, playbackIndex, playing]);
  // Both branches publish a surface observation. A projection this shell cannot
  // read is still a rendered fact about the pane, and review has to be able to
  // capture it — a template that only publishes on success is a template whose
  // failures can never be reviewed.
  if (!payload || payload.schema_version !== "synth.trace-projection.rollout-inspector.v1") {
    const detail = payload
      ? `unsupported projection schema ${payload.schema_version}`
      : "no projection payload resolved";
    return <div role="alert" {...surfaceObservationAttributes({ transportState: "error", error: detail })}>
      This visual requires a rollout-inspector Trace V5 projection ({detail}).
    </div>;
  }
  const jumpTo = () => {
    const target = items.find((item) => item.sequence === Number(jump));
    if (!target) return;
    setTab("trace"); setDensity("full"); setQuery("");
    window.setTimeout(() => document.getElementById(`trace-${target.item_id}`)?.scrollIntoView({ behavior: "smooth", block: "start" }), 0);
  };

  return <VisualChrome kicker="Trace V5 · sealed" title={title ?? payload.trace_id}
    lede={lede ?? `${visual?.task_id ?? visual?.run_id ?? "Agent trajectory"} · ${payload.trace_digest.slice(0, 23)}…`}
    testId="visual-trace-rollout-inspector" footer="trace.rollout_inspector.v1 · projection is read-only"
    observation={{
      // A sealed trace is terminal on arrival: this pane is not waiting for a
      // stream, so `terminal` is the honest transport state, and the projected
      // items are its semantic events. It renders no image frames at all.
      transportState: "terminal",
      terminal: true,
      rolloutCount: Math.max(lanes.length, 1),
      renderedFrameCount: 0,
      semanticEventCount: items.length,
      error: null
    }}>
    <MetricStrip metrics={[
      { label: "Events", value: String(summary.visual_item_count ?? items.length) },
      { label: "Duration", value: duration(items) },
      { label: "Tool calls", value: String(tools.length) },
      { label: "Evidence", value: String(evidence.length) },
      { label: "Findings", value: String(analysisFindings.length) }
    ]} />
    <CraftaxComparison summary={craftax} />
    <EvidenceReviewSummary evidence={evidence} digestBound={Boolean(payload.evidence_digest)} />
    <nav aria-label="Trace views" style={{ display: "flex", gap: 4, borderBottom: "1px solid var(--sv-border)", marginTop: 14 }}>
      {(["trace", "evidence", "metadata"] as const).map((value) => <button key={value} type="button" className="sv-btn" aria-current={tab === value ? "page" : undefined} onClick={() => setTab(value)} style={{ border: 0, borderRadius: "7px 7px 0 0", borderBottom: tab === value ? "2px solid var(--sv-accent)" : "2px solid transparent", background: tab === value ? "var(--sv-accent-soft)" : "transparent", textTransform: "capitalize" }}>{value}</button>)}
    </nav>

    {tab === "trace" ? <>
      <section className="sv-section" aria-label="Trace controls" style={{ position: "sticky", top: 0, zIndex: 2, background: "var(--sv-surface)", paddingBottom: 8 }}>
		<div style={{ display: "grid", gridTemplateColumns: "auto minmax(110px,1fr)", gap: 7 }}>
          <div role="group" aria-label="Trace density" style={{ display: "flex" }}>
            {(["focus", "full"] as const).map((value) => <button key={value} type="button" className="sv-btn" aria-pressed={density === value} onClick={() => setDensity(value)} style={{ fontSize: 10, background: density === value ? "var(--sv-text)" : "transparent", color: density === value ? "white" : "inherit" }}>{value}</button>)}
          </div>
          <select aria-label="Trace lane" value={lane} onChange={(event) => setLane(event.target.value)}><option value="all">all lanes</option>{lanes.map((value) => <option key={value.lane_id} value={value.lane_id}>{value.display_name ?? value.role ?? value.lane_id}</option>)}</select>
			<input aria-label="Search trace" placeholder="Search commands, output, messages…" value={query} onChange={(event) => setQuery(event.target.value)} style={{ gridColumn: "1 / -1", minWidth: 0 }} />
		</div>
		<div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 7, marginTop: 7 }}>
          <button type="button" className="sv-btn" aria-label={playing ? "Pause playback" : "Play playback"} onClick={() => {
            if (!playing && playbackIndex >= filtered.length) setPlaybackIndex(0);
            setPlaying((value) => !value);
          }}>{playing ? "Pause" : "Play"}</button>
          <input aria-label="Jump to sequence" inputMode="numeric" placeholder="Sequence #" value={jump} onChange={(event) => setJump(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") jumpTo(); }} style={{ width: 92 }} />
          <button type="button" className="sv-btn" onClick={jumpTo}>Jump</button>
			<label style={{ display: "flex", alignItems: "center", gap: 5, fontSize: 10 }}><input type="checkbox" checked={expandOutputs} onChange={(event) => setExpandOutputs(event.target.checked)} /> expand outputs</label>
        </div>
      </section>
      <div className="sv-mono" style={{ fontSize: 10, color: "var(--sv-text-faint)", margin: "4px 0 10px" }}>{filtered.length} of {items.length} projected items · {density === "focus" ? "operational signal" : "complete projection"}</div>
      <div ref={listRef} aria-live="polite" style={{ display: "grid", gap: 9, maxHeight: "min(62vh, 640px)", overflow: "auto", padding: 10, border: "1px solid var(--sv-border)", borderRadius: 12, background: "var(--sv-wash, #fafaf9)" }}>
        {filtered.map((item) => <EventCard key={item.item_id} item={item} findings={analysisFindings} expanded={expanded.has(item.item_id)} onToggle={() => setExpanded((current) => { const next = new Set(current); next.has(item.item_id) ? next.delete(item.item_id) : next.add(item.item_id); return next; })} />)}
        {!filtered.length ? <p style={{ color: "var(--sv-text-faint)" }}>No projected items match these filters.</p> : null}
      </div>
    </> : null}

    {tab === "evidence" ? <section className="sv-section" aria-label="Trace evidence">
      <div className="sv-section-head"><h3>Evaluation evidence</h3><span className="sv-mono">{payload.evidence_digest ? "digest bound" : "no evidence digest"}</span></div>
      {evidence.map((item) => <div key={item.item_id} style={{ padding: 12, border: "1px solid var(--sv-border)", borderLeft: `4px solid ${statusColor(item.status)}`, borderRadius: 9, marginBottom: 8 }}>
        <div style={{ display: "flex", justifyContent: "space-between", gap: 10 }}><strong>{item.title ?? item.kind}</strong><span style={{ color: statusColor(item.status), fontWeight: 700 }}>{item.status}</span></div>
		<dl style={{ display: "grid", gridTemplateColumns: "minmax(90px,.6fr) 1.4fr", gap: "6px 12px", fontSize: 11, marginBottom: 0 }}>{Object.entries(item.detail ?? {}).map(([key, value]) => <div key={key} style={{ display: "contents" }}><dt style={{ color: "var(--sv-text-faint)" }}>{key}</dt><dd style={{ margin: 0 }}><DetailValue value={value} /></dd></div>)}</dl>
      </div>)}
      {!evidence.length ? <p>No evaluation evidence was captured in this sealed trace.</p> : null}
    </section> : null}

    {tab === "metadata" ? <section className="sv-section" aria-label="Trace metadata">
      <div className="sv-section-head"><h3>Identity & provenance</h3><span className="sv-mono">{visual?.state ?? "unknown"}</span></div>
      <dl style={{ display: "grid", gridTemplateColumns: "110px minmax(0,1fr)", gap: "8px 12px", fontSize: 11 }}>
		{[["trace id", payload.trace_id], ["trace digest", payload.trace_digest], ["evidence digest", payload.evidence_digest], ["run", visual?.run_id], ["task", visual?.task_id], ["visibility", visual?.visibility_ceiling], ["lanes", lanes.length], ["failures", failures.length]].map(([key, value]) => <div key={String(key)} style={{ display: "contents" }}><dt style={{ color: "var(--sv-text-faint)" }}>{key}</dt><dd className="sv-mono" style={{ margin: 0, overflowWrap: "anywhere" }}>{String(value ?? "—")}</dd></div>)}
      </dl>
      <div className="sv-section-head" style={{ marginTop: 18 }}><h3>Usage</h3><span className="sv-mono">{text(usage.provenance) || "reported"}</span></div>
      <MetricStrip metrics={[
        { label: "Requests", value: String(usage.requests ?? "—") },
        { label: "Input tokens", value: typeof usage.prompt_tokens === "number" ? usage.prompt_tokens.toLocaleString() : "—" },
        { label: "Output tokens", value: typeof usage.completion_tokens === "number" ? usage.completion_tokens.toLocaleString() : "—" },
        { label: "Cached", value: typeof usage.cached_tokens === "number" ? usage.cached_tokens.toLocaleString() : "—" }
      ]} />
      {lanes.map((value) => <div key={value.lane_id} style={{ marginTop: 12, padding: 11, border: "1px solid var(--sv-border)", borderRadius: 9 }}><strong>{value.display_name ?? value.role ?? value.lane_id}</strong><p className="sv-mono" style={{ fontSize: 10, color: "var(--sv-text-faint)" }}>{value.detail?.status ?? "unknown"} · {value.actor_kind ?? "actor"}</p><div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>{Object.entries(value.detail?.coverage ?? {}).map(([key, coverage]) => <span key={key} style={{ padding: "3px 6px", borderRadius: 10, background: coverage === "complete" ? "#e8f6ee" : "#f2f4f7", fontSize: 9 }}>{key} · {String(coverage)}</span>)}</div></div>)}
    </section> : null}
  </VisualChrome>;
}

export default Shell;

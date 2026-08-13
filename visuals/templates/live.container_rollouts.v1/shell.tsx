import { useMemo, useState } from "react";
import { VisualChrome, MetricStrip } from "../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../chrome/useLiveEvalStream.ts";
import { formatMissingNumber, formatMissingUsd, missingNumber } from "../../runtime/liveStream.ts";
import type { LiveEvalEvent } from "../../runtime/types.ts";

type StreamPayload = { run_id?: string; events?: LiveEvalEvent[]; sse_url?: string };
type Lane = {
  name: string; status: "starting" | "running" | "finished" | "failed";
  done: number; total?: number; reward?: number; achievements: string[];
  health?: number; food?: number; drink?: number; energy?: number;
  calls?: number; tokens?: number; cost?: number; last: string; model?: string;
  frameUrl?: string;
};

function num(v: unknown): number | undefined { return missingNumber(v); }
function payloadObject(v: unknown): Record<string, unknown> { return v && typeof v === "object" ? v as Record<string, unknown> : {}; }
function timestamp(e: LiveEvalEvent) { return e.occurred_at ?? e.ts ?? ""; }
function displayTime(value: string) {
  if (!value) return "Waiting for an event";
  const parsed = new Date(value);
  return Number.isNaN(parsed.valueOf()) ? value : parsed.toLocaleString([], { month: "short", day: "numeric", hour: "numeric", minute: "2-digit", second: "2-digit" });
}
function eventDetail(e: LiveEvalEvent) {
  const p = e.payload;
  const kind = typeof p.kind === "string" ? p.kind : e.kind;
  for (const key of ["message", "detail", "action", "stopped_on", "phase"]) if (typeof p[key] === "string" && p[key]) return `${kind} · ${p[key]}`;
  return kind;
}
function laneName(e: LiveEvalEvent) { return e.lane ?? e.run_id ?? "rollout"; }
function project(events: LiveEvalEvent[]): Lane[] {
  const lanes = new Map<string, Lane>();
  for (const e of events) {
    const name = laneName(e);
    const lane = lanes.get(name) ?? { name, status: "starting", done: 0, achievements: [], last: "opening rollout" };
    const p = e.payload;
    if (e.kind === "eval.phase") { lane.status = "running"; lane.model = String(payloadObject(p.policy).model ?? p.model ?? "") || undefined; }
    if (e.kind === "snapshot") {
      lane.status = "running";
      const progress = payloadObject(p.progress);
      lane.done = num(progress.done) ?? num(progress.env_steps) ?? num(p.step) ?? lane.done;
      lane.total = num(progress.total) ?? lane.total;
      lane.reward = num(p.total_reward) ?? num(p.reward) ?? lane.reward;
      if (typeof p.frame_url === "string") lane.frameUrl = p.frame_url;
      const observation = payloadObject(payloadObject(p.readout).observation);
      const ach = p.achievements ?? observation.achievements;
      if (Array.isArray(ach)) lane.achievements = ach.map(String);
      else if (ach && typeof ach === "object") lane.achievements = Object.entries(ach as Record<string, unknown>).filter(([, value]) => Boolean(value)).map(([key]) => key);
      const vitals = payloadObject(p.vitals);
      const directInventory = payloadObject(p.inventory);
      const inv = Object.keys(directInventory).length ? directInventory : payloadObject(observation.inventory);
      lane.health = num(p.health) ?? num(vitals.health) ?? num(inv.health) ?? lane.health;
      lane.food = num(p.food) ?? num(vitals.food) ?? num(inv.food) ?? lane.food;
      lane.drink = num(p.drink) ?? num(vitals.drink) ?? num(inv.drink) ?? lane.drink;
      lane.energy = num(p.energy) ?? num(vitals.energy) ?? num(inv.energy) ?? lane.energy;
    }
    const usage = payloadObject(p.usage);
    lane.calls = num(p.calls) ?? num(usage.calls) ?? lane.calls;
    lane.tokens = num(p.tokens) ?? num(usage.total_tokens) ?? lane.tokens;
    lane.cost = num(p.cost_usd) ?? num(usage.cost_usd) ?? lane.cost;
    if (e.kind === "eval.run.terminal" || e.kind === "run_finished") { lane.status = p.error ? "failed" : "finished"; lane.reward = num(p.reward) ?? lane.reward; }
    if (e.kind === "error" || e.kind === "eval.ops.warning") lane.status = "failed";
    lane.last = eventDetail(e);
    lanes.set(name, lane);
  }
  return [...lanes.values()];
}

function Vital({ label, value }: { label: string; value?: number }) {
  const pct = value == null ? 0 : value <= 9 ? value / 9 * 100 : Math.min(100, value);
  return <div title={`${label}: ${value ?? "unknown"}`} style={{ display: "grid", gap: 3 }}><span className="sv-mono" style={{ fontSize: 9, color: "var(--sv-text-faint)" }}>{label}</span><span style={{ width: 42, height: 4, borderRadius: 9, background: "var(--sv-border)", overflow: "hidden" }}><span style={{ display: "block", width: `${pct}%`, height: "100%", background: pct < 34 ? "#d84b3f" : pct < 67 ? "#e5a226" : "#39a46b" }} /></span></div>;
}

function LaneReplay({ laneEvents, streamBase }: { laneEvents: LiveEvalEvent[]; streamBase: URL | null }) {
  const [cursor, setCursor] = useState<number | null>(null);
  const selected = cursor == null ? laneEvents.length - 1 : Math.max(0, Math.min(cursor, laneEvents.length - 1));
  const lane = project(laneEvents.slice(0, selected + 1))[0];
  if (!lane) return null;
  const pct = lane.total ? Math.min(100, lane.done / lane.total * 100) : 0;
  const selectedTime = displayTime(timestamp(laneEvents[selected]));
  return <article style={{ border: "1px solid var(--sv-border)", borderRadius: 10, padding: 12, background: lane.status === "running" ? "#fffaf7" : "var(--sv-surface)" }}>
    <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "baseline" }}><strong style={{ fontSize: 13 }}>{lane.name}</strong><span className="sv-mono" style={{ color: lane.status === "failed" ? "#c2553f" : "var(--sv-accent)", fontSize: 11 }}>{lane.status}</span></div>
    <div style={{ display: "flex", justifyContent: "space-between", gap: 12, marginTop: 10 }}><label htmlFor={`lane-time-${lane.name}`} className="sv-mono" style={{ fontSize: 10, color: "var(--sv-text-faint)" }}>Rollout time</label><time className="sv-mono" style={{ fontSize: 10 }}>{selectedTime}</time></div>
    <input id={`lane-time-${lane.name}`} type="range" min={0} max={Math.max(0, laneEvents.length - 1)} value={selected} onChange={(event) => setCursor(Number(event.currentTarget.value))} aria-label={`Replay ${lane.name}`} style={{ width: "100%", margin: "5px 0 2px" }} />
    <div style={{ height: 7, background: "var(--sv-border)", borderRadius: 8, margin: "9px 0 7px", overflow: "hidden" }}><div style={{ width: `${pct}%`, height: "100%", background: "var(--sv-accent)", transition: "width 180ms ease" }} /></div>
    <div style={{ display: "flex", justifyContent: "space-between", gap: 8, color: "var(--sv-text-muted)", fontSize: 11 }}><span className="sv-mono">{lane.done}{lane.total ? ` / ${lane.total}` : " steps"}</span><span>reward <strong style={{ color: "var(--sv-text)" }}>{formatMissingNumber(lane.reward)}</strong></span><span>{lane.achievements.length} achievements</span>{lane.calls != null ? <span>{lane.calls} calls</span> : null}{lane.cost != null ? <span>{formatMissingUsd(lane.cost)}</span> : null}</div>
    <div style={{ display: "flex", gap: 10, marginTop: 10, alignItems: "end" }}><Vital label="HLTH" value={lane.health} /><Vital label="FOOD" value={lane.food} /><Vital label="DRNK" value={lane.drink} /><Vital label="NRGY" value={lane.energy} /><span className="sv-mono" style={{ marginLeft: "auto", maxWidth: "52%", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 10, color: "var(--sv-text-faint)" }}>› {lane.last}</span></div>
    {lane.frameUrl && streamBase ? <img src={new URL(lane.frameUrl, streamBase).toString()} alt={`Craftax world for ${lane.name} at ${selectedTime}`} style={{ display: "block", width: "100%", maxHeight: 300, marginTop: 10, borderRadius: 8, objectFit: "contain", imageRendering: "pixelated", background: "#111" }} /> : null}
  </article>;
}

export function Shell(props: { title?: string; lede?: string; stream?: StreamPayload }) {
  const stream = props.stream ?? {};
  const { events, live, error, ready } = useLiveEvalStream({ sseUrl: stream.sse_url, fixtureEvents: stream.sse_url ? undefined : stream.events, replayMs: 360 });
  const hasSource = Boolean(stream.sse_url || stream.events);
  const [globalCursor, setGlobalCursor] = useState<number | null>(null);
  const selectedGlobal = globalCursor == null ? events.length - 1 : Math.max(0, Math.min(globalCursor, events.length - 1));
  const visibleEvents = events.slice(0, selectedGlobal + 1);
  const lanes = useMemo(() => project(visibleEvents), [visibleEvents]);
  const done = lanes.filter((lane) => lane.status === "finished").length;
  const rewardValues = lanes.map((lane) => lane.reward).filter((value): value is number => value != null);
  const totalReward = rewardValues.length ? rewardValues.reduce((sum, value) => sum + value, 0) : undefined;
  const allAchievements = new Set(lanes.flatMap((lane) => lane.achievements));
  const recent = visibleEvents.slice(-8).reverse();
  const streamBase = stream.sse_url ? new URL(stream.sse_url, window.location.href) : null;

  return <VisualChrome kicker="Container eval · live" live={live} title={props.title ?? "Live rollout progress"} lede={props.lede ?? "Watch real rollout position, outcomes, and engine activity as they arrive."} testId="visual-live-container-rollouts" footer="live.container_rollouts.v1 · synth.rollout.event.v1">
    <MetricStrip metrics={[{ label: "Rollouts", value: `${done}/${lanes.length || "—"} done` }, { label: "Total reward", value: formatMissingNumber(totalReward) }, { label: "Achievements", value: String(allAchievements.size) }, { label: "Stream", value: !hasSource ? "awaiting source" : !ready ? "connecting" : live ? "receiving" : done ? "complete" : "waiting" }]} />
    <section className="sv-section" aria-label="Evaluation replay"><div className="sv-section-head"><h3>Evaluation time</h3><time className="sv-mono">{events.length ? displayTime(timestamp(events[selectedGlobal])) : "Waiting for an event"}</time></div><input type="range" min={0} max={Math.max(0, events.length - 1)} value={selectedGlobal} onChange={(event) => setGlobalCursor(Number(event.currentTarget.value))} disabled={!events.length} aria-label="Replay the complete evaluation" style={{ width: "100%" }} /></section>
    {error ? <p role="alert" style={{ color: "#c2553f" }}>{error}</p> : null}
    <section className="sv-section" aria-label="Rollout lanes" aria-live="polite"><div className="sv-section-head"><h3>Rollouts</h3><span className="sv-mono">true step progress</span></div><div style={{ display: "grid", gap: 8 }}>{lanes.map((lane) => <LaneReplay key={lane.name} laneEvents={visibleEvents.filter((event) => laneName(event) === lane.name)} streamBase={streamBase} />)}{!lanes.length ? <div style={{ padding: 20, color: "var(--sv-text-faint)" }}>Waiting for the first rollout…</div> : null}</div></section>
    <section className="sv-section" aria-label="Recent activity"><div className="sv-section-head"><h3>Recent activity</h3><span className="sv-mono">{visibleEvents.length} events</span></div><ol style={{ listStyle: "none", margin: 0, padding: 0 }}>{recent.map((event, index) => <li key={`${timestamp(event)}-${index}`} style={{ display: "grid", gridTemplateColumns: "66px minmax(90px, 0.7fr) 2fr", gap: 10, padding: "7px 0", borderTop: "1px solid var(--sv-border)", fontSize: 11 }}><time className="sv-mono" style={{ color: "var(--sv-text-faint)" }}>{timestamp(event).slice(11, 19)}</time><strong style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{event.lane ?? "eval"}</strong><span className="sv-mono" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{eventDetail(event)}</span></li>)}</ol></section>
  </VisualChrome>;
}

export default Shell;

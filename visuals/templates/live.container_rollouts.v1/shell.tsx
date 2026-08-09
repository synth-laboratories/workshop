import { useMemo } from "react";
import { VisualChrome, MetricStrip } from "../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../chrome/useLiveEvalStream.ts";
import type { LiveEvalEvent } from "../../runtime/types.ts";
import fixture from "../../fixtures/live_container_rollout_events.json";

type StreamPayload = { run_id?: string; events?: LiveEvalEvent[]; sse_url?: string };
type Lane = {
  name: string; status: "starting" | "running" | "finished" | "failed";
  done: number; total?: number; reward: number; achievements: string[];
  health?: number; food?: number; drink?: number; energy?: number;
  calls?: number; tokens?: number; cost?: number; last: string; model?: string;
};

function num(v: unknown): number | undefined {
  return typeof v === "number" && Number.isFinite(v) ? v : undefined;
}
function payloadObject(v: unknown): Record<string, unknown> {
  return v && typeof v === "object" ? v as Record<string, unknown> : {};
}
function timestamp(e: LiveEvalEvent) { return e.occurred_at ?? e.ts ?? ""; }
function eventDetail(e: LiveEvalEvent) {
  const p = e.payload;
  const kind = typeof p.kind === "string" ? p.kind : e.kind;
  for (const key of ["message", "detail", "action", "stopped_on", "phase"]) {
    if (typeof p[key] === "string" && p[key]) return `${kind} · ${p[key]}`;
  }
  return kind;
}
function project(events: LiveEvalEvent[]): Lane[] {
  const lanes = new Map<string, Lane>();
  for (const e of events) {
    const name = e.lane ?? e.run_id ?? "rollout";
    const lane = lanes.get(name) ?? {
      name, status: "starting", done: 0, reward: 0, achievements: [], last: "opening rollout"
    };
    const p = e.payload;
    if (e.kind === "eval.phase") {
      lane.status = "running";
      lane.model = String(payloadObject(p.policy).model ?? p.model ?? "") || undefined;
    }
    if (e.kind === "snapshot") {
      lane.status = "running";
      const progress = payloadObject(p.progress);
      lane.done = num(progress.done) ?? num(p.step) ?? lane.done;
      lane.total = num(progress.total) ?? lane.total;
      lane.reward = num(p.total_reward) ?? num(p.reward) ?? lane.reward;
      const ach = p.achievements;
      if (Array.isArray(ach)) lane.achievements = ach.map(String);
      else if (ach && typeof ach === "object") {
        lane.achievements = Object.entries(ach as Record<string, unknown>)
          .filter(([, value]) => Boolean(value)).map(([key]) => key);
      }
      const vitals = payloadObject(p.vitals);
      const inv = payloadObject(p.inventory);
      lane.health = num(p.health) ?? num(vitals.health) ?? num(inv.health) ?? lane.health;
      lane.food = num(p.food) ?? num(vitals.food) ?? num(inv.food) ?? lane.food;
      lane.drink = num(p.drink) ?? num(vitals.drink) ?? num(inv.drink) ?? lane.drink;
      lane.energy = num(p.energy) ?? num(vitals.energy) ?? num(inv.energy) ?? lane.energy;
    }
    const usage = payloadObject(p.usage);
    lane.calls = num(p.calls) ?? num(usage.calls) ?? lane.calls;
    lane.tokens = num(p.tokens) ?? num(usage.total_tokens) ?? lane.tokens;
    lane.cost = num(p.cost_usd) ?? num(usage.cost_usd) ?? lane.cost;
    if (e.kind === "eval.run.terminal" || e.kind === "run_finished") {
      lane.status = p.error ? "failed" : "finished";
      lane.reward = num(p.reward) ?? lane.reward;
    }
    if (e.kind === "error" || e.kind === "eval.ops.warning") lane.status = "failed";
    lane.last = eventDetail(e);
    lanes.set(name, lane);
  }
  return [...lanes.values()];
}

function Vital({ label, value }: { label: string; value?: number }) {
  const pct = value == null ? 0 : value <= 9 ? value / 9 * 100 : Math.min(100, value);
  return <div title={`${label}: ${value ?? "unknown"}`} style={{ display: "grid", gap: 3 }}>
    <span className="sv-mono" style={{ fontSize: 9, color: "var(--sv-text-faint)" }}>{label}</span>
    <span style={{ width: 42, height: 4, borderRadius: 9, background: "var(--sv-border)", overflow: "hidden" }}>
      <span style={{ display: "block", width: `${pct}%`, height: "100%", background: pct < 34 ? "#d84b3f" : pct < 67 ? "#e5a226" : "#39a46b" }} />
    </span>
  </div>;
}

export function Shell(props: { title?: string; lede?: string; stream?: StreamPayload }) {
  const stream = props.stream ?? (fixture as StreamPayload);
  const { events, live, error } = useLiveEvalStream({
    sseUrl: stream.sse_url,
    fixtureEvents: stream.sse_url ? undefined : stream.events,
    replayMs: 360
  });
  const lanes = useMemo(() => project(events), [events]);
  const done = lanes.filter((l) => l.status === "finished").length;
  const totalReward = lanes.reduce((sum, l) => sum + l.reward, 0);
  const allAchievements = new Set(lanes.flatMap((l) => l.achievements));
  const recent = events.slice(-8).reverse();

  return <VisualChrome kicker="Container eval · live" live={live}
    title={props.title ?? "Live rollout progress"}
    lede={props.lede ?? "Watch real rollout position, outcomes, and engine activity as they arrive."}
    testId="visual-live-container-rollouts" footer="live.container_rollouts.v1 · evals.event-stream.v1">
    <MetricStrip metrics={[
      { label: "Rollouts", value: `${done}/${lanes.length || "—"} done` },
      { label: "Total reward", value: totalReward.toFixed(2) },
      { label: "Achievements", value: String(allAchievements.size) },
      { label: "Stream", value: live ? "receiving" : done ? "complete" : "waiting" }
    ]} />
    {error ? <p role="alert" style={{ color: "#c2553f" }}>{error}</p> : null}
    <section className="sv-section" aria-label="Rollout lanes" aria-live="polite">
      <div className="sv-section-head"><h3>Rollouts</h3><span className="sv-mono">true step progress</span></div>
      <div style={{ display: "grid", gap: 8 }}>
        {lanes.map((lane) => {
          const pct = lane.total ? Math.min(100, lane.done / lane.total * 100) : 0;
          return <article key={lane.name} style={{ border: "1px solid var(--sv-border)", borderRadius: 10, padding: 12, background: lane.status === "running" ? "#fffaf7" : "var(--sv-surface)" }}>
            <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "baseline" }}>
              <strong style={{ fontSize: 13 }}>{lane.name}</strong>
              <span className="sv-mono" style={{ color: lane.status === "failed" ? "#c2553f" : "var(--sv-accent)", fontSize: 11 }}>{lane.status}</span>
            </div>
            <div style={{ height: 7, background: "var(--sv-border)", borderRadius: 8, margin: "9px 0 7px", overflow: "hidden" }}>
              <div style={{ width: `${pct}%`, height: "100%", background: "var(--sv-accent)", transition: "width 180ms ease" }} />
            </div>
            <div style={{ display: "flex", justifyContent: "space-between", gap: 8, color: "var(--sv-text-muted)", fontSize: 11 }}>
              <span className="sv-mono">{lane.done}{lane.total ? ` / ${lane.total}` : " steps"}</span>
              <span>reward <strong style={{ color: "var(--sv-text)" }}>{lane.reward.toFixed(2)}</strong></span>
              <span>{lane.achievements.length} achievements</span>
              {lane.calls != null ? <span>{lane.calls} calls</span> : null}
              {lane.cost != null ? <span>${lane.cost.toFixed(4)}</span> : null}
            </div>
            <div style={{ display: "flex", gap: 10, marginTop: 10, alignItems: "end" }}>
              <Vital label="HLTH" value={lane.health} /><Vital label="FOOD" value={lane.food} />
              <Vital label="DRNK" value={lane.drink} /><Vital label="NRGY" value={lane.energy} />
              <span className="sv-mono" style={{ marginLeft: "auto", maxWidth: "52%", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 10, color: "var(--sv-text-faint)" }}>› {lane.last}</span>
            </div>
          </article>;
        })}
        {!lanes.length ? <div style={{ padding: 20, color: "var(--sv-text-faint)" }}>Waiting for the first rollout…</div> : null}
      </div>
    </section>
    <section className="sv-section" aria-label="Recent activity">
      <div className="sv-section-head"><h3>Recent activity</h3><span className="sv-mono">{events.length} events</span></div>
      <ol style={{ listStyle: "none", margin: 0, padding: 0 }}>
        {recent.map((e, i) => <li key={`${timestamp(e)}-${i}`} style={{ display: "grid", gridTemplateColumns: "66px minmax(90px, 0.7fr) 2fr", gap: 10, padding: "7px 0", borderTop: "1px solid var(--sv-border)", fontSize: 11 }}>
          <time className="sv-mono" style={{ color: "var(--sv-text-faint)" }}>{timestamp(e).slice(11, 19)}</time>
          <strong style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{e.lane ?? "eval"}</strong>
          <span className="sv-mono" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{eventDetail(e)}</span>
        </li>)}
      </ol>
    </section>
  </VisualChrome>;
}

export default Shell;

import { useMemo, useState } from "react";
import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../../chrome/useLiveEvalStream.ts";
import type { LiveTemplateProps } from "../../../runtime/replayClient.ts";
import type { LiveEvalEvent } from "../../../runtime/types.ts";
import "./shell.css";

type SwarmSummary = {
  schemaVersion?: string;
  swarmId?: string;
  objective?: string;
  status?: string;
  projectId?: string | null;
  maxSpendUsd?: number;
  maxConcurrentActors?: number;
  model?: string;
  provider?: string;
  streamState?: string;
};
type StreamPayload = { events?: LiveEvalEvent[]; replay_ms?: number };
export type ShellProps = LiveTemplateProps & { title?: string; lede?: string; swarm?: SwarmSummary; stream?: StreamPayload };

const text = (value: unknown) => typeof value === "string" && value.trim() ? value.trim() : null;
const number = (value: unknown) => typeof value === "number" && Number.isFinite(value) ? value : null;
const record = (value: unknown): Record<string, unknown> | null => value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
const payloadValue = (event: LiveEvalEvent, key: string) => event.payload[key] ?? record(event.payload.payload)?.[key];
const snapshotActors = (event: LiveEvalEvent) => Array.isArray(event.payload.actors)
  ? event.payload.actors.map(record).filter((value): value is Record<string, unknown> => value !== null)
  : [];
const actorIdentity = (actor: Record<string, unknown>) => text(actor.actor_id) ?? text(actor.actor_key) ?? text(actor.participant_session_id);
const statusOf = (events: LiveEvalEvent[], fallback?: string) => {
  for (let index = events.length - 1; index >= 0; index--) {
    const payload = events[index]?.payload ?? {};
    const timeline = record(payload.timeline);
    const lifecycle = record(timeline?.lifecycle);
    const found = text(payload.status) ?? text(payload.state) ?? text(payload.swarm_status)
      ?? (text(lifecycle?.terminal_phase)?.toLowerCase() === "terminal" ? "completed" : null)
      ?? text(timeline?.authority_state) ?? text(timeline?.authority_persisted_state);
    if (found) return found;
  }
  const latestActors = [...events].reverse().map(snapshotActors).find((actors) => actors.length > 0) ?? [];
  if (latestActors.length > 0) {
    return latestActors.every((actor) => text(actor.completion_status)) ? "completed" : "running";
  }
  return fallback ?? "starting";
};
const actorOf = (event: LiveEvalEvent) => text(payloadValue(event, "actor_id")) ?? text(payloadValue(event, "agent_id")) ?? text(payloadValue(event, "actor")) ?? text(payloadValue(event, "participant_session_id")) ?? text(event.lane);
const labelOf = (event: LiveEvalEvent) => text(payloadValue(event, "type")) ?? text(payloadValue(event, "kind")) ?? text(payloadValue(event, "event")) ?? event.kind;
const terminalStatus = (status: string) => ["completed", "failed", "cancelled", "canceled", "succeeded", "timed_out"].includes(status.toLowerCase());
const money = (value: number | null | undefined) => value == null ? "—" : `$${value.toFixed(value < 1 ? 3 : 2)}`;

export function Shell(props: ShellProps) {
  const swarm = props.swarm ?? {};
  const fixture = props.replay?.streams.length ? undefined : props.stream?.events;
  const liveStream = useLiveEvalStream({ replay: props.replay, fixtureEvents: fixture, replayMs: props.stream?.replay_ms, visualId: props.visualId, revision: props.revision });
  const max = Math.max(0, liveStream.events.length - 1);
  const [cut, setCut] = useState<number | null>(null);
  const effective = cut == null ? max : Math.min(cut, max);
  const visible = useMemo(() => liveStream.events.slice(0, liveStream.events.length ? effective + 1 : 0), [liveStream.events, effective]);
  const status = statusOf(visible, swarm.status);
  const terminal = terminalStatus(status);
  const actors = useMemo(() => [...new Set([
    ...visible.map(actorOf),
    ...visible.flatMap(snapshotActors).map(actorIdentity),
  ].filter((value): value is string => Boolean(value)))], [visible]);
  const transcript = visible.filter((event) => event.kind === "transcript" || text(payloadValue(event, "message")) || text(payloadValue(event, "summary")));
  const latestUsage = [...visible].reverse().find((event) => number(payloadValue(event, "spend_usd")) != null || number(payloadValue(event, "cost_usd")) != null || number(payloadValue(event, "tokens")) != null);
  const spend = latestUsage ? number(payloadValue(latestUsage, "spend_usd")) ?? number(payloadValue(latestUsage, "cost_usd")) : null;
  const tokens = latestUsage ? number(payloadValue(latestUsage, "tokens")) ?? number(payloadValue(latestUsage, "total_tokens")) : null;

  return <VisualChrome
    kicker="Synth Cloud · Swarm"
    live={!terminal && liveStream.state === "live"}
    title={props.title ?? "Live swarm"}
    lede={props.lede ?? swarm.objective ?? "Bounded multi-agent research, streamed through the pinned Synth SDK."}
    testId="visual-live-synth-swarm"
    observation={{ transportState: terminal ? "terminal" : liveStream.state, semanticEventCount: visible.length, terminal, error: liveStream.error }}
    captureBlocked={Boolean(props.replay?.streams.length) && !liveStream.ready}
    footer="live.synth_swarm.v1 · SDK SSE · sanitized structural evidence only"
  >
    <div className="sw-summary">
      <div className="sw-stat"><small>Status</small><strong data-status={status.toLowerCase()}>{status}</strong></div>
      <div className="sw-stat"><small>Actors observed</small><strong>{actors.length}{swarm.maxConcurrentActors ? ` / ${swarm.maxConcurrentActors}` : ""}</strong></div>
      <div className="sw-stat"><small>Spend</small><strong>{money(spend)}{swarm.maxSpendUsd != null ? ` / ${money(swarm.maxSpendUsd)}` : ""}</strong></div>
      <div className="sw-stat"><small>Events</small><strong>{visible.length}</strong></div>
    </div>
    <section className="sw-stage" aria-label="Swarm activity">
      <div className="sv-section-head"><h3>Live activity</h3><span className="sv-mono">{swarm.swarmId ?? visible[0]?.run_id ?? "awaiting id"}</span></div>
      <div className="sw-actors">
        {(actors.length ? actors : ["Coordinator"]).map((actor) => {
          const actorEvents = visible.filter((event) => actorOf(event) === actor);
          const latest = actorEvents.at(-1);
          const nested = [...visible].reverse().flatMap(snapshotActors).find((candidate) => actorIdentity(candidate) === actor);
          const activity = latest ? labelOf(latest) : text(nested?.last_activity_kind) ?? text(nested?.completion_reason) ?? "Waiting for assignment";
          const observations = actorEvents.length || visible.filter((event) => snapshotActors(event).some((candidate) => actorIdentity(candidate) === actor)).length;
          return <div className="sw-actor" key={actor} data-active={!terminal && !text(nested?.completion_status) ? "true" : "false"}>
            <span className="sw-orb" aria-hidden="true"/><div><strong>{actor}</strong><p>{activity}</p></div><span>{observations}</span>
          </div>;
        })}
      </div>
    </section>
    {liveStream.events.length > 1 ? <div className="sw-scrub"><span className="sv-mono">0</span><input aria-label="Swarm event cutoff" type="range" min="0" max={max} value={effective} onChange={(event) => setCut(Number(event.target.value))}/><button className="sv-button" type="button" onClick={() => setCut(null)}>Live edge</button></div> : null}
    <div className="sw-lower">
      <section className="sw-card"><div className="sv-section-head"><h3>Recent events</h3><span>{tokens == null ? "usage pending" : `${tokens.toLocaleString()} tokens`}</span></div>
        <div className="sw-events">{visible.slice(-12).reverse().map((event, index) => <div className="sw-event" key={`${event.sequence ?? event.logical_time ?? index}`}><span className="sw-kind">{labelOf(event)}</span><span>{actorOf(event) ?? "swarm"}</span><time>{event.ts ? new Date(event.ts).toLocaleTimeString() : `#${event.sequence ?? event.logical_time ?? "—"}`}</time></div>)}</div>
      </section>
      <section className="sw-card"><div className="sv-section-head"><h3>Transcript pulse</h3><span>{transcript.length} updates</span></div>
        <div className="sw-transcript">{transcript.slice(-6).reverse().map((event, index) => <article key={`${event.sequence ?? index}`}><strong>{actorOf(event) ?? "Swarm"}</strong><p>{text(payloadValue(event, "summary")) ?? text(payloadValue(event, "message")) ?? labelOf(event)}</p></article>)}{transcript.length === 0 ? <p className="sw-empty">No transcript metadata yet. Raw prompts and private outputs are not exposed here.</p> : null}</div>
      </section>
    </div>
  </VisualChrome>;
}
export default Shell;

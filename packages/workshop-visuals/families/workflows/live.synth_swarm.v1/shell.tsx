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
const statusOf = (events: LiveEvalEvent[], fallback?: string) => {
  for (let index = events.length - 1; index >= 0; index--) {
    const payload = events[index]?.payload ?? {};
    const found = text(payload.status) ?? text(payload.state) ?? text(payload.swarm_status);
    if (found) return found;
  }
  return fallback ?? "starting";
};
const actorOf = (event: LiveEvalEvent) => text(event.payload.actor_id) ?? text(event.payload.agent_id) ?? text(event.payload.actor) ?? text(event.lane);
const labelOf = (event: LiveEvalEvent) => text(event.payload.type) ?? text(event.payload.kind) ?? text(event.payload.event) ?? event.kind;
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
  const actors = useMemo(() => [...new Set(visible.map(actorOf).filter((value): value is string => Boolean(value)))], [visible]);
  const transcript = visible.filter((event) => event.kind === "transcript" || text(event.payload.message) || text(event.payload.summary));
  const latestUsage = [...visible].reverse().find((event) => number(event.payload.spend_usd) != null || number(event.payload.cost_usd) != null || number(event.payload.tokens) != null);
  const spend = number(latestUsage?.payload.spend_usd) ?? number(latestUsage?.payload.cost_usd);
  const tokens = number(latestUsage?.payload.tokens) ?? number(latestUsage?.payload.total_tokens);

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
          return <div className="sw-actor" key={actor} data-active={latest && !terminal ? "true" : "false"}>
            <span className="sw-orb" aria-hidden="true"/><div><strong>{actor}</strong><p>{latest ? labelOf(latest) : "Waiting for assignment"}</p></div><span>{actorEvents.length}</span>
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
        <div className="sw-transcript">{transcript.slice(-6).reverse().map((event, index) => <article key={`${event.sequence ?? index}`}><strong>{actorOf(event) ?? "Swarm"}</strong><p>{text(event.payload.summary) ?? text(event.payload.message) ?? labelOf(event)}</p></article>)}{transcript.length === 0 ? <p className="sw-empty">No transcript metadata yet. Raw prompts and private outputs are not exposed here.</p> : null}</div>
      </section>
    </div>
  </VisualChrome>;
}
export default Shell;

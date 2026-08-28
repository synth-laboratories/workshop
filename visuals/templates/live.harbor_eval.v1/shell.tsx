/**
 * Harbor eval live viewer (A2 posture): trial → attempt evidence as it
 * streams, verifier truth (reward.txt fails closed; native and wrapped
 * verifiers shown side by side when both report), and a bounded tool/stdout
 * stream. ATIF is a projection of this evidence, never the log itself.
 */

import { useMemo, useState } from "react";
import { Identifier } from "../../chrome/Identifier.tsx";
import { VisualChrome, MetricStrip } from "../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../chrome/useLiveEvalStream.ts";
import { formatMissingNumber } from "../../runtime/liveStream.ts";
import { projectHarborAttempts } from "../../runtime/harborEval.ts";
import { projectLiveEval } from "../../runtime/liveEvalReducer.ts";
import { bindingSlots } from "../../runtime/bind.ts";
import type { LiveEvalEvent, VisualBinding } from "../../runtime/types.ts";

type StreamPayload = { events?: LiveEvalEvent[]; sse_url?: string; replay_ms?: number };

export type ShellProps = {
  title?: string;
  lede?: string;
  stream?: StreamPayload;
  jobs?: StreamPayload;
  data?: StreamPayload;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
  sseUrl?: string;
};

const STREAM_WINDOW = 30;

function asStream(raw: unknown): StreamPayload {
  if (raw && typeof raw === "object") return raw as StreamPayload;
  return {};
}

export function Shell(props: ShellProps) {
  const stream = asStream(props.data ?? props.stream ?? props.jobs);
  const sseUrl =
    props.sseUrl ??
    stream.sse_url ??
    bindingSlots(props.bindings).find((binding) => binding.slot === "stream" && binding.kind === "live_sse")?.source;
  const fixtureEvents = useMemo(
    () => (sseUrl ? undefined : stream.events),
    [sseUrl, stream.events]
  );
  const hasSource = Boolean(sseUrl || stream.events);
  const { events, live, error, ready } = useLiveEvalStream({ sseUrl, fixtureEvents, replayMs: stream.replay_ms });
  const [showFullStream, setShowFullStream] = useState(false);
  const [eventCutoff, setEventCutoff] = useState<number | null>(null);
  const [selectedEventIndex, setSelectedEventIndex] = useState<number | null>(null);
  const liveEdge = Math.max(0, events.length - 1);
  const effectiveCutoff = eventCutoff == null ? liveEdge : Math.min(eventCutoff, liveEdge);
  const visibleEvents = useMemo(
    () => events.slice(0, events.length ? effectiveCutoff + 1 : 0),
    [effectiveCutoff, events]
  );
  const selectedEvent =
    selectedEventIndex != null && selectedEventIndex < visibleEvents.length
      ? visibleEvents[selectedEventIndex]
      : visibleEvents.at(-1);
  const projection = projectLiveEval(visibleEvents);
  const trials = useMemo(() => projectHarborAttempts(visibleEvents), [visibleEvents]);
  const status = [...visibleEvents].reverse().find((event) => event.kind === "status");
  const statusText = String(status?.payload.status ?? "");
  const terminal = ["completed", "finished", "failed", "cancelled"].includes(statusText.toLowerCase());
  const tools = visibleEvents.filter((event) => event.kind === "tools" || event.kind === "stdout" || event.kind === "stderr");
  const visibleTools = showFullStream ? tools : tools.slice(-STREAM_WINDOW);
  const verifiedCount = trials.filter((trial) => trial.phase === "scored").length;

  return (
    <VisualChrome
      kicker="Harbor · Evals"
      live={live && !terminal}
      title={props.title ?? "Harbor trial / verifier"}
      lede={props.lede}
      testId="visual-live-harbor-eval"
      footer="live.harbor_eval.v1 · ATIF is a projection of this evidence, not the log"
    >
      <MetricStrip
        metrics={[
          { label: "Trials", value: trials.length ? `${verifiedCount}/${trials.length} verified` : "—" },
          { label: "Reward", value: formatMissingNumber(projection.reward) },
          { label: "reward.txt", value: projection.has_reward_txt ? "present" : "not yet" },
          {
            label: "Status",
            value: statusText || (ready ? (live ? "live" : "idle") : hasSource ? "connecting" : "awaiting source")
          }
        ]}
      />

      {error ? (
        <p role="alert" style={{ color: "#c2553f" }}>
          {error}
        </p>
      ) : null}

      <section className="sv-section" aria-label="Replay controls" data-testid="harbor-replay-controls">
        <div className="sv-section-head">
          <h3>Event replay</h3>
          <span className="sv-mono">{events.length ? `${effectiveCutoff + 1}/${events.length}` : "0/0"}</span>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <input
            aria-label="Event timeline"
            type="range"
            min={0}
            max={liveEdge}
            value={effectiveCutoff}
            disabled={events.length === 0}
            onChange={(event) => {
              setEventCutoff(Number(event.currentTarget.value));
              setSelectedEventIndex(null);
            }}
            style={{ flex: 1 }}
          />
          <button type="button" className="sv-btn" onClick={() => setEventCutoff(null)} disabled={eventCutoff == null}>
            Live edge
          </button>
        </div>
      </section>

      <section className="sv-section" aria-label="Trials" data-testid="harbor-trials">
        <div className="sv-section-head">
          <h3>Trials</h3>
          <span className="sv-mono">{ready ? "ready" : hasSource ? "connecting" : "awaiting source"}</span>
        </div>
        {trials.length === 0 ? (
          <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: 12 }}>Waiting for trial.planned…</p>
        ) : (
          <div role="list" style={{ display: "grid", gap: 6 }}>
            {trials.map((trial) => (
              <article key={trial.key} role="listitem" style={{ padding: "9px 12px", border: "1px solid var(--sv-border)", borderRadius: 9 }}>
                <div style={{ display: "flex", flexWrap: "wrap", alignItems: "baseline", gap: 8 }}>
                  {trial.trialId ? <Identifier value={trial.trialId} label="trial" max={22} /> : <strong style={{ fontSize: 12 }}>Trial</strong>}
                  {trial.sandbox ? <Identifier value={trial.sandbox} label="sandbox" max={20} copy={false} /> : null}
                  <span
                    className="sv-chip"
                    data-tone={trial.phase === "scored" ? (trial.reward != null && trial.reward > 0 ? "ok" : "warn") : ["failed", "unavailable"].includes(trial.phase) ? "bad" : undefined}
                    style={{ marginLeft: "auto" }}
                  >
                    {trial.phase.replaceAll("_", " ")}
                  </span>
                </div>
                {trial.environmentReleaseId ? (
                  <p className="sv-mono" style={{ margin: "6px 0 0", fontSize: 11, color: "var(--sv-text-muted)" }}>
                    release · {trial.environmentReleaseId}
                    {trial.environmentStatus ? ` · ${trial.environmentStatus}` : ""}
                    {trial.prewarmState ? ` · prewarm ${trial.prewarmState}` : ""}
                  </p>
                ) : null}
                {trial.instruction ? (
                  <p style={{ margin: "6px 0 0", fontSize: 12.5 }}>{trial.instruction}</p>
                ) : null}
                {["scored", "unscored", "failed", "unavailable"].includes(trial.phase) ? (
                  <p className="sv-mono" style={{ margin: "6px 0 0", fontSize: 11, color: "var(--sv-text-muted)" }}>
                    {trial.verifierScript ?? "verifier"} · reward {trial.reward == null ? "missing (fails closed — never defaulted to 0)" : formatMissingNumber(trial.reward)}{trial.reason ? ` · ${trial.reason}` : ""}
                  </p>
                ) : null}
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="sv-section" aria-label="Tool stream" aria-live="polite" data-testid="harbor-tool-stream">
        <div className="sv-section-head">
          <h3>Tool stream</h3>
          <span className="sv-mono">{tools.length}</span>
        </div>
        {!showFullStream && tools.length > STREAM_WINDOW ? (
          <button type="button" className="sv-btn" style={{ marginBottom: 6 }} onClick={() => setShowFullStream(true)}>
            Show {tools.length - STREAM_WINDOW} earlier entries
          </button>
        ) : null}
        <ol style={{ listStyle: "none", margin: 0, padding: 0, maxHeight: 280, overflow: "auto", border: tools.length ? "1px solid var(--sv-border)" : "none", borderRadius: 8 }}>
          {visibleTools.map((event, index) => (
            <li key={`${event.ts}-${index}`} className="sv-mono" style={{ fontSize: 12, padding: "4px 10px", borderBottom: "1px solid var(--sv-border)", overflowWrap: "anywhere" }}>
              <span style={{ color: event.kind === "stderr" ? "#b23830" : "var(--sv-text-faint)", marginRight: 6 }}>{event.kind}</span>
              {String(event.payload.name ?? event.payload.text ?? "")}
            </li>
          ))}
          {tools.length === 0 ? (
            <li style={{ color: "var(--sv-text-faint)" }}>Waiting for tools…</li>
          ) : null}
        </ol>
      </section>

      <section className="sv-section" aria-label="Full trace" data-testid="harbor-full-trace">
        <div className="sv-section-head">
          <h3>Full trace</h3>
          <span className="sv-mono">{visibleEvents.length} events</span>
        </div>
        {visibleEvents.length ? (
          <div style={{ display: "grid", gridTemplateColumns: "minmax(150px, 0.8fr) minmax(220px, 1.2fr)", gap: 8 }}>
            <ol style={{ listStyle: "none", margin: 0, padding: 0, maxHeight: 260, overflow: "auto", border: "1px solid var(--sv-border)", borderRadius: 8 }}>
              {visibleEvents.slice(-100).map((event, offset) => {
                const index = Math.max(0, visibleEvents.length - 100) + offset;
                return (
                  <li key={`${event.ts ?? event.occurred_at ?? "event"}-${index}`}>
                    <button
                      type="button"
                      className="sv-btn"
                      aria-pressed={selectedEvent === event}
                      onClick={() => setSelectedEventIndex(index)}
                      style={{ width: "100%", border: 0, borderRadius: 0, textAlign: "left" }}
                    >
                      <span className="sv-mono">{event.sequence ?? index + 1}</span> · {event.kind}
                    </button>
                  </li>
                );
              })}
            </ol>
            <pre aria-label="Selected event payload" style={{ margin: 0, padding: 10, maxHeight: 260, overflow: "auto", border: "1px solid var(--sv-border)", borderRadius: 8, fontSize: 11, whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
              {JSON.stringify(selectedEvent, null, 2)}
            </pre>
          </div>
        ) : (
          <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: 12 }}>Waiting for the first trace event…</p>
        )}
      </section>
    </VisualChrome>
  );
}

export default Shell;

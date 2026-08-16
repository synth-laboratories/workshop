import { useMemo } from "react";
import { VisualChrome, MetricStrip } from "../../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../../chrome/useLiveEvalStream.ts";
import type { LiveTemplateProps } from "../../../runtime/replayClient.ts";
import type { LiveEvalEvent, VisualBinding } from "../../../runtime/types.ts";

type StreamPayload = {
  run_id?: string;
  events?: LiveEvalEvent[];
  sse_url?: string;
};

export type ShellProps = LiveTemplateProps & {
  title?: string;
  lede?: string;
  stream?: StreamPayload;
  data?: StreamPayload;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
};

function asStream(raw: unknown): StreamPayload {
  if (raw && typeof raw === "object") return raw as StreamPayload;
  return {};
}

export function Shell(props: ShellProps) {
  const stream = asStream(props.data ?? props.stream);
  const declaredStreamCount = props.replay?.streams.length ?? 0;

  const fixtureEvents = useMemo(
    () => (declaredStreamCount > 0 ? undefined : stream.events),
    [declaredStreamCount, stream.events]
  );
  const hasSource = declaredStreamCount > 0 || Boolean(stream.events);

  const { events, state, error } = useLiveEvalStream({
    replay: props.replay,
    fixtureEvents,
    visualId: props.visualId,
    revision: props.revision
  });
  const live = state === "live";

  const finished = [...events].reverse().find((e) => e.kind === "run_finished");
  const metrics = finished?.payload ?? {};

  return (
    <VisualChrome
      kicker="Live eval"
      live={live}
      title={props.title ?? `Run ${stream.run_id ?? events[0]?.run_id ?? "—"}`}
      lede={props.lede}
      testId="visual-live-eval-stream"
      footer="live.eval_stream.v1"
    >
      <MetricStrip
        metrics={[
          { label: "Events", value: String(events.length) },
          {
            label: "Mean reward",
            value:
              typeof metrics.mean_reward === "number"
                ? metrics.mean_reward.toFixed(2)
                : "—"
          },
          {
            label: "Status",
            value: live ? "streaming" : finished ? String(metrics.status ?? "done") : hasSource ? "idle" : "awaiting source"
          }
        ]}
      />

      {error ? (
        <p role="alert" style={{ color: "#c2553f" }}>
          {error}
        </p>
      ) : null}

      <section className="sv-section" aria-label="Live event log" aria-live="polite">
        <div className="sv-section-head">
          <h3>Event log</h3>
          <span className="sv-mono">{live ? "LIVE" : "paused"}</span>
        </div>
        <ol
          style={{
            listStyle: "none",
            margin: 0,
            padding: 0,
            maxHeight: 320,
            overflow: "auto",
            border: "1px solid var(--sv-border)",
            borderRadius: 8
          }}
        >
          {events.map((e, i) => (
            <li
              key={`${e.ts}-${i}`}
              style={{
                padding: "8px 10px",
                borderBottom: "1px solid var(--sv-border)",
                fontSize: 12
              }}
            >
              <span className="sv-mono" style={{ color: "var(--sv-accent)", marginRight: 8 }}>
                {e.kind}
              </span>
              <span className="sv-mono" style={{ color: "var(--sv-text-faint)", marginRight: 8 }}>
                {e.ts.slice(11, 19)}
              </span>
              <span className="sv-mono">{JSON.stringify(e.payload)}</span>
            </li>
          ))}
          {events.length === 0 ? (
            <li style={{ padding: 12, color: "var(--sv-text-faint)" }}>Waiting for events…</li>
          ) : null}
        </ol>
      </section>
    </VisualChrome>
  );
}

export default Shell;

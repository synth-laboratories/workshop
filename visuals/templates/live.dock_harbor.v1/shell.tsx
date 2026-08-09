import { useMemo } from "react";
import { VisualChrome, MetricStrip } from "../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../chrome/useLiveEvalStream.ts";
import type { LiveEvalEvent, VisualBinding } from "../../runtime/types.ts";
import liveFixture from "../../fixtures/live_eval_events.json";

type StreamPayload = { events?: LiveEvalEvent[]; sse_url?: string };

export type ShellProps = {
  title?: string;
  lede?: string;
  jobs?: StreamPayload;
  data?: StreamPayload;
  bindings?: VisualBinding[];
  sseUrl?: string;
};

function asStream(raw: unknown): StreamPayload {
  if (raw && typeof raw === "object") return raw as StreamPayload;
  return liveFixture as StreamPayload;
}

export function Shell(props: ShellProps) {
  const stream = asStream(props.data ?? props.jobs ?? liveFixture);
  const sseUrl =
    props.sseUrl ??
    stream.sse_url ??
    props.bindings?.find((b) => b.slot === "jobs" && b.kind === "live_sse")?.source;

  const fixtureEvents = useMemo(
    () => (sseUrl ? undefined : stream.events ?? (liveFixture as { events: LiveEvalEvent[] }).events),
    [sseUrl, stream.events]
  );

  const { events, live, error } = useLiveEvalStream({ sseUrl, fixtureEvents });

  const jobEvents = events.filter((e) => e.kind === "job_status");
  const rollouts = events.filter((e) => e.kind === "rollout");
  const latestJob = jobEvents[jobEvents.length - 1];

  return (
    <VisualChrome
      kicker="Dock · Harbor"
      live={live}
      title={props.title ?? "Container job stream"}
      lede={props.lede}
      testId="visual-live-dock-harbor"
      footer="live.dock_harbor.v1"
    >
      <MetricStrip
        metrics={[
          {
            label: "Job",
            value: String(latestJob?.payload.job_id ?? "—")
          },
          {
            label: "Backend",
            value: String(latestJob?.payload.backend ?? "—")
          },
          {
            label: "Status",
            value: String(latestJob?.payload.status ?? (live ? "connecting" : "idle"))
          },
          {
            label: "Rollouts",
            value: String(rollouts.length)
          }
        ]}
      />

      {error ? (
        <p role="alert" style={{ color: "#c2553f" }}>
          {error}
        </p>
      ) : null}

      <section className="sv-section">
        <div className="sv-section-head">
          <h3>Job progress</h3>
          <span className="sv-mono">
            {typeof latestJob?.payload.progress === "number"
              ? `${Math.round(Number(latestJob.payload.progress) * 100)}%`
              : "—"}
          </span>
        </div>
        <div
          role="progressbar"
          aria-label="Job progress"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={
            typeof latestJob?.payload.progress === "number"
              ? Math.round(Number(latestJob.payload.progress) * 100)
              : 0
          }
          style={{
            height: 10,
            background: "#eef0f3",
            borderRadius: 5,
            overflow: "hidden"
          }}
        >
          <div
            style={{
              width: `${
                typeof latestJob?.payload.progress === "number"
                  ? Math.round(Number(latestJob.payload.progress) * 100)
                  : 0
              }%`,
              height: "100%",
              background: "var(--sv-accent-hot)",
              transition: "width 200ms ease"
            }}
          />
        </div>
      </section>

      <section className="sv-section" aria-label="Rollout stream" aria-live="polite">
        <div className="sv-section-head">
          <h3>Rollout stream</h3>
        </div>
        <table className="sv-table">
          <thead>
            <tr>
              <th scope="col">Rollout</th>
              <th scope="col">Row</th>
              <th scope="col">Reward</th>
              <th scope="col">ACH</th>
              <th scope="col">Status</th>
            </tr>
          </thead>
          <tbody>
            {rollouts.map((e, i) => (
              <tr key={`${e.ts}-${i}`}>
                <td className="sv-mono">{String(e.payload.rollout_id ?? "—")}</td>
                <td className="sv-mono">{String(e.payload.row ?? "—")}</td>
                <td className="sv-mono">{String(e.payload.reward ?? "—")}</td>
                <td className="sv-mono">{String(e.payload.achievements ?? "—")}</td>
                <td>{String(e.payload.status ?? "—")}</td>
              </tr>
            ))}
            {rollouts.length === 0 ? (
              <tr>
                <td colSpan={5} style={{ color: "var(--sv-text-faint)" }}>
                  Waiting for rollouts…
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </section>
    </VisualChrome>
  );
}

export default Shell;

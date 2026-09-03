import { useMemo } from "react";
import { VisualChrome, MetricStrip } from "../../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../../chrome/useLiveEvalStream.ts";
import type { LiveTemplateProps } from "../../../runtime/replayClient.ts";
import type { LiveEvalEvent, VisualBinding } from "../../../runtime/types.ts";

type StreamPayload = { events?: LiveEvalEvent[]; sse_url?: string };

export type ShellProps = LiveTemplateProps & {
  title?: string;
  lede?: string;
  acceptance?: StreamPayload;
  data?: StreamPayload;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
};

function asStream(raw: unknown): StreamPayload {
  if (raw && typeof raw === "object") return raw as StreamPayload;
  return {};
}

const DECISION_COLOR: Record<string, string> = {
  pass: "#6f9a4d",
  fail: "#c2553f",
  hold: "#c99b3f",
  pending: "#5c6573"
};

export function Shell(props: ShellProps) {
  const stream = asStream(props.data ?? props.acceptance);
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
  const cells = events.filter((e) => e.kind === "acceptance");
  const passes = cells.filter((e) => e.payload.decision === "pass").length;
  const fails = cells.filter((e) => e.payload.decision === "fail").length;

  return (
    <VisualChrome
      kicker="Intern · acceptance"
      live={live}
      title={props.title ?? "Acceptance cell stream"}
      lede={props.lede ?? "Sync mailbox and async background acceptance gates."}
      testId="visual-live-intern-acceptance"
      footer="live.intern_acceptance.v1"
    >
      <MetricStrip
        metrics={[
          { label: "Cells", value: String(cells.length) },
          { label: "Pass", value: String(passes) },
          { label: "Fail", value: String(fails) },
          { label: "Mode", value: live ? "live" : hasSource ? "idle" : "awaiting source" }
        ]}
      />

      {error ? (
        <p role="alert" style={{ color: "#c2553f" }}>
          {error}
        </p>
      ) : null}

      <section className="sv-section" aria-label="Acceptance cells" aria-live="polite">
        <div className="sv-section-head">
          <h3>Cells</h3>
          <span className="sv-mono">{live ? "LIVE" : "caught up"}</span>
        </div>
        <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
          {cells.map((e, i) => {
            const decision = String(e.payload.decision ?? "pending");
            return (
              <li
                key={`${e.ts}-${i}`}
                style={{
                  display: "grid",
                  gridTemplateColumns: "100px 1fr",
                  gap: 10,
                  padding: "10px 12px",
                  marginBottom: 6,
                  border: "1px solid var(--sv-border)",
                  borderRadius: 8,
                  borderLeft: `3px solid ${DECISION_COLOR[decision] ?? "#5c6573"}`
                }}
              >
                <div>
                  <div
                    className="sv-mono"
                    style={{
                      color: DECISION_COLOR[decision] ?? "inherit",
                      fontWeight: 650,
                      textTransform: "uppercase"
                    }}
                  >
                    {decision}
                  </div>
                  <div className="sv-mono" style={{ color: "var(--sv-text-faint)" }}>
                    {e.ts.slice(11, 19)}
                  </div>
                </div>
                <div>
                  <div style={{ fontWeight: 600 }}>{String(e.payload.cell ?? "cell")}</div>
                  <div style={{ color: "var(--sv-text-muted)", fontSize: 12 }}>
                    {String(e.payload.note ?? "")}
                  </div>
                </div>
              </li>
            );
          })}
          {cells.length === 0 ? (
            <li style={{ color: "var(--sv-text-faint)", padding: 8 }}>
              Waiting for acceptance decisions…
            </li>
          ) : null}
        </ul>
      </section>
    </VisualChrome>
  );
}

export default Shell;

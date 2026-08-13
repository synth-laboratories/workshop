/**
 * dig.bench live viewer (A8 posture): text evidence only — observation, legal
 * actions, lives/level/steps, and full action/observation history per lane.
 * Two harnesses (e.g. basic ReAct vs agentic MCP) appear as two lanes on the
 * same game. `/reward` comes from env status (completed / game_over); an
 * incomplete run stays null — never a fabricated zero. No fake frames, ever.
 */

import { useMemo, useState } from "react";
import { Identifier } from "../../chrome/Identifier.tsx";
import { VisualChrome, MetricStrip } from "../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../chrome/useLiveEvalStream.ts";
import { formatMissingNumber } from "../../runtime/liveStream.ts";
import { projectLiveEval } from "../../runtime/liveEvalReducer.ts";
import type { LiveEvalEvent, VisualBinding } from "../../runtime/types.ts";

type StreamScope = { game_id?: string; rollout_ids?: string[]; selection?: { initial_rollout_id?: string } };
type StreamPayload = { events?: LiveEvalEvent[]; sse_url?: string; scope?: StreamScope };

export type ShellProps = {
  title?: string;
  lede?: string;
  stream?: StreamPayload;
  data?: StreamPayload;
  bindings?: VisualBinding[];
  sseUrl?: string;
};

const HISTORY_WINDOW = 40;

function asStream(raw: unknown): StreamPayload {
  if (raw && typeof raw === "object") return raw as StreamPayload;
  return {};
}

function laneOf(event: LiveEvalEvent): string {
  return event.lane || event.run_id || "session";
}

function lastOf(events: LiveEvalEvent[], kind: string): LiveEvalEvent | undefined {
  return [...events].reverse().find((event) => event.kind === kind);
}

type HistoryEntry = {
  key: string;
  kind: "action" | "observation" | "status";
  text: string;
  detail?: string;
};

function historyOf(events: LiveEvalEvent[]): HistoryEntry[] {
  const entries: HistoryEntry[] = [];
  for (const [index, event] of events.entries()) {
    const payload = (event.payload ?? {}) as Record<string, unknown>;
    if (event.kind === "action") {
      entries.push({
        key: `${index}-action`,
        kind: "action",
        text: String(payload.action ?? payload.name ?? "action"),
        detail: typeof payload.harness === "string" ? payload.harness : undefined
      });
    } else if (event.kind === "observation" && typeof payload.text === "string") {
      entries.push({ key: `${index}-obs`, kind: "observation", text: payload.text });
    } else if (event.kind === "status" && typeof payload.status === "string") {
      entries.push({ key: `${index}-status`, kind: "status", text: payload.status });
    }
  }
  return entries;
}

export function Shell(props: ShellProps) {
  const stream = asStream(props.data ?? props.stream);
  const sseUrl =
    props.sseUrl ??
    stream.sse_url ??
    props.bindings?.find((b) => b.slot === "stream" && b.kind === "live_sse")?.source;
  const scope = stream.scope;
  const fixtureEvents = useMemo(
    () => (sseUrl ? undefined : stream.events),
    [sseUrl, stream.events]
  );
  const hasSource = Boolean(sseUrl || stream.events);
  const { events, live, error, ready } = useLiveEvalStream({ sseUrl, fixtureEvents });
  const scopedEvents = useMemo(() => {
    if (!scope?.rollout_ids?.length) return events;
    const allowed = new Set(scope.rollout_ids);
    return events.filter((event) => allowed.has(laneOf(event)));
  }, [events, scope?.rollout_ids]);
  const lanes = useMemo(() => [...new Set(scopedEvents.map(laneOf))], [scopedEvents]);
  const [chosenLane, setChosenLane] = useState<string | null>(scope?.selection?.initial_rollout_id ?? null);
  const [showFullHistory, setShowFullHistory] = useState(false);
  const selectedLane = chosenLane && lanes.includes(chosenLane) ? chosenLane : lanes[0];
  const laneEvents = useMemo(
    () => scopedEvents.filter((event) => laneOf(event) === selectedLane),
    [scopedEvents, selectedLane]
  );
  const projection = projectLiveEval(laneEvents);
  const observation = lastOf(laneEvents, "observation");
  const legal = lastOf(laneEvents, "legal_actions");
  const stats = lastOf(laneEvents, "stats");
  const status = lastOf(laneEvents, "status");
  const statusText = String(status?.payload.status ?? "");
  const terminal = ["completed", "game_over", "failed", "cancelled"].includes(statusText.toLowerCase());
  const actions = Array.isArray(legal?.payload.actions) ? (legal?.payload.actions as string[]) : [];
  const obsText = typeof observation?.payload.text === "string" ? observation.payload.text : null;
  const history = historyOf(laneEvents);
  const visibleHistory = showFullHistory ? history : history.slice(-HISTORY_WINDOW);
  const harness = (() => {
    const opened = lastOf(laneEvents, "policy.session.opened") ?? lastOf(laneEvents, "session");
    const payload = (opened?.payload ?? {}) as Record<string, unknown>;
    return typeof payload.harness === "string" ? payload.harness : undefined;
  })();

  return (
    <VisualChrome
      kicker="dig.bench"
      live={live && !terminal}
      title={props.title ?? "dig.bench"}
      lede={props.lede}
      testId="visual-live-digbench"
      footer="live.digbench.v1"
    >
      <MetricStrip
        metrics={[
          { label: "Lives", value: formatMissingNumber(stats?.payload.lives, 0) },
          { label: "Level", value: formatMissingNumber(stats?.payload.level, 0) },
          { label: "Steps left", value: formatMissingNumber(stats?.payload.steps_remaining, 0) },
          {
            label: "/reward",
            value: terminal ? formatMissingNumber(projection.reward) : projection.reward != null ? formatMissingNumber(projection.reward) : "pending"
          },
          {
            label: "Status",
            value: statusText || (ready ? (terminal ? "finished" : "in play") : hasSource ? "connecting" : "awaiting source")
          }
        ]}
      />
      {scope?.game_id ? (
        <p style={{ margin: "6px 0 0" }}>
          <Identifier value={scope.game_id} label="game" max={26} />
        </p>
      ) : null}
      {error ? <p role="alert" style={{ color: "#c2553f" }}>{error}</p> : null}

      {lanes.length > 1 ? (
        <nav aria-label="Harness lanes" style={{ display: "flex", flexWrap: "wrap", gap: 6, marginTop: 10 }}>
          {lanes.map((lane) => (
            <button
              key={lane}
              type="button"
              className="sv-btn"
              aria-pressed={lane === selectedLane}
              aria-label={`Select session ${lane}`}
              onClick={() => setChosenLane(lane)}
            >
              <Identifier value={lane} max={22} copy={false} />
            </button>
          ))}
        </nav>
      ) : null}

      <section className="sv-section" aria-label="Observation" data-testid="digbench-observation">
        <div className="sv-section-head">
          <h3>Observation</h3>
          <span className="sv-mono">{harness ? `harness ${harness}` : "text evidence only"}</span>
        </div>
        {obsText ? (
          <pre style={{ margin: 0, padding: "10px 12px", border: "1px solid var(--sv-border)", borderRadius: 8, background: "var(--sv-surface-muted)", whiteSpace: "pre-wrap", overflowWrap: "anywhere", font: "12px/1.5 var(--sv-mono)" }}>
            {obsText}
          </pre>
        ) : (
          <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: 12 }}>
            No observation has been emitted yet. dig.bench is a hosted text game — this view never fabricates frames.
          </p>
        )}
      </section>

      <section className="sv-section" aria-label="Legal actions" data-testid="digbench-legal-actions">
        <div className="sv-section-head">
          <h3>Legal actions</h3>
          <span className="sv-mono">{actions.length}</span>
        </div>
        {actions.length ? (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
            {actions.map((action) => (
              <span key={action} className="sv-mono" style={{ padding: "3px 8px", border: "1px solid var(--sv-border)", borderRadius: 6, background: "var(--sv-surface-muted)", fontSize: 11 }}>
                {action}
              </span>
            ))}
          </div>
        ) : (
          <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: 12 }}>Not yet advertised by the environment.</p>
        )}
      </section>

      <section className="sv-section" aria-label="Session history" data-testid="digbench-history">
        <div className="sv-section-head">
          <h3>History</h3>
          <span className="sv-mono">{history.length} entries</span>
        </div>
        {history.length === 0 ? (
          <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: 12 }}>Actions and observations appear here in order.</p>
        ) : (
          <>
            {!showFullHistory && history.length > HISTORY_WINDOW ? (
              <button type="button" className="sv-btn" style={{ marginBottom: 6 }} onClick={() => setShowFullHistory(true)}>
                Show {history.length - HISTORY_WINDOW} earlier entries
              </button>
            ) : null}
            <ol style={{ listStyle: "none", margin: 0, padding: 0, maxHeight: 320, overflow: "auto", border: "1px solid var(--sv-border)", borderRadius: 8 }}>
              {visibleHistory.map((entry) => (
                <li key={entry.key} style={{ display: "flex", gap: 8, padding: "6px 10px", borderBottom: "1px solid var(--sv-border)", fontSize: 12 }}>
                  <span className="sv-mono" style={{ flexShrink: 0, width: 84, color: entry.kind === "action" ? "var(--sv-accent)" : "var(--sv-text-faint)" }}>
                    {entry.kind}
                  </span>
                  <span style={{ overflowWrap: "anywhere", whiteSpace: "pre-wrap" }}>
                    {entry.text}
                    {entry.detail ? <em style={{ marginLeft: 6, color: "var(--sv-text-faint)", fontStyle: "normal", fontSize: 11 }}>{entry.detail}</em> : null}
                  </span>
                </li>
              ))}
            </ol>
          </>
        )}
      </section>
    </VisualChrome>
  );
}

export default Shell;

import { BenchmarkStatus } from "../../../chrome/BenchmarkStatus.tsx";
import { benchmarkSnapshotRows, latestBenchmarkObservations } from "../../../runtime/benchmarkObservation.ts";
/**
 * Harbor eval live viewer (A2 posture): trial → attempt evidence as it
 * streams, verifier truth (reward.txt fails closed; native and wrapped
 * verifiers shown side by side when both report), and a bounded tool/stdout
 * stream. ATIF is a projection of this evidence, never the log itself.
 */

import { useVisualState } from "@synth/visuals-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Identifier } from "../../../chrome/Identifier.tsx";
import { VisualChrome, MetricStrip } from "../../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../../chrome/useLiveEvalStream.ts";
import { formatMissingNumber } from "../../../runtime/liveStream.ts";
import { foldHarborTrials as foldTrials, harborSkillProgress } from "../../../runtime/harborTrials.ts";
import { projectLiveEval } from "../../../runtime/liveEvalReducer.ts";
import { harborEvalSnapshot, type HarborEvalSnapshot } from "../../../runtime/harborEvalSnapshot.ts";
import type { LiveTemplateProps } from "../../../runtime/replayClient.ts";
import type { LiveEvalEvent, VisualBinding } from "../../../runtime/types.ts";
import type {
  LiveWorkspaceClient,
  LiveWorkspaceEntry,
  LiveWorkspaceRead,
  LiveWorkspaceStatus
} from "../../../runtime/workspaceClient.ts";

type StreamPayload = {
  events?: LiveEvalEvent[];
  sse_url?: string;
  replay_ms?: number;
  poll_url?: string;
  transports?: { poll?: { url?: string }; sse?: { url?: string } };
};

export type ShellProps = LiveTemplateProps & {
  title?: string;
  lede?: string;
  stream?: StreamPayload;
  jobs?: StreamPayload;
  data?: StreamPayload;
  /**
   * The persisted `synth.experiment.overview.v1` projection for this run.
   * Present on every optimizer-minted Harbor visual and the only thing that
   * still speaks after the producer's stream has closed.
   */
  experiment?: unknown;
  workspace?: LiveWorkspaceClient;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
};

const STREAM_WINDOW = 30;

function asStream(raw: unknown): StreamPayload {
  if (raw && typeof raw === "object") return raw as StreamPayload;
  return {};
}

function parentPath(path: string): string {
  const parts = path.split("/").filter(Boolean);
  parts.pop();
  return parts.join("/");
}

function workspaceErrorMessage(reason: unknown): string {
  if (reason instanceof Error && reason.message) return reason.message;
  if (reason && typeof reason === "object") {
    const value = reason as Record<string, unknown>;
    for (const key of ["message", "error", "detail", "reason"]) {
      if (typeof value[key] === "string" && value[key]) return value[key];
    }
    try {
      return JSON.stringify(reason);
    } catch {
      return "The live workspace request failed.";
    }
  }
  return String(reason || "The live workspace request failed.");
}

function WorkspacePanel({ client }: { client: LiveWorkspaceClient }) {
  const [status, setStatus] = useState<LiveWorkspaceStatus | null>(null);
  const [path, setPath] = useState("");
  const [entries, setEntries] = useState<LiveWorkspaceEntry[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [preview, setPreview] = useState<LiveWorkspaceRead | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [changed, setChanged] = useState<Set<string>>(new Set());
  const signatures = useRef(new Map<string, string>());

  useEffect(() => {
    let disposed = false;
    let running = false;
    const refresh = async () => {
      if (running) return;
      running = true;
      try {
        const nextStatus = await client.status();
        if (disposed) return;
        setStatus(nextStatus);
        if (nextStatus.state !== "observing") {
          setError(nextStatus.message ?? null);
          setEntries([]);
          setSelected(null);
          setPreview(null);
          signatures.current = new Map();
          return;
        }
        const listing = await client.list(path);
        if (disposed) return;
        const nextSignatures = new Map<string, string>();
        const nextChanged = new Set<string>();
        for (const entry of listing.entries) {
          const signature = `${entry.kind}:${entry.size}:${entry.modifiedAt ?? ""}`;
          nextSignatures.set(entry.path, signature);
          const previous = signatures.current.get(entry.path);
          if (previous && previous !== signature) nextChanged.add(entry.path);
          if (!previous && signatures.current.size > 0) nextChanged.add(entry.path);
        }
        signatures.current = nextSignatures;
        setChanged(nextChanged);
        setEntries(listing.entries);
        setError(null);
        const selectedStillExists = selected
          ? listing.entries.some((entry) => entry.path === selected && entry.kind !== "directory")
          : false;
        if (selected && !selectedStillExists) {
          setSelected(null);
          setPreview(null);
        } else if (selected) {
          const value = await client.read(selected);
          if (!disposed) setPreview(value);
        }
      } catch (reason) {
        if (!disposed) setError(workspaceErrorMessage(reason));
      } finally {
        running = false;
      }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 2000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [client, path, selected]);

  const open = async (entry: LiveWorkspaceEntry) => {
    if (entry.kind === "directory") {
      setPath(entry.path);
      setSelected(null);
      setPreview(null);
      signatures.current = new Map();
      return;
    }
    setSelected(entry.path);
    setPreview(null);
    try {
      setPreview(await client.read(entry.path));
      setError(null);
    } catch (reason) {
      setError(workspaceErrorMessage(reason));
    }
  };

  return (
    <section className="sv-section" aria-label="Live workspace" data-testid="harbor-live-workspace">
      <div className="sv-section-head">
        <h3>Workspace</h3>
        <span className="sv-chip" data-tone={status?.state === "observing" ? "ok" : status?.state === "unavailable" ? "bad" : undefined}>
          {status?.state ?? "connecting"}
        </span>
      </div>
      <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 8 }}>
        <button
          type="button"
          className="sv-btn"
          disabled={!path}
          onClick={() => {
            setPath(parentPath(path));
            setSelected(null);
            setPreview(null);
            signatures.current = new Map();
          }}
        >
          Up
        </button>
        <span className="sv-mono" style={{ overflowWrap: "anywhere" }}>
          {status?.workspace ?? "/workspace"}{path ? `/${path}` : ""}
        </span>
        <span className="sv-mono" style={{ marginLeft: "auto", color: "var(--sv-text-faint)" }}>
          read-only · refresh 2s
        </span>
      </div>
      {error ? <p role="alert" style={{ color: "#c2553f", margin: "6px 0" }}>{error}</p> : null}
      <div style={{ display: "grid", gridTemplateColumns: "minmax(150px, .75fr) minmax(220px, 1.25fr)", gap: 8 }}>
        <ol style={{ listStyle: "none", margin: 0, padding: 0, maxHeight: 320, overflow: "auto", border: "1px solid var(--sv-border)", borderRadius: 8 }}>
          {entries.map((entry) => (
            <li key={entry.path}>
              <button
                type="button"
                className="sv-btn"
                aria-pressed={selected === entry.path}
                onClick={() => void open(entry)}
                style={{ width: "100%", border: 0, borderRadius: 0, textAlign: "left", display: "flex", gap: 7 }}
              >
                <span aria-hidden="true">{entry.kind === "directory" ? "▸" : entry.kind === "symlink" ? "↗" : "·"}</span>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{entry.name}</span>
                {changed.has(entry.path) ? <span className="sv-chip" data-tone="ok" style={{ marginLeft: "auto" }}>changed</span> : null}
              </button>
            </li>
          ))}
          {status?.state === "observing" && entries.length === 0 ? (
            <li style={{ padding: 10, color: "var(--sv-text-faint)" }}>This directory is empty.</li>
          ) : null}
        </ol>
        <div style={{ minHeight: 180, maxHeight: 320, overflow: "auto", border: "1px solid var(--sv-border)", borderRadius: 8 }}>
          {!selected ? (
            <p style={{ padding: 10, color: "var(--sv-text-faint)" }}>Select a file to preview it live.</p>
          ) : error ? (
            <p style={{ padding: 10, color: "var(--sv-text-faint)" }}>Preview unavailable.</p>
          ) : !preview ? (
            <p role="status" style={{ padding: 10 }}>Loading {selected}…</p>
          ) : preview.binary ? (
            <p style={{ padding: 10 }}>Binary file · {preview.byteSize.toLocaleString()} bytes</p>
          ) : preview.truncated ? (
            <p style={{ padding: 10 }}>Preview limited · {preview.byteSize.toLocaleString()} bytes</p>
          ) : (
            <pre aria-label={`Live preview of ${selected}`} style={{ margin: 0, padding: 10, fontSize: 11, whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
              {preview.content ?? ""}
            </pre>
          )}
        </div>
      </div>
    </section>
  );
}

/** A settled snapshot renders instead of the live fold, never beside it. */
function SnapshotTrials({ snapshot }: { snapshot: HarborEvalSnapshot }) {
  const [selected, setSelected] = useVisualState<string | null>("harbor.selectedTrial", null);
  const trial = snapshot.trials.find((row) => row.id === selected) ?? snapshot.trials[0];
  return (
    <section className="sv-section" aria-label="Trials" data-testid="harbor-snapshot-trials">
      <div className="sv-section-head">
        <h3>Trials</h3>
        <span className="sv-mono">restored from the settled projection</span>
      </div>
      <div role="list" style={{ display: "grid", gap: 6 }}>
        {snapshot.trials.map((row) => (
          <article
            key={row.id}
            role="listitem"
            style={{
              padding: "9px 12px",
              border: `1px solid ${row.id === trial?.id ? "var(--sv-accent)" : "var(--sv-border)"}`,
              borderRadius: 9
            }}
          >
            <button
              type="button"
              className="sv-btn"
              aria-pressed={row.id === trial?.id}
              onClick={() => setSelected(row.id)}
              style={{ width: "100%", border: 0, borderRadius: 0, textAlign: "left", padding: 0, background: "none" }}
            >
              <div style={{ display: "flex", flexWrap: "wrap", alignItems: "baseline", gap: 8 }}>
                {/* The task, not the seed: `seed 0` names nothing a reader can act on. */}
                <strong style={{ fontSize: 12.5 }}>{row.label}</strong>
                {row.seed == null ? null : (
                  <span className="sv-mono" style={{ color: "var(--sv-text-faint)", fontSize: 11 }}>
                    seed {row.seed}
                  </span>
                )}
                <span
                  className="sv-chip"
                  data-tone={row.status === "failed" ? "bad" : row.reward != null && row.reward > 0 ? "ok" : "warn"}
                  style={{ marginLeft: "auto" }}
                >
                  {row.status}
                </span>
              </div>
            </button>
            {row.taskInstanceId ? (
              <p className="sv-mono" style={{ margin: "4px 0 0", fontSize: 11, color: "var(--sv-text-muted)" }}>
                {row.taskInstanceId}
              </p>
            ) : null}
            {row.id === trial?.id && row.stopReason ? (
              <p style={{ margin: "6px 0 0", fontSize: 12 }} data-testid="harbor-snapshot-stop-reason">
                {row.stopReason}
              </p>
            ) : null}
            {row.id === trial?.id ? (
              <p className="sv-mono" style={{ margin: "6px 0 0", fontSize: 11, color: "var(--sv-text-muted)" }}>
                reward {row.reward == null ? "not scored" : formatMissingNumber(row.reward)} · trace{" "}
                {row.traceId ?? "not retained"}
                {row.workbenchVisualId ? (
                  <>
                    {" · "}
                    <a href={`synth://visual/${row.workbenchVisualId}`} data-testid="harbor-snapshot-workbench-link">
                      open the trace workstation
                    </a>
                  </>
                ) : null}
              </p>
            ) : null}
          </article>
        ))}
      </div>
    </section>
  );
}

export function Shell(props: ShellProps) {
  const stream = asStream(props.stream ?? props.jobs ?? props.data);
  const declaredStreamCount = props.replay?.streams.length ?? 0;
  const fixtureEvents = useMemo(
    () => (declaredStreamCount > 0 ? undefined : stream.events),
    [declaredStreamCount, stream.events]
  );
  const hasSource = declaredStreamCount > 0 || Boolean(stream.events);
  const { events, state, error, ready, hostEvidence } = useLiveEvalStream({
    replay: props.replay,
    fixtureEvents,
    replayMs: stream.replay_ms,
    visualId: props.visualId,
    revision: props.revision
  });
  const live = state === "live";
  const [showFullStream, setShowFullStream] = useVisualState("harbor.showFullStream", false);
  const [eventCutoff, setEventCutoff] = useVisualState<number | null>("harbor.eventCutoff", null, {"type":"number"});
  const [selectedEventIndex, setSelectedEventIndex] = useVisualState<number | null>("harbor.selectedEventIndex", null, {"type":"number"});
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
  // The host projection covers its whole observed prefix. A historical
  // selection still needs the specialized view of that selected event cut.
  const projection = eventCutoff == null && hostEvidence?.projection
    ? hostEvidence.projection : projectLiveEval(visibleEvents);
  const trials = useMemo(() => foldTrials(visibleEvents), [visibleEvents]);
  const skills = useMemo(() => harborSkillProgress(visibleEvents), [visibleEvents]);
  const status = [...visibleEvents].reverse().find((event) => event.kind === "status");
  const statusText = String(status?.payload.status ?? "");
  const snapshot = useMemo(
    () => harborEvalSnapshot(props.experiment ?? (props.data as { experiment?: unknown } | undefined)?.experiment),
    [props.experiment, props.data]
  );
  const benchmarkRows = latestBenchmarkObservations([
    ...benchmarkSnapshotRows(props.experiment ?? props.data), ...projectLiveEval(visibleEvents).benchmarkObservations
  ]);
  const settled = snapshot?.lifecycle === "terminal" ||
    (benchmarkRows.length > 0 && benchmarkRows.every((row) => row.terminal));
  const terminal =
    settled || ["completed", "finished", "failed", "cancelled"].includes(statusText.toLowerCase());
  // A reopened terminal visual has no stream to rejoin: the producer sealed
  // and closed it. Restoring from the persisted snapshot is the only honest
  // surface, and it must never claim to be `connecting`.
  const restored = settled && events.length === 0;
  const statusLabel = restored
    ? snapshot?.status ?? "terminal"
    : statusText || (ready ? (live ? "live" : "idle") : hasSource ? "connecting" : "awaiting source");
  const tools = visibleEvents.filter((event) => ["tools", "stdout", "stderr", "agent.action", "agent.message"].includes(event.kind));
  const visibleTools = showFullStream ? tools : tools.slice(-STREAM_WINDOW);
  const verifiedCount = trials.filter((trial) => trial.status === "verified").length;

  return (
    <VisualChrome
      kicker="Harbor · Evals"
      live={live && !terminal}
      title={props.title ?? "Harbor trial / verifier"}
      lede={props.lede}
      testId="visual-live-harbor-eval"
      footer="live.harbor_eval.v1 · ATIF is a projection of this evidence, not the log"
    >
      <BenchmarkStatus rows={benchmarkRows} />
      <MetricStrip
        metrics={
          restored && snapshot
            ? [
                {
                  label: "Trials",
                  value: `${snapshot.work.succeeded}/${snapshot.work.planned} completed · ${snapshot.work.failed} failed`
                },
                { label: "Mean reward", value: formatMissingNumber(snapshot.meanReward) },
                // Zero is a measurement. An unreconciled ledger is not one, so
                // it says so rather than printing a total nobody measured.
                { label: "Usage", value: snapshot.usage.tokens ?? "unavailable" },
                { label: "Cost", value: snapshot.usage.cost ?? "unavailable" },
                { label: "Status", value: statusLabel }
              ]
            : [
                { label: "Trials", value: trials.length ? `${verifiedCount}/${trials.length} verified` : "—" },
                { label: "Reward", value: formatMissingNumber(projection.reward) },
                { label: "reward.txt", value: projection.has_reward_txt ? "present" : "not yet" },
                { label: "Status", value: statusLabel }
              ]
        }
      />

      {props.workspace ? <WorkspacePanel client={props.workspace} /> : null}

      {!restored && skills.length > 0 ? (
        <section className="sv-section" aria-label="Skill progress">
          <h3>Skill progress</h3>
          <table style={{ width: "100%", textAlign: "left" }}>
            <thead><tr><th>Rollout</th><th>Skill</th><th>XP</th><th>XP/min</th><th>Samples</th></tr></thead>
            <tbody>{skills.map((row) => <tr key={JSON.stringify([row.lane, row.skill])}>
              <td><Identifier value={row.lane} label="rollout" max={20} copy={false} /></td>
              <td>{row.skill}</td><td>{formatMissingNumber(row.xp)}</td>
              <td>{formatMissingNumber(row.xpPerMin)}</td><td>{row.samples}</td>
            </tr>)}</tbody>
          </table>
        </section>
      ) : null}

      {restored && snapshot ? (
        <section className="sv-section" aria-label="Settled run" data-testid="harbor-restored">
          <div className="sv-section-head">
            <h3>Settled run</h3>
            <span className="sv-mono">{snapshot.elapsed ?? "duration not recorded"}</span>
          </div>
          <p style={{ margin: 0, fontSize: 12.5 }}>{snapshot.assessment.summary ?? "This run reached a terminal state."}</p>
          <p className="sv-mono" style={{ margin: "6px 0 0", fontSize: 11, color: "var(--sv-text-muted)" }}>
            {[snapshot.runtime.policy, snapshot.runtime.model, snapshot.runtime.provider]
              .filter(Boolean)
              .join(" · ")}
          </p>
          <p className="sv-mono" style={{ margin: "6px 0 0", fontSize: 11, color: "var(--sv-text-muted)" }}>
            evidence {snapshot.evidence.completeness ?? "unknown"} · {snapshot.evidence.refCount} retained reference
            {snapshot.evidence.refCount === 1 ? "" : "s"}
            {snapshot.evidence.reason ? ` · ${snapshot.evidence.reason}` : ""}
          </p>
          {snapshot.limitations.length ? (
            <ul style={{ margin: "6px 0 0", paddingLeft: 16, fontSize: 12 }}>
              {snapshot.limitations.map((limitation) => (
                <li key={limitation}>{limitation}</li>
              ))}
            </ul>
          ) : null}
        </section>
      ) : null}

      {error ? (
        <p role="alert" style={{ color: "#c2553f" }}>
          {error}
        </p>
      ) : null}

      {restored && snapshot ? <SnapshotTrials snapshot={snapshot} /> : null}

      {restored ? null : (
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

      )}

      {restored ? null : (
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
                    data-tone={trial.status === "verified" ? (trial.reward != null && trial.reward > 0 ? "ok" : "warn") : trial.status === "failed" ? "bad" : undefined}
                    style={{ marginLeft: "auto" }}
                  >
                    {trial.status}
                  </span>
                </div>
                {trial.instruction ? (
                  <p style={{ margin: "6px 0 0", fontSize: 12.5 }}>{trial.instruction}</p>
                ) : null}
                {trial.status === "verified" ? (
                  <p className="sv-mono" style={{ margin: "6px 0 0", fontSize: 11, color: "var(--sv-text-muted)" }}>
                    {trial.verifierScript ?? "verifier"} · reward.txt {trial.reward == null ? "missing (fails closed — never defaulted to 0)" : formatMissingNumber(trial.reward)}
                  </p>
                ) : trial.status === "completed" || trial.status === "failed" ? (
                  <p className="sv-mono">Reported reward: {formatMissingNumber(trial.reward)}</p>
                ) : null}
              </article>
            ))}
          </div>
        )}
      </section>

      )}

      {/* Both remaining panes read a closed stream. On a restored terminal
          visual they can only say "waiting", which is the exact false claim
          this template was reopened showing. */}
      {restored ? null : (
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
              {String(event.payload.name ?? event.payload.text ?? event.payload.message ?? "")}
            </li>
          ))}
          {tools.length === 0 ? (
            <li style={{ color: "var(--sv-text-faint)" }}>Waiting for tools…</li>
          ) : null}
        </ol>
      </section>

      )}

      {restored ? null : (
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
      )}
    </VisualChrome>
  );
}

export default Shell;

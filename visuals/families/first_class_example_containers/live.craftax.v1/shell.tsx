import { useEffect, useMemo, useState } from "react";
import { Identifier } from "../../../chrome/Identifier.tsx";
import { useLiveEvalStream } from "../../../chrome/useLiveEvalStream.ts";
import { formatMissingNumber, formatMissingUsd } from "../../../runtime/liveStream.ts";
import type { LiveTemplateProps } from "../../../runtime/replayClient.ts";
import { callForSequence, projectAgentTurns, reconcileCallSelection, type EvidenceField } from "../../../runtime/agentTranscript.ts";
import type { LiveEvalEvent, VisualBinding } from "../../../runtime/types.ts";
import {
  craftaxEventLane,
  craftaxEventSequence,
  craftaxRewardValue,
  craftaxTruthLabel,
  craftaxTruthState,
  groupTraceByStep,
  projectCraftaxViewer,
  scopeCraftaxEvents,
  semanticCheckpointIndexes,
  type CraftaxSemanticTraceItem
} from "./projectCraftax.ts";
import "./viewer.css";

// Vite turns these template-local fixtures into packaged assets. Persisted
// visuals intentionally keep only the small fixture reference in SQLite; the
// shell resolves that reference here instead of requiring a megabyte-scale
// inline binding to be copied through the visual service.
const BUNDLED_FIXTURES = import.meta.glob("./examples/*.json", {
  eager: true,
  import: "default"
}) as Record<string, unknown>;

type StreamScope = {
  campaign_id?: string;
  rollout_ids?: string[];
  selection?: { initial_rollout_id?: string };
};
type StreamPayload = {
  events?: LiveEvalEvent[];
  replay_ms?: number;
  sse_url?: string;
  poll_url?: string;
  transports?: { poll?: { url?: string }; sse?: { url?: string } };
  scope?: StreamScope;
};
type VisualMetadata = { visualConfig?: Partial<ViewerConfig>; qualityGate?: { ready?: boolean; revision?: number } };
type ViewerConfig = {
  density: "comfortable" | "compact";
  theme: "ember" | "light";
  showActivity: boolean;
  showTraceInspector: boolean;
  showPlots: boolean;
};

export type ShellProps = LiveTemplateProps & {
  title?: string;
  lede?: string;
  stream?: StreamPayload;
  data?: StreamPayload;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
  visualMetadata?: VisualMetadata;
};

const DEFAULT_CONFIG: ViewerConfig = {
  density: "comfortable",
  theme: "ember",
  showActivity: true,
  showTraceInspector: true,
  showPlots: true
};

/** Bound number of step groups rendered before the "Show earlier" expander. */
const TRACE_GROUP_WINDOW = 30;
const TRANSCRIPT_CALL_WINDOW = 200;

function object(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function finite(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function asStream(raw: unknown): StreamPayload {
  return object(raw) as StreamPayload;
}

function bundledFixtureStream(bindings: VisualBinding[]): StreamPayload | undefined {
  const source = bindings.find((binding) => binding.slot === "stream" && binding.kind === "fixture")?.source;
  if (!source) return undefined;
  const fileName = source.split("/").pop();
  if (!fileName) return undefined;
  const fixture = BUNDLED_FIXTURES[`./examples/${fileName}`];
  const stream = asStream(fixture);
  return Array.isArray(stream.events) ? stream : undefined;
}

function timeLabel(event: LiveEvalEvent | undefined, precise = false): string {
  const raw = event?.occurred_at ?? event?.ts;
  const parsed = Date.parse(raw ?? "");
  if (!Number.isFinite(parsed)) return "time unavailable";
  return new Date(parsed).toLocaleTimeString([], precise
    ? { hour: "2-digit", minute: "2-digit", second: "2-digit", fractionalSecondDigits: 3 }
    : { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function eventStep(event: LiveEvalEvent, fallback: number): number {
  const payload = object(event.payload);
  const readout = object(payload.readout);
  return finite(payload.step) ?? finite(payload.env_steps) ?? finite(readout.env_steps) ?? fallback;
}

function latestObservation(events: LiveEvalEvent[]): Record<string, unknown> {
  const event = [...events].reverse().find((candidate) => candidate.kind === "observation");
  const payload = object(event?.payload);
  const readout = object(payload.readout);
  return Object.keys(readout).length ? readout : payload;
}

function inventoryFrom(observation: Record<string, unknown>): Record<string, unknown> {
  const direct = object(observation.inventory);
  const publicState = object(observation.public);
  return Object.keys(direct).length ? direct : object(publicState.inventory);
}

function usefulInventory(inventory: Record<string, unknown>): Array<[string, string]> {
  const skip = new Set(["health", "food", "drink", "energy", "mana", "xp", "dexterity", "strength", "intelligence"]);
  const output: Array<[string, string]> = [];
  for (const [key, value] of Object.entries(inventory)) {
    if (skip.has(key)) continue;
    if (typeof value === "number" && value > 0) output.push([key, String(value)]);
    else if (typeof value === "string" && value && value !== "none") output.push([key, value]);
    else if (Array.isArray(value) && value.some(Boolean)) output.push([key, value.filter(Boolean).join(", ")]);
    else if (value && typeof value === "object") {
      for (const [child, childValue] of Object.entries(value)) {
        if (typeof childValue === "number" && childValue > 0) output.push([`${key}.${child}`, String(childValue)]);
      }
    }
  }
  return output.slice(0, 16);
}

function sparkline(values: number[], width = 640, height = 190): string {
  if (!values.length) return "";
  const max = Math.max(1, ...values);
  return values.map((value, index) => {
    const x = 28 + (values.length === 1 ? 0 : index * (width - 56) / (values.length - 1));
    const y = height - 24 - (value / max) * (height - 52);
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(" ");
}

function traceText(value: unknown): string {
  if (value == null || value === "") return "not emitted";
  if (typeof value === "string") return value;
  try { return JSON.stringify(value, null, 2); }
  catch { return String(value); }
}

const EVIDENCE_LABELS: Record<EvidenceField["state"], string> = {
  visible: "Recorded and visible", redacted: "Recorded but redacted", not_emitted: "Not emitted by provider",
  not_applicable: "Not applicable to this call", contract_defect: "Missing: producer-contract defect", pending: "Awaiting durable evidence"
};

function Evidence({ label, field }: { label: string; field: EvidenceField }) {
  return <section className={`cv-evidence state-${field.state}`}><div><h5>{label}</h5><span>{EVIDENCE_LABELS[field.state]}</span></div>
    {field.state === "visible" ? <pre>{traceText(field.value)}</pre> : <p>{field.detail ?? EVIDENCE_LABELS[field.state]}</p>}</section>;
}

function semanticTraceText(item: CraftaxSemanticTraceItem | undefined, field: "input" | "thinking" | "output" | "tools"): string {
  const interaction = item?.interaction;
  if (!interaction) return field === "input" ? "Not applicable for this event" : "Not applicable";
  const value = interaction[field];
  if (value != null && value !== "") return traceText(value);
  if (field === "output" && interaction.responseType === "tool_call") return "Tool-only response (no text output)";
  if (interaction.responseType === "pending") return "Pending";
  return "Not emitted";
}

type LaneSummary = { reward?: number; achievements: number; terminal: boolean };

/** One O(n) pass instead of a full projection per lane per render. */
function summarizeLanes(events: LiveEvalEvent[]): Map<string, LaneSummary> {
  const summaries = new Map<string, LaneSummary>();
  for (const event of events) {
    const lane = craftaxEventLane(event);
    const summary = summaries.get(lane) ?? { achievements: 0, terminal: false };
    if (event.kind === "reward_signal") {
      const value = craftaxRewardValue(event.payload);
      if (value != null) summary.reward = (summary.reward ?? 0) + value;
    } else if (event.kind === "snapshot") {
      const payload = object(event.payload);
      const reward = finite(payload.total_reward);
      if (reward != null) summary.reward = reward;
      const achievements = Array.isArray(payload.achievements) ? payload.achievements.length : finite(payload.achievement_count);
      if (achievements != null) summary.achievements = Math.max(summary.achievements, achievements);
    } else if (event.kind === "achievement_unlocked") {
      summary.achievements += 1;
    } else if (event.kind === "eval.run.terminal") {
      const reward = finite(object(event.payload).reward);
      if (reward != null) summary.reward = reward;
      summary.terminal = true;
    } else if (event.kind === "trace.reconciled") {
      summary.terminal = true;
    } else if (event.kind === "status") {
      const status = String(object(event.payload).status ?? "").toLowerCase();
      if (["completed", "finished", "failed", "cancelled"].includes(status)) summary.terminal = true;
    }
    summaries.set(lane, summary);
  }
  return summaries;
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div><span>{label}</span><strong>{value}</strong></div>;
}

function truthNumber(value: number | undefined, terminal: boolean, format: (value: number) => string): string {
  const state = craftaxTruthState(value, { terminal });
  return state === "present" ? format(value as number) : craftaxTruthLabel(state);
}

export function Shell(props: ShellProps) {
  const bindingList = Array.isArray(props.bindings) ? props.bindings : props.bindings?.slots ?? [];
  const stream = asStream(props.data ?? props.stream ?? bundledFixtureStream(bindingList));
  const declaredStreamCount = props.replay?.streams.length ?? 0;
  const scope = stream.scope;
  // A fixture is authoring evidence. It never stands in for a declared stream,
  // and a declared stream is never inferred from the fixture's own fields.
  const fixtureEvents = useMemo(
    () => (declaredStreamCount > 0 ? undefined : stream.events),
    [declaredStreamCount, stream.events]
  );
  const { events, state, error, ready, recovered } = useLiveEvalStream({
    replay: props.replay,
    fixtureEvents,
    replayMs: stream.replay_ms,
    visualId: props.visualId,
    revision: props.revision
  });
  const frameBaseUrl = props.replay?.streams[0]?.sseUrl ?? props.replay?.streams[0]?.pollUrl;
  const missingTransportCount = props.replayMissingTransport?.length ?? 0;
  const bindingError = missingTransportCount > 0
    ? `${missingTransportCount} live stream${missingTransportCount === 1 ? " is" : "s are"} missing required poll transport`
    : null;
  const config = { ...DEFAULT_CONFIG, ...props.visualMetadata?.visualConfig };
  const scopedEvents = useMemo(() => {
    // A visual bound to specific rollouts must never silently import every
    // run sharing the producer's storage root.
    return scopeCraftaxEvents(events, scope?.rollout_ids);
  }, [events, scope?.rollout_ids]);
  const fullProjection = useMemo(() => projectCraftaxViewer(scopedEvents), [scopedEvents]);
  const [chosenLane, setChosenLane] = useState<string | null>(scope?.selection?.initial_rollout_id ?? null);
  const [evaluationCutoff, setEvaluationCutoff] = useState<number | null>(null);
  const [laneCutoff, setLaneCutoff] = useState<number | null>(null);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const [selectedTraceId, setSelectedTraceId] = useState<string | null>(null);
  const [traceMode, setTraceMode] = useState<"focus" | "full">("full");
  const [surface, setSurface] = useState<"replay" | "transcript" | "raw" | "metrics" | "integrity">("transcript");
  const [selectedCallId, setSelectedCallId] = useState<string | null>(null);
  const [transcriptMode, setTranscriptMode] = useState<"focus" | "full">("full");
  const [showEarlierGroups, setShowEarlierGroups] = useState(false);
  const [framePlaying, setFramePlaying] = useState(false);
  const [frameFps, setFrameFps] = useState(4);
  const [failedFrameUrl, setFailedFrameUrl] = useState<string | null>(null);
  const checkpoints = useMemo(() => semanticCheckpointIndexes(fullProjection.ordered), [fullProjection.ordered]);
  const evaluationIndex = fullProjection.ordered.length
    ? evaluationCutoff == null ? fullProjection.ordered.length - 1 : Math.min(evaluationCutoff, fullProjection.ordered.length - 1)
    : -1;
  const checkpointPosition = checkpoints.length
    ? Math.max(0, checkpoints.findLastIndex((index) => index <= evaluationIndex))
    : -1;
  const evaluationEvents = evaluationIndex < 0 ? [] : fullProjection.ordered.slice(0, evaluationIndex + 1);
  const viewer = useMemo(() => projectCraftaxViewer(evaluationEvents, chosenLane, laneCutoff), [evaluationEvents, chosenLane, laneCutoff]);
  const { lanes, selectedLane, laneEvents, visibleEvents, visibleIndex, rewardSignals, achievements, traceEvents, semanticTrace, frameEvents, policy } = viewer;
  const laneSummaries = useMemo(() => summarizeLanes(evaluationEvents), [evaluationEvents]);
  const latest = visibleEvents.at(-1);
  const observation = latestObservation(visibleEvents);
  const inventory = inventoryFrom(observation);
  const terminalLanes = [...laneSummaries.values()].filter((summary) => summary.terminal).length;
  const allLanesTerminal = lanes.length > 0 && terminalLanes === lanes.length;
  const visualLive = state === "live" && terminalLanes < lanes.length;
  const inspectedItems = traceMode === "focus"
    ? semanticTrace.filter((item) => item.category === "policy" || item.category === "evidence")
    : semanticTrace;
  const selectedTrace = inspectedItems.find((item) => item.id === selectedTraceId) ?? (traceMode === "focus" ? inspectedItems.find((item) => item.category === "policy") : inspectedItems.at(-1));
  const turns = useMemo(() => projectAgentTurns(visibleEvents), [visibleEvents]);
  const selectedCall = turns.calls.find((call) => call.id === selectedCallId) ?? turns.calls.find((call) => call.id === reconcileCallSelection(turns.calls, selectedCallId, transcriptMode === "focus"));
  const renderedCalls = turns.calls.length <= TRANSCRIPT_CALL_WINDOW ? turns.calls : (() => {
    const recent = turns.calls.slice(-TRANSCRIPT_CALL_WINDOW);
    return selectedCall && !recent.some((call) => call.id === selectedCall.id) ? [selectedCall, ...recent.slice(1)] : recent;
  })();
  const traceGroups = useMemo(() => groupTraceByStep(inspectedItems), [inspectedItems]);
  const visibleGroups = showEarlierGroups || traceGroups.length <= TRACE_GROUP_WINDOW
    ? traceGroups
    : traceGroups.slice(-TRACE_GROUP_WINDOW);
  const hiddenGroupCount = traceGroups.length - visibleGroups.length;
  const frameUrl = useMemo(() => {
    if (!viewer.frameUrl || viewer.frameUrl === failedFrameUrl) return undefined;
    try {
      // Frame paths are relative to the stream that emitted them.
      return new URL(viewer.frameUrl, frameBaseUrl ?? window.location.href).toString();
    } catch {
      return undefined;
    }
  }, [viewer.frameUrl, failedFrameUrl, frameBaseUrl]);
  const rewardSeries = rewardSignals.length
    ? rewardSignals.reduce<number[]>((series, event) => {
        series.push((series.at(-1) ?? 0) + (craftaxRewardValue(event.payload) ?? 0));
        return series;
      }, [])
    : visibleEvents.flatMap((event) => {
        if (event.kind !== "snapshot") return [];
        const value = finite(object(event.payload).total_reward);
        return value == null ? [] : [value];
      });
  const achievementSeries = visibleEvents.reduce<number[]>((series, event) => {
    if (event.kind === "achievement_unlocked") series.push((series.at(-1) ?? 0) + 1);
    else if (event.kind === "reward_signal") series.push(series.at(-1) ?? 0);
    return series;
  }, []);
  const lastDurableSequence = craftaxEventSequence(fullProjection.ordered.at(-1) ?? ({} as LiveEvalEvent), -1);
  // The transport state is the hook's; this only names it for a reader. Every
  // state here is reached deliberately, including the ones that used to be the
  // absence of a state.
  const transportState = bindingError ? "error" : state;
  const connectionState = bindingError
    ? "binding error"
    : transportState === "error"
      ? `transport error · last durable seq ${lastDurableSequence >= 0 ? lastDurableSequence : "—"}`
      : transportState === "idle"
        ? "no stream declared"
        : transportState === "declared"
          ? "opening declared streams"
          : transportState === "replaying"
            ? `replaying${recovered ? ` · recovered ${recovered}` : ""}`
            : transportState === "terminal"
              ? "replay complete"
              : ready
                ? "streaming"
                : "streaming · waiting for evidence";

  useEffect(() => {
    // Replay advances one semantic checkpoint (environment step / policy-call
    // boundary / reward) per tick — never one transport delta per tick.
    if (!playing || !checkpoints.length) return;
    const timer = window.setInterval(() => {
      setEvaluationCutoff((current) => {
        const currentIndex = current ?? -1;
        const next = checkpoints.find((index) => index > currentIndex);
        if (next == null || next >= (checkpoints.at(-1) ?? 0)) {
          setPlaying(false);
          return null;
        }
        return next;
      });
    }, 700 / speed);
    return () => window.clearInterval(timer);
  }, [playing, speed, checkpoints]);

  useEffect(() => {
    setSelectedTraceId(null);
  }, [traceMode, selectedLane]);

  useEffect(() => {
    setSelectedCallId((current) => reconcileCallSelection(turns.calls, current, transcriptMode === "focus"));
  }, [turns.calls, transcriptMode, props.visualMetadata?.qualityGate?.revision]);

  useEffect(() => {
    if (!framePlaying || !frameEvents.length) return;
    const timer = window.setInterval(() => {
      const currentSequence = craftaxEventSequence(visibleEvents.at(-1) ?? frameEvents[0], -1);
      const currentIndex = frameEvents.findLastIndex((event) => craftaxEventSequence(event, -1) <= currentSequence);
      const next = frameEvents[(currentIndex + 1) % frameEvents.length];
      const laneIndex = laneEvents.indexOf(next);
      if (laneIndex >= 0) setLaneCutoff(laneIndex);
    }, 1000 / frameFps);
    return () => window.clearInterval(timer);
  }, [framePlaying, frameFps, frameEvents, laneEvents, visibleEvents]);

  return (
    <div
		className={`craftax-live-viewer theme-${config.theme} density-${config.density}`}
		data-testid="visual-live-craftax"
		data-visual-landmark="gameplay-dashboard"
		data-visual-transport-state={transportState}
		data-visual-rollout-count={lanes.length}
		data-visual-rendered-frame-count={frameUrl ? frameEvents.length : 0}
		data-visual-semantic-event-count={semanticTrace.length}
		data-visual-terminal={allLanesTerminal ? "true" : "false"}
		data-visual-error={bindingError ?? error ?? ""}
		data-active-surface={surface}
	>
      <header className="cv-topbar">
        <div><p className="cv-eyebrow">Live eval · Craftax{scope?.campaign_id ? <> · <Identifier value={scope.campaign_id} label="campaign" max={18} copy={false} /></> : null}</p><h2>{props.title ?? "Policy through time"}</h2>{props.lede ? <p className="cv-lede">{props.lede}</p> : null}</div>
        <div className="cv-connection" role="status"><span className={visualLive ? "live" : ready ? "ready" : ""} />{connectionState}</div>
      </header>

      <nav className="cv-surfaces" aria-label="Trace viewer surfaces">
        {([ ["replay", "Replay"], ["transcript", "Agent transcript"], ["raw", "Raw trace"], ["metrics", "Metrics"], ["integrity", "Integrity"] ] as const).map(([id, label]) =>
          <button key={id} type="button" aria-current={surface === id ? "page" : undefined} onClick={() => setSurface(id)}>{label}</button>)}
      </nav>

      <section className="cv-summary cv-surface-replay" aria-label="Run summary">
        <Metric label="Rollouts" value={String(lanes.length || "—")} />
        <Metric label="Selected step" value={latest ? String(eventStep(latest, visibleIndex)) : "—"} />
        <Metric label="Reward" value={truthNumber(viewer.reward, viewer.terminal, (value) => formatMissingNumber(value))} />
        <Metric label="Achievements" value={String(achievements.length)} />
        <Metric label="Policy cost" value={truthNumber(finite(policy.usage.cost_usd), viewer.terminal, formatMissingUsd)} />
        <Metric label="Trace" value={`${semanticTrace.length} semantic events`} />
      </section>

      {bindingError || error ? <p role="alert" className="cv-error">{bindingError ?? error}</p> : null}
      <nav className="cv-lanes cv-surface-replay" aria-label="Rollout lanes">
        {lanes.map((lane) => {
          const summary = laneSummaries.get(lane);
          return <button key={lane} type="button" aria-current={lane === selectedLane} aria-label={`Select rollout ${lane}`} onClick={() => { setChosenLane(lane); setLaneCutoff(null); }}>
            <span><Identifier value={lane} max={20} copy={false} style={{ fontWeight: 700 }} /><em>{summary?.terminal ? "done" : "live"}</em></span>
            <small>reward {formatMissingNumber(summary?.reward)} · {summary?.achievements ?? 0} achievements</small>
          </button>;
        })}
      </nav>

      <section className="cv-workspace cv-surface-replay" data-visual-landmark="primary-surface">
        <article className="cv-panel cv-game">
          <div className="cv-heading"><div><p className="cv-eyebrow">Selected rollout</p><h3>{selectedLane ? <Identifier value={selectedLane} max={30} style={{ font: "inherit" }} /> : "Waiting for events"}</h3></div><span>{viewer.terminal ? "finished" : visualLive ? "live" : "waiting"}</span></div>
          <div className="cv-frame">
            {frameUrl ? <img src={frameUrl} alt="Craftax gameplay frame" onError={() => setFailedFrameUrl(viewer.frameUrl ?? null)} /> : (failedFrameUrl || viewer.frameUnavailable) ? <p>Gameplay PNG is unavailable. Reopen uses the live spool digest — this view does not substitute ASCII for a missing image.</p> : viewer.ascii ? <pre aria-label="Craftax symbolic gameplay frame">{viewer.ascii}</pre> : <p>No renderable gameplay frame was emitted at this point in the trace.</p>}
            <div className="cv-frame-caption"><span>step {latest ? eventStep(latest, visibleIndex) : "—"}</span><span>{timeLabel(latest, true)}</span></div>
          </div>
          <div className="cv-video-controls" data-visual-landmark="image-replay">
            <div><strong>Image replay</strong><span>{frameEvents.length} PNG frames from Containers</span></div>
            <button type="button" onClick={() => setFramePlaying(!framePlaying)} disabled={!frameEvents.length}>{framePlaying ? "Pause video" : "Play video"}</button>
            <select aria-label="Image replay speed" value={frameFps} onChange={(event) => setFrameFps(Number(event.currentTarget.value))}><option value={2}>2 fps</option><option value={4}>4 fps</option><option value={8}>8 fps</option><option value={12}>12 fps</option></select>
            <input aria-label="Replay gameplay frames" type="range" min={0} max={Math.max(0, frameEvents.length - 1)} value={Math.max(0, frameEvents.findLastIndex((event) => craftaxEventSequence(event, -1) <= craftaxEventSequence(latest ?? frameEvents[0], Number.MAX_SAFE_INTEGER)))} onChange={(event) => { const frame = frameEvents[Number(event.currentTarget.value)]; const index = laneEvents.indexOf(frame); if (index >= 0) setLaneCutoff(index); setFramePlaying(false); }} />
          </div>
        </article>

        <aside className="cv-panel cv-details">
          <section><p className="cv-eyebrow">Policy</p><h3>{policy.model ?? "Unavailable"}</h3><dl>
            <div><dt>Provider</dt><dd>{policy.provider ?? "—"}</dd></div>
            <div><dt>Actions</dt><dd>{policy.actions.length}</dd></div>
            <div><dt>Tokens</dt><dd>{truthNumber(finite(policy.usage.total_tokens), viewer.terminal, (value) => formatMissingNumber(value, 0))}</dd></div>
            <div><dt>Authority</dt><dd>{policy.actionAuthority ?? "—"}</dd></div>
          </dl>{policy.actions.length ? <p className="cv-plan">{policy.actions.join(" → ")}</p> : null}</section>
          <section><p className="cv-eyebrow">Environment</p><dl>
            {(["health", "food", "drink", "energy", "mana", "xp"] as const).map((key) => <div key={key}><dt>{key}</dt><dd>{formatMissingNumber(finite(inventory[key]), 0)}</dd></div>)}
          </dl><h4>Resources &amp; gear</h4><div className="cv-tokens">{usefulInventory(inventory).map(([name, value]) => <span key={name}>{name} {value}</span>)}{!usefulInventory(inventory).length ? <i>None carried</i> : null}</div>
          <h4>Achievements</h4><div className="cv-tokens">{achievements.map((name) => <span key={name}>{name}</span>)}{!achievements.length ? <i>None yet</i> : null}</div></section>
        </aside>
      </section>

      <section className="cv-panel cv-timeline cv-shared-cursor" data-visual-landmark="temporal-controls">
        <div className="cv-heading"><div><p className="cv-eyebrow">Evaluation time</p><h3>{Math.max(0, checkpointPosition) + 1} / {checkpoints.length || 0} checkpoints · {timeLabel(fullProjection.ordered[evaluationIndex], true)}</h3></div><div className="cv-replay"><button onClick={() => setPlaying(!playing)} disabled={!checkpoints.length}>{playing ? "Pause" : "Play"}</button><select aria-label="Replay speed" value={speed} onChange={(event) => setSpeed(Number(event.currentTarget.value))}><option value={0.5}>0.5×</option><option value={1}>1×</option><option value={2}>2×</option><option value={4}>4×</option></select><button onClick={() => { setEvaluationCutoff(null); setLaneCutoff(null); setPlaying(false); }}>{visualLive ? "Follow live" : "Jump to end"}</button></div></div>
        <input
          aria-label="Replay evaluation through semantic checkpoints"
          type="range"
          min={0}
          max={Math.max(0, checkpoints.length - 1)}
          value={Math.max(0, checkpointPosition)}
          onChange={(event) => { setEvaluationCutoff(checkpoints[Number(event.currentTarget.value)] ?? null); setPlaying(false); }}
        />
        <div className="cv-lane-timeline"><span>Rollout time (raw events)</span><input aria-label="Replay selected rollout by raw event" type="range" min={0} max={Math.max(0, laneEvents.length - 1)} value={Math.max(0, visibleIndex)} onChange={(event) => setLaneCutoff(Number(event.currentTarget.value))} /></div>
      </section>

      {config.showPlots ? <section className="cv-plots cv-surface-replay" data-visual-landmark="outcome-plots">
        <article className="cv-panel"><div className="cv-heading"><div><p className="cv-eyebrow">Selected rollout</p><h3>Cumulative reward</h3></div><strong>{formatMissingNumber(viewer.cumulativeReward)}</strong></div><svg viewBox="0 0 640 190" role="img" aria-label="Cumulative reward by step"><line x1="28" y1="166" x2="612" y2="166"/><polyline points={sparkline(rewardSeries)} /></svg></article>
        <article className="cv-panel"><div className="cv-heading"><div><p className="cv-eyebrow">Selected rollout</p><h3>Achievements through time</h3></div><strong>{achievements.length}</strong></div><svg viewBox="0 0 640 190" role="img" aria-label="Cumulative achievements by step"><line x1="28" y1="166" x2="612" y2="166"/><polyline className="secondary" points={sparkline(achievementSeries)} /></svg></article>
      </section> : null}

      <section className="cv-panel cv-transcript cv-surface-transcript" data-visual-landmark="agent-transcript">
        <div className="cv-heading"><div><p className="cv-eyebrow">Chronological model calls</p><h3>Agent transcript</h3></div><div className="cv-trace-mode"><button type="button" aria-pressed={transcriptMode === "focus"} onClick={() => setTranscriptMode("focus")}>Focus</button><button type="button" aria-pressed={transcriptMode === "full"} onClick={() => setTranscriptMode("full")}>Full</button><span>{turns.calls.length} calls · cutoff seq {craftaxEventSequence(visibleEvents.at(-1) ?? ({} as LiveEvalEvent), 0)}</span></div></div>
        <div className="cv-step-links" aria-label="Environment step to policy navigation">{semanticTrace.filter((item) => item.kind === "environment.step").slice(-40).map((item) => { const callId = item.step == null ? callForSequence(turns.calls, item.sequenceStart)?.id : turns.callIdByEnvironmentStep.get(item.step); return <button type="button" key={item.id} disabled={!callId} onClick={() => { if (callId) setSelectedCallId(callId); }}>step {item.step ?? "—"}</button>; })}</div>
        <div className="cv-transcript-grid"><ol className="cv-call-list" aria-label="Model calls">{turns.calls.length > renderedCalls.length ? <li className="cv-call-window">Showing {renderedCalls.length} of {turns.calls.length} calls at this cutoff</li> : null}{renderedCalls.map((call) => <li key={call.id}><button type="button" aria-current={call.id === selectedCall?.id} onClick={() => setSelectedCallId(call.id)}><span>Call {call.callNumber}</span><strong>{call.model ?? "Model not recorded"}</strong><small>steps {call.environmentStepStart ?? "—"}{call.environmentStepEnd !== call.environmentStepStart ? `–${call.environmentStepEnd ?? "—"}` : ""} · seq {call.sourceSequenceStart}–{call.sourceSequenceEnd}</small></button></li>)}</ol>
          <article className="cv-call-card" aria-live="polite">{selectedCall ? <><header><div><p className="cv-eyebrow">Call {selectedCall.callNumber} · environment steps {selectedCall.environmentStepStart ?? "—"}–{selectedCall.environmentStepEnd ?? "—"}</p><h4>{selectedCall.model ?? "Model identity not recorded"}</h4></div><span>{selectedCall.complete ? "complete" : "streaming"}</span></header><dl><div><dt>Provider</dt><dd>{selectedCall.provider ?? "not emitted"}</dd></div><div><dt>Authority</dt><dd>{selectedCall.authority ?? "not emitted"}</dd></div><div><dt>Source</dt><dd>seq {selectedCall.sourceSequenceStart}–{selectedCall.sourceSequenceEnd}</dd></div><div><dt>Envelopes</dt><dd>{selectedCall.rawEvents.length}</dd></div></dl>
            <Evidence label="Input / observation" field={selectedCall.input}/><Evidence label="Reasoning" field={selectedCall.reasoning}/><Evidence label="Output / actions" field={selectedCall.output}/><Evidence label="Tool calls" field={selectedCall.toolCalls}/><Evidence label="Tool results" field={selectedCall.toolResults}/>
            <details><summary>Raw Trace V5 evidence ({selectedCall.rawEvents.length} envelopes)</summary><pre>{JSON.stringify(selectedCall.rawEvents, null, 2)}</pre></details></> : <p>No policy.call has been emitted at this temporal cutoff.</p>}</article></div>
      </section>

      {config.showActivity ? <section className="cv-panel cv-activity cv-surface-raw" data-visual-landmark="ordered-activity"><div className="cv-heading"><div><p className="cv-eyebrow">Semantic activity</p><h3>Recent activity</h3></div><span>{semanticTrace.length} events · {visibleEvents.length} raw</span></div><ol>{semanticTrace.slice(-12).reverse().map((item) => <li key={item.id}><time>seq {item.sequenceEnd}</time><strong>{item.category}</strong><span>{item.kind}</span><p>{item.label}</p></li>)}</ol></section> : null}

      {config.showTraceInspector ? <section className="cv-panel cv-trace cv-surface-raw" data-visual-landmark="trace-inspector">
        <div className="cv-heading"><div><p className="cv-eyebrow">Same temporal cutoff</p><h3>Trace V5 viewer</h3></div><div className="cv-trace-mode"><button type="button" aria-pressed={traceMode === "focus"} onClick={() => setTraceMode("focus")}>Policy focus</button><button type="button" aria-pressed={traceMode === "full"} onClick={() => setTraceMode("full")}>Full trace</button><button type="button" onClick={() => setSelectedTraceId(inspectedItems.at(-1)?.id ?? null)} disabled={!inspectedItems.length}>Jump to latest</button><span>{viewer.terminal ? "sealed/reconciled" : "live · unsealed"}</span></div></div>
        <p className="cv-trace-summary">{traceMode === "full" ? `${semanticTrace.length} semantic events folded from ${visibleEvents.length} durable envelopes, grouped by environment step.` : `${inspectedItems.length} policy calls and trace-authority events; ${traceEvents.length} raw policy partials are folded.`}</p>
        <div className="cv-trace-grid">
          <div className="cv-trace-list">
            {hiddenGroupCount > 0 ? (
              <button type="button" className="cv-trace-earlier" onClick={() => setShowEarlierGroups(true)}>
                Show {hiddenGroupCount} earlier step group{hiddenGroupCount === 1 ? "" : "s"}
              </button>
            ) : null}
            {visibleGroups.map((group) => {
              const containsSelection = group.items.some((item) => item.id === selectedTrace?.id);
              const isLast = group === visibleGroups.at(-1);
              return (
                <details key={group.key} className="cv-trace-group" open={containsSelection || isLast}>
                  <summary>
                    <strong>{group.label}</strong>
                    <span>{group.items.length} event{group.items.length === 1 ? "" : "s"}</span>
                  </summary>
                  <ol aria-label={`${group.label} events`}>
                    {group.items.map((item) => (
                      <li key={item.id}>
                        <button type="button" aria-current={item.id === selectedTrace?.id} aria-label={`${item.kind}: ${item.label}`} onClick={() => setSelectedTraceId(item.id)}>
                          <span>seq {item.sequenceStart === item.sequenceEnd ? item.sequenceStart : `${item.sequenceStart}–${item.sequenceEnd}`}</span>
                          <strong>{item.kind}</strong>
                          <em>{item.label}</em>
                        </button>
                      </li>
                    ))}
                  </ol>
                </details>
              );
            })}
          </div>
          <aside>
            <p className="cv-eyebrow">Selected event</p>
            <h4>{selectedTrace?.kind ?? "Nothing selected"}</h4>
            <div className="cv-trace-io" aria-label="Model interaction details">
              <section><h5>Input</h5><pre data-testid="trace-input">{semanticTraceText(selectedTrace, "input")}</pre></section>
              <section><h5>Thinking</h5><pre data-testid="trace-thinking">{semanticTraceText(selectedTrace, "thinking")}</pre></section>
              <section><h5>Output</h5><pre data-testid="trace-output">{semanticTraceText(selectedTrace, "output")}</pre></section>
              <section><h5>Tool calls</h5><pre data-testid="trace-tools">{semanticTraceText(selectedTrace, "tools")}</pre></section>
            </div>
            <details><summary>Raw evidence ({selectedTrace?.rawEvents.length ?? 0} envelopes)</summary><pre>{selectedTrace ? JSON.stringify(selectedTrace.rawEvents.map((event) => ({ sequence: event.sequence, kind: event.kind, occurred_at: event.occurred_at ?? event.ts, lane: craftaxEventLane(event), payload: event.payload })), null, 2) : "Structural fields appear here."}</pre></details>
          </aside>
        </div>
      </section> : null}

      <section className="cv-panel cv-surface-metrics cv-facts"><div className="cv-heading"><div><p className="cv-eyebrow">At current cutoff</p><h3>Metrics</h3></div></div><dl><div><dt>Model calls</dt><dd>{turns.calls.length}</dd></div><div><dt>Total tokens</dt><dd>{formatMissingNumber(turns.calls.reduce((sum, call) => sum + (finite(call.usage.total_tokens) ?? 0), 0), 0)}</dd></div><div><dt>Latency</dt><dd>{formatMissingNumber(turns.calls.reduce((sum, call) => sum + (call.latencyMs ?? 0), 0), 0)} ms</dd></div><div><dt>Cost</dt><dd>{formatMissingUsd(turns.calls.reduce((sum, call) => sum + (call.costUsd ?? 0), 0))}</dd></div><div><dt>Reward</dt><dd>{truthNumber(viewer.reward, viewer.terminal, formatMissingNumber)}</dd></div><div><dt>Authority</dt><dd>{[...new Set(turns.calls.map((call) => call.authority).filter(Boolean))].join(", ") || "not emitted"}</dd></div></dl></section>
      <section className="cv-panel cv-surface-integrity cv-integrity"><div className="cv-heading"><div><p className="cv-eyebrow">Evidence health</p><h3>Integrity</h3></div><span>{viewer.terminal ? "sealed/reconciled" : "live · unsealed"}</span></div><ul><li><strong>Reconciliation</strong><span>{semanticTrace.some((item) => item.kind === "trace.reconciled") ? "recorded and visible" : viewer.terminal ? "missing due to producer-contract defect" : "pending"}</span></li><li><strong>Model identity</strong><span>{turns.calls.every((call) => call.model && call.provider) ? "recorded and visible" : "missing on one or more calls"}</span></li><li><strong>Repairs / fallbacks</strong><span>{policy.fallback ? "recorded fallback" : "none recorded"}</span></li><li><strong>Malformed calls</strong><span>{turns.missingPolicyEnvelopeCount || "none"}</span></li><li><strong>Reasoning disclosure</strong><span>{turns.calls.some((call) => call.reasoning.state === "visible") ? "provider emitted visible reasoning evidence" : "Thinking not emitted"}</span></li></ul></section>

      <footer>live.craftax.v1 · synth.trace-stream-event.v1 · {props.visualMetadata?.qualityGate?.ready ? `ready rev ${props.visualMetadata.qualityGate.revision ?? "—"}` : "draft visual"}</footer>
    </div>
  );
}

export default Shell;

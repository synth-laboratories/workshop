import { useEffect, useMemo, useState } from "react";
import { Identifier } from "../../../chrome/Identifier.tsx";
import { useLiveEvalStream } from "../../../chrome/useLiveEvalStream.ts";
import { formatMissingNumber, formatMissingUsd } from "../../../runtime/liveStream.ts";
import { mediaRefFrom } from "../../../runtime/mediaClient.ts";
import type { LiveTemplateProps } from "../../../runtime/replayClient.ts";
import { callForSequence, projectAgentTurns, reconcileCallSelection, type EvidenceField } from "../../../runtime/agentTranscript.ts";
import type { LiveEvalEvent, VisualBinding } from "../../../runtime/types.ts";
import { bindingInputName } from "../../../runtime/types.ts";
import {
  craftaxEventLane,
  craftaxEventSequence,
  craftaxRewardValue,
  craftaxTruthLabel,
  craftaxTruthState,
  groupTraceByStep,
  craftaxReplayAvailability,
  mergeCraftaxOptimizerJournalEvents,
  projectCraftaxViewer,
  scopeCraftaxEvents,
  environmentStepCount,
  replayMomentIndexes,
  type CraftaxSemanticTraceItem
} from "./projectCraftax.ts";
import { summarizeCraftaxRun, type CraftaxRolloutAggregate, type CraftaxRunAggregate } from "./aggregateCraftax.ts";
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
type RunLifecycle = {
  status: string;
  terminal: boolean;
  failed: boolean;
  reason?: string;
  rollouts: Array<{
    lane: string;
    seed?: number;
    status: string;
    reward?: number;
    steps?: number;
    calls?: number;
    tokens?: number;
    costUsd?: number;
    achievements?: string[];
    authority?: string;
  }>;
  evidence: {
    state: "pending" | "accepted" | "partial" | "missing" | "rejected";
    valid: number;
    rejected: number;
    missing: number;
    sealedTraces: number;
    failures: Array<{ seed?: number; rolloutId?: string; trialId?: string; code: string; sequence?: number; detail: string }>;
    gaps: Array<{ seed?: number; rolloutId?: string; trialId?: string; code: string; detail: string }>;
  };
  usage: {
    calls?: number;
    costUsd?: number;
    costCapUsd?: number;
    costSource: "workshop_proxy" | "provider" | "container" | "unavailable";
    provider?: string;
    promptTokens?: number;
    completionTokens?: number;
  };
  modelIdentity?: { provider?: string; model?: string; authority?: string };
};
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
  runLifecycle?: RunLifecycle;
  experiment?: Record<string, unknown>;
  /** Durable optimizer journal envelopes supplied by VisualHost. */
  events?: LiveEvalEvent[];
  /** Post-terminal retained trial evidence; not authoritative for run status. */
  enrichmentEvents?: LiveEvalEvent[];
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

function completeSum(values: Array<number | undefined>): number | undefined {
  if (!values.length || values.some((value) => value === undefined)) return undefined;
  return values.reduce<number>((sum, value) => sum + (value as number), 0);
}

function runCostLabel(lifecycle: RunLifecycle | undefined, producerCost: number | undefined): string {
  const cost = lifecycle?.usage.costUsd ?? producerCost;
  if (cost == null) {
    if (lifecycle?.usage.costSource === "workshop_proxy") {
      const calls = lifecycle.usage.calls;
      return calls == null
        ? "unavailable · Workshop proxy receipt omitted cost"
        : `unavailable · Workshop proxy receipt counted ${calls} calls but omitted cost`;
    }
    return "not emitted";
  }
  const amount = cost < 0.1 ? `$${cost.toFixed(6).replace(/0+$/, "").replace(/\.$/, "")}` : formatMissingUsd(cost);
  const source = lifecycle?.usage.costSource === "workshop_proxy" ? "Workshop proxy"
    : lifecycle?.usage.costSource === "provider" ? "provider receipt"
      : lifecycle?.usage.costSource === "container" ? "container telemetry"
        : producerCost != null ? "trace telemetry" : undefined;
  return source ? `${amount} · ${source}` : amount;
}

function asStream(raw: unknown): StreamPayload {
  return object(raw) as StreamPayload;
}

function bundledFixtureStream(bindings: VisualBinding[]): StreamPayload | undefined {
  const source = bindings.find((binding) => bindingInputName(binding) === "stream" && binding.kind === "fixture")?.source;
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

function eventStep(event: LiveEvalEvent): number | undefined {
  const payload = object(event.payload);
  const readout = object(payload.readout);
  return finite(payload.step) ?? finite(payload.step_index) ?? finite(payload.env_steps) ?? finite(readout.env_steps);
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

function OverviewStat({ label, value, detail }: { label: string; value: string; detail: string }) {
  return <div className="cv-stat"><span>{label}</span><strong>{value}</strong><small>{detail}</small></div>;
}

function rangeLabel(min: number | undefined, max: number | undefined, suffix: string): string {
  if (min == null || max == null) return `No ${suffix} reported`;
  return min === max ? `${min} ${suffix} each` : `${min}–${max} ${suffix} per rollout`;
}

function compactUsd(value: number): string {
  return value < 0.1
    ? `$${value.toFixed(6).replace(/0+$/, "").replace(/\.$/, "")}`
    : formatMissingUsd(value);
}

function runCostSummary(
  lifecycle: RunLifecycle | undefined,
  producerCost: number | undefined,
  aggregate: CraftaxRunAggregate
): { value: string; detail: string } {
  const label = runCostLabel(lifecycle, producerCost);
  if (label.startsWith("unavailable") || label === "not emitted") {
    const calls = lifecycle?.usage.calls;
    if (aggregate.totalCostUsd != null) {
      return {
        value: compactUsd(aggregate.totalCostUsd),
        detail: lifecycle?.usage.costSource === "workshop_proxy"
          ? `Complete across ${aggregate.reportedCosts} rollout records · proxy aggregate omitted`
          : `Complete across ${aggregate.reportedCosts} rollout records · per-rollout telemetry`
      };
    }
    if (aggregate.knownCostUsd != null) {
      return {
        value: `Known ${compactUsd(aggregate.knownCostUsd)}`,
        detail: `${aggregate.reportedCosts}/${aggregate.rollouts.length} rollouts priced · exact total unavailable`
      };
    }
    if (label === "not emitted") return { value: "Not emitted", detail: "No authoritative cost amount" };
    return {
      value: "Unavailable",
      detail: calls == null ? "Workshop proxy omitted an amount" : `${calls} proxy calls counted · amount omitted`
    };
  }
  const [value, detail] = label.split(" · ", 2);
  return { value, detail: detail ?? "Authoritative run telemetry" };
}

function distributionHeight(value: number | undefined, maximum: number): string {
  if (value == null || maximum <= 0) return "0%";
  return `${Math.max(3, Math.min(100, Math.abs(value) / maximum * 100)).toFixed(1)}%`;
}

type DistributionMetric = {
  key: "reward" | "achievements" | "steps" | "calls" | "tokens" | "cost";
  label: string;
  className: string;
  value: (rollout: CraftaxRolloutAggregate) => number | undefined;
  format: (value: number | undefined) => string;
};

function AggregateDistribution({
  title,
  detail,
  rollouts,
  metrics,
  selectedLane,
  onSelect
}: {
  title: string;
  detail: string;
  rollouts: CraftaxRolloutAggregate[];
  metrics: DistributionMetric[];
  selectedLane?: string;
  onSelect: (lane: string) => void;
}) {
  const maxima = new Map(metrics.map((metric) => [
    metric.key,
    Math.max(0, ...rollouts.flatMap((rollout) => {
      const value = metric.value(rollout);
      return value == null ? [] : [Math.abs(value)];
    }))
  ]));
  return <figure className="cv-distribution">
    <figcaption><div><strong>{title}</strong><span>{detail}</span></div><div className="cv-distribution-legend" aria-label={`${title} legend`}>{metrics.map((metric) => <i className={metric.className} key={metric.key}>{metric.label}</i>)}</div></figcaption>
    <ol className="cv-distribution-chart" aria-label={`${title} across all evaluation rollouts`}>
      {rollouts.map((rollout) => {
        const values = metrics.map((metric) => metric.value(rollout));
        const label = `${rollout.seed != null ? `seed ${rollout.seed}` : rollout.lane}: ${metrics.map((metric, index) => `${metric.label} ${metric.format(values[index])}`).join(", ")}`;
        return <li key={rollout.lane} data-rollout-status={rollout.status}>
          <button type="button" title={label} aria-label={`${label}. Inspect rollout.`} aria-current={rollout.lane === selectedLane} onClick={() => onSelect(rollout.lane)}>
            <span className="cv-distribution-bars" aria-hidden="true">{metrics.map((metric, index) => <span className={`cv-distribution-bar ${metric.className}${values[index] == null ? " missing" : values[index]! < 0 ? " negative" : ""}`} key={metric.key}><i style={{ height: distributionHeight(values[index], maxima.get(metric.key) ?? 0) }} /><b>{metric.format(values[index])}</b></span>)}</span>
            <span className="cv-distribution-id">{rollout.seed != null ? `seed ${rollout.seed}` : <Identifier value={rollout.lane} max={11} copy={false} />}</span>
            <em>{rollout.status ?? "in progress"}</em>
          </button>
        </li>;
      })}
    </ol>
  </figure>;
}

function RunDistributions({ rollouts, selectedLane, onSelect }: { rollouts: CraftaxRolloutAggregate[]; selectedLane?: string; onSelect: (lane: string) => void }) {
  const outcomeMetrics: DistributionMetric[] = [
    { key: "reward", label: "Reward", className: "reward", value: (rollout) => rollout.reward, format: (value) => formatMissingNumber(value) },
    { key: "achievements", label: "Achievements", className: "achievements", value: (rollout) => rollout.achievementsReported ? rollout.achievements.length : undefined, format: (value) => formatMissingNumber(value, 0) }
  ];
  const usageMetrics: DistributionMetric[] = [
    { key: "steps", label: "Steps", className: "steps", value: (rollout) => rollout.steps, format: (value) => formatMissingNumber(value, 0) },
    { key: "calls", label: "Retained calls", className: "calls", value: (rollout) => rollout.calls, format: (value) => formatMissingNumber(value, 0) },
    { key: "tokens", label: "Tokens", className: "tokens", value: (rollout) => rollout.tokens, format: (value) => formatMissingNumber(value, 0) },
    { key: "cost", label: "Cost", className: "cost", value: (rollout) => rollout.costUsd, format: (value) => value == null ? "—" : compactUsd(value) }
  ];
  const outcomeCoverage = `${rollouts.filter((rollout) => rollout.reward != null).length}/${rollouts.length} rewards · ${rollouts.filter((rollout) => rollout.achievementsReported).length}/${rollouts.length} achievement sets`;
  const usageCoverage = `${rollouts.filter((rollout) => rollout.steps != null).length}/${rollouts.length} steps · ${rollouts.filter((rollout) => rollout.tokens != null).length}/${rollouts.length} tokens · ${rollouts.filter((rollout) => rollout.costUsd != null).length}/${rollouts.length} costs`;
  return <section className="cv-run-distributions" aria-label="Combined evaluation distributions">
    <AggregateDistribution title="Outcome distribution" detail={outcomeCoverage} rollouts={rollouts} metrics={outcomeMetrics} selectedLane={selectedLane} onSelect={onSelect} />
    <AggregateDistribution title="Work and usage distribution" detail={usageCoverage} rollouts={rollouts} metrics={usageMetrics} selectedLane={selectedLane} onSelect={onSelect} />
    <p className="cv-distribution-note">Each metric is normalized to its own maximum within this evaluation. Hover or focus for exact values; select a rollout to inspect its trace.</p>
  </section>;
}

function truthNumber(value: number | undefined, terminal: boolean, format: (value: number) => string): string {
  const state = craftaxTruthState(value, { terminal });
  return state === "present" ? format(value as number) : craftaxTruthLabel(state);
}

export function Shell(props: ShellProps) {
  const bindingList = Array.isArray(props.bindings) ? props.bindings : props.bindings?.slots ?? [];
  // `stream` is the declared input. `data` remains a compatibility fallback
  // for direct previews that pass one anonymous fixture payload.
  const stream = asStream(props.stream ?? props.data ?? bundledFixtureStream(bindingList));
  const declaredStreamCount = props.replay?.streams.length ?? 0;
  const scope = stream.scope;
  const optimizerEvents = useMemo(
    () => mergeCraftaxOptimizerJournalEvents(props.events, props.enrichmentEvents),
    [props.events, props.enrichmentEvents]
  );
  const optimizerJournalBound = bindingList.some((binding) => bindingInputName(binding) === "optimizer_run");
  // A fixture is authoring evidence. It never stands in for a declared stream,
  // and a declared stream is never inferred from the fixture's own fields.
  const fixtureEvents = useMemo(
    () => (optimizerEvents || declaredStreamCount > 0 ? undefined : stream.events),
    [optimizerEvents, declaredStreamCount, stream.events]
  );
  const liveStream = useLiveEvalStream({
    replay: props.replay,
    fixtureEvents,
    replayMs: stream.replay_ms,
    visualId: props.visualId,
    revision: props.revision
  });
  // Optimizer-run subscriptions are a durable journal, not an authoring
  // fixture. Rendering them directly is both immediate and reopen-safe. The
  // old fixture path replayed one of 3,354 envelopes every 800 ms, making a
  // completed run look nearly empty for roughly 45 minutes.
  const events = optimizerEvents ?? liveStream.events;
  const state = optimizerEvents
    ? props.runLifecycle?.terminal ? "terminal" : "live"
    : liveStream.state;
  const error = liveStream.error;
  const ready = optimizerEvents ? true : liveStream.ready;
  const recovered = liveStream.recovered;
  const frameBaseUrl = props.replay?.streams[0]?.sseUrl ?? props.replay?.streams[0]?.pollUrl;
  const missingTransportCount = props.replayMissingTransport?.length ?? 0;
  const bindingError = missingTransportCount > 0
    ? `${missingTransportCount} live stream${missingTransportCount === 1 ? " is" : "s are"} missing required poll transport`
    : null;
  const config = { ...DEFAULT_CONFIG, ...props.visualMetadata?.visualConfig };
  const experimentRuntime = object(object(props.experiment).runtime);
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
  const [surface, setSurface] = useState<"replay" | "transcript" | "raw" | "metrics" | "integrity">("replay");
  const [selectedCallId, setSelectedCallId] = useState<string | null>(null);
  const [transcriptMode, setTranscriptMode] = useState<"focus" | "full">("full");
  const [showEarlierGroups, setShowEarlierGroups] = useState(false);
  const [framePlaying, setFramePlaying] = useState(false);
  const [frameFps, setFrameFps] = useState(4);
  const [failedFrameUrl, setFailedFrameUrl] = useState<string | null>(null);
  const [loadedFrame, setLoadedFrame] = useState<{ digest: string; dataUrl: string } | null>(null);
  const [failedMediaDigest, setFailedMediaDigest] = useState<string | null>(null);
  const moments = useMemo(() => replayMomentIndexes(fullProjection.ordered), [fullProjection.ordered]);
  const environmentSteps = useMemo(() => environmentStepCount(fullProjection.ordered), [fullProjection.ordered]);
  const replayAvailability = useMemo(
    () => craftaxReplayAvailability(fullProjection.ordered, props.runLifecycle?.evidence.state),
    [fullProjection.ordered, props.runLifecycle?.evidence.state]
  );
  const evaluationIndex = fullProjection.ordered.length
    ? evaluationCutoff == null ? fullProjection.ordered.length - 1 : Math.min(evaluationCutoff, fullProjection.ordered.length - 1)
    : -1;
  const momentPosition = moments.length
    ? Math.max(0, moments.findLastIndex((index) => index <= evaluationIndex))
    : -1;
  const evaluationEvents = useMemo(
    () => evaluationIndex < 0 ? [] : fullProjection.ordered.slice(0, evaluationIndex + 1),
    [evaluationIndex, fullProjection.ordered]
  );
  const lifecycleTerminal = props.runLifecycle?.terminal === true;
  const lifecycleFailed = props.runLifecycle?.failed === true;
  const viewer = useMemo(() => projectCraftaxViewer(evaluationEvents, chosenLane, laneCutoff), [evaluationEvents, chosenLane, laneCutoff]);
  const { lanes, selectedLane, laneEvents, visibleEvents, visibleIndex, rewardSignals, achievements, traceEvents, semanticTrace, frameEvents, policy } = viewer;
  const laneSummaries = useMemo(() => summarizeLanes(evaluationEvents), [evaluationEvents]);
  const terminalRollouts = lifecycleTerminal && props.runLifecycle?.rollouts?.length
    ? props.runLifecycle.rollouts
    : undefined;
  const runAggregate = useMemo(
    () => summarizeCraftaxRun(evaluationEvents, terminalRollouts),
    [evaluationEvents, terminalRollouts]
  );
  const latest = visibleEvents.at(-1);
  const selectedEnvironmentStep = useMemo(
    () => [...visibleEvents].reverse().map(eventStep).find((step) => step != null),
    [visibleEvents]
  );
  const observation = latestObservation(visibleEvents);
  const inventory = inventoryFrom(observation);
  const runCost = runCostSummary(props.runLifecycle, finite(policy.usage.cost_usd), runAggregate);
  const receiptCalls = props.runLifecycle?.usage.calls;
  const retainedCalls = runAggregate.totalCalls;
  const callValue = receiptCalls == null ? formatMissingNumber(retainedCalls, 0) : `${formatMissingNumber(receiptCalls, 0)} billed`;
  const callDetail = receiptCalls == null
    ? `${runAggregate.reportedCalls}/${runAggregate.rollouts.length} rollout journals reported calls`
    : retainedCalls == null
      ? `Workshop proxy receipt · retained call starts unavailable`
      : `${formatMissingNumber(retainedCalls, 0)} retained call starts · Workshop receipt covers ${formatMissingNumber(receiptCalls, 0)}`;
  const promptTokens = props.runLifecycle?.usage.promptTokens;
  const completionTokens = props.runLifecycle?.usage.completionTokens;
  const receiptTokens = promptTokens != null && completionTokens != null ? promptTokens + completionTokens : undefined;
  const tokenValue = receiptTokens ?? runAggregate.totalTokens;
  const tokenDetail = receiptTokens != null
    ? `${formatMissingNumber(promptTokens, 0)} prompt + ${formatMissingNumber(completionTokens, 0)} completion · Workshop receipt${runAggregate.totalTokens == null ? "" : ` · ${formatMissingNumber(runAggregate.totalTokens, 0)} runtime across terminal records`}`
    : tokenValue == null
      ? `${runAggregate.reportedTokens}/${runAggregate.rollouts.length} rollouts reported complete token usage`
      : `Complete across ${runAggregate.reportedTokens} rollout records`;
  const terminalByLane = new Map((terminalRollouts ?? []).map((rollout) => [rollout.lane, rollout]));
  const selectedTerminal = selectedLane ? terminalByLane.get(selectedLane) : undefined;
  const terminalLanes = terminalRollouts?.length ?? [...laneSummaries.values()].filter((summary) => summary.terminal).length;
  const allLanesTerminal = lanes.length > 0 && terminalLanes === lanes.length;
  const lifecycleGaps = props.runLifecycle?.evidence.gaps ?? [];
  const missingRewardFacts = lifecycleGaps.filter((gap) => gap.code === "evaluator_numeric_reward_missing").length;
  const missingStepFacts = lifecycleGaps.filter((gap) => gap.code === "full_trace_step_count_missing").length;
  const visualTerminal = lifecycleTerminal || allLanesTerminal;
  const visualLive = !lifecycleTerminal && state === "live" && terminalLanes < lanes.length;
  const trustworthyReplay = replayAvailability.replayable;
  const inspectedItems = traceMode === "focus"
    ? semanticTrace.filter((item) => item.category === "policy" || item.category === "evidence")
    : semanticTrace;
  const selectedTrace = inspectedItems.find((item) => item.id === selectedTraceId) ?? (traceMode === "focus" ? inspectedItems.find((item) => item.category === "policy") : inspectedItems.at(-1));
  const turns = useMemo(() => projectAgentTurns(visibleEvents), [visibleEvents]);
  const totalTokens = completeSum(turns.calls.map((call) => finite(call.usage.total_tokens)));
  const totalLatencyMs = completeSum(turns.calls.map((call) => call.latencyMs));
  const totalCostUsd = completeSum(turns.calls.map((call) => call.costUsd));
  const selectedRolloutTokens = selectedTerminal?.tokens ?? totalTokens;
  const selectedRolloutAuthority = selectedTerminal?.authority
    ?? props.runLifecycle?.modelIdentity?.authority
    ?? [...new Set(turns.calls.map((call) => call.authority).filter(Boolean))].join(", ")
    ?? undefined;
  const pinnedProvider = props.runLifecycle?.modelIdentity?.provider
    ?? (typeof experimentRuntime.provider === "string" ? experimentRuntime.provider : undefined)
    ?? props.runLifecycle?.usage.provider;
  const pinnedModel = props.runLifecycle?.modelIdentity?.model
    ?? (typeof experimentRuntime.model === "string" ? experimentRuntime.model : undefined);
  const integrityAccepted = props.runLifecycle?.evidence.state === "accepted";
  const reconciliationLabel = props.runLifecycle?.evidence.state === "rejected"
    ? `${props.runLifecycle.evidence.rejected} rejected · ${props.runLifecycle.evidence.sealedTraces} sealed`
    : integrityAccepted
      ? `${props.runLifecycle?.evidence.valid ?? 0} terminal records accepted · ${props.runLifecycle?.evidence.sealedTraces ?? 0} sealed traces retained`
      : lifecycleGaps.length > 0 && props.runLifecycle
        ? `${props.runLifecycle.evidence.sealedTraces} sealed trace${props.runLifecycle.evidence.sealedTraces === 1 ? "" : "s"} retained · evaluation facts incomplete`
        : semanticTrace.some((item) => item.kind === "trace.reconciled")
          ? "recorded and visible"
          : viewer.terminal ? "terminal trace retained; no reconciliation event emitted" : "pending";
  const modelIdentityLabel = pinnedModel || pinnedProvider
    ? `${pinnedProvider ?? "provider unavailable"}${pinnedModel ? ` · ${pinnedModel}` : ""} · pinned run identity`
    : turns.calls.every((call) => call.model && call.provider)
      ? "recorded on every retained call"
      : "not recorded by the run or retained calls";
  // Keep the fallback render-derived. Persisting it in an effect adds a passive
  // state update for every replay page even when the selected call did not
  // change; an explicit click is the only reason to pin a call in state.
  const selectedCall = turns.calls.find((call) => call.id === selectedCallId)
    ?? turns.calls.find((call) => call.id === reconcileCallSelection(turns.calls, selectedCallId, transcriptMode === "focus"));
  const renderedCalls = turns.calls.length <= TRANSCRIPT_CALL_WINDOW ? turns.calls : (() => {
    const recent = turns.calls.slice(-TRANSCRIPT_CALL_WINDOW);
    return selectedCall && !recent.some((call) => call.id === selectedCall.id) ? [selectedCall, ...recent.slice(1)] : recent;
  })();
  const traceGroups = useMemo(() => groupTraceByStep(inspectedItems), [inspectedItems]);
  const visibleGroups = showEarlierGroups || traceGroups.length <= TRACE_GROUP_WINDOW
    ? traceGroups
    : traceGroups.slice(-TRACE_GROUP_WINDOW);
  const hiddenGroupCount = traceGroups.length - visibleGroups.length;
  const retainedFrameDigests = useMemo(
    () => frameEvents.flatMap((event) => {
      const reference = mediaRefFrom(event.payload);
      return reference ? [reference.casDigest] : [];
    }),
    [frameEvents]
  );
  const selectedMediaDigest = viewer.frameMedia?.casDigest;
  useEffect(() => {
    if (!selectedMediaDigest || !props.media) {
      setLoadedFrame(null);
      return;
    }
    const cached = props.media.peek(selectedMediaDigest);
    if (cached) {
      setLoadedFrame({ digest: selectedMediaDigest, dataUrl: cached.dataUrl });
      setFailedMediaDigest(null);
      return;
    }
    let cancelled = false;
    const selectedIndex = Math.max(0, retainedFrameDigests.indexOf(selectedMediaDigest));
    void props.media.warm(retainedFrameDigests, selectedIndex).then((loaded) => {
      if (cancelled) return;
      if (!loaded) {
        setFailedMediaDigest(selectedMediaDigest);
        return;
      }
      setLoadedFrame({ digest: loaded.casDigest, dataUrl: loaded.dataUrl });
      setFailedMediaDigest(null);
    }).catch(() => {
      if (!cancelled) setFailedMediaDigest(selectedMediaDigest);
    });
    return () => { cancelled = true; };
  }, [props.media, retainedFrameDigests, selectedMediaDigest]);
  const directFrameUrl = useMemo(() => {
    if (!viewer.frameUrl || viewer.frameUrl === failedFrameUrl) return undefined;
    try {
      // Frame paths are relative to the stream that emitted them.
      // Without a declared stream base, a relative rollout URL is not an
      // authority: resolving it against tauri://localhost only creates a 404.
      if (!frameBaseUrl && !/^https?:|^data:/i.test(viewer.frameUrl)) return undefined;
      return frameBaseUrl
        ? new URL(viewer.frameUrl, frameBaseUrl).toString()
        : new URL(viewer.frameUrl).toString();
    } catch {
      return undefined;
    }
  }, [viewer.frameUrl, failedFrameUrl, frameBaseUrl]);
  const retainedFrameUrl = loadedFrame?.digest === selectedMediaDigest && selectedMediaDigest != null && failedMediaDigest !== selectedMediaDigest
    ? loadedFrame?.dataUrl
    : undefined;
  const frameUrl = retainedFrameUrl ?? directFrameUrl;
  const retainedFrameLoading = Boolean(selectedMediaDigest && props.media && !retainedFrameUrl && failedMediaDigest !== selectedMediaDigest);
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
  const journalHydrating = optimizerJournalBound && optimizerEvents === undefined && !bindingError && !error;
  // The transport state is the hook's; this only names it for a reader. Every
  // state here is reached deliberately, including the ones that used to be the
  // absence of a state.
  const transportState = bindingError ? "error" : state;
  const connectionState = journalHydrating
    ? "loading durable journal"
    : lifecycleTerminal
    ? lifecycleFailed
      ? `failed${props.runLifecycle?.reason ? ` · ${props.runLifecycle.reason}` : ""}`
      : props.runLifecycle?.status.replaceAll("_", " ") ?? "finished"
    : bindingError
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
    // Replay advances one replay moment (environment step / policy-call
    // boundary / reward) per tick — never one transport delta per tick.
    if (!playing || !moments.length) return;
    const timer = window.setInterval(() => {
      setEvaluationCutoff((current) => {
        const currentIndex = current ?? -1;
        const next = moments.find((index) => index > currentIndex);
        if (next == null || next >= (moments.at(-1) ?? 0)) {
          setPlaying(false);
          return null;
        }
        return next;
      });
    }, 700 / speed);
    return () => window.clearInterval(timer);
  }, [playing, speed, moments]);

  useEffect(() => {
    setSelectedTraceId(null);
  }, [traceMode, selectedLane]);

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
		data-visual-event-source={optimizerEvents ? "optimizer-journal" : declaredStreamCount > 0 ? "declared-stream" : "fixture"}
		data-visual-rollout-count={lanes.length}
		data-visual-rendered-frame-count={frameUrl ? frameEvents.length : 0}
		data-visual-semantic-event-count={semanticTrace.length}
		data-visual-terminal={visualTerminal ? "true" : "false"}
		data-run-evidence-state={props.runLifecycle?.evidence.state}
		data-run-sealed-traces={props.runLifecycle?.evidence.sealedTraces}
		data-visual-error={bindingError ?? error ?? ""}
		data-active-surface={surface}
		data-journal-hydrating={journalHydrating ? "true" : "false"}
	>
      <header className="cv-topbar">
        <div><p className="cv-eyebrow">Live eval · Craftax{scope?.campaign_id ? <> · <Identifier value={scope.campaign_id} label="campaign" max={18} copy={false} /></> : null}</p><h2>{props.title ?? "Policy through time"}</h2>{props.lede ? <p className="cv-lede">{props.lede}</p> : null}</div>
        <div className="cv-connection" role="status"><span className={visualLive ? "live" : !lifecycleFailed && ready ? "ready" : lifecycleFailed ? "failed" : ""} />{connectionState}</div>
      </header>

      <nav className="cv-surfaces" aria-label="Trace viewer surfaces">
        {([ ["replay", "Replay"], ["transcript", "Agent transcript"], ["raw", "Raw trace"], ["metrics", "Metrics"], ["integrity", "Integrity"] ] as const).map(([id, label]) =>
          <button key={id} type="button" aria-current={surface === id ? "page" : undefined} onClick={() => setSurface(id)}>{label}</button>)}
      </nav>

      {journalHydrating ? <section className="cv-hydrating" role="status" aria-live="polite" data-testid="craftax-journal-hydrating">
        <span className="cv-hydrating-mark" aria-hidden="true" />
        <div><p className="cv-eyebrow">Durable replay</p><h3>Loading retained rollout journals…</h3><p>Workshop is rebuilding the visual from persisted optimizer evidence. Counts and replay controls will appear only after the journal is available.</p></div>
      </section> : <>
      <section className="cv-overview cv-surface-replay" aria-label="Overall run summary" data-visual-landmark="run-overview">
        <div className="cv-overview-heading"><div><p className="cv-eyebrow">Overall · all rollouts</p><h3>Evaluation overview</h3></div><span>Combined at the current evaluation cutoff</span></div>
        <div className="cv-overview-grid">
          <OverviewStat label="Rollouts" value={String(runAggregate.rollouts.length || "—")} detail={`${terminalLanes} terminal`} />
          <OverviewStat label="Terminal reward" value={formatMissingNumber(runAggregate.rewardMean)} detail={runAggregate.reportedRewards ? `mean · median ${formatMissingNumber(runAggregate.rewardMedian)} · range ${formatMissingNumber(runAggregate.rewardMin)}–${formatMissingNumber(runAggregate.rewardMax)} · ${runAggregate.reportedRewards}/${runAggregate.rollouts.length} scored` : "No terminal numeric rewards reported"} />
          <OverviewStat label="Environment steps" value={formatMissingNumber(runAggregate.totalSteps, 0)} detail={`${rangeLabel(runAggregate.minSteps, runAggregate.maxSteps, "steps")} · ${runAggregate.reportedSteps}/${runAggregate.rollouts.length} reported`} />
          <OverviewStat label="Provider calls" value={callValue} detail={callDetail} />
          <OverviewStat label="Provider tokens" value={tokenValue == null ? "Not emitted" : `${formatMissingNumber(tokenValue, 0)}${receiptTokens == null ? "" : " billed"}`} detail={tokenDetail} />
          <OverviewStat label="Achievements" value={runAggregate.totalAchievements == null ? "Not emitted" : `${runAggregate.totalAchievements} unlocks`} detail={runAggregate.totalAchievements == null ? `${runAggregate.reportedAchievements}/${runAggregate.rollouts.length} terminal records reported` : `median ${formatMissingNumber(runAggregate.achievementMedian)} · range ${formatMissingNumber(runAggregate.minAchievements, 0)}–${formatMissingNumber(runAggregate.maxAchievements, 0)} · ${runAggregate.achievementNames.length} unique · ${runAggregate.reportedAchievements}/${runAggregate.rollouts.length} reported`} />
        </div>
        <div className="cv-cost-line" data-cost-authority={props.runLifecycle?.usage.costSource}><span>Run cost</span><strong>{runCost.value}</strong><small>{runCost.detail}</small></div>
        {runAggregate.achievementNames.length ? <div className="cv-coverage" aria-label="Achievements unlocked across all rollouts"><span>Across run</span>{runAggregate.achievementNames.map((name) => <i key={name}>{name}</i>)}</div> : null}
        <RunDistributions rollouts={runAggregate.rollouts} selectedLane={selectedLane} onSelect={(lane) => { setChosenLane(lane); setLaneCutoff(null); setSurface("replay"); }} />
      </section>

      {bindingError || error ? <p role="alert" className="cv-error">{bindingError ?? error}</p> : null}
      {props.runLifecycle?.evidence.state === "rejected" ? <section className="cv-evidence-rejected" role="alert" data-testid="craftax-rejected-evidence">
        <strong>Trace evidence was rejected, not missing.</strong>
        <p>{props.runLifecycle.usage.calls == null ? "Provider call count unavailable" : `${props.runLifecycle.usage.calls} provider calls occurred`}; {props.runLifecycle.evidence.rejected} rollout journal{props.runLifecycle.evidence.rejected === 1 ? "" : "s"} failed integrity verification. No rejected event is used for replay or sealing.</p>
        <ul>{props.runLifecycle.evidence.failures.map((failure, index) => <li key={`${failure.rolloutId ?? failure.seed ?? "failure"}:${index}`}><code>{failure.code}</code>{failure.sequence != null ? ` at sequence ${failure.sequence}` : ""}{failure.seed != null ? ` · seed ${failure.seed}` : ""}</li>)}</ul>
      </section> : null}
      {props.runLifecycle && props.runLifecycle.evidence.state !== "rejected" && props.runLifecycle.evidence.sealedTraces > 0 && lifecycleGaps.length > 0 ? <section className="cv-evidence-incomplete" role="status" data-testid="craftax-evaluation-gaps">
        <strong>Trace replay retained; evaluation result incomplete.</strong>
        <p>{props.runLifecycle.evidence.sealedTraces} sealed trace{props.runLifecycle.evidence.sealedTraces === 1 ? " is" : "s are"} available and replayable. {missingRewardFacts || "Some"} rollout{missingRewardFacts === 1 ? " is" : "s are"} missing a numeric reward; {missingStepFacts || "some"} {missingStepFacts === 1 ? "is" : "are"} missing the terminal environment-step fact required by the evaluation contract. These missing facts do not invalidate the sealed replay.</p>
      </section> : null}
      <nav className="cv-lanes cv-surface-replay" aria-label="Rollout lanes">
        {lanes.map((lane) => {
          const summary = laneSummaries.get(lane);
          const terminal = terminalByLane.get(lane);
          return <button key={lane} type="button" aria-current={lane === selectedLane} aria-label={`Select rollout ${lane}`} onClick={() => { setChosenLane(lane); setLaneCutoff(null); }}>
            <span><Identifier value={lane} max={20} copy={false} style={{ fontWeight: 700 }} /><em>{terminal?.status ?? (summary?.terminal ? "done" : lifecycleFailed ? "failed" : "live")}</em></span>
            <small>terminal reward {formatMissingNumber(terminalRollouts ? terminal?.reward : summary?.reward)} · {terminalRollouts ? terminal?.achievements?.length ?? "—" : summary?.achievements ?? 0} achievements</small>
          </button>;
        })}
      </nav>

      <section className="cv-workspace cv-surface-replay" data-visual-landmark="primary-surface">
        <article className="cv-panel cv-game">
          <div className="cv-heading"><div><p className="cv-eyebrow">Selected rollout</p><h3>{selectedLane ? <Identifier value={selectedLane} max={30} style={{ font: "inherit" }} /> : "Waiting for events"}</h3></div><span>{lifecycleFailed ? "failed" : viewer.terminal ? "finished" : visualLive ? "live" : "waiting"}</span></div>
          <div className="cv-frame">
            {frameUrl ? <img src={frameUrl} alt="Craftax gameplay frame" onError={() => retainedFrameUrl ? setFailedMediaDigest(selectedMediaDigest ?? null) : setFailedFrameUrl(viewer.frameUrl ?? null)} /> : retainedFrameLoading ? <p>Loading retained gameplay PNG…</p> : failedMediaDigest === selectedMediaDigest ? <p>Retained gameplay PNG failed integrity-checked media loading. No symbolic frame is substituted.</p> : (failedFrameUrl || viewer.frameUnavailable) ? <p>Gameplay PNG is unavailable. No symbolic frame is substituted for missing image evidence.</p> : viewer.ascii ? <pre aria-label="Craftax symbolic gameplay frame">{viewer.ascii}</pre> : <p>No renderable gameplay frame was emitted at this point in the trace.</p>}
            <div className="cv-frame-caption"><span>step {selectedEnvironmentStep ?? "—"}</span><span>{timeLabel(latest, true)}</span></div>
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
            <div><dt>Provider</dt><dd>{policy.provider ?? props.runLifecycle?.usage.provider ?? "—"}</dd></div>
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
        <div className="cv-heading"><div><p className="cv-eyebrow">Evaluation time</p><h3>{moments.length} {environmentSteps === 0 && lifecycleTerminal ? "run marker" : "replay moment"}{moments.length === 1 ? "" : "s"} · {environmentSteps} environment step{environmentSteps === 1 ? "" : "s"} · {timeLabel(fullProjection.ordered[evaluationIndex], true)}</h3></div><div className="cv-replay"><button onClick={() => setPlaying(!playing)} disabled={!trustworthyReplay}>{playing ? "Pause" : "Play"}</button><select aria-label="Replay speed" value={speed} onChange={(event) => setSpeed(Number(event.currentTarget.value))} disabled={!trustworthyReplay}><option value={0.5}>0.5×</option><option value={1}>1×</option><option value={2}>2×</option><option value={4}>4×</option></select><button onClick={() => { setEvaluationCutoff(null); setLaneCutoff(null); setPlaying(false); }} disabled={lifecycleTerminal && !trustworthyReplay}>{visualLive ? "Follow live" : "Jump to end"}</button></div></div>
        {!trustworthyReplay && lifecycleTerminal ? <p className="cv-control-reason" data-testid="craftax-replay-disabled-reason">Replay unavailable — this run produced 0 trustworthy environment steps{props.runLifecycle?.evidence.state === "rejected" ? " because its evidence was rejected" : ""}.</p> : null}
        <input
          aria-label="Replay evaluation through replay moments"
          type="range"
          min={0}
          max={Math.max(0, moments.length - 1)}
          value={Math.max(0, momentPosition)}
          onChange={(event) => { setEvaluationCutoff(moments[Number(event.currentTarget.value)] ?? null); setPlaying(false); }}
        />
        <div className="cv-lane-timeline"><span>Rollout time (raw events)</span><input aria-label="Replay selected rollout by raw event" type="range" min={0} max={Math.max(0, laneEvents.length - 1)} value={Math.max(0, visibleIndex)} onChange={(event) => setLaneCutoff(Number(event.currentTarget.value))} /></div>
      </section>

      {config.showPlots ? <section className="cv-plots cv-surface-replay" data-visual-landmark="outcome-plots">
        <article className="cv-panel"><div className="cv-heading"><div><p className="cv-eyebrow">Selected rollout</p><h3>Cumulative reward</h3></div><strong>{formatMissingNumber(viewer.cumulativeReward)}</strong></div><svg viewBox="0 0 640 190" role="img" aria-label="Cumulative reward by step"><line x1="28" y1="166" x2="612" y2="166"/><polyline points={sparkline(rewardSeries)} /></svg></article>
        <article className="cv-panel"><div className="cv-heading"><div><p className="cv-eyebrow">Selected rollout</p><h3>Achievements through time</h3></div><strong>{achievements.length}</strong></div><svg viewBox="0 0 640 190" role="img" aria-label="Cumulative achievements by step"><line x1="28" y1="166" x2="612" y2="166"/><polyline className="secondary" points={sparkline(achievementSeries)} /></svg></article>
      </section> : null}

      <section className="cv-panel cv-transcript cv-surface-transcript" data-visual-landmark="agent-transcript">
        <div className="cv-heading"><div><p className="cv-eyebrow">Chronological model calls</p><h3>Agent transcript</h3></div><div className="cv-trace-mode"><button type="button" aria-pressed={transcriptMode === "focus"} onClick={() => setTranscriptMode("focus")}>Focus</button><button type="button" aria-pressed={transcriptMode === "full"} onClick={() => setTranscriptMode("full")}>Full</button><span>{turns.calls.length} calls · selected rollout · cutoff seq {craftaxEventSequence(visibleEvents.at(-1) ?? ({} as LiveEvalEvent), 0)}</span></div></div>
        <div className="cv-step-links" role="navigation" aria-label="Environment step to policy navigation">{semanticTrace.filter((item) => item.kind === "environment.step").slice(-40).map((item) => { const callId = item.step == null ? callForSequence(turns.calls, item.sequenceStart)?.id : turns.callIdByEnvironmentStep.get(item.step); return <button type="button" key={item.id} disabled={!callId} onClick={() => { if (callId) setSelectedCallId(callId); }}>step {item.step ?? "—"}</button>; })}</div>
        <div className="cv-transcript-grid"><ol className="cv-call-list" aria-label="Model calls">{turns.calls.length > renderedCalls.length ? <li className="cv-call-window">Showing {renderedCalls.length} of {turns.calls.length} calls at this cutoff</li> : null}{renderedCalls.map((call) => <li key={call.id}><button type="button" aria-current={call.id === selectedCall?.id} onClick={() => setSelectedCallId(call.id)}><span>Call {call.callNumber}</span><strong>{call.model ?? "Model not recorded"}</strong><small>steps {call.environmentStepStart ?? "—"}{call.environmentStepEnd !== call.environmentStepStart ? `–${call.environmentStepEnd ?? "—"}` : ""} · seq {call.sourceSequenceStart}–{call.sourceSequenceEnd}</small></button></li>)}</ol>
          <article className="cv-call-card" aria-live="polite">{selectedCall ? <><header><div><p className="cv-eyebrow">Call {selectedCall.callNumber} · environment steps {selectedCall.environmentStepStart ?? "—"}–{selectedCall.environmentStepEnd ?? "—"}</p><h4>{selectedCall.model ?? "Model identity not recorded"}</h4></div><span>{selectedCall.outcome?.replaceAll("_", " ") ?? "streaming"}</span></header><dl><div><dt>Provider</dt><dd>{selectedCall.provider ?? props.runLifecycle?.usage.provider ?? "not emitted"}</dd></div><div><dt>Authority</dt><dd>{selectedCall.authority ?? "not emitted"}</dd></div><div><dt>Source</dt><dd>seq {selectedCall.sourceSequenceStart}–{selectedCall.sourceSequenceEnd}</dd></div><div><dt>Closure</dt><dd>{selectedCall.closure ? `${selectedCall.closure.reason.replaceAll("_", " ")} · ${selectedCall.closure.source}` : "pending"}</dd></div><div><dt>Envelopes</dt><dd>{selectedCall.rawEvents.length}</dd></div></dl>
            <Evidence label="Input / observation" field={selectedCall.input}/><Evidence label="Reasoning" field={selectedCall.reasoning}/><Evidence label="Output / actions" field={selectedCall.output}/><Evidence label="Tool calls" field={selectedCall.toolCalls}/><Evidence label="Tool results" field={selectedCall.toolResults}/>
            <details><summary>Raw Trace V5 evidence ({selectedCall.rawEvents.length} envelopes)</summary><pre>{JSON.stringify(selectedCall.rawEvents, null, 2)}</pre></details></> : props.runLifecycle?.evidence.state === "rejected" ? <p>{props.runLifecycle.usage.calls ?? "Provider"} calls occurred, but their journal evidence failed integrity verification and cannot be displayed as a trusted transcript.</p> : <p>No policy.call has been emitted at this temporal cutoff.</p>}</article></div>
      </section>

      {config.showActivity ? <section className="cv-panel cv-activity cv-surface-raw" data-visual-landmark="ordered-activity"><div className="cv-heading"><div><p className="cv-eyebrow">Semantic activity</p><h3>Recent activity</h3></div><span>{semanticTrace.length} events · {visibleEvents.length} raw</span></div><ol>{semanticTrace.slice(-12).reverse().map((item) => <li key={item.id}><time>seq {item.sequenceEnd}</time><strong>{item.category}</strong><span>{item.kind}</span><p>{item.label}</p></li>)}</ol></section> : null}

      {config.showTraceInspector ? <section className="cv-panel cv-trace cv-surface-raw" data-visual-landmark="trace-inspector">
        <div className="cv-heading"><div><p className="cv-eyebrow">Same temporal cutoff</p><h3>Trace V5 viewer</h3></div><div className="cv-trace-mode"><button type="button" aria-pressed={traceMode === "focus"} onClick={() => setTraceMode("focus")}>Policy focus</button><button type="button" aria-pressed={traceMode === "full"} onClick={() => setTraceMode("full")}>Full trace</button><button type="button" onClick={() => setSelectedTraceId(inspectedItems.at(-1)?.id ?? null)} disabled={!inspectedItems.length}>Jump to latest</button><span>{integrityAccepted ? "sealed · accepted" : viewer.terminal ? "terminal trace" : "live · unsealed"}</span></div></div>
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

      <section className="cv-panel cv-surface-metrics cv-facts"><div className="cv-heading"><div><p className="cv-eyebrow">Terminal record + current cutoff</p><h3>Metrics</h3></div></div><dl><div><dt>Selected retained calls</dt><dd>{turns.calls.length}</dd></div><div><dt>Run provider calls</dt><dd>{receiptCalls == null ? "not emitted" : `${formatMissingNumber(receiptCalls, 0)} billed · Workshop receipt`}</dd></div><div><dt>Selected rollout tokens</dt><dd>{selectedRolloutTokens === undefined ? "not emitted" : `${formatMissingNumber(selectedRolloutTokens, 0)}${selectedTerminal?.tokens != null ? " · terminal runtime record" : " · retained calls"}`}</dd></div><div><dt>Run runtime tokens</dt><dd>{runAggregate.totalTokens == null ? "not emitted" : `${formatMissingNumber(runAggregate.totalTokens, 0)} · terminal records`}</dd></div><div><dt>Run provider tokens</dt><dd>{receiptTokens == null ? "not emitted" : `${formatMissingNumber(receiptTokens, 0)} billed · Workshop receipt`}</dd></div><div><dt>Latency</dt><dd>{totalLatencyMs === undefined ? "not emitted" : `${formatMissingNumber(totalLatencyMs, 0)} ms`}</dd></div><div><dt>Run cost</dt><dd>{runCostLabel(props.runLifecycle, totalCostUsd)}</dd></div><div><dt>Terminal reward</dt><dd>{selectedTerminal?.reward == null ? truthNumber(viewer.reward, viewer.terminal, formatMissingNumber) : formatMissingNumber(selectedTerminal.reward)}</dd></div><div><dt>Selected authority</dt><dd>{selectedRolloutAuthority || "not emitted"}</dd></div></dl></section>
      <section className="cv-panel cv-surface-integrity cv-integrity"><div className="cv-heading"><div><p className="cv-eyebrow">Evidence health</p><h3>Integrity</h3></div><span>{props.runLifecycle?.evidence.state === "rejected" ? "rejected" : lifecycleGaps.length > 0 ? "trace sealed · facts incomplete" : integrityAccepted ? "sealed · accepted" : viewer.terminal ? "terminal" : "live · unsealed"}</span></div><ul><li><strong>Reconciliation</strong><span>{reconciliationLabel}</span></li><li><strong>Model identity</strong><span>{modelIdentityLabel}</span></li><li><strong>Repairs / fallbacks</strong><span>{policy.fallback ? "recorded fallback" : "none recorded"}</span></li><li><strong>Malformed calls</strong><span>{turns.missingPolicyEnvelopeCount || "none"}</span></li><li><strong>Reasoning disclosure</strong><span>{turns.calls.some((call) => call.reasoning.state === "visible") ? "provider emitted visible reasoning evidence" : "Thinking not emitted"}</span></li></ul>{props.runLifecycle?.evidence.state === "rejected" ? <p className="cv-control-reason" data-testid="craftax-seal-disabled-reason">Seal unavailable — run failed because {props.runLifecycle.evidence.rejected} rollout journal{props.runLifecycle.evidence.rejected === 1 ? " was" : "s were"} rejected.</p> : lifecycleFailed && props.runLifecycle && props.runLifecycle.evidence.sealedTraces > 0 ? <p className="cv-control-reason" data-testid="craftax-trace-retained-status">Trace replay remains available from {props.runLifecycle.evidence.sealedTraces} sealed trace{props.runLifecycle.evidence.sealedTraces === 1 ? "" : "s"}; the evaluation failure does not reject them.</p> : null}</section>
      </>}

      <footer>live.craftax.v1 · synth.trace-stream-event.v1 · {props.visualMetadata?.qualityGate?.ready ? `ready rev ${props.visualMetadata.qualityGate.revision ?? "—"}` : "draft visual"}</footer>
    </div>
  );
}

export default Shell;

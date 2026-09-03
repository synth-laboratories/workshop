import { type ReactNode, useState } from "react";
import { VisualChrome } from "../../../../../chrome/VisualChrome.tsx";
import { formatMissingNumber, formatMissingUsd } from "../../../../../runtime/liveStream.ts";
import type { VisualBinding } from "../../../../../runtime/types.ts";
import { bindingInputName } from "../../../../../runtime/types.ts";
import { GlobalTimeline, RunHeader } from "./RunChrome.tsx";
import { algorithmLabel } from "./algorithmLabel.ts";
import {
  type OptimizerEvent,
  type OptimizerRun,
  type ProjectedState
} from "./projectEvents.ts";
import { type OptimizerRunViewV2Like } from "./projectRunViewV2.ts";
import { normalizeOptimizerEvents } from "./normalizeEvents.ts";
import {
  useHistoricalCursor,
  type EvidenceHydrationState,
  type EvidenceIntentClient,
  type HistoryClient
} from "./useHistoricalCursor.ts";
import type { RunCollectionsClient } from "./workspace/CollectionBrowser.tsx";

type FixturePayload = {
  run?: OptimizerRun;
  events?: OptimizerEvent[];
  runViewV2?: OptimizerRunViewV2Like;
  runProgress?: RunProgressAgreementLike;
};

type RunProgressAgreementLike = {
  status: string;
  terminal: boolean;
  costUsd: number | null;
  promptTokens: number | null;
  completionTokens: number | null;
  resultHeadline?: string;
  resultAbsentReason?: string;
};

export type FamilyShellProps = {
  templateId: string;
  kicker: string;
  testId: string;
  title?: string;
  lede?: string;
  data?: unknown;
  optimizer_run?: unknown;
  bindings?: VisualBinding[] | { inputs?: VisualBinding[]; slots?: VisualBinding[] };
  /**
   * Events the host injected up front. Projection-first hosts inject none;
   * the historical cursor reads a bounded window on intent instead.
   */
  events?: OptimizerEvent[];
  run?: OptimizerRun;
  runViewV2?: OptimizerRunViewV2Like;
  runProgress?: RunProgressAgreementLike;
  /** Where raw-evidence hydration has got to, from the host. */
  evidenceState?: EvidenceHydrationState;
  /** Lazy, range-addressed journal access. Absent in fixtures. */
  evidence?: EvidenceIntentClient;
  /** Backend checkpointed historical projections. Absent in fixtures. */
  history?: HistoryClient;
  /** Keyset-paged durable collections bound to this run. Absent in fixtures. */
  collections?: RunCollectionsClient;
  /** Durable journal tail when the host knows it. */
  tailCursor?: number;
  loadError?: string;
  showTimeline?: boolean;
  /** "workspace" hides the legacy run header/timeline; children own the chrome. */
  chrome?: "full" | "workspace";
  extraMetrics?: Array<{ label: string; value: string }>;
  children: (ctx: {
    run: OptimizerRun;
    /** The bounded timeline window; never the whole journal by default. */
    events: OptimizerEvent[];
    projected: ProjectedState;
    /** Durable collections client, when the host provides one. */
    collections?: RunCollectionsClient;
    selectedCandidate: string | null;
    setSelectedCandidate: (id: string | null) => void;
    cursor: {
      index: number;
      followLive: boolean;
      terminal: boolean;
      loading: boolean;
      historySource: "backend" | "local" | "none";
      canLoadEarlier: boolean;
      loadEarlier: () => void;
      onScrub: (index: number) => void;
      onFollowLive: () => void;
    };
  }) => ReactNode;
};

function asPayload(raw: unknown): FixturePayload | null {
  if (raw && typeof raw === "object") {
    const obj = raw as Record<string, unknown>;
    if ("events" in obj || "run" in obj) return raw as FixturePayload;
    if ("algorithmId" in obj || "algorithm_id" in obj) {
      return { run: normalizeRun(obj), events: [] };
    }
  }
  return null;
}

function normalizeRun(raw: Record<string, unknown>): OptimizerRun {
  return {
    id: String(raw.id ?? "opt_unknown"),
    algorithmId: String(raw.algorithmId ?? raw.algorithm_id ?? "gepa"),
    status: String(raw.status ?? "unknown"),
    source: raw.source ? String(raw.source) : undefined,
    objective: raw.objective ? String(raw.objective) : undefined,
    cursorSeq: typeof raw.cursorSeq === "number" ? raw.cursorSeq : undefined,
    capabilities: (raw.capabilities as Record<string, boolean> | undefined) ?? undefined,
    summary: (raw.summary as Record<string, unknown> | undefined) ?? undefined,
    usage: (raw.usage as OptimizerRun["usage"]) ?? undefined,
    executionBindings: (raw.executionBindings as Array<Record<string, unknown>>) ??
      (raw.execution_bindings as Array<Record<string, unknown>>) ??
      undefined,
    // Why the run failed. This normalizer rebuilds the run field by field, and
    // omitting `error` here is what made every "Why this run failed" panel
    // inert: GEPA's and SFT/CISPO's both test `run.error`, the host delivers it
    // -- it is in the run's stored payload -- and it was dropped on the way in.
    // A failed Banking77 CISPO run showed a four-item checklist headed "What is
    // still needed" while its own record held "training job failed: POST
    // .../v1/runs: Connection refused".
    error: raw.error ?? undefined
  };
}

export function OptimizerFamilyShell(props: FamilyShellProps) {
  const bindingList = Array.isArray(props.bindings) ? props.bindings : (props.bindings?.inputs ?? props.bindings?.slots);
  const binding = bindingList?.find((b) => bindingInputName(b) === "optimizer_run");
  const hintAlgorithm = (() => {
    const candidate = props.run ?? props.optimizer_run;
    if (candidate && typeof candidate === "object" && "algorithmId" in candidate) {
      return String((candidate as OptimizerRun).algorithmId);
    }
    if (candidate && typeof candidate === "object" && "algorithm_id" in candidate) {
      return String((candidate as { algorithm_id?: string }).algorithm_id);
    }
    return undefined;
  })();
  const payload = asPayload(
    props.data ??
      props.optimizer_run ??
      props.run ??
      binding?.data
  );
  const unresolvedRunId = (() => {
    const raw = props.optimizer_run;
    if (!raw || typeof raw !== "object") return binding?.source;
    const value = raw as Record<string, unknown>;
    return String(value.optimizer_run_id ?? value.id ?? binding?.source ?? "");
  })();
  const run = normalizeRun((props.run ?? payload?.run ?? {
    id: unresolvedRunId || "optimizer run",
    algorithmId: hintAlgorithm ?? "unknown",
    status: "loading"
  }) as Record<string, unknown>);
  const injectedEvents = normalizeOptimizerEvents(
    (props.events ?? payload?.events ?? []) as unknown[]
  );
  const runViewV2 = props.runViewV2 ?? payload?.runViewV2;
  const runProgress = props.runProgress ?? payload?.runProgress;

  const [selectedCandidate, setSelectedCandidate] = useState<string | null>(null);
  const cursor = useHistoricalCursor({
    run,
    injectedEvents,
    runViewV2,
    evidence: props.evidence,
    history: props.history,
    evidenceState: props.evidenceState,
    tailCursor: props.tailCursor ?? run.cursorSeq
  });
  const { followLive, timelineEvents, displayed } = cursor;
  const kicker = run.algorithmId && run.algorithmId !== "unknown"
    ? algorithmLabel(run.algorithmId)
    : props.kicker;

  if (!payload && !props.run) {
    return (
      <VisualChrome
        kicker={kicker}
        live={false}
        title={props.title ?? unresolvedRunId ?? props.templateId}
        lede={props.lede}
        testId={props.testId}
        footer={props.templateId}
      >
        <section className="sv-section" role="status">
          <div className="sv-section-head"><h3>Run data unavailable</h3></div>
          <p className="sv-lede">
            {props.loadError
              ? `The optimizer binding could not be loaded: ${props.loadError}`
              : "Waiting for the bound optimizer record and canonical event stream. No demo data is being shown."}
          </p>
          {unresolvedRunId ? <p className="sv-mono">run · {unresolvedRunId}</p> : null}
        </section>
      </VisualChrome>
    );
  }
  if (!displayed) {
    const historical = !followLive && (cursor.loading || cursor.hydrating);
    return (
      <VisualChrome
        kicker={kicker}
        live={false}
        title={props.title ?? run.id}
        lede={props.lede}
        testId={props.testId}
        footer={props.templateId}
      >
        {historical ? (
          <section className="sv-section" role="status" data-testid="optimizer-history-loading">
            <div className="sv-section-head"><h3>Loading run history</h3></div>
            <p className="sv-lede">The projection at this point is being folded from the durable journal. Live metrics remain current.</p>
            <button type="button" className="sv-button" onClick={cursor.onFollowLive}>Back to live</button>
          </section>
        ) : (
          <section className="sv-section" role="alert" data-testid="optimizer-run-view-v2-unavailable">
            <div className="sv-section-head"><h3>Canonical run view unavailable</h3></div>
            <p className="sv-lede">Live optimizer state requires OptimizerRunViewV2. Raw events are available only after selecting a historical cursor.</p>
          </section>
        )}
      </VisualChrome>
    );
  }

  const summary = displayed.summary;
  const nested = (summary.summary as Record<string, unknown> | undefined) ?? {};
  const bestScore = nested.bestScore ?? summary.bestScore;

  const terminal = followLive && runProgress
    ? runProgress.terminal
    : ["completed", "failed", "canceled", "cancelled", "succeeded"].includes(
        String(summary.status ?? run.status)
      );
  const cursorContext = {
    index: cursor.cursorIndex,
    followLive,
    terminal,
    loading: cursor.loading,
    historySource: cursor.historySource,
    canLoadEarlier: cursor.canLoadEarlier,
    loadEarlier: cursor.loadEarlier,
    onScrub: cursor.onScrub,
    onFollowLive: cursor.onFollowLive
  };

  return (
    <VisualChrome
      kicker={kicker}
      live={followLive && String(summary.status) === "running"}
      title={props.title ?? String(summary.objective ?? run.id)}
      lede={props.lede}
      testId={props.testId}
      footer={props.chrome === "workspace" ? undefined : props.templateId}
      layout={props.chrome === "workspace" ? "workspace" : "document"}
    >
      {props.chrome !== "workspace" ? (
        <RunHeader
          algorithmId={run.algorithmId}
          status={String(summary.status ?? run.status)}
          objective={run.objective}
          metrics={[
            { label: "Cursor", value: String(displayed.cursorSeq) },
            followLive && runProgress
              ? {
                  label: "Result",
                  value: runProgress.resultHeadline ?? runProgress.resultAbsentReason ?? "—"
                }
              : { label: "Best", value: formatMissingNumber(bestScore) },
            { label: "Cost", value: formatMissingUsd(displayed.usage.costUsd) },
            { label: "Source", value: String(run.source ?? "—") },
            ...(props.extraMetrics ?? [])
          ]}
        />
      ) : null}
      {props.chrome !== "workspace" && props.showTimeline !== false ? (
        cursor.hydrating && timelineEvents.length === 0 ? (
          <p className="sv-lede" role="status" data-testid="optimizer-evidence-hydrating">
            Loading run history for time travel. Metrics above are already current.
          </p>
        ) : (
          <GlobalTimeline
            events={(followLive ? displayed.timeline : timelineEvents.map((e) => ({
              sequence: e.sequenceNumber,
              type: e.type,
              occurredAt: e.occurredAt
            }))).map((e) => ({
              sequence: Number(e.sequence),
              type: String(e.type),
              occurredAt: String(e.occurredAt)
            }))}
            cursorIndex={cursor.cursorIndex}
            onScrub={cursor.onScrub}
            followLive={followLive}
            terminal={terminal}
            onFollowLive={cursor.onFollowLive}
          />
        )
      ) : null}
      {!followLive && cursor.historySource === "backend" ? (
        <p className="sv-mono" role="status" data-testid="optimizer-history-source">
          history · backend checkpoint fold{cursor.loading ? " · loading" : ""}
          {cursor.canLoadEarlier ? (
            <>
              {" · "}
              <button type="button" className="sv-link" onClick={cursor.loadEarlier} disabled={cursor.loading}>
                load earlier events
              </button>
            </>
          ) : null}
        </p>
      ) : null}
      {cursor.error ? (
        <p className="sv-lede" role="alert" data-testid="optimizer-history-error">{cursor.error}</p>
      ) : null}
      {props.children({
        run,
        events: timelineEvents,
        projected: displayed,
        collections: props.collections,
        selectedCandidate,
        setSelectedCandidate,
        cursor: cursorContext
      })}
    </VisualChrome>
  );
}

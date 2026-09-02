import { useState } from "react";
import { VisualChrome } from "../../../../chrome/VisualChrome.tsx";
import type { VisualBinding } from "../../../../runtime/types.ts";
import { bindingInputName } from "../../../../runtime/types.ts";
import {
  ArtifactList,
  ExecutionBindings,
  EventLog,
  GlobalTimeline,
  RunHeader,
  UsageCards
} from "./components/RunChrome.tsx";
import {
  type OptimizerEvent,
  type OptimizerRun
} from "./components/projectEvents.ts";
import {
  type OptimizerRunViewV2Like
} from "./components/projectRunViewV2.ts";
import { normalizeOptimizerEvents } from "./components/normalizeEvents.ts";
import {
  useHistoricalCursor,
  type EvidenceHydrationState,
  type EvidenceIntentClient,
  type HistoryClient
} from "./components/useHistoricalCursor.ts";
import { DagOverlay } from "./overlays/dag.tsx";
import { GepaOverlay } from "./overlays/gepa.tsx";
import { GoExOverlay } from "./overlays/go-ex.tsx";
import { SftOverlay } from "./overlays/sft.tsx";

type FixturePayload = {
  run?: OptimizerRun;
  events?: OptimizerEvent[];
  runViewV2?: OptimizerRunViewV2Like;
  runProgress?: RunProgressAgreementLike;
  evidenceState?: EvidenceHydrationState;
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

export type ShellProps = {
  title?: string;
  lede?: string;
  data?: FixturePayload;
  optimizer_run?: FixturePayload | OptimizerRun;
  bindings?: VisualBinding[] | { inputs?: VisualBinding[]; slots?: VisualBinding[] };
  /** Desktop can inject live/reconciled events for an optimizer_run binding. */
  events?: OptimizerEvent[];
  /**
   * Where raw-evidence hydration has got to.
   *
   * The live workspace formats the backend projection and never needs this.
   * Time travel does: an empty `events` array while evidence is still loading
   * is not the same claim as a run that produced nothing, and a scrubber that
   * cannot tell them apart silently offers an empty history as if it were the
   * whole one.
   */
  evidenceState?: EvidenceHydrationState;
  /**
   * Lazy, range-addressed access to the journal. Present in the desktop host;
   * absent in previews and fixtures, where `events` is whatever was injected.
   */
  evidence?: EvidenceIntentClient;
  /** Backend checkpointed historical projections. Present in the desktop host. */
  history?: HistoryClient;
  tailCursor?: number;
  run?: OptimizerRun;
  runViewV2?: OptimizerRunViewV2Like;
  runProgress?: RunProgressAgreementLike;
  loadError?: string;
  visualId?: string;
  revision?: number;
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
      undefined
  };
}

export function Shell(props: ShellProps) {
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
  const evidenceState = props.evidenceState ?? payload?.evidenceState;

  const [selectedCandidate, setSelectedCandidate] = useState<string | null>(null);
  // One evidence-on-intent implementation, shared with the family shell.
  // Nothing is fetched while the user is watching the aggregate.
  const cursor = useHistoricalCursor({
    run,
    injectedEvents,
    runViewV2,
    evidence: props.evidence,
    history: props.history,
    evidenceState,
    tailCursor: props.tailCursor ?? run.cursorSeq
  });
  const { followLive, timelineEvents, displayed } = cursor;

  if (!payload && !props.run) {
    return (
      <VisualChrome
        kicker="Optimizer run"
        live={false}
        title={props.title ?? unresolvedRunId ?? "Optimizer run"}
        lede={props.lede}
        testId="visual-optimizer-run"
        footer="optimizer.run.v1"
      >
        <section className="sv-section" role="status" data-testid="optimizer-run-unavailable">
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
    return (
      <VisualChrome
        kicker="Optimizer run"
        live={false}
        title={props.title ?? run.id}
        lede={props.lede}
        testId="visual-optimizer-run"
        footer="optimizer.run.v1"
      >
        {!followLive && (cursor.loading || cursor.hydrating) ? (
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
  const bestScore = (summary.summary as Record<string, unknown> | undefined)?.bestScore;

  return (
    <VisualChrome
      kicker={`Optimizer · ${run.algorithmId}`}
      live={followLive && String(summary.status) === "running"}
      title={props.title ?? String(summary.objective ?? run.id)}
      lede={props.lede}
      testId="visual-optimizer-run"
      footer="optimizer.run.v1"
    >
      <RunHeader
        algorithmId={run.algorithmId}
        status={String(summary.status ?? run.status)}
        objective={run.objective}
        metrics={[
          { label: "Cursor", value: String(displayed.cursorSeq) },
          runProgress && followLive
            ? {
                label: "Result",
                value: runProgress.resultHeadline ?? runProgress.resultAbsentReason ?? "—"
              }
            : {
                label: "Best",
                value: typeof bestScore === "number" ? bestScore.toFixed(2) : "—"
              },
          { label: "Cost", value: displayed.usage.costUsd == null ? "—" : `$${displayed.usage.costUsd.toFixed(2)}` },
          { label: "Source", value: String(run.source ?? "—") }
        ]}
      />

      {/*
        The aggregate above is authoritative already; only time travel needs the
        journal. While it is still arriving, say so — an empty timeline would
        otherwise read as "this run produced nothing", which is a different and
        wrong claim.
      */}
      {cursor.hydrating && timelineEvents.length === 0 ? (
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
          onFollowLive={cursor.onFollowLive}
        />
      )}
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

      {run.algorithmId === "gepa" ? (
        <GepaOverlay
          state={displayed}
          selectedId={selectedCandidate}
          onSelect={setSelectedCandidate}
          visualId={props.visualId}
          visualRevision={props.revision}
        />
      ) : null}
      {run.algorithmId === "go-ex" ? <GoExOverlay state={displayed} /> : null}
      {run.algorithmId === "sft" ? <SftOverlay state={displayed} /> : null}
      {run.algorithmId === "dag" || run.algorithmId.startsWith("dag.") ? <DagOverlay state={displayed} /> : null}

      <ExecutionBindings bindings={displayed.execution.bindings} />
      <UsageCards usage={displayed.usage} />
      <EventLog entries={displayed.logs} />
      <ArtifactList artifacts={displayed.artifacts} />
    </VisualChrome>
  );
}

export default Shell;

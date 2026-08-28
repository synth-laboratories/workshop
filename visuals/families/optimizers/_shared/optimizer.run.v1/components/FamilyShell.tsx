import { useEffect, useMemo, useState, type ReactNode } from "react";
import { VisualChrome } from "../../../../../chrome/VisualChrome.tsx";
import { formatMissingNumber, formatMissingUsd } from "../../../../../runtime/liveStream.ts";
import type { VisualBinding } from "../../../../../runtime/types.ts";
import { bindingInputName } from "../../../../../runtime/types.ts";
import { GlobalTimeline, RunHeader } from "./RunChrome.tsx";
import { algorithmLabel } from "./algorithmLabel.ts";
import {
  projectAtCursor,
  type OptimizerEvent,
  type OptimizerRun,
  type ProjectedState
} from "./projectEvents.ts";
import {
  projectRunViewV2,
  type OptimizerRunViewV2Like
} from "./projectRunViewV2.ts";
import { normalizeOptimizerEvents } from "./normalizeEvents.ts";

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
  events?: OptimizerEvent[];
  run?: OptimizerRun;
  runViewV2?: OptimizerRunViewV2Like;
  runProgress?: RunProgressAgreementLike;
  loadError?: string;
  showTimeline?: boolean;
  /** "workspace" hides the legacy run header/timeline; children own the chrome. */
  chrome?: "full" | "workspace";
  extraMetrics?: Array<{ label: string; value: string }>;
  children: (ctx: {
    run: OptimizerRun;
    events: OptimizerEvent[];
    projected: ProjectedState;
    selectedCandidate: string | null;
    setSelectedCandidate: (id: string | null) => void;
    cursor: {
      index: number;
      followLive: boolean;
      terminal: boolean;
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
      undefined
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
  const events = normalizeOptimizerEvents(
    (props.events ?? payload?.events ?? []) as unknown[]
  );
  const runViewV2 = props.runViewV2 ?? payload?.runViewV2;
  const runProgress = props.runProgress ?? payload?.runProgress;

  const [followLive, setFollowLive] = useState(true);
  const [cursorIndex, setCursorIndex] = useState(Math.max(0, events.length - 1));
  const [selectedCandidate, setSelectedCandidate] = useState<string | null>(null);

  useEffect(() => {
    if (followLive) setCursorIndex(Math.max(0, events.length - 1));
  }, [events.length, followLive]);

  const atSeq = events[cursorIndex]?.sequenceNumber;
  const projected = useMemo(
    () => (followLive ? null : projectAtCursor(run, events, atSeq)),
    [followLive, run, events, atSeq]
  );
  // The raw reducer is retained only for explicit timeline/time-travel. The
  // live algorithm workspace formats the backend-owned projection directly.
  const displayed = useMemo<ProjectedState | null>(() => {
    if (!followLive) return projected;
    return runViewV2 ? projectRunViewV2(run, runViewV2) : null;
  }, [followLive, projected, run, runViewV2]);
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
    return (
      <VisualChrome
        kicker={kicker}
        live={false}
        title={props.title ?? run.id}
        lede={props.lede}
        testId={props.testId}
        footer={props.templateId}
      >
        <section className="sv-section" role="alert" data-testid="optimizer-run-view-v2-unavailable">
          <div className="sv-section-head"><h3>Canonical run view unavailable</h3></div>
          <p className="sv-lede">Live optimizer state requires OptimizerRunViewV2. Raw events are available only after selecting a historical cursor.</p>
        </section>
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
  const cursor = {
    index: cursorIndex,
    followLive,
    terminal,
    onScrub: (index: number) => {
      setFollowLive(false);
      setCursorIndex(index);
    },
    onFollowLive: () => {
      setFollowLive(true);
      setCursorIndex(Math.max(0, events.length - 1));
    }
  };

  return (
    <VisualChrome
      kicker={kicker}
      live={followLive && String(summary.status) === "running"}
      title={props.title ?? String(summary.objective ?? run.id)}
      lede={props.lede}
      testId={props.testId}
      footer={props.templateId}
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
        <GlobalTimeline
          events={displayed.timeline.map((e) => ({
            sequence: Number(e.sequence),
            type: String(e.type),
            occurredAt: String(e.occurredAt)
          }))}
          cursorIndex={cursorIndex}
          onScrub={cursor.onScrub}
          followLive={followLive}
          terminal={terminal}
          onFollowLive={cursor.onFollowLive}
        />
      ) : null}
      {props.children({
        run,
        events,
        projected: displayed,
        selectedCandidate,
        setSelectedCandidate,
        cursor
      })}
    </VisualChrome>
  );
}

import { useEffect, useMemo, useState } from "react";
import { VisualChrome } from "../../chrome/VisualChrome.tsx";
import type { VisualBinding } from "../../runtime/types.ts";
import {
  ArtifactList,
  ExecutionBindings,
  EventLog,
  GlobalTimeline,
  RunHeader,
  UsageCards
} from "./components/RunChrome.tsx";
import {
  projectAtCursor,
  type OptimizerEvent,
  type OptimizerRun
} from "./components/projectEvents.ts";
import { GepaOverlay } from "./overlays/gepa.tsx";
import { GoExOverlay } from "./overlays/go-ex.tsx";
import { SftOverlay } from "./overlays/sft.tsx";

type FixturePayload = {
  run?: OptimizerRun;
  events?: OptimizerEvent[];
};

export type ShellProps = {
  title?: string;
  lede?: string;
  data?: FixturePayload;
  optimizer_run?: FixturePayload | OptimizerRun;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
  /** Desktop can inject live/reconciled events for an optimizer_run binding. */
  events?: OptimizerEvent[];
  run?: OptimizerRun;
  loadError?: string;
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

function normalizeEvents(events: unknown[]): OptimizerEvent[] {
  return events.map((event) => {
    const e = event as Record<string, unknown>;
    return {
      schemaVersion: e.schemaVersion ? String(e.schemaVersion) : undefined,
      eventId: e.eventId ? String(e.eventId) : undefined,
      type: String(e.type ?? e.event_type ?? "unknown"),
      sequenceNumber: Number(e.sequenceNumber ?? e.sequence_number ?? 0),
      occurredAt: String(e.occurredAt ?? e.occurred_at ?? e.created_at ?? ""),
      optimizerRunId: String(e.optimizerRunId ?? e.optimizer_run_id ?? e.run_id ?? ""),
      algorithmId: String(e.algorithmId ?? e.algorithm_id ?? "unknown"),
      level: e.level ? String(e.level) : undefined,
      item: e.item as OptimizerEvent["item"],
      delta: (e.delta as Record<string, unknown>) ?? {},
      snapshot: e.snapshot as Record<string, unknown> | undefined,
      usageDelta: e.usageDelta as Record<string, number> | undefined ??
        (e.usage_delta as Record<string, number> | undefined),
      artifactRefs: (e.artifactRefs as unknown[]) ?? (e.artifact_refs as unknown[]) ?? [],
      error: e.error,
      raw: e.raw
    };
  });
}

export function Shell(props: ShellProps) {
  const bindingList = Array.isArray(props.bindings) ? props.bindings : props.bindings?.slots;
  const binding = bindingList?.find((b) => b.slot === "optimizer_run");
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
  const events = normalizeEvents(
    (props.events ?? payload?.events ?? []) as unknown[]
  );

  const [followLive, setFollowLive] = useState(true);
  const [cursorIndex, setCursorIndex] = useState(Math.max(0, events.length - 1));
  const [selectedCandidate, setSelectedCandidate] = useState<string | null>(null);

  useEffect(() => {
    if (followLive) setCursorIndex(Math.max(0, events.length - 1));
  }, [events.length, followLive]);

  const atSeq = events[cursorIndex]?.sequenceNumber;
  const projected = useMemo(
    () => projectAtCursor(run, events, atSeq),
    [run, events, atSeq]
  );

  const summary = projected.summary;
  const bestScore = (summary.summary as Record<string, unknown> | undefined)?.bestScore;

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
          { label: "Cursor", value: String(projected.cursorSeq) },
          {
            label: "Best",
            value: typeof bestScore === "number" ? bestScore.toFixed(2) : "—"
          },
          { label: "Cost", value: projected.usage.costUsd == null ? "—" : `$${projected.usage.costUsd.toFixed(2)}` },
          { label: "Source", value: String(run.source ?? "—") }
        ]}
      />

      <GlobalTimeline
        events={projected.timeline.map((e) => ({
          sequence: Number(e.sequence),
          type: String(e.type),
          occurredAt: String(e.occurredAt)
        }))}
        cursorIndex={cursorIndex}
        onScrub={(index) => {
          setFollowLive(false);
          setCursorIndex(index);
        }}
        followLive={followLive}
        onFollowLive={() => {
          setFollowLive(true);
          setCursorIndex(Math.max(0, events.length - 1));
        }}
      />

      {run.algorithmId === "gepa" ? (
        <GepaOverlay
          state={projected}
          selectedId={selectedCandidate}
          onSelect={setSelectedCandidate}
        />
      ) : null}
      {run.algorithmId === "go-ex" ? <GoExOverlay state={projected} /> : null}
      {run.algorithmId === "sft" ? <SftOverlay state={projected} /> : null}

      <ExecutionBindings bindings={projected.execution.bindings} />
      <UsageCards usage={projected.usage} />
      <EventLog entries={projected.logs} />
      <ArtifactList artifacts={projected.artifacts} />
    </VisualChrome>
  );
}

export default Shell;

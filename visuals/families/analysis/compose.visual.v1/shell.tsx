import { useMemo, useState } from "react";
import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../../chrome/useLiveEvalStream.ts";
import { CandidateInspector } from "../../../components/candidate_inspector.v1/CandidateInspector.tsx";
import { DetailModal } from "../../../components/detail_modal.v1/DetailModal.tsx";
import { EventStream } from "../../../components/event_stream.v1/EventStream.tsx";
import { Metrics } from "../../../components/metrics.v1/Metrics.tsx";
import { Scrubber } from "../../../components/scrubber.v1/Scrubber.tsx";
import {
  composeEventStreamSlot,
  composePlacementNeedsOptimizerRun,
  composePlacementNeedsStream,
  parseComposeSpec,
  type ComposePlacement
} from "../../../runtime/composeSpec.ts";
import { optimizerEventsToLiveEval } from "../../../runtime/optimizerCompose.ts";
import type { LiveTemplateProps, TransportState } from "../../../runtime/replayClient.ts";
import type { LiveEvalEvent, VisualBinding } from "../../../runtime/types.ts";

type StreamPayload = {
  events?: LiveEvalEvent[];
};

type OptimizerSlotPayload = {
  events?: unknown[];
  optimizer_run_id?: string;
  run?: { status?: string };
};

export type ShellProps = LiveTemplateProps & {
  title?: string;
  lede?: string;
  spec?: unknown;
  stream?: StreamPayload;
  optimizer_run?: OptimizerSlotPayload;
  /** Host live optimizer payload (`subscribeToRun`), not eval SSE. */
  events?: unknown[];
  run?: { id?: string; algorithmId?: string; status?: string };
  data?: OptimizerSlotPayload & { run?: { status?: string } };
  bindings?: VisualBinding[] | { inputs?: VisualBinding[]; slots?: VisualBinding[] };
};

function asRecord(raw: unknown): Record<string, unknown> | undefined {
  return raw && typeof raw === "object" && !Array.isArray(raw)
    ? (raw as Record<string, unknown>)
    : undefined;
}

function asStream(raw: unknown): StreamPayload {
  const row = asRecord(raw);
  if (!row) return {};
  return { events: Array.isArray(row.events) ? (row.events as LiveEvalEvent[]) : undefined };
}

function boundOptimizer(props: ShellProps): {
  events: unknown[] | undefined;
  live: boolean;
  status?: string;
} {
  const slot = asRecord(props.optimizer_run);
  if (slot && Array.isArray(slot.events)) {
    const run = asRecord(slot.run);
    return {
      events: slot.events,
      live: false,
      status: typeof run?.status === "string" ? run.status : undefined
    };
  }
  const data = asRecord(props.data);
  const dataRun = asRecord(data?.run);
  if (data && Array.isArray(data.events) && (dataRun || slot?.optimizer_run_id)) {
    return {
      events: data.events,
      live: true,
      status: typeof dataRun?.status === "string" ? dataRun.status : props.run?.status
    };
  }
  if (Array.isArray(props.events) && props.run) {
    return { events: props.events, live: true, status: props.run.status };
  }
  return {
    events: undefined,
    live: typeof slot?.optimizer_run_id === "string" && slot.optimizer_run_id.length > 0,
    status: props.run?.status
  };
}

function optimizerTransportState(bound: { live: boolean; status?: string }, eventCount: number): TransportState {
  if (["completed", "failed", "cancelled", "succeeded"].includes(bound.status ?? "")) return "terminal";
  if (bound.live) return eventCount > 0 ? "live" : "idle";
  return eventCount > 0 ? "terminal" : "idle";
}

type Cursor = {
  placementId: string;
  identity: string;
  event: LiveEvalEvent;
  sequence?: number | string | null;
};

export function Shell(props: ShellProps) {
  const parsed = parseComposeSpec(props.spec);
  const stream = asStream(props.stream);
  const declaredStreamCount = props.replay?.streams.length ?? 0;
  const needsStream = parsed.ok && parsed.spec.placements.some(composePlacementNeedsStream);
  const needsOptimizerRun = parsed.ok && parsed.spec.placements.some(composePlacementNeedsOptimizerRun);
  const fixtureEvents = useMemo(
    () => (declaredStreamCount > 0 || !needsStream ? undefined : stream.events),
    [declaredStreamCount, needsStream, stream.events]
  );
  const hasStreamSource = declaredStreamCount > 0 || Boolean(stream.events?.length);
  const { events, state, error } = useLiveEvalStream({
    replay: needsStream ? props.replay : undefined,
    fixtureEvents,
    visualId: props.visualId,
    revision: props.revision
  });
  const optimizerBound = boundOptimizer(props);
  const optimizerMapped = useMemo(
    () => optimizerEventsToLiveEval(optimizerBound.events),
    [optimizerBound.events]
  );
  const [cursor, setCursor] = useState<Cursor | null>(null);

  if (!parsed.ok) {
    return (
      <VisualChrome kicker="Compose" title={props.title ?? "Compose visual"} testId="visual-compose">
        <p role="alert" data-testid="visual-compose-invalid">
          {parsed.error}
        </p>
      </VisualChrome>
    );
  }

  if (needsStream && !hasStreamSource) {
    return (
      <VisualChrome
        kicker="Compose"
        title={props.title ?? parsed.spec.title ?? "Compose visual"}
        testId="visual-compose"
      >
        <p role="alert" data-testid="visual-compose-invalid">
          Placement requires a bound stream input
        </p>
      </VisualChrome>
    );
  }

  const hasOptimizerSource = optimizerBound.events !== undefined || optimizerBound.live;
  if (needsOptimizerRun && !hasOptimizerSource) {
    return (
      <VisualChrome
        kicker="Compose"
        title={props.title ?? parsed.spec.title ?? "Compose visual"}
        testId="visual-compose"
      >
        <p role="alert" data-testid="visual-compose-invalid">
          Placement requires a bound optimizer_run input
        </p>
      </VisualChrome>
    );
  }

  if (needsOptimizerRun && !optimizerMapped.ok) {
    return (
      <VisualChrome
        kicker="Compose"
        title={props.title ?? parsed.spec.title ?? "Compose visual"}
        testId="visual-compose"
      >
        <p role="alert" data-testid="visual-compose-invalid">
          {optimizerMapped.error}
        </p>
      </VisualChrome>
    );
  }

  const optimizerEvents = optimizerMapped.ok ? optimizerMapped.events : [];
  const optimizerState = optimizerTransportState(optimizerBound, optimizerEvents.length);
  const live = (needsStream && state === "live") || (needsOptimizerRun && optimizerState === "live");

  function sourceFor(placement: ComposePlacement): {
    events: LiveEvalEvent[];
    state: TransportState;
    error: string | null;
  } {
    const dialect =
      placement.component === "candidate_inspector.v1"
        ? "optimizer_run"
        : composeEventStreamSlot(placement);
    if (dialect === "optimizer_run") {
      return { events: optimizerEvents, state: optimizerState, error: null };
    }
    return { events, state, error };
  }

  function selectCursor(
    placementId: string,
    event: LiveEvalEvent,
    identity: string,
    sequence?: number | string | null
  ) {
    setCursor({
      placementId,
      identity,
      event,
      sequence: sequence ?? event.sequence ?? null
    });
  }

  return (
    <VisualChrome
      kicker="Compose"
      live={live}
      title={props.title ?? parsed.spec.title ?? "Compose visual"}
      lede={props.lede ?? parsed.spec.lede}
      testId="visual-compose"
      footer="compose.visual.v1"
    >
      {parsed.spec.placements.map((placement) => {
        if (placement.component === "event_stream.v1") {
          const source = sourceFor(placement);
          return (
            <EventStream
              key={placement.id}
              events={source.events}
              state={source.state}
              error={source.error}
              includeKinds={placement.config?.includeKinds}
              cursorId={cursor?.placementId === placement.id ? cursor.identity : null}
              onSelect={(event, identity) => selectCursor(placement.id, event, identity)}
            />
          );
        }
        if (placement.component === "metrics.v1") {
          return <Metrics key={placement.id} events={sourceFor(placement).events} />;
        }
        if (placement.component === "scrubber.v1") {
          const source = sourceFor(placement);
          return (
            <Scrubber
              key={placement.id}
              events={source.events}
              cursorId={cursor?.placementId === placement.id ? cursor.identity : null}
              onSelect={(event, identity, sequence) =>
                selectCursor(placement.id, event, identity, sequence)
              }
            />
          );
        }
        if (placement.component === "candidate_inspector.v1") {
          const source = sourceFor(placement);
          return (
            <CandidateInspector
              key={placement.id}
              events={source.events}
              cursorId={cursor?.placementId === placement.id ? cursor.identity : null}
              onSelect={(event, identity) => selectCursor(placement.id, event, identity)}
            />
          );
        }
        return (
          <DetailModal
            key={placement.id}
            event={cursor && cursor.placementId === placement.from ? cursor.event : null}
            onClose={() => setCursor(null)}
          />
        );
      })}
    </VisualChrome>
  );
}

export default Shell;

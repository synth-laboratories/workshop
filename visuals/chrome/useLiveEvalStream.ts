import { useEffect, useRef, useState } from "react";
import type { LiveEvalEvent } from "../runtime/types.ts";
import { emptyLiveIngest, ingestLiveEnvelopeBatch, type LiveEnvelope } from "../runtime/liveStream.ts";
import type { ReplayClient, TransportState } from "../runtime/replayClient.ts";
import { useLiveEvalStreams, type LiveEvalStreamsView } from "./useLiveEvalStreams.ts";

/** A client with nothing declared. Stable so the hook below keeps one identity. */
const NO_STREAMS: ReplayClient = {
  streams: [],
  poll: async () => {
    throw new Error("no replay transport is declared for this visual");
  }
};

/**
 * One live stream, or a bundled fixture, for templates that show a single
 * rollout.
 *
 * This is a thin adapter over `useLiveEvalStreams`: transport is the host's
 * `ReplayClient`, and the only thing owned here is fixture playback, which is
 * a local authoring aid rather than a transport. Templates no longer read
 * bindings to discover URLs — deriving a transport inside a template is what
 * let a visual declare ten streams and open none of them.
 *
 * See: docs/contracts/visual_replay_transport.md.
 */
export function useLiveEvalStream(options: {
  replay?: ReplayClient;
  fixtureEvents?: LiveEvalEvent[];
  replayMs?: number;
  /** Identity for correlated diagnostics. Absent outside Workshop. */
  visualId?: string | null;
  revision?: number | null;
}): LiveEvalStreamsView {
  const { replay, fixtureEvents, replayMs = 800, visualId, revision } = options;
  const declared = (replay?.streams.length ?? 0) > 0;
  const live = useLiveEvalStreams(declared ? replay! : NO_STREAMS, { visualId, revision });
  const fixture = useFixtureReplay(
    declared ? undefined : fixtureEvents,
    replayMs,
    fixtureReplayIdentity(fixtureEvents, visualId, revision)
  );
  return declared ? live : fixture;
}

function fixtureReplayIdentity(events: LiveEvalEvent[] | undefined, visualId?: string | null, revision?: number | null): string {
  const eventIdentity = (event: LiveEvalEvent | undefined) => event
    ? `${event.kind}:${event.run_id ?? ""}:${event.sequence ?? (event as LiveEvalEvent & { sequence_number?: unknown }).sequence_number ?? ""}`
    : "none";
  return [visualId ?? "fixture", revision ?? "draft", events?.length ?? 0, eventIdentity(events?.[0]), eventIdentity(events?.at(-1))].join(":");
}

/**
 * Replay a bundled fixture on an interval.
 *
 * A finite local fixture is already complete evidence, so it reports `ready`
 * without waiting for a live-only `stream.subscribed` control envelope — and
 * reaches `terminal` when it runs out, rather than resting in a pending state
 * that reads as a stalled connection.
 */
function useFixtureReplay(
  fixtureEvents: LiveEvalEvent[] | undefined,
  replayMs: number,
  fixtureIdentity: string
): LiveEvalStreamsView {
  const [events, setEvents] = useState<LiveEvalEvent[]>([]);
  const [state, setState] = useState<TransportState>(() => fixtureEvents?.length ? "live" : "idle");
  const ingest = useRef(emptyLiveIngest());
  const index = useRef(0);
  const activeIdentity = useRef(fixtureIdentity);
  const fixtureEventsRef = useRef(fixtureEvents);
  fixtureEventsRef.current = fixtureEvents;

  useEffect(() => {
    const replayEvents = fixtureEventsRef.current;
    const identityChanged = activeIdentity.current !== fixtureIdentity;
    activeIdentity.current = fixtureIdentity;
    ingest.current = emptyLiveIngest();
    index.current = 0;
    if (identityChanged) setEvents((current) => current.length ? [] : current);
    if (!replayEvents?.length) {
      if (identityChanged) setState((current) => current === "idle" ? current : "idle");
      return;
    }
    if (identityChanged) setState((current) => current === "live" ? current : "live");
    // Browsers cannot present more than roughly one visual update per frame.
    // Coalesce faster fixture cadences so a dense trace does not force one
    // React render per transport envelope (and trip React's nested-update
    // guard) while preserving the fixture's requested average replay rate.
    const batchSize = Math.max(1, Math.ceil(16 / Math.max(1, replayMs)));
    const timer = window.setInterval(() => {
      const end = Math.min(replayEvents.length, index.current + batchSize);
      const next = replayEvents.slice(index.current, end) as LiveEnvelope[];
      index.current = end;
      ingest.current = ingestLiveEnvelopeBatch(ingest.current, next);
      setEvents(ingest.current.events as LiveEvalEvent[]);
      if (index.current >= replayEvents.length) {
        window.clearInterval(timer);
        setState("terminal");
      }
    }, Math.max(replayMs, replayMs * batchSize));
    return () => window.clearInterval(timer);
  }, [fixtureIdentity, replayMs]);

  return {
    events,
    state,
    closed: state === "terminal" ? 1 : 0,
    ready: Boolean(fixtureEvents?.length),
    recovered: 0,
    error: null
  };
}

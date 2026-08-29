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
  // Binding resolution may reconstruct an equivalent inline array while its
  // visual revision is unchanged. Array identity is therefore not fixture
  // identity; using it as an effect dependency recursively resets playback.
  const fixtureKey = declared || !fixtureEvents
    ? ""
    : `${visualId ?? "preview"}:${revision ?? "draft"}:${JSON.stringify(fixtureEvents)}`;
  const fixture = useFixtureReplay(declared ? undefined : fixtureEvents, replayMs, fixtureKey);
  return declared ? live : fixture;
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
  fixtureKey: string
): LiveEvalStreamsView {
  const [events, setEvents] = useState<LiveEvalEvent[]>([]);
  const [state, setState] = useState<TransportState>("idle");
  const ingest = useRef(emptyLiveIngest());
  const index = useRef(0);

  useEffect(() => {
    ingest.current = emptyLiveIngest();
    index.current = 0;
    setEvents([]);
    if (!fixtureEvents?.length) {
      setState("idle");
      return;
    }
    setState("live");
    const frameMs = 16;
    const rowsPerTick = Math.max(1, Math.ceil(frameMs / Math.max(1, replayMs)));
    const timer = window.setInterval(() => {
      if (index.current >= fixtureEvents.length) {
        window.clearInterval(timer);
        setState("terminal");
        return;
      }
      const end = Math.min(fixtureEvents.length, index.current + rowsPerTick);
      const next = fixtureEvents.slice(index.current, end) as LiveEnvelope[];
      index.current = end;
      ingest.current = ingestLiveEnvelopeBatch(ingest.current, next);
      setEvents(ingest.current.events as LiveEvalEvent[]);
    }, Math.max(frameMs, replayMs));
    return () => window.clearInterval(timer);
  // `fixtureKey` is the immutable content/revision identity. See caller.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fixtureKey, replayMs]);

  return {
    events,
    state,
    closed: state === "terminal" ? 1 : 0,
    ready: Boolean(fixtureEvents?.length),
    recovered: 0,
    error: null
  };
}

import { useEffect, useRef, useState } from "react";
import type { LiveEvalEvent } from "../runtime/types.ts";
import { reportVisualDiagnostic, VISUAL_STREAM_CODES } from "../runtime/diagnostics.ts";
import { emptyLiveIngest, ingestLiveEnvelopeBatch } from "../runtime/liveStream.ts";
import {
  REPLAY_FIRST_RESPONSE_TIMEOUT_MS,
  REPLAY_PAGE_LIMIT,
  REPLAY_PAGE_LIMIT_MAX,
  type ReplayClient,
  type TransportState
} from "../runtime/replayClient.ts";

export type LiveEvalStreamsView = {
  events: LiveEvalEvent[];
  state: TransportState;
  /** Streams that have reported closed, out of `client.streams.length`. */
  closed: number;
  ready: boolean;
  recovered: number;
  error: string | null;
};

const POLL_INTERVAL_MS = 500;

/**
 * Fold every declared rollout stream into one viewer from its durable poll
 * authority.
 *
 * A live visual has one semantic `stream` slot, but an eval can bind several
 * rollout-local authorities to it. Polling each one means a terminal
 * EventSource close is not data loss and a completed evaluation reopens without
 * converting Trace V5 into a different input schema.
 *
 * The state it reports is a state machine, not a ladder of derived strings.
 * That matters more than it sounds: the pane it replaces could rest forever at
 * `connecting` with streams declared and no poll ever issued, and nothing in
 * the type said that was impossible. Here it is impossible — `declared` without
 * a first response inside `REPLAY_FIRST_RESPONSE_TIMEOUT_MS` becomes `error`.
 *
 * See: docs/contracts/visual_replay_transport.md.
 */
export function useLiveEvalStreams(
  client: ReplayClient,
  identity: { visualId?: string | null; revision?: number | null } = {}
): LiveEvalStreamsView {
  const [events, setEvents] = useState<LiveEvalEvent[]>([]);
  const [state, setState] = useState<TransportState>("idle");
  const [closed, setClosed] = useState(0);
  const [ready, setReady] = useState(false);
  const [recovered, setRecovered] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const ingest = useRef(emptyLiveIngest());
  const clientRef = useRef(client);
  clientRef.current = client;
  const streamKey = client.streams.map((stream) => stream.streamId).join("\n");
  const { visualId, revision } = identity;

  useEffect(() => {
    ingest.current = emptyLiveIngest();
    setEvents([]);
    setReady(false);
    setRecovered(0);
    setError(null);
    setClosed(0);

    const streams = clientRef.current.streams;
    if (streams.length === 0) {
      setState("idle");
      return;
    }
    setState("declared");

    let stopped = false;
    let answered = false;
    let timer: number | undefined;
    const cursors = new Map(streams.map((stream) => [stream.streamId, 0]));
    const closedStreams = new Set<string>();

    const fail = (message: string, code: string) => {
      if (stopped) return;
      setError(message);
      setState("error");
      reportVisualDiagnostic({
        severity: "error",
        event: "stream.replay.failed",
        code,
        message,
        retryable: true,
        visualId,
        details: { revision, declaredStreams: streams.length, closed: closedStreams.size }
      });
    };

    // A declared stream that never answers is the failure this hook exists to
    // make impossible to sit in quietly.
    const deadline = window.setTimeout(() => {
      if (stopped || answered) return;
      fail(
        `No declared stream answered within ${Math.round(REPLAY_FIRST_RESPONSE_TIMEOUT_MS / 1000)}s (${streams.length} declared)`,
        VISUAL_STREAM_CODES.streamSubscribeTimeout
      );
    }, REPLAY_FIRST_RESPONSE_TIMEOUT_MS);

    const publish = (rows: Parameters<typeof ingestLiveEnvelopeBatch>[1]) => {
      const before = ingest.current.events.length;
      ingest.current = ingestLiveEnvelopeBatch(ingest.current, rows);
      setEvents(ingest.current.events as LiveEvalEvent[]);
      setReady(ingest.current.ready);
      setRecovered((value) => value + Math.max(0, ingest.current.events.length - before));
      // Conflicts are the one defect a renderer can act on: two bodies for one
      // identity means the pane is showing one of them and cannot say which.
      // Sequence gaps are not reported here — the host observes them at the
      // poll seam and emits `STREAM_REPLAY_GAP` with the visual, the revision
      // and both bracketing sequences, which is evidence rather than a
      // sentence, and the readiness gate reads that. See `stream_fold.rs`.
      setError(
        ingest.current.conflicts.length
          ? ingest.current.conflicts.at(-1) ?? "Conflicting replay envelope"
          : null
      );
    };

    const pollOne = async (stream: (typeof streams)[number]) => {
      let after = cursors.get(stream.streamId) ?? 0;
      for (let pageNumber = 0; pageNumber < REPLAY_PAGE_LIMIT_MAX; pageNumber++) {
        const page = await clientRef.current.poll(stream, after, REPLAY_PAGE_LIMIT);
        answered = true;
        publish(page.events);
        const { next, hasMore, closed: streamClosed } = page.cursor;
        if (next < after) {
          throw new Error(`replay cursor regressed from ${after} to ${next} on ${stream.streamId}`);
        }
        cursors.set(stream.streamId, next);
        if (streamClosed) {
          closedStreams.add(stream.streamId);
          return;
        }
        if (!hasMore) return;
        if (next === after) {
          throw new Error(`replay made no progress after sequence ${after} on ${stream.streamId}`);
        }
        after = next;
      }
      throw new Error(`replay exceeded ${REPLAY_PAGE_LIMIT_MAX} pages on ${stream.streamId}`);
    };

    const pollAll = async () => {
      if (stopped) return;
      if (!answered) setState("replaying");
      try {
        await Promise.all(
          streams.filter((stream) => !closedStreams.has(stream.streamId)).map(pollOne)
        );
        if (stopped) return;
        setClosed(closedStreams.size);
        const allClosed = closedStreams.size === streams.length;
        setState(allClosed ? "terminal" : "live");
        if (!allClosed) timer = window.setTimeout(() => void pollAll(), POLL_INTERVAL_MS);
      } catch (reason) {
        fail(reason instanceof Error ? reason.message : "replay error", VISUAL_STREAM_CODES.streamInterrupted);
      }
    };

    void pollAll();
    return () => {
      stopped = true;
      window.clearTimeout(deadline);
      if (timer != null) window.clearTimeout(timer);
    };
    // streamKey is the stable descriptor identity; hosts rebuild the array.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [streamKey, visualId, revision]);

  return { events, state, closed, ready, recovered, error };
}

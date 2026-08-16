import { useEffect, useRef, useState } from "react";
import type { LiveEvalEvent } from "../runtime/types.ts";
import {
  emptyLiveIngest,
  ingestLiveEnvelopeBatch,
  type LiveEnvelope
} from "../runtime/liveStream.ts";

export type DeclaredLiveStream = {
  sseUrl?: string;
  pollUrl: string;
};

/**
 * Fold several declared rollout streams into one viewer from their durable
 * poll authorities. A live visual has one semantic `stream` slot, but an eval
 * can bind several rollout-local descriptors to that slot. Polling each
 * descriptor avoids treating a terminal EventSource close as data loss and
 * makes completed evaluations reopenable without converting Trace V5 into a
 * different visual input schema.
 */
export function useLiveEvalStreams(streams: DeclaredLiveStream[]): {
  events: LiveEvalEvent[];
  live: boolean;
  ready: boolean;
  recovering: boolean;
  recovered: number;
  error: string | null;
} {
  const [events, setEvents] = useState<LiveEvalEvent[]>([]);
  const [live, setLive] = useState(false);
  const [ready, setReady] = useState(false);
  const [recovering, setRecovering] = useState(false);
  const [recovered, setRecovered] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const ingest = useRef(emptyLiveIngest());
  const streamKey = streams.map((stream) => `${stream.sseUrl ?? ""}\n${stream.pollUrl}`).join("\n\n");

  useEffect(() => {
    ingest.current = emptyLiveIngest();
    setEvents([]);
    setReady(false);
    setRecovered(0);
    setError(null);
    if (streams.length === 0) {
      setLive(false);
      setRecovering(false);
      return;
    }

    const abort = new AbortController();
    let timer: number | undefined;
    let stopped = false;
    const cursors = new Map(streams.map((stream) => [stream.pollUrl, 0]));
    const closed = new Set<string>();

    const publish = (rows: LiveEnvelope[]) => {
      const before = ingest.current.events.length;
      ingest.current = ingestLiveEnvelopeBatch(ingest.current, rows);
      setEvents(ingest.current.events as LiveEvalEvent[]);
      setReady(ingest.current.ready);
      setRecovered((value) => value + Math.max(0, ingest.current.events.length - before));
      if (ingest.current.conflicts.length) setError(ingest.current.conflicts.at(-1) ?? "Conflicting replay envelope");
      else if (ingest.current.gaps.length) setError(`Evidence gap after sequence ${ingest.current.gaps.at(-1)?.after}`);
      else setError(null);
    };

    const pollOne = async (stream: DeclaredLiveStream) => {
      let after = cursors.get(stream.pollUrl) ?? 0;
      for (let pageNumber = 0; pageNumber < 1000; pageNumber++) {
        const url = new URL(stream.pollUrl, stream.sseUrl);
        url.searchParams.set("after", String(after));
        url.searchParams.set("limit", "500");
        const response = await fetch(url, { signal: abort.signal, headers: { Accept: "application/json" } });
        if (!response.ok) throw new Error(`poll recovery HTTP ${response.status} for ${stream.pollUrl}`);
        const body = await response.json() as {
          events?: LiveEnvelope[];
          page?: { events?: LiveEnvelope[] };
          cursor?: { next?: number; high_water?: number; has_more?: boolean; closed?: boolean };
        } | LiveEnvelope[];
        const rows = Array.isArray(body) ? body : body.page?.events ?? body.events ?? [];
        publish(rows);
        if (Array.isArray(body)) break;
        const cursor = body.cursor;
        const sequences = rows.map((row) => Number(row.sequence_number ?? row.sequence)).filter(Number.isFinite);
        const next = cursor?.next ?? (sequences.length ? Math.max(...sequences) : after);
        const highWater = cursor?.high_water;
        const hasMore = cursor?.has_more ?? (highWater != null && next < highWater);
        if (next < after) throw new Error(`poll recovery cursor regressed from ${after} to ${next}`);
        cursors.set(stream.pollUrl, next);
        if (cursor?.closed) {
          closed.add(stream.pollUrl);
          break;
        }
        if (!hasMore) break;
        if (next === after) throw new Error(`poll recovery made no progress after sequence ${after}`);
        after = next;
        if (pageNumber === 999) throw new Error("poll recovery exceeded 1000 pages");
      }
    };

    const pollAll = async () => {
      if (stopped) return;
      setRecovering(true);
      try {
        await Promise.all(streams.filter((stream) => !closed.has(stream.pollUrl)).map(pollOne));
        const allClosed = closed.size === streams.length;
        setLive(!allClosed);
        if (!allClosed && !stopped) timer = window.setTimeout(() => void pollAll(), 500);
      } catch (reason) {
        if (!abort.signal.aborted) {
          setError(reason instanceof Error ? reason.message : "poll recovery error");
          setLive(false);
        }
      } finally {
        setRecovering(false);
      }
    };

    setLive(true);
    void pollAll();
    return () => {
      stopped = true;
      if (timer != null) window.clearTimeout(timer);
      abort.abort();
      setLive(false);
    };
  // streamKey is the stable descriptor identity; callers often rebuild arrays.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [streamKey]);

  return { events, live, ready, recovering, recovered, error };
}

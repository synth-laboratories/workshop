import { useEffect, useRef, useState } from "react";
import type { LiveEvalEvent } from "../runtime/types.ts";
import {
  emptyLiveIngest,
  ingestLiveEnvelope,
  ingestLiveEnvelopeBatch,
  type LiveEnvelope
} from "../runtime/liveStream.ts";

/**
 * Replay fixture events on an interval, or attach a real EventSource when sseUrl is set.
 * Control records (`stream.subscribed`, heartbeats) set ready but are not evidence.
 * Duplicate identities are dropped (persist-before-publish / reconnect).
 */
export function useLiveEvalStream(options: {
  sseUrl?: string;
  pollUrl?: string;
  fixtureEvents?: LiveEvalEvent[];
  replayMs?: number;
}): { events: LiveEvalEvent[]; live: boolean; ready: boolean; recovering: boolean; recovered: number; error: string | null } {
  const { sseUrl, pollUrl, fixtureEvents, replayMs = 800 } = options;
  const [events, setEvents] = useState<LiveEvalEvent[]>([]);
  const [live, setLive] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [recovering, setRecovering] = useState(false);
  const [recovered, setRecovered] = useState(0);
  const idx = useRef(0);
  const ingest = useRef(emptyLiveIngest());

  useEffect(() => {
    ingest.current = emptyLiveIngest();
    setEvents([]);
    setError(null);
    setReady(false);
    setRecovering(false);
    setRecovered(0);
    idx.current = 0;

    let frame: number | null = null;
    const pending: LiveEnvelope[] = [];
    const publish = () => {
      frame = null;
      if (pending.length) {
        ingest.current = ingestLiveEnvelopeBatch(ingest.current, pending.splice(0));
      }
      setReady(ingest.current.ready);
      setEvents(ingest.current.events as LiveEvalEvent[]);
    };
    const push = (parsed: LiveEnvelope) => {
      pending.push(parsed);
      if (frame == null) frame = window.requestAnimationFrame(publish);
    };
    const pushBatch = (rows: LiveEnvelope[]) => {
      if (frame != null) {
        window.cancelAnimationFrame(frame);
        frame = null;
      }
      if (pending.length) rows = [...pending.splice(0), ...rows];
      ingest.current = ingestLiveEnvelopeBatch(ingest.current, rows);
      publish();
    };

    if (sseUrl && typeof EventSource !== "undefined") {
      setLive(true);
      let es: EventSource | undefined;
      const abort = new AbortController();
      let recovery: Promise<void> | null = null;
      const backfill = async () => {
        if (!pollUrl || recovery || abort.signal.aborted) return recovery;
        recovery = (async () => {
          setRecovering(true);
          try {
            const before = ingest.current.events.length;
            let after = [...ingest.current.lastSequenceByScope.values()].reduce((max, value) => Math.max(max, value), 0);
            for (let pageNumber = 0; pageNumber < 1000; pageNumber++) {
              const url = new URL(pollUrl, sseUrl);
              url.searchParams.set("after", String(after));
              url.searchParams.set("limit", "500");
              const response = await fetch(url, { signal: abort.signal, headers: { Accept: "application/json" } });
              if (!response.ok) throw new Error(`poll recovery HTTP ${response.status}`);
              const body = await response.json() as {
                events?: LiveEnvelope[];
                page?: { events?: LiveEnvelope[] };
                cursor?: { next?: number; high_water?: number; has_more?: boolean; closed?: boolean };
              } | LiveEnvelope[];
              const rows = Array.isArray(body) ? body : body.page?.events ?? body.events ?? [];
              pushBatch(rows);
              if (Array.isArray(body)) break;
              const cursor = body.cursor;
              const evidenceSequences = rows.map((row) => Number(row.sequence_number ?? row.sequence)).filter(Number.isFinite);
              const next = cursor?.next ?? (evidenceSequences.length ? Math.max(...evidenceSequences) : after);
              const highWater = cursor?.high_water;
              const hasMore = cursor?.has_more ?? (highWater != null && next < highWater);
              if (next < after) throw new Error(`poll recovery cursor regressed from ${after} to ${next}`);
              if (!hasMore) break;
              if (next === after) throw new Error(`poll recovery made no progress after sequence ${after}`);
              after = next;
              if (pageNumber === 999) throw new Error("poll recovery exceeded 1000 pages");
            }
            setRecovered((value) => value + Math.max(0, ingest.current.events.length - before));
            if (ingest.current.conflicts.length) setError(ingest.current.conflicts.at(-1) ?? "Conflicting replay envelope");
            else if (ingest.current.gaps.length) setError(`Evidence gap after sequence ${ingest.current.gaps.at(-1)?.after}`);
            else setError(null);
          } catch (e) {
            if (!abort.signal.aborted) setError(e instanceof Error ? e.message : "poll recovery error");
          } finally {
            setRecovering(false);
            recovery = null;
          }
        })();
        return recovery;
      };
      const receive = (msg: MessageEvent<string>) => {
        try {
          const parsed = JSON.parse(msg.data) as LiveEnvelope;
          push(parsed);
          const kind = String(parsed.kind ?? parsed.type ?? "");
          if (kind === "run_finished" || kind === "eval.stream.terminal" || kind === "eval.run.terminal") {
            setLive(false);
            es?.close();
            abort.abort();
          }
        } catch (e) {
          setError(e instanceof Error ? e.message : "SSE parse error");
        }
      };
      try {
        es = new EventSource(sseUrl);
        es.onmessage = receive;
        for (const kind of ["snapshot", "eval.run.terminal", "rollout.progress", "rollout.frame", "stream.subscribed"]) {
          es.addEventListener(kind, receive as EventListener);
        }
        es.onerror = () => {
          if (es?.readyState !== EventSource.CLOSED) {
            setError("SSE connection interrupted");
            void backfill();
          }
        };
      } catch {
        // WKWebView rejects EventSource from the tauri origin to loopback HTTP.
        // Streaming fetch is allowed by the same CORS policy and preserves the
        // standard SSE wire format, including named events.
        void (async () => {
          try {
            const response = await fetch(sseUrl, { signal: abort.signal, headers: { Accept: "text/event-stream" } });
            if (!response.ok || !response.body) throw new Error(`SSE HTTP ${response.status}`);
            const reader = response.body.getReader();
            const decoder = new TextDecoder();
            let buffer = "";
            while (!abort.signal.aborted) {
              const { value, done } = await reader.read();
              if (done) break;
              buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, "\n");
              let boundary = buffer.indexOf("\n\n");
              while (boundary >= 0) {
                const block = buffer.slice(0, boundary);
                buffer = buffer.slice(boundary + 2);
                const data = block.split("\n").filter((line) => line.startsWith("data:"))
                  .map((line) => line.slice(5).trimStart()).join("\n");
                if (data) receive(new MessageEvent("message", { data }));
                boundary = buffer.indexOf("\n\n");
              }
            }
          } catch (e) {
            if (!abort.signal.aborted) {
              setError(e instanceof Error ? e.message : "SSE connection error");
              await backfill();
            }
          }
        })();
      }
      void backfill();
      return () => {
        if (frame != null) window.cancelAnimationFrame(frame);
        es?.close();
        abort.abort();
        setLive(false);
      };
    }

    if (fixtureEvents?.length) {
      // A finite fixture is already available locally. It must not depend on a
      // live-only `stream.subscribed` control envelope to leave "connecting".
      setReady(true);
      setLive(true);
      const id = window.setInterval(() => {
        if (idx.current >= fixtureEvents.length) {
          window.clearInterval(id);
          setLive(false);
          return;
        }
        const next = fixtureEvents[idx.current++];
        push(next as LiveEnvelope);
      }, replayMs);
      return () => {
        if (frame != null) window.cancelAnimationFrame(frame);
        window.clearInterval(id);
        setLive(false);
      };
    }

    return undefined;
  }, [sseUrl, pollUrl, fixtureEvents, replayMs]);

  return { events, live, ready, recovering, recovered, error };
}

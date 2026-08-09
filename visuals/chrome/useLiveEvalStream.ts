import { useEffect, useRef, useState } from "react";
import type { LiveEvalEvent } from "../runtime/types.ts";

/**
 * Replay fixture events on an interval, or attach a real EventSource when sseUrl is set.
 * Desktop injects live SSE; agents can pass fixture events for offline demos.
 */
export function useLiveEvalStream(options: {
  sseUrl?: string;
  fixtureEvents?: LiveEvalEvent[];
  replayMs?: number;
}): { events: LiveEvalEvent[]; live: boolean; error: string | null } {
  const { sseUrl, fixtureEvents, replayMs = 800 } = options;
  const [events, setEvents] = useState<LiveEvalEvent[]>([]);
  const [live, setLive] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const idx = useRef(0);

  useEffect(() => {
    setEvents([]);
    setError(null);
    idx.current = 0;

    if (sseUrl && typeof EventSource !== "undefined") {
      setLive(true);
      const es = new EventSource(sseUrl);
      es.onmessage = (msg) => {
        try {
          const parsed = JSON.parse(msg.data) as LiveEvalEvent;
          setEvents((prev) => [...prev, parsed]);
          if (
            parsed.kind === "run_finished" ||
            parsed.kind === "eval.stream.terminal"
          ) {
            setLive(false);
            es.close();
          }
        } catch (e) {
          setError(e instanceof Error ? e.message : "SSE parse error");
        }
      };
      es.onerror = () => {
        // EventSource reports an error after a server cleanly closes. Preserve
        // the completed visual instead of replacing it with a false failure.
        if (es.readyState !== EventSource.CLOSED) {
          setError("SSE connection error");
        }
      };
      return () => {
        es.close();
        setLive(false);
      };
    }

    if (fixtureEvents?.length) {
      setLive(true);
      const id = window.setInterval(() => {
        if (idx.current >= fixtureEvents.length) {
          window.clearInterval(id);
          setLive(false);
          return;
        }
        const next = fixtureEvents[idx.current++];
        setEvents((prev) => [...prev, next]);
      }, replayMs);
      return () => {
        window.clearInterval(id);
        setLive(false);
      };
    }

    return undefined;
  }, [sseUrl, fixtureEvents, replayMs]);

  return { events, live, error };
}

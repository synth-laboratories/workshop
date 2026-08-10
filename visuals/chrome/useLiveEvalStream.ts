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
      let es: EventSource | undefined;
      const abort = new AbortController();
      const receive = (msg: MessageEvent<string>) => {
        try {
          const parsed = JSON.parse(msg.data) as LiveEvalEvent;
          setEvents((prev) => [...prev, parsed]);
          if (
            parsed.kind === "run_finished" ||
            parsed.kind === "eval.stream.terminal" || parsed.kind === "eval.run.terminal"
          ) {
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
        for (const kind of ["snapshot", "eval.run.terminal", "rollout.progress", "rollout.frame"]) {
          es.addEventListener(kind, receive as EventListener);
        }
        es.onerror = () => {
          if (es?.readyState !== EventSource.CLOSED) setError("SSE connection error");
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
            if (!abort.signal.aborted) setError(e instanceof Error ? e.message : "SSE connection error");
          }
        })();
      }
      return () => {
        es?.close();
        abort.abort();
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

import { useEffect, useMemo, useRef, useState } from "react";
import type { LiveEvalEvent } from "../runtime/types.ts";
import { reportVisualDiagnostic, VISUAL_STREAM_CODES } from "../runtime/diagnostics.ts";
import { emptyLiveIngest, ingestLiveEnvelopeBatch } from "../runtime/liveStream.ts";
import type { HostLiveEvidence } from "../runtime/hostLiveEvidence.ts";
import { REPLAY_FIRST_RESPONSE_TIMEOUT_MS, type ReplayClient, type TransportState } from "../runtime/replayClient.ts";

export type LiveEvalStreamsView = {
  events: LiveEvalEvent[];
  state: TransportState;
  closed: number;
  ready: boolean;
  recovered: number;
  error: string | null;
  hostEvidence?: HostLiveEvidence;
};

const pixelCutPending = () => document.documentElement.hasAttribute("data-synth-review-capture")
  || document.documentElement.hasAttribute("data-synth-capture");
type CapturePrefix = { events: LiveEvalEvent[]; ready: boolean; state: TransportState; closed: number };
const capturePrefixes = new Map<string, CapturePrefix>();
const CAPTURE_PREFIX_LIMIT = 8;
function retainCapturePrefix(key: string | null, value: CapturePrefix) {
  if (!key) return;
  capturePrefixes.delete(key);
  capturePrefixes.set(key, value);
  while (capturePrefixes.size > CAPTURE_PREFIX_LIMIT) capturePrefixes.delete(capturePrefixes.keys().next().value!);
}

/** Fold every declared stream from its SSE authority. There is deliberately no
 * timer and no poll fallback: producers replay their durable prefix on connect,
 * then EventSource repairs disconnects with Last-Event-ID. */
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
  const streamKey = JSON.stringify(client.streams.map(({ streamId, sseUrl }) => [streamId, sseUrl]));
  const { visualId, revision } = identity;
  const captureKey = visualId && revision != null ? JSON.stringify([visualId, revision, streamKey]) : null;

  useEffect(() => {
    ingest.current = emptyLiveIngest();
    setEvents([]);
    setReady(false);
    setRecovered(0);
    setError(null);
    setClosed(0);
    const streams = clientRef.current.streams;
    if (streams.length === 0) { setState("idle"); return; }

    const prefix = pixelCutPending() && captureKey ? capturePrefixes.get(captureKey) : undefined;
    if (prefix) {
      ingest.current = ingestLiveEnvelopeBatch(emptyLiveIngest(), prefix.events);
      setEvents(prefix.events); setReady(prefix.ready); setRecovered(prefix.events.length);
      setClosed(prefix.closed); setState(prefix.state); return;
    }

    setState("declared");
    let stopped = false;
    let answered = false;
    const closedStreams = new Set<string>();
    const unsubscribers: Array<() => void> = [];
    const deadline = window.setTimeout(() => {
      if (stopped || answered) return;
      const message = `No declared SSE stream answered within ${Math.round(REPLAY_FIRST_RESPONSE_TIMEOUT_MS / 1000)}s (${streams.length} declared)`;
      setError(message); setState("error");
      reportVisualDiagnostic({ severity: "error", event: "stream.subscribe.failed",
        code: VISUAL_STREAM_CODES.streamSubscribeTimeout, message, retryable: true, visualId,
        details: { revision, declaredStreams: streams.length } });
    }, REPLAY_FIRST_RESPONSE_TIMEOUT_MS);

    const publish = (row: LiveEvalEvent) => {
      if (stopped || row.kind === "stream.closed") return;
      answered = true; window.clearTimeout(deadline);
      const before = ingest.current.events.length;
      ingest.current = ingestLiveEnvelopeBatch(ingest.current, [row]);
      const next = ingest.current.events as LiveEvalEvent[];
      setEvents(next); setReady(ingest.current.ready);
      setRecovered((value) => value + Math.max(0, next.length - before));
      const problem = ingest.current.conflicts.at(-1)
        ?? (ingest.current.gaps.length ? `Evidence gap after sequence ${ingest.current.gaps.at(-1)?.after}` : null);
      setError(problem); setState("live");
      retainCapturePrefix(captureKey, { events: next, ready: ingest.current.ready, state: "live", closed: closedStreams.size });
    };

    for (const stream of streams) {
      try {
        unsubscribers.push(clientRef.current.subscribe(stream, {
          onOpen: () => { if (!stopped && !answered) setState("replaying"); },
          onEvent: (event) => publish(event as LiveEvalEvent),
          onError: (reason) => {
            if (stopped) return;
            // EventSource reconnects automatically; preserve the last good prefix.
            setError(reason.message);
            reportVisualDiagnostic({ severity: "warn", event: "stream.sse.interrupted",
              code: VISUAL_STREAM_CODES.streamInterrupted, message: reason.message,
              retryable: true, visualId, details: { revision, streamId: stream.streamId } });
          },
          onClosed: () => {
            if (stopped) return;
            answered = true; window.clearTimeout(deadline); closedStreams.add(stream.streamId);
            setClosed(closedStreams.size);
            const terminal = closedStreams.size === streams.length;
            setState(terminal ? "terminal" : "live"); setError(null);
            retainCapturePrefix(captureKey, { events: ingest.current.events as LiveEvalEvent[],
              ready: ingest.current.ready, state: terminal ? "terminal" : "live", closed: closedStreams.size });
          }
        }));
      } catch (reason) {
        setError(reason instanceof Error ? reason.message : "SSE subscription failed");
        setState("error");
      }
    }
    return () => {
      stopped = true; window.clearTimeout(deadline);
      for (const unsubscribe of unsubscribers) unsubscribe();
    };
  }, [streamKey, visualId, revision, captureKey]);

  // Evidence cuts are canonical JSON. Omit unavailable optional fields instead
  // of materializing `undefined`, which makes the live fold impossible to
  // digest and leaves the retained source control unanchored.
  return useMemo(() => ({ events, state, closed, ready: state !== "error" && ready,
    recovered, error }), [events, state, closed, ready, recovered, error]);
}

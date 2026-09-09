/**
 * `useHistoricalCursor` — the one evidence-on-intent implementation shared by
 * the generic `optimizer.run.v1` shell and the algorithm family shell.
 *
 * Live is the default and costs nothing beyond the projection the host
 * already holds. Leaving live is the intent signal, and what it buys is
 * bounded on both axes:
 *
 *   · the *timeline* the scrubber moves over is a window of raw events —
 *     the newest `HISTORY_WINDOW` by default, extended backwards on request —
 *     read through the evidence client, never the whole journal;
 *   · the *state* at the cursor comes from the backend's checkpointed
 *     `projectionAt`, which folds a short suffix server-side. The renderer
 *     never reduces the journal to reach a historical point.
 *
 * Fixtures and previews have neither client. They keep the injected events
 * and the local reducer, which is exactly what they had before — the hook
 * degrades to the old behaviour rather than to an empty history.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useVisualState } from "@synth/visuals-react/session";
import {
  projectAtCursor,
  type OptimizerEvent,
  type OptimizerRun,
  type ProjectedState
} from "./projectEvents.ts";
import { normalizeOptimizerEvents } from "./normalizeEvents.ts";
import { projectRunViewV2, type OptimizerRunViewV2Like } from "./projectRunViewV2.ts";

/** Raw events fetched per scrubber window. Bounded by construction. */
export const HISTORY_WINDOW = 500;

export type EvidenceIntentClient = {
  load(window: { from: number; to: number }): Promise<unknown[]>;
  tail(): number;
};

export type HistoricalProjectionLike = {
  asOfSequence: number;
  checkpointSequence?: number | null;
  replayedEvents?: number;
  view: OptimizerRunViewV2Like;
};

export type HistoryClient = {
  projectionAt(sequence: number): Promise<HistoricalProjectionLike>;
};

export type EvidenceHydrationState = "pending" | "loading" | "ready" | "partial" | "unavailable";

export type HistoricalCursorInput = {
  /** Explicit bundled fixtures open their retained history, never claim a live read model. */
  fixture?:boolean;
  run: OptimizerRun;
  /** Events the host injected up front (fixtures, `full` consumers). */
  injectedEvents: OptimizerEvent[];
  runViewV2?: OptimizerRunViewV2Like;
  evidence?: EvidenceIntentClient;
  history?: HistoryClient;
  evidenceState?: EvidenceHydrationState;
  /** Durable tail when the host knows it; otherwise the evidence client's. */
  tailCursor?: number;
};

export type HistoricalCursor = {
  followLive: boolean;
  /** Index into `timelineEvents`; meaningful only when not following live. */
  cursorIndex: number;
  /** The bounded window the scrubber moves over. */
  timelineEvents: OptimizerEvent[];
  /** Sequence at the cursor, when one is selected. */
  cursorSequence: number | null;
  /** What to render: the live projection, or the historical one. */
  displayed: ProjectedState | null;
  /** Raw window or historical projection still arriving. */
  loading: boolean;
  /** Where `displayed` came from when not live. */
  historySource: "backend" | "local" | "none";
  error?: string;
  /** Earlier events exist beyond the loaded window. */
  canLoadEarlier: boolean;
  loadEarlier: () => void;
  onScrub: (index: number) => void;
  onFollowLive: () => void;
  /** Hydration is still pending and no window has been read yet. */
  hydrating: boolean;
};

/**
 * Decide the raw window to read when leaving live. Pure, so the rule is
 * testable without React: nothing is fetched while live, nothing is fetched
 * without a client, and an injected journal that already reaches the tail
 * is used as-is.
 */
export function planEvidenceWindow(input: {
  followLive: boolean;
  hasClient: boolean;
  tail: number;
  injectedCount: number;
  injectedTail: number;
  loadedFrom: number | null;
  loading: boolean;
}): { from: number; to: number } | null {
  if (input.followLive || !input.hasClient || input.loading) return null;
  if (input.tail <= 0) return null;
  if (input.injectedCount > 0 && input.injectedTail >= input.tail) return null;
  if (input.loadedFrom != null) return null;
  return { from: Math.max(1, input.tail - HISTORY_WINDOW + 1), to: input.tail };
}

function sequenceOf(event: OptimizerEvent): number {
  return Number(event.sequenceNumber) || 0;
}

export function useHistoricalCursor(input: HistoricalCursorInput): HistoricalCursor {
  const { run, injectedEvents, runViewV2, evidence, history, evidenceState } = input;
  const [followLive, setFollowLive] = useVisualState("optimizer.followLive", !input.fixture);
  const [pinnedSequence, setPinnedSequence] = useVisualState<number | null>("optimizer.sequence", null, {type:"number",minimum:0,clock:"optimizer.sequence"});
  const [cursorIndex, setCursorIndex] = useState(()=>input.fixture?Math.max(0,injectedEvents.length-1):0);
  const [windowEvents, setWindowEvents] = useState<OptimizerEvent[] | null>(null);
  const [loadedFrom, setLoadedFrom] = useState<number | null>(null);
  const [windowLoading, setWindowLoading] = useState(false);
  const [historical, setHistorical] = useState<{ sequence: number; state: ProjectedState } | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [error, setError] = useState<string | undefined>();
  const requestEpoch = useRef(0);
  const earlierEpoch=useRef(0);
  useEffect(()=>()=>{earlierEpoch.current++;},[run.id,evidence]);

  const injectedTail = injectedEvents.length > 0 ? sequenceOf(injectedEvents[injectedEvents.length - 1]) : 0;
  const tail = Math.max(input.tailCursor ?? 0, evidence?.tail() ?? 0, injectedTail);
  const timelineEvents = windowEvents ?? injectedEvents;

  // Leaving live reads one bounded window of the newest events — or none,
  // when the injected journal already reaches the tail.
  useEffect(() => {
    let plan = planEvidenceWindow({
      followLive,
      hasClient: Boolean(evidence),
      tail,
      injectedCount: injectedEvents.length,
      injectedTail,
      loadedFrom,
      // This effect owns its request lifecycle; loading state must not cancel
      // the request immediately after setWindowLoading(true).
      loading: false
    });
    if(!followLive && evidence && pinnedSequence!=null && !timelineEvents.some(event=>sequenceOf(event)===pinnedSequence)){
      plan={from:Math.max(1,pinnedSequence-Math.floor(HISTORY_WINDOW/2)),to:Math.min(tail,pinnedSequence+Math.floor(HISTORY_WINDOW/2))};
    }
    if (plan && pinnedSequence != null && (pinnedSequence < plan.from || pinnedSequence > plan.to)) {
      plan = { from: Math.max(1,pinnedSequence - Math.floor(HISTORY_WINDOW/2)), to: Math.min(tail,pinnedSequence + Math.floor(HISTORY_WINDOW/2)) };
    }
    if (!plan || !evidence) {setWindowLoading(false);return;}
    let cancelled = false;
    setWindowLoading(true);
    void evidence
      .load(plan)
      .then((rows) => {
        if (cancelled) return;
        const events = normalizeOptimizerEvents(rows);
        setWindowEvents(events);
        setLoadedFrom(plan.from);
        const restored = pinnedSequence == null ? -1 : events.findIndex(event => sequenceOf(event) === pinnedSequence);
        setCursorIndex(restored < 0 ? Math.max(0, events.length - 1) : restored);
      })
      .catch((reason) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (!cancelled) setWindowLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [followLive, evidence, tail, injectedEvents.length, injectedTail, pinnedSequence]);

  const loadEarlier = useCallback(() => {
    if (!evidence || loadedFrom == null || loadedFrom <= 1 || windowLoading) return;
    const from = Math.max(1, loadedFrom - HISTORY_WINDOW);
    const to = loadedFrom - 1;
    const epoch=++earlierEpoch.current;
    setWindowLoading(true);
    void evidence
      .load({ from, to })
      .then((rows) => {
        if(epoch!==earlierEpoch.current)return;
        const earlier = normalizeOptimizerEvents(rows);
        setWindowEvents((current) => [...earlier, ...(current ?? [])]);
        setLoadedFrom(from);
        setCursorIndex((index) => index + earlier.length);
      })
      .catch((reason) => {if(epoch===earlierEpoch.current)setError(reason instanceof Error ? reason.message : String(reason));})
      .finally(() => {if(epoch===earlierEpoch.current)setWindowLoading(false);});
  }, [evidence, loadedFrom, windowLoading]);

  useEffect(() => {
    if (followLive) setCursorIndex(Math.max(0, timelineEvents.length - 1));
    else if(pinnedSequence!=null){const index=timelineEvents.findIndex(event=>sequenceOf(event)===pinnedSequence);if(index>=0)setCursorIndex(index);}
  }, [timelineEvents, followLive,pinnedSequence]);

  const cursorSequence = followLive ? null : pinnedSequence ?? (timelineEvents[cursorIndex] ? sequenceOf(timelineEvents[cursorIndex]) : null);

  // The state at the cursor comes from the backend checkpoint fold when a
  // history client exists. Requests are epoch-guarded so a slow answer for
  // an abandoned position never overwrites the current one.
  useEffect(() => {
    if (followLive || cursorSequence == null || !history) {setHistoryLoading(false);return;}
    if (historical?.sequence === cursorSequence) {setHistoryLoading(false);return;}
    const epoch = ++requestEpoch.current;
    setHistoryLoading(true);
    void history
      .projectionAt(cursorSequence)
      .then((answer) => {
        if (epoch !== requestEpoch.current) return;
        setHistorical({ sequence: cursorSequence, state: projectRunViewV2(run, answer.view) });
        setError(undefined);
      })
      .catch((reason) => {
        if (epoch !== requestEpoch.current) return;
        setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (epoch === requestEpoch.current) setHistoryLoading(false);
      });
    return()=>{if(epoch===requestEpoch.current)requestEpoch.current++;};
  }, [followLive, cursorSequence, history, run, historical?.sequence]);

  const localProjected = useMemo(
    () => (!followLive && !history && cursorSequence != null ? projectAtCursor(run, timelineEvents, cursorSequence) : null),
    [followLive, history, run, timelineEvents, cursorSequence]
  );

  const displayed = useMemo<ProjectedState | null>(() => {
    if (followLive) return runViewV2 ? projectRunViewV2(run, runViewV2) : null;
    if (history) return historical?.sequence === cursorSequence ? historical.state : (historical?.state ?? null);
    return localProjected;
  }, [followLive, runViewV2, run, history, historical, cursorSequence, localProjected]);

  const onScrub = useCallback((index: number) => {
    setFollowLive(false);
    setCursorIndex(index);
    setPinnedSequence(timelineEvents[index] ? sequenceOf(timelineEvents[index]) : null);
  }, [setFollowLive,setPinnedSequence,timelineEvents]);
  const onFollowLive = useCallback(() => {
    requestEpoch.current += 1;
    setFollowLive(true);
    setPinnedSequence(null);
    setHistoryLoading(false);
    setCursorIndex(Math.max(0, timelineEvents.length - 1));
  }, [timelineEvents.length,setFollowLive,setPinnedSequence]);

  const hydrating = (evidenceState === "pending" || evidenceState === "loading") && timelineEvents.length === 0 && !windowEvents;

  return {
    followLive,
    cursorIndex,
    timelineEvents,
    cursorSequence,
    displayed,
    loading: windowLoading || historyLoading,
    historySource: followLive ? "none" : history ? "backend" : timelineEvents.length > 0 ? "local" : "none",
    error,
    canLoadEarlier: Boolean(evidence) && loadedFrom != null && loadedFrom > 1,
    loadEarlier,
    onScrub,
    onFollowLive,
    hydrating
  };
}

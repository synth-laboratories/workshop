/**
 * Craftax trace workstation.
 *
 * The drill-down this replaces showed one textual observation per finished
 * seed. Everything the container had already recorded — the native PNGs, the
 * policy's own messages, its reasoning, what it asked the environment to do and
 * what the environment actually did — arrived nowhere, and the pane only moved
 * when a whole seed finished.
 *
 * Layout follows the published Craftax viewer's standard: the world is
 * dominant on the left, the complete trajectory is a bounded rail on the right,
 * and the per-call detail answers four questions in order — what did the policy
 * see, what did it decide, what did the environment apply, what changed.
 *
 * Three behaviours are specific to this being *live*:
 *
 * - It follows the newest call and frame by default, and stops the moment the
 *   reviewer scrubs backwards. Chasing playback under someone reading a call is
 *   the fastest way to make a live viewer unusable.
 * - New data appends. Selection, scroll position and open disclosures survive
 *   every update, because the selection is held by index and identity here
 *   rather than being recomputed from the projection.
 * - A policy call that is still open is *shown* as still open. Hiding it would
 *   make the rail jump when it lands.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import {
  craftaxTrialsFromRun,
	craftaxTraceFromSealedTrace,
  localMapRows,
	reconcileCraftaxTrace,
  type EvalTraceView,
  type TraceFrame,
  type TraceStep,
  type TrialView
} from "../../../runtime/craftaxTraceView.ts";
import { NO_MEDIA, type LoadedMedia, type MediaClient } from "../../../runtime/mediaClient.ts";

type Any = Record<string, any>;

export type ShellProps = {
  title?: string;
  lede?: string;
  run?: Any;
  events?: Any[];
  enrichmentEvents?: Any[];
  data?: Any;
  media?: MediaClient;
  loadError?: string;
  visualId?: string | null;
  revision?: number | null;
	sealedTraceProjections?: Array<{
		trialId: string;
		rolloutId: string | null;
		digest: string;
		projection: Any;
	}>;
};

const MISSING = "—";

const mono = { fontFamily: "var(--sv-mono)" } as const;

function reward(value: number | null): string {
  if (value === null) return MISSING;
  return `${value >= 0 ? "+" : ""}${value.toFixed(2)}`;
}

function tokens(value: number | null): string {
  if (value === null) return MISSING;
  return value >= 1000 ? `${(value / 1000).toFixed(1)}k` : String(value);
}

/** Vitals a Craftax readout carries. Colours match the published viewer. */
const VITALS: [string, string][] = [
  ["health", "#c2553f"],
  ["food", "#c99b3f"],
  ["drink", "#3d78bb"],
  ["energy", "#6f9a4d"],
  ["mana", "#8a5fd0"]
];

function Chip({
  label,
  tone = "muted",
  title
}: {
  label: string;
  tone?: "muted" | "ok" | "bad" | "warn" | "accent";
  title?: string;
}) {
  const palette = {
    muted: ["var(--sv-surface-muted)", "var(--sv-text-muted)", "var(--sv-border)"],
    ok: ["var(--sv-ok-bg)", "var(--sv-ok-fg)", "var(--sv-ok-edge)"],
    bad: ["var(--sv-bad-bg)", "var(--sv-bad-fg)", "var(--sv-bad-edge)"],
    warn: ["var(--sv-warn-bg)", "var(--sv-warn-fg)", "var(--sv-warn-edge)"],
    accent: ["var(--sv-accent-soft)", "var(--sv-accent-hot)", "var(--sv-accent-soft)"]
  }[tone];
  return (
    <span
      title={title}
      style={{
        display: "inline-block",
        padding: "1px var(--sv-sp-2)",
        border: `1px solid ${palette[2]}`,
        borderRadius: 99,
        background: palette[0],
        color: palette[1],
        fontSize: "var(--sv-fs-micro)",
        whiteSpace: "nowrap"
      }}
    >
      {label}
    </span>
  );
}

function Disclosure({
  summary,
  count,
  children
}: {
  summary: string;
  count?: number | null;
  children: React.ReactNode;
}) {
  return (
    <details style={{ marginTop: "var(--sv-sp-2)" }}>
      <summary
        style={{
          cursor: "pointer",
          color: "var(--sv-text-muted)",
          fontSize: "var(--sv-fs-meta)"
        }}
      >
        {summary}
        {count != null ? ` · ${count}` : ""}
      </summary>
      <div style={{ marginTop: "var(--sv-sp-2)" }}>{children}</div>
    </details>
  );
}

/**
 * The environment picture.
 *
 * A native PNG when the relay retained one. When it did not, the *map rows* —
 * never the whole textual observation, which is what the previous ASCII
 * fallback was handed and painted as a wall of tiles. When there is no map
 * either, the observation is shown as what it is: text.
 */
function FrameCanvas({
  frame,
  step,
  media,
  loaded
}: {
  frame: TraceFrame | null;
  step: TraceStep | null;
  media: MediaClient;
  loaded: LoadedMedia | undefined;
}) {
  const label = frame ? `Craftax frame at step ${frame.step}` : "No frame for this call";
  const surface: React.CSSProperties = {
    display: "grid",
    placeItems: "center",
    minHeight: 320,
    border: "1px solid var(--sv-border)",
    borderRadius: "var(--sv-radius-lg)",
    background: "#12160f",
    overflow: "hidden"
  };
  if (frame?.media && loaded) {
    return (
      <div style={surface} data-testid="craftax-native-frame">
        <img
          src={loaded.dataUrl}
          alt={label}
          style={{ width: "100%", height: "auto", imageRendering: "pixelated", display: "block" }}
        />
      </div>
    );
  }
  if (frame?.media && !loaded) {
    const failure = media.failures().get(frame.media.casDigest);
    return (
      <div style={{ ...surface, color: "#8d968a", fontSize: "var(--sv-fs-meta)" }}>
        {failure ? `This frame could not be loaded: ${failure}` : "Loading frame…"}
      </div>
    );
  }
  const rows = localMapRows(step);
  if (rows) {
    return (
      <div style={surface}>
        <pre
          role="img"
          aria-label={`${label} (symbolic map)`}
          style={{
            ...mono,
            margin: 0,
            padding: "var(--sv-sp-4)",
            color: "#dbe9d5",
            fontSize: 15,
            lineHeight: 1.15,
            letterSpacing: 2
          }}
        >
          {rows.join("\n")}
        </pre>
      </div>
    );
  }
  return (
    <div
      style={{
        ...surface,
        padding: "var(--sv-sp-4)",
        color: "#8d968a",
        fontSize: "var(--sv-fs-meta)",
        textAlign: "center"
      }}
    >
      {frame?.unavailable ?? "This call recorded no environment frame."}
    </div>
  );
}

/** Vitals and inventory, read from the structured readout only. */
function Hud({ step }: { step: TraceStep | null }) {
  const readout = step?.content.readout ?? null;
  const vitals = (readout?.vitals ?? readout?.stats ?? null) as Any | null;
  const inventory = (readout?.inventory ?? null) as Any | null;
  if (!vitals && !inventory) return null;
  return (
    <div
      style={{
        display: "grid",
        gap: "var(--sv-sp-2)",
        marginTop: "var(--sv-sp-3)",
        padding: "var(--sv-sp-3)",
        border: "1px solid var(--sv-border)",
        borderRadius: "var(--sv-radius)",
        background: "var(--sv-surface-muted)"
      }}
    >
      {vitals ? (
        <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--sv-sp-3)" }}>
          {VITALS.filter(([name]) => vitals[name] != null).map(([name, colour]) => (
            <span key={name} style={{ display: "flex", alignItems: "center", gap: 4 }}>
              <span
                aria-hidden
                style={{ width: 7, height: 7, borderRadius: 99, background: colour }}
              />
              <span style={{ color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-micro)" }}>
                {name}
              </span>
              <strong style={{ ...mono, fontSize: "var(--sv-fs-meta)" }}>{vitals[name]}</strong>
            </span>
          ))}
        </div>
      ) : null}
      {inventory ? (
        <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--sv-sp-1)" }}>
          {Object.entries(inventory)
            .filter(([, count]) => Number(count) > 0)
            .map(([name, count]) => (
              <Chip key={name} label={`${name} ${count}`} />
            ))}
        </div>
      ) : null}
    </div>
  );
}

/** The complete ordered trajectory. Every call, always — playback never hides one. */
function TrajectoryRail({
  view,
  selected,
  onSelect,
  query,
  onQuery
}: {
  view: EvalTraceView;
  selected: number;
  onSelect: (index: number) => void;
  query: string;
  onQuery: (value: string) => void;
}) {
  const railRef = useRef<HTMLDivElement | null>(null);
  const activeRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    // Auto-follow scrolls the rail, never the document. The reviewer's page
    // position is theirs; only this bounded box moves.
    const rail = railRef.current;
    const active = activeRef.current;
    if (!rail || !active) return;
    const top = active.offsetTop - rail.offsetTop;
    if (top < rail.scrollTop || top + active.offsetHeight > rail.scrollTop + rail.clientHeight) {
      rail.scrollTop = top - rail.clientHeight / 2 + active.offsetHeight / 2;
    }
  }, [selected, view.steps.length]);

  const needle = query.trim().toLowerCase();
  const matches = (step: TraceStep) => {
    if (!needle) return true;
    const haystack = [
      step.title,
      step.content.reasoning,
      step.content.message,
      ...step.action.proposed,
      ...step.action.applied.map((row) => row.name),
      ...step.action.rejected.map((row) => row.name),
      ...step.achievements,
      ...step.tool_calls.map((call) => call.name)
    ]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return haystack.includes(needle);
  };

  return (
    <div style={{ display: "grid", gridTemplateRows: "auto 1fr", gap: "var(--sv-sp-2)", minHeight: 0 }}>
      <input
        value={query}
        onChange={(event) => onQuery(event.target.value)}
        placeholder="Search timeline, actions, achievements"
        aria-label="Search the trajectory"
        style={{
          padding: "var(--sv-sp-2)",
          border: "1px solid var(--sv-border)",
          borderRadius: "var(--sv-radius-sm)",
          background: "var(--sv-surface)",
          color: "var(--sv-text)",
          fontSize: "var(--sv-fs-meta)"
        }}
      />
      <div
        ref={railRef}
        role="listbox"
        aria-label="Policy and agent timeline"
        style={{ overflowY: "auto", minHeight: 0, display: "grid", gap: 3, alignContent: "start" }}
      >
        {view.steps.map((step, index) => {
          const active = index === selected;
          const dim = !matches(step);
          return (
            <button
              key={step.id}
              ref={active ? activeRef : undefined}
              type="button"
              role="option"
              aria-selected={active}
              onClick={() => onSelect(index)}
              style={{
                display: "grid",
                gridTemplateColumns: "auto 1fr auto",
                gap: "var(--sv-sp-2)",
                alignItems: "center",
                padding: "var(--sv-sp-2)",
                border: `1px solid ${active ? "var(--sv-accent)" : "var(--sv-border)"}`,
                borderRadius: "var(--sv-radius-sm)",
                background: active ? "var(--sv-accent-soft)" : "var(--sv-surface)",
                color: "var(--sv-text)",
                cursor: "pointer",
                textAlign: "left",
                opacity: dim ? 0.35 : 1
              }}
            >
              <span style={{ ...mono, fontSize: "var(--sv-fs-micro)", color: "var(--sv-text-faint)" }}>
                {String(step.index).padStart(2, "0")}
              </span>
              <span style={{ display: "grid", gap: 2, minWidth: 0 }}>
                <span
                  style={{
                    fontSize: "var(--sv-fs-meta)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap"
                  }}
                >
                  {step.action.applied.length
                    ? step.action.applied.map((row) => row.name).join(" · ")
                    : step.status === "running"
                      ? "deciding…"
                      : "no environment action"}
                </span>
                <span style={{ display: "flex", gap: 3, flexWrap: "wrap" }}>
                  {step.status === "running" ? <Chip label="running" tone="accent" /> : null}
                  {step.achievements.map((name) => (
                    <Chip key={name} label={name} tone="warn" />
                  ))}
                  {step.action.rejected.length ? (
                    <Chip label={`${step.action.rejected.length} rejected`} tone="bad" />
                  ) : null}
                </span>
              </span>
              <span style={{ ...mono, fontSize: "var(--sv-fs-micro)", color: "var(--sv-text-faint)" }}>
                {step.turn_start === null
                  ? MISSING
                  : step.turn_end === null || step.turn_end === step.turn_start
                    ? `t${step.turn_start}`
                    : `t${step.turn_start}–${step.turn_end}`}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** Observation → decision → applied → outcome, with raw behind disclosure. */
function CallDetail({ view, step }: { view: EvalTraceView; step: TraceStep | null }) {
  if (!step) {
    return (
      <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-meta)" }}>
        This rollout has recorded no policy or agent timeline item yet.
      </p>
    );
  }
  const section: React.CSSProperties = {
    paddingTop: "var(--sv-sp-3)",
    borderTop: "1px solid var(--sv-border)"
  };
  const heading: React.CSSProperties = {
    margin: 0,
    color: "var(--sv-text-faint)",
    fontSize: "var(--sv-fs-micro)",
    letterSpacing: ".06em",
    textTransform: "uppercase"
  };
  const body: React.CSSProperties = {
    margin: "var(--sv-sp-1) 0 0",
    fontSize: "var(--sv-fs-body)",
    lineHeight: 1.5,
    whiteSpace: "pre-wrap"
  };
  const pre: React.CSSProperties = {
    ...mono,
    margin: 0,
    padding: "var(--sv-sp-2)",
    maxHeight: 220,
    overflow: "auto",
    border: "1px solid var(--sv-border)",
    borderRadius: "var(--sv-radius-sm)",
    background: "var(--sv-surface-muted)",
    fontSize: "var(--sv-fs-micro)",
    whiteSpace: "pre-wrap"
  };
  return (
    <div style={{ display: "grid", gap: "var(--sv-sp-3)", overflowY: "auto", minHeight: 0 }}>
      <div>
        <p style={heading}>Observed</p>
        <p style={{ ...body, color: "var(--sv-text-muted)" }}>
          {step.content.observation
            ? step.content.observation.split("\n").slice(0, 3).join("\n")
            : "No observation was recorded before this call."}
        </p>
        {view.system_prompt ? (
          <Disclosure summary="System prompt">
            <pre style={pre}>{view.system_prompt}</pre>
          </Disclosure>
        ) : null}
        {step.content.input_messages.length ? (
          <Disclosure summary="Policy-visible messages" count={step.content.input_messages.length}>
            <div style={{ display: "grid", gap: "var(--sv-sp-2)" }}>
              {step.content.input_messages.map((message, index) => (
                <div key={index}>
                  <Chip label={message.role} />
                  <pre style={{ ...pre, marginTop: 3 }}>{message.content || MISSING}</pre>
                </div>
              ))}
            </div>
          </Disclosure>
        ) : null}
        {step.content.observation ? (
          <Disclosure summary="Full observation">
            <pre style={pre}>{step.content.observation}</pre>
          </Disclosure>
        ) : null}
      </div>

      <div style={section}>
        <p style={heading}>Decided</p>
        {step.content.reasoning ? (
          <p style={{ ...body, color: "var(--sv-text-muted)", fontStyle: "italic" }}>
            {step.content.reasoning}
          </p>
        ) : null}
        {step.content.message ? <p style={body}>{step.content.message}</p> : null}
        {!step.content.reasoning && !step.content.message ? (
          <p style={{ ...body, color: "var(--sv-text-faint)" }}>
            {step.status === "running"
              ? "This call is still open; the model has not answered yet."
              : "No reasoning or message was recorded for this call."}
          </p>
        ) : null}
        {step.tool_calls.map((call, index) => (
          <div key={call.id ?? index} style={{ marginTop: "var(--sv-sp-2)" }}>
            <Chip label={call.name} tone="accent" />
            <Disclosure summary="Tool arguments">
              <pre style={pre}>
                {typeof call.arguments === "string"
                  ? call.argumentsText
                  : JSON.stringify(call.arguments, null, 2)}
              </pre>
            </Disclosure>
          </div>
        ))}
        {step.action.proposed.length ? (
          <Disclosure summary="Proposed actions" count={step.action.proposed.length}>
            <div style={{ display: "flex", gap: 3, flexWrap: "wrap" }}>
              {step.action.proposed.map((name, index) => (
                <Chip key={`${name}-${index}`} label={name} />
              ))}
            </div>
            <p
              style={{
                margin: "var(--sv-sp-2) 0 0",
                color: "var(--sv-text-faint)",
                fontSize: "var(--sv-fs-micro)"
              }}
            >
              What the model asked for. The environment's own record is below.
            </p>
          </Disclosure>
        ) : null}
      </div>

      <div style={section}>
        <p style={heading}>Applied by the environment</p>
        {step.action.applied.length ? (
          <ol
            style={{
              margin: "var(--sv-sp-2) 0 0",
              paddingLeft: "var(--sv-sp-5)",
              display: "grid",
              gap: 3
            }}
          >
            {step.action.applied.map((row, index) => {
              const noop = step.action.noop.some(
                (other) => other.turn === row.turn && other.name === row.name
              );
              return (
                <li key={`${row.name}-${row.turn}-${index}`} style={{ fontSize: "var(--sv-fs-meta)" }}>
                  <span style={mono}>{row.name}</span>
                  <span style={{ color: "var(--sv-text-faint)" }}>
                    {row.turn === null ? "" : ` · t${row.turn}`}
                  </span>
                  {noop ? <> <Chip label="no effect" /></> : null}
                </li>
              );
            })}
          </ol>
        ) : (
          <p style={{ ...body, color: "var(--sv-text-faint)" }}>
            The environment applied no action for this call.
          </p>
        )}
        {step.action.rejected.length ? (
          <div style={{ marginTop: "var(--sv-sp-2)", display: "grid", gap: 3 }}>
            {step.action.rejected.map((row, index) => (
              <div key={`${row.name}-${index}`} style={{ fontSize: "var(--sv-fs-meta)" }}>
                <Chip label="rejected" tone="bad" />{" "}
                <span style={mono}>{row.name}</span>
                <span style={{ color: "var(--sv-text-faint)" }}>
                  {row.reason ? ` · ${row.reason}` : " · no reason recorded"}
                </span>
              </div>
            ))}
          </div>
        ) : null}
      </div>

      <div style={section}>
        <p style={heading}>Changed</p>
        <div
          style={{
            display: "flex",
            gap: "var(--sv-sp-2)",
            flexWrap: "wrap",
            marginTop: "var(--sv-sp-2)"
          }}
        >
          <Chip label={`reward ${reward(step.reward)}`} tone={step.reward ? "ok" : "muted"} />
          <Chip label={`in ${tokens(step.tokens.input)}`} />
          <Chip label={`out ${tokens(step.tokens.output)}`} />
          {step.achievements.map((name) => (
            <Chip key={name} label={name} tone="warn" />
          ))}
        </div>
        {step.state_delta.length ? (
          <Disclosure summary="State deltas" count={step.state_delta.length}>
            <div style={{ display: "grid", gap: 2 }}>
              {step.state_delta.map((delta, index) => (
                <div
                  key={`${delta.field}-${index}`}
                  style={{ ...mono, fontSize: "var(--sv-fs-micro)" }}
                >
                  {delta.field}: {String(delta.before ?? MISSING)} → {String(delta.after ?? MISSING)}
                  {delta.turn === null ? "" : `  ·  t${delta.turn}`}
                </div>
              ))}
            </div>
          </Disclosure>
        ) : null}
        <Disclosure summary="Raw producer events" count={step.raw.length}>
          <pre style={pre}>
            {JSON.stringify(
              view.events.filter((event) => step.raw.includes(event.sequence)),
              null,
              2
            )}
          </pre>
        </Disclosure>
      </div>
    </div>
  );
}

export function Shell(props: ShellProps) {
  const run = (props.run ?? props.data?.run ?? null) as Any | null;
  const optimizerEvents = useMemo(
    () => [
      ...(Array.isArray(props.events) ? props.events : []),
      ...(Array.isArray(props.enrichmentEvents) ? props.enrichmentEvents : [])
    ],
    [props.events, props.enrichmentEvents]
  );
  const media = props.media ?? NO_MEDIA;

	const liveTrials = useMemo(
    () => (run ? craftaxTrialsFromRun(run, optimizerEvents) : []),
    [run, optimizerEvents]
  );
	const trials = useMemo(() => liveTrials.map((row) => {
		const sealed = props.sealedTraceProjections?.find((candidate) =>
			candidate.trialId === row.trialId ||
			(Boolean(row.rolloutId) && candidate.rolloutId === row.rolloutId)
		);
		if (!sealed) return row;
		const sealedView = craftaxTraceFromSealedTrace(sealed.projection, {
			traceId: row.rolloutId ?? row.trialId,
			scenario: row.view.task.scenario,
			seed: row.seed,
			status: row.state,
			model: row.view.run.model,
			provider: row.view.run.provider,
			effort: row.view.run.effort,
			totalReward: row.reward,
			contentDigest: sealed.digest
		});
		return { ...row, view: reconcileCraftaxTrace(row.view, sealedView).view ?? row.view };
	}), [liveTrials, props.sealedTraceProjections]);

  // Selection is held by identity, not by object. A trial folded again on the
  // next append is a new object with the same id, and a selection keyed on the
  // object would reset on every update — which is precisely the "resets while
  // you are reading it" failure this pane exists to avoid.
  const [selectedTrialId, setSelectedTrialId] = useState<string | null>(null);
  const [selectedCall, setSelectedCall] = useState(0);
  const [selectedFrame, setSelectedFrame] = useState<number | null>(null);
  const [following, setFollowing] = useState(true);
  const [playing, setPlaying] = useState(false);
  const [query, setQuery] = useState("");
  const [loaded, setLoaded] = useState<LoadedMedia | undefined>(undefined);

  const trial: TrialView | null =
    trials.find((row) => row.trialId === selectedTrialId) ??
    trials.find((row) => row.state === "running") ??
    trials[0] ??
    null;
  const view = trial?.view ?? null;

  const frameDigests = useMemo(
    () => (view?.frames ?? []).map((frame) => frame.media?.casDigest ?? ""),
    [view]
  );

  // Follow the newest call and frame — until the reviewer takes over.
  useEffect(() => {
    if (!following || !view) return;
    const lastCall = Math.max(0, view.steps.length - 1);
    setSelectedCall(lastCall);
    setSelectedFrame(view.frames.length ? view.frames.length - 1 : null);
  }, [following, view?.steps.length, view?.frames.length]);

  const step = view?.steps[selectedCall] ?? null;
  const frameIndex =
    selectedFrame ?? (step?.frames.length ? step.frames[step.frames.length - 1] : null);
  const frame = frameIndex === null ? null : (view?.frames[frameIndex] ?? null);

  useEffect(() => {
    if (frameIndex === null || !frameDigests[frameIndex]) {
      setLoaded(undefined);
      return;
    }
    let cancelled = false;
    // Only the selection and a small window around it. A 500-step episode is
    // 500 PNGs, and warming all of them to show one is how a pane stops
    // responding to the scrubber it is meant to serve.
    void media.warm(frameDigests, frameIndex).then((result) => {
      if (!cancelled) setLoaded(result);
    });
    return () => {
      cancelled = true;
    };
  }, [media, frameDigests, frameIndex]);

  /** Any deliberate move backwards stops auto-follow; forwards at the tip keeps it. */
  const gotoFrame = useCallback(
    (next: number) => {
      if (!view) return;
      const clamped = Math.max(0, Math.min(view.frames.length - 1, next));
      setSelectedFrame(clamped);
      if (clamped < view.frames.length - 1) {
        setFollowing(false);
        setPlaying(false);
      }
      const owner = view.steps.findIndex((candidate) => candidate.frames.includes(clamped));
      if (owner >= 0) setSelectedCall(owner);
    },
    [view]
  );

  const selectCall = useCallback(
    (index: number) => {
      if (!view) return;
      setSelectedCall(index);
      const owned = view.steps[index]?.frames ?? [];
      setSelectedFrame(owned.length ? owned[owned.length - 1] : null);
      // Selecting a call is a manual act, and playback pauses on one.
      setPlaying(false);
      if (index < view.steps.length - 1) setFollowing(false);
    },
    [view]
  );

  useEffect(() => {
    if (!playing || !view?.frames.length) return;
    const timer = window.setInterval(() => {
      setSelectedFrame((current) => {
        const next = (current ?? 0) + 1;
        if (next >= view.frames.length) {
          setPlaying(false);
          return current;
        }
        const owner = view.steps.findIndex((candidate) => candidate.frames.includes(next));
        if (owner >= 0) setSelectedCall(owner);
        return next;
      });
    }, 450);
    return () => window.clearInterval(timer);
  }, [playing, view]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      // Shortcuts do nothing while the reviewer is typing in the rail's search.
      if (target && /^(INPUT|SELECT|TEXTAREA)$/.test(target.tagName)) return;
      if (event.key === "j" || event.key === "ArrowDown") {
        selectCall(Math.min((view?.steps.length ?? 1) - 1, selectedCall + 1));
      } else if (event.key === "k" || event.key === "ArrowUp") {
        selectCall(Math.max(0, selectedCall - 1));
      } else if (event.key === "ArrowLeft") {
        gotoFrame((frameIndex ?? 0) - 1);
      } else if (event.key === "ArrowRight") {
        gotoFrame((frameIndex ?? -1) + 1);
      } else {
        return;
      }
      event.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectCall, gotoFrame, selectedCall, frameIndex, view?.steps.length]);

  const sealed = view?.integrity.status === "sealed";
  const terminal = trial ? trial.state === "done" || trial.state === "failed" : false;
  const button: React.CSSProperties = {
    padding: "var(--sv-sp-1) var(--sv-sp-3)",
    border: "1px solid var(--sv-border-strong)",
    borderRadius: "var(--sv-radius-sm)",
    background: "var(--sv-surface)",
    color: "var(--sv-text)",
    fontSize: "var(--sv-fs-meta)",
    cursor: "pointer"
  };

  return (
    <VisualChrome
      kicker={`Craftax · ${trials.filter((row) => row.state === "done" || row.state === "failed").length}/${trials.length} seeds`}
      title={props.title ?? "Craftax trace workstation"}
      lede={props.lede}
      live={!terminal}
      testId="craftax-trace-workbench"
      observation={{
        transportState: props.loadError ? "error" : terminal ? "terminal" : "live",
        rolloutCount: trials.length,
        renderedFrameCount: view?.frames.filter((row) => row.media).length ?? 0,
        semanticEventCount: view?.events.length ?? 0,
        terminal,
        error: props.loadError ?? null
      }}
      footer={
        <span style={{ ...mono, fontSize: "var(--sv-fs-micro)" }}>
          {sealed ? "Sealed Trace V5" : "Live relay"}
          {view?.integrity.content_digest ? ` · ${view.integrity.content_digest.slice(0, 20)}` : ""}
          {view ? ` · ${view.coverage.framesRetained}/${view.coverage.framesDeclared} frames retained` : ""}
        </span>
      }
    >
      {props.loadError ? (
        <p
          style={{
            margin: "0 0 var(--sv-sp-3)",
            color: "var(--sv-bad-fg)",
            fontSize: "var(--sv-fs-meta)"
          }}
        >
          {props.loadError}
        </p>
      ) : null}

      <div
        style={{
          display: "flex",
          gap: "var(--sv-sp-1)",
          flexWrap: "wrap",
          marginBottom: "var(--sv-sp-3)"
        }}
      >
        {trials.map((row) => (
          <button
            key={row.trialId}
            type="button"
            onClick={() => {
              setSelectedTrialId(row.trialId);
              setFollowing(row.state === "running");
              setSelectedCall(0);
              setSelectedFrame(null);
            }}
            style={{
              ...button,
              borderColor: row.trialId === trial?.trialId ? "var(--sv-accent)" : "var(--sv-border)",
              background:
                row.trialId === trial?.trialId ? "var(--sv-accent-soft)" : "var(--sv-surface)"
            }}
          >
            <span style={mono}>seed {row.seed ?? MISSING}</span>
            <span style={{ color: "var(--sv-text-faint)" }}>
              {row.state === "done" ? ` · ${reward(row.reward)}` : ` · ${row.state}`}
            </span>
          </button>
        ))}
      </div>

      {!view ? (
        <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-meta)" }}>
          No trial has been dispatched yet.
        </p>
      ) : (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "minmax(320px, 3fr) minmax(240px, 2fr)",
            gap: "var(--sv-sp-4)",
            height: 720,
            minHeight: 0
          }}
        >
          <section style={{ display: "grid", gridTemplateRows: "1fr auto auto", minHeight: 0 }}>
            <FrameCanvas frame={frame} step={step} media={media} loaded={loaded} />
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: "var(--sv-sp-2)",
                marginTop: "var(--sv-sp-3)",
                flexWrap: "wrap"
              }}
            >
              <button type="button" style={button} onClick={() => gotoFrame((frameIndex ?? 0) - 1)}>
                ◀ prev
              </button>
              <button
                type="button"
                style={button}
                onClick={() => {
                  setPlaying((value) => !value);
                  if (!playing) setFollowing(false);
                }}
              >
                {playing ? "❚❚ pause" : "▶ play"}
              </button>
              <button type="button" style={button} onClick={() => gotoFrame((frameIndex ?? -1) + 1)}>
                next ▶
              </button>
              <input
                type="range"
                min={0}
                max={Math.max(0, view.frames.length - 1)}
                value={frameIndex ?? 0}
                aria-label="Frame scrubber"
                onChange={(event) => gotoFrame(Number(event.target.value))}
                style={{ flex: 1, minWidth: 120 }}
              />
              <span style={{ ...mono, fontSize: "var(--sv-fs-micro)", color: "var(--sv-text-faint)" }}>
                frame {view.frames.length ? (frameIndex ?? 0) + 1 : 0}/{view.frames.length} · item{" "}
                {view.steps.length ? selectedCall + 1 : 0}/{view.steps.length}
                {frame ? ` · t${frame.step}` : ""}
              </span>
              {!following && !terminal ? (
                <button
                  type="button"
                  style={{ ...button, borderColor: "var(--sv-accent)", color: "var(--sv-accent-hot)" }}
                  onClick={() => {
                    setFollowing(true);
                    setPlaying(false);
                  }}
                >
                  Follow live
                </button>
              ) : null}
            </div>
            <Hud step={step} />
            {view.coverage.degradations.length ? (
              <Disclosure summary="Retention receipts" count={view.coverage.degradations.length}>
                <div style={{ display: "grid", gap: 3 }}>
                  {view.coverage.degradations.map((row, index) => (
                    <div key={index} style={{ fontSize: "var(--sv-fs-micro)" }}>
                      <Chip label={row.reason} tone="warn" />{" "}
                      <span style={{ color: "var(--sv-text-muted)" }}>{row.detail}</span>
                    </div>
                  ))}
                </div>
              </Disclosure>
            ) : null}
          </section>

          <section
            style={{
              display: "grid",
              gridTemplateRows: "minmax(140px, 40%) 1fr",
              gap: "var(--sv-sp-3)",
              minHeight: 0
            }}
          >
            <TrajectoryRail
              view={view}
              selected={selectedCall}
              onSelect={selectCall}
              query={query}
              onQuery={setQuery}
            />
            <CallDetail view={view} step={step} />
          </section>
        </div>
      )}
    </VisualChrome>
  );
}

export default Shell;

/**
 * Harbor eval live viewer (A2 posture): trial → attempt evidence as it
 * streams, verifier truth (reward.txt fails closed; native and wrapped
 * verifiers shown side by side when both report), and a bounded tool/stdout
 * stream. ATIF is a projection of this evidence, never the log itself.
 */

import { useMemo, useState } from "react";
import { Identifier } from "../../chrome/Identifier.tsx";
import { VisualChrome, MetricStrip } from "../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../chrome/useLiveEvalStream.ts";
import { formatMissingNumber } from "../../runtime/liveStream.ts";
import { projectLiveEval } from "../../runtime/liveEvalReducer.ts";
import type { LiveEvalEvent, VisualBinding } from "../../runtime/types.ts";

type StreamPayload = { events?: LiveEvalEvent[]; sse_url?: string };

export type ShellProps = {
  title?: string;
  lede?: string;
  stream?: StreamPayload;
  jobs?: StreamPayload;
  data?: StreamPayload;
  bindings?: VisualBinding[];
  sseUrl?: string;
};

const STREAM_WINDOW = 30;

function asStream(raw: unknown): StreamPayload {
  if (raw && typeof raw === "object") return raw as StreamPayload;
  return {};
}

type TrialView = {
  key: string;
  instruction?: string;
  sandbox?: string;
  trialId?: string;
  status: "planned" | "launched" | "verified" | "failed";
  reward?: number | null;
  verifierScript?: string;
};

/** Fold trial.planned / trial.launched / verifier / status into trial cards. */
function foldTrials(events: LiveEvalEvent[]): TrialView[] {
  const trials = new Map<string, TrialView>();
  let anonymous = 0;
  const keyOf = (payload: Record<string, unknown>) =>
    String(payload.trial_id ?? payload.trialId ?? payload.attempt_id ?? `trial_${anonymous}`);
  for (const event of events) {
    const payload = (event.payload ?? {}) as Record<string, unknown>;
    if (event.kind === "trial.planned") {
      anonymous += 1;
      const key = keyOf(payload);
      trials.set(key, {
        key,
        instruction: typeof payload.instruction === "string" ? payload.instruction : undefined,
        sandbox: typeof payload.sandbox === "string" ? payload.sandbox : undefined,
        trialId: typeof payload.trial_id === "string" ? payload.trial_id : undefined,
        status: "planned"
      });
    } else if (event.kind === "trial.launched") {
      const key = keyOf(payload);
      const existing = trials.get(key) ?? { key, status: "planned" as const };
      trials.set(key, {
        ...existing,
        sandbox: typeof payload.sandbox === "string" ? payload.sandbox : existing.sandbox,
        status: "launched"
      });
    } else if (event.kind === "verifier") {
      const key = payload.trial_id != null ? String(payload.trial_id) : [...trials.keys()].at(-1) ?? keyOf(payload);
      const existing = trials.get(key) ?? { key, status: "launched" as const };
      const rewardTxt = payload["reward.txt"];
      trials.set(key, {
        ...existing,
        status: "verified",
        verifierScript: typeof payload.script === "string" ? payload.script : existing.verifierScript,
        reward: typeof rewardTxt === "number" && Number.isFinite(rewardTxt) ? rewardTxt : null
      });
    }
  }
  return [...trials.values()];
}

export function Shell(props: ShellProps) {
  const stream = asStream(props.data ?? props.stream ?? props.jobs);
  const sseUrl =
    props.sseUrl ??
    stream.sse_url ??
    props.bindings?.find((b) => b.slot === "stream" && b.kind === "live_sse")?.source;
  const fixtureEvents = useMemo(
    () => (sseUrl ? undefined : stream.events),
    [sseUrl, stream.events]
  );
  const hasSource = Boolean(sseUrl || stream.events);
  const { events, live, error, ready } = useLiveEvalStream({ sseUrl, fixtureEvents });
  const [showFullStream, setShowFullStream] = useState(false);
  const projection = projectLiveEval(events);
  const trials = useMemo(() => foldTrials(events), [events]);
  const status = [...events].reverse().find((event) => event.kind === "status");
  const statusText = String(status?.payload.status ?? "");
  const terminal = ["completed", "finished", "failed", "cancelled"].includes(statusText.toLowerCase());
  const tools = events.filter((event) => event.kind === "tools" || event.kind === "stdout" || event.kind === "stderr");
  const visibleTools = showFullStream ? tools : tools.slice(-STREAM_WINDOW);
  const verifiedCount = trials.filter((trial) => trial.status === "verified").length;

  return (
    <VisualChrome
      kicker="Harbor · Evals"
      live={live && !terminal}
      title={props.title ?? "Harbor trial / verifier"}
      lede={props.lede}
      testId="visual-live-harbor-eval"
      footer="live.harbor_eval.v1 · ATIF is a projection of this evidence, not the log"
    >
      <MetricStrip
        metrics={[
          { label: "Trials", value: trials.length ? `${verifiedCount}/${trials.length} verified` : "—" },
          { label: "Reward", value: formatMissingNumber(projection.reward) },
          { label: "reward.txt", value: projection.has_reward_txt ? "present" : "not yet" },
          {
            label: "Status",
            value: statusText || (ready ? (live ? "live" : "idle") : hasSource ? "connecting" : "awaiting source")
          }
        ]}
      />

      {error ? (
        <p role="alert" style={{ color: "#c2553f" }}>
          {error}
        </p>
      ) : null}

      <section className="sv-section" aria-label="Trials" data-testid="harbor-trials">
        <div className="sv-section-head">
          <h3>Trials</h3>
          <span className="sv-mono">{ready ? "ready" : hasSource ? "connecting" : "awaiting source"}</span>
        </div>
        {trials.length === 0 ? (
          <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: 12 }}>Waiting for trial.planned…</p>
        ) : (
          <div role="list" style={{ display: "grid", gap: 6 }}>
            {trials.map((trial) => (
              <article key={trial.key} role="listitem" style={{ padding: "9px 12px", border: "1px solid var(--sv-border)", borderRadius: 9 }}>
                <div style={{ display: "flex", flexWrap: "wrap", alignItems: "baseline", gap: 8 }}>
                  {trial.trialId ? <Identifier value={trial.trialId} label="trial" max={22} /> : <strong style={{ fontSize: 12 }}>Trial</strong>}
                  {trial.sandbox ? <Identifier value={trial.sandbox} label="sandbox" max={20} copy={false} /> : null}
                  <span
                    className="sv-chip"
                    data-tone={trial.status === "verified" ? (trial.reward != null && trial.reward > 0 ? "ok" : "warn") : trial.status === "failed" ? "bad" : undefined}
                    style={{ marginLeft: "auto" }}
                  >
                    {trial.status}
                  </span>
                </div>
                {trial.instruction ? (
                  <p style={{ margin: "6px 0 0", fontSize: 12.5 }}>{trial.instruction}</p>
                ) : null}
                {trial.status === "verified" ? (
                  <p className="sv-mono" style={{ margin: "6px 0 0", fontSize: 11, color: "var(--sv-text-muted)" }}>
                    {trial.verifierScript ?? "verifier"} · reward.txt {trial.reward == null ? "missing (fails closed — never defaulted to 0)" : formatMissingNumber(trial.reward)}
                  </p>
                ) : null}
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="sv-section" aria-label="Tool stream" aria-live="polite" data-testid="harbor-tool-stream">
        <div className="sv-section-head">
          <h3>Tool stream</h3>
          <span className="sv-mono">{tools.length}</span>
        </div>
        {!showFullStream && tools.length > STREAM_WINDOW ? (
          <button type="button" className="sv-btn" style={{ marginBottom: 6 }} onClick={() => setShowFullStream(true)}>
            Show {tools.length - STREAM_WINDOW} earlier entries
          </button>
        ) : null}
        <ol style={{ listStyle: "none", margin: 0, padding: 0, maxHeight: 280, overflow: "auto", border: tools.length ? "1px solid var(--sv-border)" : "none", borderRadius: 8 }}>
          {visibleTools.map((event, index) => (
            <li key={`${event.ts}-${index}`} className="sv-mono" style={{ fontSize: 12, padding: "4px 10px", borderBottom: "1px solid var(--sv-border)", overflowWrap: "anywhere" }}>
              <span style={{ color: event.kind === "stderr" ? "#b23830" : "var(--sv-text-faint)", marginRight: 6 }}>{event.kind}</span>
              {String(event.payload.name ?? event.payload.text ?? "")}
            </li>
          ))}
          {tools.length === 0 ? (
            <li style={{ color: "var(--sv-text-faint)" }}>Waiting for tools…</li>
          ) : null}
        </ol>
      </section>
    </VisualChrome>
  );
}

export default Shell;

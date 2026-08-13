/**
 * Chronological proposer trace viewer. The narrative (context → generation →
 * output → registered candidates) leads; runtime IDs, paths, and raw usage
 * live in a collapsed technical-details section. The proposer's message-level
 * transcript is not part of the optimizer event contract today, so the steps
 * reflect the milestones the stream actually carries.
 */

import type { GepaProposerReflection, GepaProposerTrace, GepaState, GepaTruncatedText } from "../../components/projectEvents.ts";
import { formatClock } from "./model.ts";

function truncatedBody(value?: GepaTruncatedText): string | null {
  if (!value || value.text == null || value.text === "") return null;
  return value.truncated
    ? `${value.text}\n… truncated (${value.totalChars ?? "?"} chars total; full text in the workspace artifacts)`
    : value.text;
}

function ReflectionBlock({ title, value }: { title: string; value?: GepaTruncatedText }) {
  const body = truncatedBody(value);
  if (!body) return null;
  return (
    <div style={{ marginTop: 8 }}>
      <strong style={{ display: "block", fontSize: 11, color: "var(--sv-text-muted)", textTransform: "uppercase", letterSpacing: ".06em" }}>{title}</strong>
      <p style={{ margin: "3px 0 0", fontSize: 12, whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{body}</p>
    </div>
  );
}

function ReflectionSection({ reflection }: { reflection: GepaProposerReflection }) {
  return (
    <div data-testid="proposer-reflection" style={{ marginTop: 10, padding: "9px 11px", border: "1px solid var(--sv-border)", borderRadius: 8, background: "var(--sv-surface-muted)" }}>
      <ReflectionBlock title="Critique of the parent prompt" value={reflection.critique} />
      {reflection.failurePatterns.length > 0 ? (
        <div style={{ marginTop: 8 }}>
          <strong style={{ display: "block", fontSize: 11, color: "var(--sv-text-muted)", textTransform: "uppercase", letterSpacing: ".06em" }}>What failed</strong>
          <ul style={{ margin: "3px 0 0", paddingLeft: 16, fontSize: 12 }}>
            {reflection.failurePatterns.map((pattern, index) => (
              <li key={index} style={{ overflowWrap: "anywhere" }}>{truncatedBody(pattern)}</li>
            ))}
          </ul>
        </div>
      ) : null}
      {reflection.winningPatterns.length > 0 ? (
        <div style={{ marginTop: 8 }}>
          <strong style={{ display: "block", fontSize: 11, color: "var(--sv-text-muted)", textTransform: "uppercase", letterSpacing: ".06em" }}>What worked</strong>
          <ul style={{ margin: "3px 0 0", paddingLeft: 16, fontSize: 12 }}>
            {reflection.winningPatterns.map((pattern, index) => (
              <li key={index} style={{ overflowWrap: "anywhere" }}>{truncatedBody(pattern)}</li>
            ))}
          </ul>
        </div>
      ) : null}
      <ReflectionBlock title="Rationale" value={reflection.rationale} />
      {reflection.proposals.map((proposal, index) => (
        <div key={index} style={{ marginTop: 8 }}>
          <strong style={{ display: "block", fontSize: 11, color: "var(--sv-text-muted)", textTransform: "uppercase", letterSpacing: ".06em" }}>
            Proposal {index + 1}{proposal.proposalType ? ` · ${proposal.proposalType.replaceAll("_", " ")}` : ""}
          </strong>
          {truncatedBody(proposal.rationale) ? (
            <p style={{ margin: "3px 0 0", fontSize: 12, whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{truncatedBody(proposal.rationale)}</p>
          ) : null}
          {truncatedBody(proposal.proposedPayload) ? (
            <details style={{ marginTop: 4 }}>
              <summary style={{ cursor: "pointer", fontSize: 11, color: "var(--sv-text-muted)" }}>Proposed prompt text</summary>
              <pre className="sv-mono" style={{ margin: "5px 0 0", padding: 8, borderRadius: 6, background: "var(--sv-surface)", whiteSpace: "pre-wrap", overflowWrap: "anywhere", fontSize: 11 }}>
                {truncatedBody(proposal.proposedPayload)}
              </pre>
            </details>
          ) : null}
        </div>
      ))}
    </div>
  );
}

function TraceCard({
  trace,
  onSelectCandidate
}: {
  trace: GepaProposerTrace;
  onSelectCandidate?: (id: string) => void;
}) {
  const running = trace.status === "running";
  const headerBits = [
    trace.model ?? "proposer model",
    running ? "streaming" : trace.status,
    trace.wallSeconds != null ? `${trace.wallSeconds.toFixed(1)} s` : null,
    typeof trace.usage?.total_tokens === "number" ? `${(trace.usage.total_tokens as number).toLocaleString()} tokens` : null
  ].filter(Boolean);
  return (
    <article
      data-testid={`inspect-proposer-trace-${trace.generation}`}
      style={{ border: "1px solid var(--sv-border)", borderRadius: 9, padding: "10px 12px" }}
    >
      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "baseline", gap: 8 }}>
        <strong style={{ fontSize: 12.5 }}>Generation {trace.generation}</strong>
        <span className="sv-mono" style={{ color: "var(--sv-text-muted)" }}>{headerBits.join(" · ")}</span>
        {running ? <span className="sv-chip" data-tone="live" style={{ marginLeft: "auto" }}><span className="sv-live-dot" aria-hidden="true" />live</span> : null}
      </div>
      <ol className="sv-trace-steps" style={{ marginTop: 10 }} aria-live={running ? "polite" : undefined}>
        {(trace.steps ?? []).map((step, index) => {
          const isTail = index === (trace.steps?.length ?? 0) - 1;
          return (
            <li key={`${step.sequence}-${index}`} className="sv-trace-step" data-kind={step.kind} data-live={running && isTail}>
              <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                <strong style={{ fontSize: 12 }}>{step.label}</strong>
                <span className="sv-mono" style={{ color: "var(--sv-text-faint)", fontSize: 10 }}>{formatClock(step.at)}</span>
              </div>
              {step.detail ? <div style={{ color: "var(--sv-text-muted)", fontSize: 11.5 }}>{step.detail}</div> : null}
              {step.kind === "candidate" && step.candidateId ? (
                <button
                  type="button"
                  className="sv-btn"
                  style={{ marginTop: 3, padding: "1px 8px", fontSize: 11 }}
                  onClick={() => onSelectCandidate?.(step.candidateId!)}
                  data-testid={`trace-open-candidate-${step.candidateId}`}
                >
                  Open candidate →
                </button>
              ) : null}
            </li>
          );
        })}
        {running ? (
          <li className="sv-trace-step" data-kind="generation" data-live="true">
            <strong style={{ fontSize: 12 }}>Waiting for the proposer to return…</strong>
            <div style={{ color: "var(--sv-text-muted)", fontSize: 11.5 }}>New milestones stream in as the optimizer reports them.</div>
          </li>
        ) : null}
      </ol>
      {trace.streaming && Object.values(trace.streaming).some((text) => text.length > 0) ? (
        <div data-testid="proposer-streaming" aria-live={running ? "polite" : undefined} style={{ marginTop: 8 }}>
          {Object.entries(trace.streaming).filter(([, text]) => text.length > 0).map(([channel, text]) => (
            <div key={channel} style={{ marginTop: 6 }}>
              <strong style={{ display: "block", fontSize: 11, color: "var(--sv-text-muted)", textTransform: "uppercase", letterSpacing: ".06em" }}>
                {channel}{running ? " · streaming" : ""}
              </strong>
              <pre className="sv-mono" style={{ margin: "4px 0 0", padding: 8, borderRadius: 6, background: "var(--sv-surface-muted)", whiteSpace: "pre-wrap", overflowWrap: "anywhere", fontSize: 11, maxHeight: 220, overflow: "auto" }}>
                {text}
              </pre>
            </div>
          ))}
        </div>
      ) : null}
      {trace.reflection ? <ReflectionSection reflection={trace.reflection} /> : null}
      <details style={{ marginTop: 8 }}>
        <summary style={{ width: "fit-content", cursor: "pointer", color: "var(--sv-text-muted)", fontSize: 11, fontWeight: 650 }}>
          Technical details
        </summary>
        <dl className="sv-mono" style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "3px 10px", margin: "8px 0 0", fontSize: 11 }}>
          <dt style={{ color: "var(--sv-text-faint)" }}>Provider</dt>
          <dd style={{ margin: 0 }}>{trace.provider ?? "—"}</dd>
          <dt style={{ color: "var(--sv-text-faint)" }}>Backend</dt>
          <dd style={{ margin: 0 }}>{trace.backend ?? "—"}</dd>
          <dt style={{ color: "var(--sv-text-faint)" }}>Substrate</dt>
          <dd style={{ margin: 0 }}>{trace.runtimeSubstrate ?? "—"}</dd>
          <dt style={{ color: "var(--sv-text-faint)" }}>Runtime effect</dt>
          <dd style={{ margin: 0, overflowWrap: "anywhere" }}>{trace.runtimeEffectId ?? trace.jobId ?? "—"}</dd>
          <dt style={{ color: "var(--sv-text-faint)" }}>Workspace</dt>
          <dd style={{ margin: 0, overflowWrap: "anywhere" }}>{trace.workspace ?? "—"}</dd>
        </dl>
        {trace.usage ? (
          <pre className="sv-mono" style={{ margin: "8px 0 0", padding: 8, borderRadius: 6, background: "var(--sv-surface-muted)", whiteSpace: "pre-wrap", fontSize: 11 }}>
            {JSON.stringify(trace.usage, null, 2)}
          </pre>
        ) : null}
        <p style={{ margin: "8px 0 0", fontSize: 11, color: "var(--sv-text-faint)" }}>
          Reflection content arrives as <code>proposer.delta</code> chunks live and is reconciled from
          <code> .agent_artifacts/</code> under the workspace path above via <code>proposer.transcript.loaded</code> after completion.
          Raw transport (JSON-RPC / SSE) stays on disk only.
        </p>
      </details>
    </article>
  );
}

export function ProposerTracePanel({
  gepa,
  onSelectCandidate
}: {
  gepa: GepaState;
  onSelectCandidate?: (id: string) => void;
}) {
  return (
    <section className="sv-section" aria-label="Proposer traces" data-testid="gepa-proposer-traces">
      <div className="sv-section-head">
        <h3>Proposer</h3>
        <span className="sv-mono">{gepa.proposerTraces.length} trace{gepa.proposerTraces.length === 1 ? "" : "s"}</span>
      </div>
      {gepa.proposerTraces.length === 0 ? (
        <p style={{ color: "var(--sv-text-faint)", fontSize: 12, margin: 0 }}>
          The proposer starts after the seed evaluation; its trace streams here.
        </p>
      ) : (
        <div style={{ display: "grid", gap: 8 }}>
          {gepa.proposerTraces.map((trace) => (
            <TraceCard key={trace.generation} trace={trace} onSelectCandidate={onSelectCandidate} />
          ))}
        </div>
      )}
    </section>
  );
}

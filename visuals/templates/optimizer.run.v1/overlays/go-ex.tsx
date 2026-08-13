import { formatMissingNumber } from "../../../runtime/liveStream.ts";
import type { ProjectedState } from "../components/projectEvents.ts";

function rows(value: unknown, key?: string): Array<Record<string, unknown>> {
  const nested = key && value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)[key]
    : value;
  return Array.isArray(nested) ? nested as Array<Record<string, unknown>> : [];
}

function text(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map(text).filter(Boolean).join("\n");
  if (value && typeof value === "object") {
    const record = value as Record<string, unknown>;
    return text(record.text ?? record.content ?? record.summary ?? "");
  }
  return "";
}

function traceLine(event: Record<string, unknown>): { title: string; body: string } {
  const params = event.params && typeof event.params === "object" ? event.params as Record<string, unknown> : {};
  const item = params.item && typeof params.item === "object" ? params.item as Record<string, unknown> : {};
  const kind = String(item.type ?? event.method ?? "event");
  const body = text(item.text ?? item.content ?? item.summary)
    || String(item.command ?? item.aggregatedOutput ?? "");
  return { title: kind.replaceAll(/([a-z])([A-Z])/g, "$1 $2"), body };
}

export function GoExOverlay({ state }: { state: ProjectedState }) {
  const goex = state.goex;
  if (!goex) return null;
  const phase = String(goex.board.phase ?? goex.board.current_phase ?? "preparing");
  const tick = goex.board.tick ?? goex.board.tick_index;
  const frontier = goex.frontier.candidate_frontier && typeof goex.frontier.candidate_frontier === "object"
    ? goex.frontier.candidate_frontier as Record<string, unknown>
    : {};
  const frontierIds = new Set(Array.isArray(frontier.global) ? frontier.global.map(String) : []);
  const proposer = goex.agents.coreProposer && typeof goex.agents.coreProposer === "object"
    ? goex.agents.coreProposer as Record<string, unknown>
    : undefined;
  const proposerRun = rows(goex.agents.agent_runs).find((run) => run.role === "core_proposer");
  const proposerTrace = proposerRun?.trace && typeof proposerRun.trace === "object"
    ? proposerRun.trace as Record<string, unknown>
    : undefined;
  const traceEvents = rows(proposerTrace?.received).filter((event) => event.method === "item/completed");
  const proposerStreaming = proposer?.streaming && typeof proposer.streaming === "object" && !Array.isArray(proposer.streaming)
    ? proposer.streaming as Record<string, unknown>
    : {};
  const proposerText = Object.values(proposerStreaming).map(text).filter(Boolean).join("\n");

  return (
    <>
      <section className="sv-section" aria-label="GELO live progress" data-testid="gelo-live-progress">
        <div className="sv-section-head">
          <h3>GELO progress</h3>
          <span className="sv-mono">{phase.replaceAll("_", " ")}</span>
        </div>
        <div className="sv-metrics">
          <div className="sv-metric"><span>Phase</span><strong>{phase.replaceAll("_", " ")}</strong></div>
          <div className="sv-metric"><span>Tick</span><strong>{formatMissingNumber(tick)}</strong></div>
          <div className="sv-metric"><span>Candidates</span><strong>{goex.candidates.length}</strong></div>
          <div className="sv-metric"><span>Child rollouts</span><strong>{goex.rollouts.length}</strong></div>
        </div>
        {goex.board.reason ? <p style={{ margin: "9px 0 0", color: "var(--sv-text-muted)", fontSize: 12 }}>{String(goex.board.reason)}</p> : null}
      </section>

      <section className="sv-section" aria-label="GELO proposer activity" data-testid="gelo-proposer-activity">
        <div className="sv-section-head"><h3>Proposer</h3><span className="sv-mono">{String(proposer?.status ?? "waiting")}</span></div>
        <div style={{ padding: 10, border: "1px solid var(--sv-border)", borderRadius: 8, background: "var(--sv-surface-muted)" }}>
          <strong>{proposer?.status === "running" ? "Creating themes and candidates…" : proposer?.status === "completed" ? "Proposal round complete" : "Waiting for proposer"}</strong>
          {proposer?.round_index != null ? <span className="sv-mono" style={{ marginLeft: 8 }}>round {String(proposer.round_index)}</span> : null}
        </div>
        {proposerRun ? (
          <details style={{ marginTop: 8, border: "1px solid var(--sv-border)", borderRadius: 8, padding: "8px 10px" }}>
            <summary style={{ cursor: "pointer" }}><strong>Inspect proposer trace</strong> · {traceEvents.length} completed items</summary>
            <dl style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "4px 10px", margin: "10px 0" }}>
              <dt>Model</dt><dd className="sv-mono" style={{ margin: 0 }}>{String(proposerRun.model ?? "—")}</dd>
              <dt>Thread</dt><dd className="sv-mono" style={{ margin: 0, overflowWrap: "anywhere" }}>{String(proposerRun.thread_id ?? "—")}</dd>
              <dt>Turn</dt><dd className="sv-mono" style={{ margin: 0, overflowWrap: "anywhere" }}>{String(proposerRun.turn_id ?? "—")}</dd>
            </dl>
            <div role="list" style={{ display: "grid", gap: 6 }}>
              {traceEvents.map((event, index) => {
                const line = traceLine(event);
                return (
                  <details role="listitem" key={`${String(event.emittedAtMs ?? index)}-${index}`} style={{ borderLeft: "2px solid var(--sv-border-strong)", paddingLeft: 9 }}>
                    <summary style={{ cursor: "pointer", textTransform: "capitalize" }}>{line.title}</summary>
                    {line.body ? <pre style={{ margin: "7px 0 0", whiteSpace: "pre-wrap", overflowWrap: "anywhere", color: "var(--sv-text-muted)", font: "inherit" }}>{line.body}</pre> : null}
                  </details>
                );
              })}
            </div>
          </details>
        ) : null}
        {proposerText ? (
          <details style={{ marginTop: 8, border: "1px solid var(--sv-border)", borderRadius: 8, padding: "8px 10px" }}>
            <summary style={{ cursor: "pointer" }}><strong>Inspect proposer stream</strong></summary>
            <pre style={{ margin: "9px 0 0", maxHeight: 420, overflow: "auto", whiteSpace: "pre-wrap", overflowWrap: "anywhere", color: "var(--sv-text-muted)", font: "inherit" }}>{proposerText}</pre>
          </details>
        ) : null}
      </section>

      <section className="sv-section" aria-label="GELO themes" data-testid="gelo-themes">
        <div className="sv-section-head"><h3>Themes</h3><span className="sv-mono">{goex.themes.length}</span></div>
        {goex.themes.length === 0 ? (
          <p style={{ color: "var(--sv-text-faint)", fontSize: 12 }}>Themes will appear when the core proposer returns evidence-backed directions.</p>
        ) : (
          <div role="list" style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))", gap: 8 }}>
            {goex.themes.map((theme, index) => (
              <article role="listitem" key={String(theme.theme_id ?? theme.theme ?? index)} style={{ padding: 10, border: "1px solid var(--sv-border)", borderRadius: 8 }}>
                <strong>{String(theme.title ?? theme.theme ?? theme.theme_id ?? `Theme ${index + 1}`)}</strong>
                <div style={{ marginTop: 5, color: "var(--sv-text-muted)", fontSize: 11 }}>
                  saturation {formatMissingNumber(theme.saturation ?? theme.saturation_score)} · evidence {formatMissingNumber(theme.evidence_count ?? theme.rollout_count)}
                </div>
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="sv-section" aria-label="GELO candidate frontier" data-testid="gelo-candidate-frontier">
        <div className="sv-section-head"><h3>Candidates & frontier</h3><span className="sv-mono">{frontierIds.size} frontier · {goex.candidates.length} total</span></div>
        <div role="list" style={{ display: "grid", gap: 7 }}>
          {goex.candidates.map((candidate, index) => {
            const id = String(candidate.candidate_id ?? candidate.id ?? `candidate ${index + 1}`);
            const onFrontier = candidate.on_frontier === true || frontierIds.has(id);
            return (
              <details role="listitem" key={id} style={{ padding: "9px 11px", border: `1px solid ${onFrontier ? "var(--sv-accent)" : "var(--sv-border)"}`, borderRadius: 8 }}>
                <summary style={{ cursor: "pointer" }}>
                  <strong className="sv-mono">{id}</strong>
                  <span style={{ marginLeft: 8, color: "var(--sv-text-muted)", fontSize: 11 }}>
                    search {formatMissingNumber(candidate.search_mean ?? candidate.mean_reward)} · {String(candidate.status ?? candidate.final_status ?? "registered")}
                    {onFrontier ? " · frontier" : ""}
                  </span>
                </summary>
                <dl style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "5px 10px", margin: "10px 0 0" }}>
                  <dt>Parent</dt><dd className="sv-mono" style={{ margin: 0 }}>{String(candidate.parent_id ?? candidate.parent_candidate_id ?? "baseline")}</dd>
                  <dt>Heldout</dt><dd style={{ margin: 0 }}>{formatMissingNumber(candidate.heldout_mean ?? candidate.heldout_mean_reward)}</dd>
                  <dt>Decision</dt><dd style={{ margin: 0 }}>{String(candidate.decision ?? candidate.final_status ?? "—")}</dd>
                  <dt>Prompt</dt><dd style={{ margin: 0, whiteSpace: "pre-wrap" }}>{String(candidate.prompt_text ?? candidate.react_system_prompt ?? "—")}</dd>
                  <dt>Rationale</dt><dd style={{ margin: 0 }}>{String((candidate.metadata as Record<string, unknown> | undefined)?.rationale ?? (candidate.annotations as Record<string, unknown> | undefined)?.rationale ?? "—")}</dd>
                  <dt>Rollouts</dt><dd className="sv-mono" style={{ margin: 0, overflowWrap: "anywhere" }}>{Array.isArray(candidate.rollout_ids) ? candidate.rollout_ids.join("\n") : "—"}</dd>
                </dl>
              </details>
            );
          })}
        </div>
      </section>

      <section className="sv-section" aria-label="Craftax child rollouts" data-testid="gelo-craftax-rollouts">
        <div className="sv-section-head"><h3>Craftax rollouts</h3><span className="sv-mono">{goex.rollouts.length}</span></div>
        {goex.rollouts.length === 0 ? (
          <p style={{ color: "var(--sv-text-faint)", fontSize: 12 }}>Declared child streams will appear as GELO schedules Craftax evaluations.</p>
        ) : (
          <div role="list" style={{ display: "grid", gap: 6 }}>
            {goex.rollouts.map((rollout) => (
              <details key={rollout.ref.id} role="listitem" style={{ border: "1px solid var(--sv-border)", borderRadius: 8, padding: "8px 10px", fontSize: 12 }}>
                <summary style={{ cursor: "pointer" }}>
                  <strong>seed {rollout.seed ?? "—"}</strong> · {rollout.split ?? rollout.lane ?? "evaluation"} · reward {formatMissingNumber(rollout.reward)} · {rollout.status ?? "running"}
                </summary>
                <dl style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "5px 10px", margin: "10px 0 0" }}>
                  <dt>Rollout</dt><dd className="sv-mono" style={{ margin: 0, overflowWrap: "anywhere" }}>{rollout.ref.id}</dd>
                  <dt>Stream</dt><dd className="sv-mono" style={{ margin: 0, overflowWrap: "anywhere" }}>{rollout.ref.attributes?.stream_id ?? "unavailable"}</dd>
                  <dt>Candidate</dt><dd className="sv-mono" style={{ margin: 0, overflowWrap: "anywhere" }}>{rollout.candidateId ?? "—"}</dd>
                  <dt>Role</dt><dd style={{ margin: 0 }}>{rollout.lane ?? rollout.split ?? "—"}</dd>
                </dl>
              </details>
            ))}
          </div>
        )}
      </section>

      {state.execution.bindings.length > 0 ? (
        <section className="sv-section" aria-label="GELO execution bindings">
          <div className="sv-section-head"><h3>Execution</h3></div>
          <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
            {state.execution.bindings.map((binding, index) => (
              <li key={index}><strong>{String(binding.label ?? binding.kind)}</strong> · {String(binding.status ?? "")}</li>
            ))}
          </ul>
        </section>
      ) : null}
    </>
  );
}

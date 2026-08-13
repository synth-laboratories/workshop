/**
 * The GEPA workspace: sticky run header, semantic stage timeline, search
 * canvas (frontier + candidates + inspector), scalable evaluation browser,
 * chronological proposer traces, and an optional run comparison. Raw events
 * and artifacts live behind the debug section the host template provides.
 */

import { useMemo, useRef, useState } from "react";
import { formatMissingUsd } from "../../../../runtime/liveStream.ts";
import type { ReactNode } from "react";
import {
  projectAtCursor,
  type OptimizerEvent,
  type OptimizerRun,
  type ProjectedState
} from "../../components/projectEvents.ts";
import { normalizeOptimizerEvents } from "../../components/normalizeEvents.ts";
import {
  StageTimeline,
  WorkspaceHeader,
  type WorkspaceLane,
  type WorkspaceMetric
} from "../../components/workspace/WorkspaceChrome.tsx";
import { RolloutBrowser, type RolloutGroup, type RolloutRow } from "../../components/workspace/RolloutBrowser.tsx";
import { CandidateInspector, CandidateList } from "./CandidateBoard.tsx";
import { ComparisonCard, type ComparisonColumn } from "./ComparisonCard.tsx";
import { FrontierPanel } from "./FrontierPanel.tsx";
import { HillClimbPanel } from "./HillClimbPanel.tsx";
import { ProposerTracePanel } from "./ProposerTracePanel.tsx";
import {
  candidateName,
  elapsedLabel,
  limitOf,
  minibatchComparison,
  stageTitle
} from "./model.ts";

const STAGE_FILTER_TO_EVAL: Record<string, string[]> = {
  seed: ["seed_full_train"],
  minibatch: ["parent_minibatch_reference", "candidate_minibatch"],
  full_train: ["candidate_full_train"],
  heldout: ["heldout"]
};

function statusPresentation(status: string, terminal: boolean): {
  text: string;
  tone?: "live" | "ok" | "bad" | "warn";
  dot: boolean;
} {
  if (status === "failed") return { text: "Failed", tone: "bad", dot: false };
  if (["canceled", "cancelled"].includes(status)) return { text: "Canceled", tone: "warn", dot: false };
  if (terminal) return { text: "Completed", tone: "ok", dot: false };
  if (["created", "queued", "pending", "loading"].includes(status)) {
    return { text: status[0].toUpperCase() + status.slice(1), dot: false };
  }
  // Any in-flight optimizer phase (proposing, rollout_running, evaluating, …)
  // is run-level "Running"; the stage timeline carries the phase detail.
  return { text: "Running", tone: "live", dot: true };
}

function EvidenceIntegrity({ gepa }: { gepa: NonNullable<ProjectedState["gepa"]> }) {
  const required = gepa.coverage.reduce((sum, row) => sum + row.required, 0);
  const scored = gepa.coverage.reduce((sum, row) => sum + row.scored, 0);
  const failed = gepa.coverage.reduce((sum, row) => sum + row.failed, 0);
  const pending = gepa.coverage.reduce((sum, row) => sum + row.pending, 0);
  const complete = gepa.coverage.length > 0 && failed === 0 && pending === 0 && scored === required;
  if (gepa.coverage.length === 0 && gepa.failedAttempts.length === 0 && !gepa.heldout?.blocked) return null;
  return (
    <section className="sv-section" aria-label="Evaluation evidence integrity" data-testid="gepa-evidence-integrity" style={{ marginTop: 0 }}>
      <div className="sv-section-head">
        <h3>Evidence integrity</h3>
        <span className="sv-mono" style={{ color: complete ? "var(--sv-accent)" : failed > 0 ? "#b23830" : "var(--sv-text-muted)" }}>
          {complete ? "complete" : failed > 0 ? "incomplete · promotion blocked" : "collecting"}
        </span>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(4, minmax(0, 1fr))", border: "1px solid var(--sv-border)", borderRadius: 9, overflow: "hidden" }}>
        {[["Required", required], ["Scored", scored], ["Failed", failed], ["Pending", pending]].map(([label, value]) => (
          <div key={String(label)} style={{ padding: "10px 12px", borderRight: label === "Pending" ? undefined : "1px solid var(--sv-border)" }}>
            <div style={{ color: "var(--sv-text-faint)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".08em" }}>{label}</div>
            <div className="sv-mono" style={{ marginTop: 3, fontSize: 17 }}>{value}</div>
          </div>
        ))}
      </div>
      {gepa.failedAttempts.length > 0 ? (
        <details style={{ marginTop: 8 }}>
          <summary style={{ cursor: "pointer", color: "#b23830", fontSize: 12, fontWeight: 650 }}>
            {gepa.failedAttempts.length} exhausted rollout {gepa.failedAttempts.length === 1 ? "attempt" : "attempts"} — excluded from scores
          </summary>
          <div style={{ display: "grid", gap: 6, marginTop: 7 }}>
            {gepa.failedAttempts.slice(-12).map((failure) => (
              <div key={`${failure.sequence}-${failure.exampleId ?? "unknown"}`} style={{ border: "1px solid var(--sv-border)", borderRadius: 7, padding: "8px 10px", fontSize: 11.5 }}>
                <span className="sv-mono">{failure.candidateId ?? "candidate"} · {failure.stage ?? "evaluation"} · {failure.exampleId ?? "unknown example"}</span>
                <div style={{ color: "var(--sv-text-muted)", marginTop: 3 }}>
                  {failure.failureClass ?? "rollout_failed"}{failure.attempt != null ? ` · attempt ${failure.attempt}${failure.maxAttempts != null ? `/${failure.maxAttempts}` : ""}` : ""}
                  {failure.message ? ` · ${failure.message}` : ""}
                </div>
              </div>
            ))}
          </div>
        </details>
      ) : null}
      {gepa.heldout?.blocked ? (
        <p style={{ margin: "8px 0 0", color: "#b23830", fontSize: 12 }}>
          Heldout promotion was blocked because the evidence set was incomplete. No missing result was treated as zero.
        </p>
      ) : null}
    </section>
  );
}

export type GepaComparisonPayload = {
  run: OptimizerRun;
  events: OptimizerEvent[];
  label?: string;
};

export function GepaWorkspace({
  projected,
  run,
  comparison,
  debug,
  selectedCandidate,
  setSelectedCandidate,
  embedded = false
}: {
  projected: ProjectedState;
  run: OptimizerRun;
  comparison?: GepaComparisonPayload | null;
  debug?: ReactNode;
  selectedCandidate: string | null;
  setSelectedCandidate: (id: string | null) => void;
  embedded?: boolean;
}) {
  const gepa = projected.gepa;
  const [stageFilter, setStageFilter] = useState<string | null>(null);
  const tracesRef = useRef<HTMLDivElement | null>(null);
  const comparisonProjection = useMemo(() => {
    if (!comparison) return null;
    try {
      // The host hands over the sibling run's raw persisted page; normalize
      // wire aliases exactly like the primary run's shell does.
      const events = normalizeOptimizerEvents(comparison.events as unknown[]);
      const other = projectAtCursor(comparison.run, events);
      return other.gepa ? { runId: comparison.run.id, gepa: other.gepa, label: comparison.label } : null;
    } catch {
      // A malformed sibling page must never take down the primary view.
      return null;
    }
  }, [comparison]);

  const evaluationData = useMemo(() => {
    if (!gepa) return { groups: [] as RolloutGroup[], rows: [] as RolloutRow[] };
    const candidateById = new Map(gepa.candidates.map((candidate) => [String(candidate.id), candidate]));
    const stageAllowList = stageFilter ? STAGE_FILTER_TO_EVAL[stageFilter] : undefined;
    const rows: RolloutRow[] = [];
    const groups: RolloutGroup[] = [];
    const seenGroups = new Set<string>();
    for (const evaluation of gepa.evaluations) {
      const stage = evaluation.stage ?? "unknown";
      if (stageAllowList && !stageAllowList.includes(stage)) continue;
      const candidateId = evaluation.candidateId ?? "unknown";
      const groupKey = `${candidateId}::${stage}`;
      rows.push({
        id: evaluation.ref.id,
        groupKey,
        sequence: evaluation.sequence,
        exampleId: evaluation.exampleId,
        stage,
        reward: evaluation.reward,
        costUsd: evaluation.costUsd,
        usage: evaluation.usage,
        streamId: evaluation.ref.attributes?.stream_id,
        rewardUrl: evaluation.ref.attributes?.reward_url,
        occurredAt: evaluation.occurredAt
      });
      if (!seenGroups.has(groupKey)) {
        seenGroups.add(groupKey);
        const candidate = candidateById.get(candidateId);
        const parentId = candidate?.parentId == null ? undefined : String(candidate.parentId);
        const extras = stage === "candidate_minibatch" && parentId
          ? (() => {
              const comparisonRows = minibatchComparison(gepa.evaluations, parentId, candidateId)
                .filter((row) => row.parent !== undefined || row.candidate !== undefined);
              if (comparisonRows.length === 0) return undefined;
              const parent = candidateById.get(parentId);
              return (
                <details style={{ marginBottom: 8 }} data-testid={`minibatch-compare-${candidateId}`}>
                  <summary style={{ width: "fit-content", cursor: "pointer", fontSize: 11.5, fontWeight: 650, color: "var(--sv-text-muted)" }}>
                    Compare with parent on the same minibatch
                  </summary>
                  <table className="sv-table" style={{ marginTop: 6 }}>
                    <thead>
                      <tr>
                        <th scope="col">Example</th>
                        <th scope="col">{parent ? candidateName(parent) : "Parent"}</th>
                        <th scope="col">{candidate ? candidateName(candidate) : "Proposal"}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {comparisonRows.map((row) => {
                        const differs = row.parent != null && row.candidate != null && row.parent !== row.candidate;
                        return (
                          <tr key={row.exampleId} style={differs ? { background: "var(--sv-accent-soft)" } : undefined}>
                            <td className="sv-mono">{row.exampleId}</td>
                            <td className="sv-mono">{row.parent == null ? "—" : row.parent.toFixed(2)}</td>
                            <td className="sv-mono">{row.candidate == null ? "—" : row.candidate.toFixed(2)}</td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </details>
              );
            })()
          : undefined;
        groups.push({
          key: groupKey,
          title: candidate ? candidateName(candidate) : candidateId,
          subtitle: stageTitle(stage),
          extras
        });
      }
    }
    return { groups, rows };
  }, [gepa, stageFilter]);

  if (!gepa) return null;

  const status = String(projected.summary.status ?? run.status ?? "");
  const terminal = gepa.activity.terminal;
  const presentation = statusPresentation(status, terminal);

  const rolloutLimit = limitOf(gepa, "total_rollouts");
  const costLimit = limitOf(gepa, "cost_usd");
  const proposerLimit = limitOf(gepa, "proposer_calls");
  const rolloutSpentValue = Math.max(rolloutLimit?.spent ?? 0, gepa.rolloutsCompleted);
  const rolloutSpent = rolloutSpentValue > 0 ? rolloutSpentValue : undefined;
  const proposerSpent = proposerLimit?.spent ??
    (gepa.proposerTraces.filter((trace) => trace.status === "completed").length || undefined);
  const costSpent = costLimit?.spent ?? projected.usage.costUsd;
  const heldoutValue = gepa.heldout?.blocked
    ? "blocked"
    : gepa.heldout?.skipped
    ? "skipped"
    : gepa.heldout?.reward != null
      ? gepa.heldout.reward.toFixed(2)
      : gepa.best?.heldoutReward != null
        ? gepa.best.heldoutReward.toFixed(2)
        : "—";
  const bestScore = gepa.best?.trainReward ??
    (typeof (projected.summary.summary as Record<string, unknown> | undefined)?.bestScore === "number"
      ? (projected.summary.summary as Record<string, number>).bestScore
      : undefined);

  const metrics: WorkspaceMetric[] = [
    { label: "Best train", value: bestScore != null ? bestScore.toFixed(2) : "—" },
    { label: "Heldout", value: heldoutValue },
    {
      label: "Rollouts",
      value: rolloutSpent != null
        ? `${Math.round(rolloutSpent)}${rolloutLimit?.max != null ? ` / ${Math.round(rolloutLimit.max)}` : ""}`
        : "—"
    },
    {
      label: "Proposer calls",
      value: proposerSpent != null ? `${Math.round(proposerSpent)}` : "—"
    },
    {
      label: "Cost",
      value: costSpent != null && costSpent > 0 ? formatMissingUsd(costSpent) : "unavailable",
      title: costSpent != null && costSpent > 0
        ? costLimit?.max != null ? `Budget ceiling ${formatMissingUsd(costLimit.max)}` : undefined
        : "This run did not report usable cost telemetry"
    },
    { label: "Elapsed", value: elapsedLabel(gepa.timing, terminal) },
    ...(gepa.models.proposer ? [{ label: "Proposer", value: gepa.models.proposer }] : []),
    ...(gepa.models.policy ? [{ label: "Policy", value: gepa.models.policy }] : [])
  ];

  const lanes: WorkspaceLane[] = terminal ? [] : [
    {
      id: "proposing",
      label: "Proposing",
      active: gepa.activity.proposalActive,
      detail: gepa.activity.proposalActive
        ? `${gepa.models.proposer ?? "proposer"} · gen ${gepa.activity.generation ?? 0}`
        : "idle"
    },
    {
      id: "evaluating",
      label: "Evaluating",
      active: gepa.activity.evaluationActive,
      detail: gepa.activity.evaluationActive
        ? `${gepa.activity.activeCandidateIds.length || ""} ${gepa.activity.activeCandidateIds.length === 1 ? "candidate" : "candidates"} · ${(gepa.activity.evaluationStage ?? "rollouts").replaceAll("_", " ")}`.trim()
        : "idle"
    }
  ];

  const showTrace = () => {
    tracesRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <div className="sv-workspace" data-testid="gepa-workspace">
      {!embedded ? (
        <WorkspaceHeader
          statusText={presentation.text}
          statusTone={presentation.tone}
          live={presentation.dot}
          headline={gepa.activity.label}
          detail={gepa.activity.detail !== gepa.activity.label ? gepa.activity.detail : undefined}
          metrics={metrics}
          lanes={lanes}
          testId="gepa-run-header"
        />
      ) : null}
      <StageTimeline
        stages={gepa.stages}
        selected={stageFilter}
        onSelect={(id) => {
          setStageFilter(id);
          if (id === "proposal") showTrace();
        }}
        testId="gepa-stage-timeline"
      />
      <EvidenceIntegrity gepa={gepa} />
      <HillClimbPanel gepa={gepa} onSelect={setSelectedCandidate} />
      <div className="sv-workspace-canvas">
        <div>
          <FrontierPanel gepa={gepa} selectedId={selectedCandidate} onSelect={setSelectedCandidate} />
          <CandidateList gepa={gepa} selectedId={selectedCandidate} onSelect={setSelectedCandidate} />
        </div>
        <div>
          <CandidateInspector
            gepa={gepa}
            selectedId={selectedCandidate}
            onSelect={setSelectedCandidate}
            onShowTrace={showTrace}
          />
        </div>
      </div>
      <RolloutBrowser
        groups={evaluationData.groups}
        rows={evaluationData.rows}
        emptyText={stageFilter
          ? "No rollouts for the selected stage yet. Clear the stage filter to see everything."
          : "Evaluation rollouts appear as candidates are scored."}
        testId="gepa-child-evaluations"
      />
      <div ref={tracesRef}>
        <ProposerTracePanel gepa={gepa} onSelectCandidate={setSelectedCandidate} />
      </div>
      {comparisonProjection ? (
        <ComparisonCard
          columns={[
            { runId: run.id, label: gepa.models.proposer ?? "This run", gepa },
            {
              runId: comparisonProjection.runId,
              label: comparisonProjection.label ?? comparisonProjection.gepa.models.proposer ?? "Comparison run",
              gepa: comparisonProjection.gepa
            } satisfies ComparisonColumn
          ]}
        />
      ) : null}
      {debug ? (
        <details data-testid="gepa-debug">
          <summary style={{ width: "fit-content", cursor: "pointer", color: "var(--sv-text-muted)", fontSize: 12, fontWeight: 650 }}>
            Debug · raw events, artifacts, usage
          </summary>
          {debug}
        </details>
      ) : null}
    </div>
  );
}

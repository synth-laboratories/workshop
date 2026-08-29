import React from "react";
import { createRoot } from "react-dom/client";
import "../../apps/synth_desktop/src/renderer/src/styles/app.css";
import "../chrome/tokens.css";
import { GepaWorkspace } from "../families/optimizers/_shared/optimizer.run.v1/overlays/gepa/GepaWorkspace.tsx";
import { projectAtCursor, type OptimizerRun, type ProjectedState } from "../families/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import { normalizeOptimizerEvents } from "../families/optimizers/_shared/optimizer.run.v1/components/normalizeEvents.ts";

const candidates = [
  { id: "gepa_seed", source: "seed", status: "full_train_evaluated", score: 0.71, train_reward: 0.71, sequence: 12 },
  { id: "gepa_1", parentId: "gepa_seed", generation: 0, proposal_index: 0, status: "rejected", score: 0.68, train_reward: 0.68, sequence: 31 },
  { id: "gepa_2", parentId: "gepa_seed", generation: 1, proposal_index: 0, status: "accepted", score: 0.79, train_reward: 0.79, sequence: 54 },
  { id: "gepa_3", parentId: "gepa_2", generation: 2, proposal_index: 0, status: "evaluating", score: 0.82, train_reward: 0.82, sequence: 76 }
];
const evaluations = candidates.flatMap((candidate, candidateIndex) =>
  Array.from({ length: 5 }, (_, index) => ({
    candidateId: candidate.id, sequence: candidate.sequence + index,
    ref: { kind: "container_rollout" as const, id: `${candidate.id}-roll-${index}`, role: "candidate_evaluation", schema: "synth.resource-ref.v1", attributes: { stream_id: `${candidate.id}-stream-${index}` } },
    stage: candidateIndex === 3 ? "candidate_minibatch" : "candidate_full_train", exampleId: `banking77:${index}`,
    reward: index < Math.round(candidate.score * 5) ? 1 : 0, costUsd: 0.004, usage: { total_tokens: 420 + index }
  }))
);
const fixtureProjected: ProjectedState = {
  cursorSeq: 82, summary: { status: "running" }, timeline: [], usage: { costUsd: 0.08 }, logs: [], artifacts: [], execution: { bindings: [] },
  gepa: {
    candidates, frontier: [{ candidateId: "gepa_2" }, { candidateId: "gepa_3" }], reflections: [], budget: {}, limits: [{ kind: "total_rollouts", max: 100, spent: 80 }, { kind: "cost_usd", max: 2, spent: 0.08 }],
    contract: { task: { id: "banking77", name: "Banking77 intent classification" }, program: { id: "banking77_classifier", mutableFields: ["system_prompt"] }, objectiveSet: { frontierType: "per_example", selectionObjective: "accuracy", objectives: [{ name: "accuracy", direction: "maximize", aggregation: "mean" }] }, splits: { minibatch: 10, reflection: 10, pareto: 50, heldout: 20 }, container: { rewardAuthority: "container", policyHarness: "chat_completions" } },
    frontierHistory: [{ sequence: 12, bestCandidateId: "gepa_seed", bestTrainReward: .71, bestCandidateSolved: 35, optimisticSolved: 35, totalExamples: 50, coverageSemantics: "solved_reward_positive", frontierSize: 1, addedCandidateIds: ["gepa_seed"], removedCandidateIds: [] }, { sequence: 54, generation: 1, bestCandidateId: "gepa_2", bestTrainReward: .79, bestCandidateSolved: 40, optimisticSolved: 44, totalExamples: 50, coverageSemantics: "solved_reward_positive", frontierSize: 2, addedCandidateIds: ["gepa_2"], removedCandidateIds: [] }],
    stages: [
      { id: "seed", label: "Seed evaluation", status: "completed" }, { id: "proposal", label: "Reflection + proposal", status: "completed" },
      { id: "minibatch", label: "Minibatch gate", status: "active" }, { id: "full_train", label: "Full train evaluation", status: "pending" },
      { id: "heldout", label: "Heldout", status: "pending" }, { id: "complete", label: "Complete", status: "pending" }
    ],
    evaluations, failedAttempts: [{ candidateId: "gepa_3", sequence: 81, stage: "candidate_minibatch", exampleId: "banking77:5", jobId: "job-5", attempt: 3, maxAttempts: 3, failureClass: "stream_timeout", message: "policy stream timed out after recorded cursor 218" }],
    coverage: [{ candidateId: "gepa_3", stage: "candidate_minibatch", required: 10, scored: 8, failed: 1, pending: 1, complete: false, promotionEligible: false, sequence: 82 }],
    proposerTraces: [], activity: { phase: "rollout_running", label: "Evaluating minibatch", proposalActive: false, evaluationActive: true, evaluationStage: "candidate_minibatch", activeCandidateIds: ["gepa_3"], generation: 2, sequence: 82, terminal: false },
    incumbentId: "gepa_2", best: { candidateId: "gepa_3", trainReward: 0.82 }, models: { proposer: "gpt-5.6-sol", policy: "gpt-5.6-luna" }, timing: { startedAt: "2026-08-13T01:00:00Z", lastEventAt: "2026-08-13T01:08:00Z" }, rolloutsCompleted: 80,
    runtime: { activeWorkers: 3, semaphoreSize: 3, queuedRollouts: 17, rolloutsPerMinute: 12.4 }
  }
};

function App() {
  const [selected, setSelected] = React.useState<string | null>(null);
  const [live, setLive] = React.useState<{ run: OptimizerRun; events: unknown[] } | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    let active = true;
    let timer: number | undefined;
    const poll = async () => {
      try {
        const response = await fetch("/api/gepa-events", { cache: "no-store" });
        if (!response.ok) throw new Error(`live event bridge returned HTTP ${response.status}`);
        const payload = await response.json() as { run: OptimizerRun; events: unknown[] };
        if (active) {
          setLive(payload);
          setError(null);
        }
      } catch (cause) {
        if (active) setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        if (active) timer = window.setTimeout(poll, 750);
      }
    };
    void poll();
    return () => {
      active = false;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, []);

  const run = live?.run ?? { id: "banking77-gepa-qa", algorithmId: "gepa", status: "running" };
  const projected = React.useMemo(() => {
    if (!live) return fixtureProjected;
    const events = normalizeOptimizerEvents(live.events);
    return projectAtCursor(run, events);
  }, [live, run]);

  React.useEffect(() => {
    const ids = projected.gepa?.candidates
      .map((candidate) => String(candidate.id ?? candidate.candidateId ?? candidate.candidate_id ?? ""))
      .filter(Boolean) ?? [];
    if (selected === null || !ids.includes(selected)) {
      setSelected(projected.gepa?.incumbentId ?? ids.at(-1) ?? null);
    }
  }, [projected, selected]);

  const jobState = projected.gepa?.runtime.job?.state;
  const bridgeLabel = live
    ? `${jobState && jobState !== "running" ? jobState.toUpperCase() : "LIVE"} · ${run.id} · ${live.events.length} recorded events`
    : `FIXTURE · ${error ?? "connecting to live event bridge"}`;
  const bridgeColor = jobState === "terminated" || jobState === "failed" ? "#b23830" : live ? "#16a36a" : "#a36b16";

  return <main className="synth-visual-root" style={{ maxWidth: 1320, margin: "0 auto" }}>
    <div style={{ padding: "10px 14px", fontFamily: "ui-monospace, monospace", fontSize: 12, color: bridgeColor }}>
      {bridgeLabel}
    </div>
    <GepaWorkspace projected={projected} run={run} selectedCandidate={selected} setSelectedCandidate={setSelected} />
  </main>;
}

const qaGlobal = globalThis as typeof globalThis & { __synthGepaQaRoot?: ReturnType<typeof createRoot> };
const qaRoot = qaGlobal.__synthGepaQaRoot ?? createRoot(document.getElementById("root")!);
qaGlobal.__synthGepaQaRoot = qaRoot;
qaRoot.render(<App />);

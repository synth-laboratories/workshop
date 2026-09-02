import type { ReactNode } from "react";
import type { GepaState } from "../../components/projectEvents.ts";
import { formatDurationMs } from "./model.ts";

function label(value?: string): string {
  return value ? value.replaceAll("_", " ") : "pending";
}

function count(value?: number): string {
  return value == null ? "pending" : value.toLocaleString();
}

function limitLabel(kind: string): string {
  return ({ total_rollouts: "Rollouts", proposer_calls: "Proposer calls", cost_usd: "Cost (USD)", wall_time_seconds: "Wall time" } as Record<string, string>)[kind] ?? label(kind);
}

function DetailCard({ title, eyebrow, children, testId }: { title: string; eyebrow?: string; children: ReactNode; testId?: string }) {
  return (
    <div data-testid={testId} style={{ minWidth: 0, border: "1px solid var(--sv-border)", borderRadius: 9, padding: 11, background: "var(--sv-surface)" }}>
      {eyebrow ? <div style={{ color: "var(--sv-text-faint)", fontSize: 9, letterSpacing: ".08em", textTransform: "uppercase", marginBottom: 3 }}>{eyebrow}</div> : null}
      <strong style={{ display: "block", fontSize: 12 }}>{title}</strong>
      <dl style={{ display: "grid", gridTemplateColumns: "minmax(78px, .7fr) minmax(0, 1.5fr)", gap: "5px 9px", margin: "8px 0 0", fontSize: 11 }}>
        {children}
      </dl>
    </div>
  );
}

function Detail({ name, children, title }: { name: string; children: ReactNode; title?: string }) {
  return <>
    <dt style={{ color: "var(--sv-text-faint)" }}>{name}</dt>
    <dd title={title} style={{ minWidth: 0, margin: 0, overflowWrap: "anywhere" }}>{children}</dd>
  </>;
}

export function SearchOverviewPanel({ gepa }: { gepa: GepaState }) {
  const contract = gepa.contract;
  const objective = contract.objectiveSet;
  const nearest = gepa.nearestLimit;
  const dataset = contract.dataset;
  const container = contract.container;
  const seed = gepa.candidates.find((candidate) => String(candidate.source ?? "") === "seed" || candidate.parentId == null);
  const seedScore = typeof seed?.train_reward === "number" ? seed.train_reward : typeof seed?.score === "number" ? seed.score : undefined;
  const bestScore = gepa.best?.trainReward;
  const lift = seedScore != null && bestScore != null ? bestScore - seedScore : undefined;
  const proposerRunning = gepa.proposerTraces.filter((trace) => trace.status === "running").length;
  const proposerCompleted = gepa.proposerTraces.filter((trace) => trace.status === "completed").length;
  const proposerFailed = gepa.proposerTraces.filter((trace) => ["failed", "cancelled", "canceled"].includes(trace.status)).length;
  const scored = gepa.evaluations.filter((evaluation) => evaluation.reward != null).length;
  const attached = gepa.evaluations.filter((evaluation) => evaluation.reward == null).length;
  const endpoint = container?.url?.replace(/^https?:\/\//, "") ?? "pending";

  return (
    <section className="sv-section" aria-label="GEPA experiment setup" data-testid="gepa-search-overview" style={{ marginTop: 0 }}>
      <div className="sv-section-head">
        <h3>Experiment setup</h3>
        <span className="sv-mono">configuration · dataset · container · related work</span>
      </div>
      <div data-testid="gepa-experiment-context" style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(min(238px, 100%), 1fr))", gap: 9 }}>
        <DetailCard title={contract.task?.name ?? contract.task?.id ?? "Task pending"} eyebrow="Configuration" testId="gepa-config-card">
          <Detail name="Task ID">{contract.task?.id ?? "pending"}</Detail>
          <Detail name="Program">{contract.program?.id ?? "pending"}</Detail>
          <Detail name="Objective">{objective?.selectionObjective ?? objective?.objectives[0]?.name ?? "pending"}</Detail>
          <Detail name="Mutable">{contract.program?.mutableFields.join(", ") || "pending"}</Detail>
          <Detail name="Policy">{gepa.models.policy ?? container?.policyConfig ?? "pending"}</Detail>
          <Detail name="Proposer">{gepa.models.proposer ?? "pending"}</Detail>
        </DetailCard>

        <DetailCard title={dataset?.source ?? "Dataset pending"} eyebrow="Dataset" testId="gepa-dataset-card">
          <Detail name="Version">{[dataset?.config, dataset?.revision].filter(Boolean).join(" · ") || "pending"}</Detail>
          <Detail name="Catalog">{count(dataset?.rowCount)} rows · {count(dataset?.labelCount)} labels</Detail>
          <Detail name="Source splits">train {count(dataset?.splits?.train)} · selection {count(dataset?.splits?.selection)} · heldout {count(dataset?.splits?.heldout)}</Detail>
          <Detail name="Run taskset">train {count(contract.splits?.train)} · heldout {count(contract.splits?.heldout)}</Detail>
          <Detail name="Search pools">mini {count(contract.splits?.minibatch)} · reflect {count(contract.splits?.reflection)} · Pareto {count(contract.splits?.pareto)}</Detail>
          <Detail name="Digest" title={dataset?.digest}>{dataset?.digest ? `${dataset.digest.slice(0, 18)}…` : "pending"}</Detail>
        </DetailCard>

        <DetailCard title={container?.specId ?? container?.targetId ?? "Container pending"} eyebrow="Container" testId="gepa-container-card">
          <Detail name="Binding">{container?.verified ? "contract verified" : "pending"}</Detail>
          <Detail name="Instance">{container?.workshopInstance ?? "pending"}</Detail>
          <Detail name="Endpoint" title={container?.url}>{endpoint}</Detail>
          <Detail name="Runtime">{label(container?.runtimeFamily)}</Detail>
          <Detail name="Evaluator">{container?.evaluatorId ?? "pending"}</Detail>
          <Detail name="Reward owner">{label(container?.rewardAuthority)}</Detail>
          <Detail name="Credential">{label(container?.credentialMode)}</Detail>
        </DetailCard>

        <DetailCard title={`${gepa.candidates.length} candidate${gepa.candidates.length === 1 ? "" : "s"} · ${scored} scored`} eyebrow="Related work" testId="gepa-related-work-card">
          <Detail name="Rollouts">{gepa.rolloutsCompleted.toLocaleString()} completed · {attached.toLocaleString()} attached</Detail>
          <Detail name="Runtime">{gepa.runtime.activeWorkers ?? 0} active / {gepa.runtime.semaphoreSize ?? "?"} slots · {gepa.runtime.queuedRollouts ?? 0} queued</Detail>
          <Detail name="Proposer">{proposerRunning} running · {proposerCompleted} complete · {proposerFailed} failed</Detail>
          <Detail name="Failures">{gepa.failedAttempts.length.toLocaleString()} exhausted attempts</Detail>
          <Detail name="Current phase">{gepa.activity.label}</Detail>
          <Detail name="Generation">{gepa.activity.generation ?? (gepa.proposerTraces.length ? Math.max(...gepa.proposerTraces.map((trace) => trace.generation)) : "pending")}</Detail>
        </DetailCard>
      </div>

      <div className="sv-gepa-contract-outcome" style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.35fr) minmax(230px, .65fr)", gap: 9, marginTop: 9 }}>
        <div style={{ minWidth: 0, border: "1px solid var(--sv-border)", borderRadius: 9, padding: 11 }}>
          <strong style={{ fontSize: 12 }}>Search contract</strong>
          <div style={{ display: "flex", flexWrap: "wrap", gap: "7px 13px", marginTop: 8, fontSize: 11 }}>
            <span><span style={{ color: "var(--sv-text-faint)" }}>Frontier</span> · {label(objective?.frontierType)}</span>
            <span><span style={{ color: "var(--sv-text-faint)" }}>Select on</span> · {objective?.selectionObjective ?? "pending"}</span>
            <span><span style={{ color: "var(--sv-text-faint)" }}>Retention</span> · {label(container?.retention)}</span>
            <span><span style={{ color: "var(--sv-text-faint)" }}>Scale leases</span> · {container?.scaleLeases ?? "pending"}</span>
          </div>
          {contract.task?.description ? <p style={{ margin: "8px 0 0", color: "var(--sv-text-muted)", fontSize: 10.5 }}>{contract.task.description}</p> : null}
        </div>
        <div style={{ border: "1px solid var(--sv-border)", borderRadius: 9, padding: 11 }} data-testid="gepa-outcome-card">
          <strong style={{ fontSize: 12 }}>Outcome so far</strong>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 6, marginTop: 9 }}>
            {[["Seed", seedScore], ["Incumbent", bestScore], ["Lift", lift]].map(([name, value]) => <div key={String(name)}><div style={{ color: "var(--sv-text-faint)", fontSize: 9.5, textTransform: "uppercase" }}>{name}</div><div className="sv-mono" style={{ marginTop: 3, fontSize: 15 }}>{typeof value === "number" ? `${name === "Lift" && value >= 0 ? "+" : ""}${value.toFixed(3)}` : "—"}</div></div>)}
          </div>
          <p style={{ margin: "9px 0 0", color: "var(--sv-text-muted)", fontSize: 10.5 }}>Heldout stays isolated from candidate selection.</p>
        </div>
      </div>

      {gepa.limits.length ? <div style={{ display: "grid", gap: 7, marginTop: 10 }}>
        {gepa.limits.map((limit) => {
          const max = limit.max ?? 0;
          const spent = limit.spent ?? 0;
          const reserved = limit.reserved ?? 0;
          const usedPct = max > 0 ? Math.min(100, spent / max * 100) : 0;
          const reservedPct = max > 0 ? Math.min(100 - usedPct, reserved / max * 100) : 0;
          const isNearest = nearest?.kind === limit.kind;
          return <div key={limit.kind} className="sv-gepa-limit-row" style={{ display: "grid", gap: 10, alignItems: "center", fontSize: 10.5 }}>
            <span className="sv-gepa-limit-label"><strong>{limitLabel(limit.kind)}</strong>{isNearest ? <span className="sv-chip" data-tone="warn" style={{ marginLeft: 5 }}>nearest</span> : null}</span>
            <span className="sv-gepa-limit-meter" style={{ height: 11, position: "relative", borderRadius: 99, background: "var(--sv-surface-muted)", overflow: "hidden" }}><span style={{ position: "absolute", inset: 0, width: `${usedPct}%`, background: "var(--sv-accent)" }} /><span style={{ position: "absolute", top: 0, bottom: 0, left: `${usedPct}%`, width: `${reservedPct}%`, background: "var(--sv-border-strong)" }} /></span>
            <span className="sv-gepa-limit-receipt sv-mono" style={{ color: "var(--sv-text-muted)" }}>{limit.spent ?? "—"} spent + {limit.reserved ?? 0} reserved / {limit.max ?? "—"}{limit.forecast?.secondsToLimit != null ? ` · ~${formatDurationMs(limit.forecast.secondsToLimit * 1000)} left` : ""}</span>
          </div>;
        })}
      </div> : null}
    </section>
  );
}
